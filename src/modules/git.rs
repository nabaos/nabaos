//! Safe git wrappers using `std::process::Command`.
//!
//! SECURITY: No shell invocation. Environment is cleared and only PATH, HOME,
//! and GIT_TERMINAL_PROMPT=0 are set. All paths are sanitized against option
//! injection (rejecting args starting with `-`). Clone URLs are restricted to
//! https-only with SSRF hostname validation. Push requires explicit remote+branch.

use std::path::Path;

/// Output of a git operation.
#[derive(Debug, Clone)]
pub struct GitOutput {
    /// Whether the command succeeded (exit code 0).
    pub success: bool,
    /// Standard output from git.
    pub stdout: String,
    /// Standard error from git.
    pub stderr: String,
}

/// Timeout defaults (seconds).
const TIMEOUT_SHORT: u64 = 30; // status, diff
const TIMEOUT_LONG: u64 = 120; // clone, push, commit

/// Sanitize a user-provided path string.
/// Rejects paths that start with `-` (option injection), contain null bytes,
/// or contain `..` (traversal).
pub fn sanitize_path(p: &str) -> Result<String, String> {
    if p.is_empty() {
        return Err("Path must not be empty".into());
    }
    if p.starts_with('-') {
        return Err(format!("Path must not start with '-': '{}'", p));
    }
    if p.contains('\0') {
        return Err("Path must not contain null bytes".into());
    }
    if p.contains("..") {
        return Err(format!("Path traversal blocked: '{}'", p));
    }
    Ok(p.to_string())
}

/// Validate a clone URL: must be https-only, hostname must pass SSRF check.
/// Returns the extracted hostname on success.
pub fn validate_clone_url(url: &str) -> Result<String, String> {
    // Must start with https://
    if !url.starts_with("https://") {
        return Err(format!(
            "git.clone: only https URLs are allowed (got '{}')",
            url
        ));
    }

    // Extract hostname
    let authority = url
        .split("//")
        .nth(1)
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("");

    // Strip userinfo (anything before @) to prevent authority confusion
    let host_port = if let Some(at_pos) = authority.rfind('@') {
        &authority[at_pos + 1..]
    } else {
        authority
    };

    // Handle IPv6 brackets
    let host = if host_port.starts_with('[') {
        host_port
            .split(']')
            .next()
            .map(|s| &s[1..])
            .unwrap_or(host_port)
    } else {
        host_port.split(':').next().unwrap_or(host_port)
    };

    if host.is_empty() {
        return Err("git.clone: URL has no hostname".into());
    }

    // SSRF check — delegate to the runtime's is_blocked_host
    // We inline a minimal check here; the full check happens in host_functions
    // when we have access to is_blocked_host().
    // For the module level, reject obvious internal hosts:
    let host_lower = host.to_lowercase();
    if host_lower == "localhost"
        || host_lower == "0.0.0.0"
        || host_lower == "::1"
        || host_lower.ends_with(".localhost")
        || host_lower.ends_with(".local")
        || host_lower.starts_with("10.")
        || host_lower.starts_with("192.168.")
        || host_lower.starts_with("172.16.")
        || host_lower.starts_with("127.")
    {
        return Err(format!(
            "git.clone: SSRF blocked — cannot clone from internal host '{}'",
            host
        ));
    }

    Ok(host.to_string())
}

/// Run a git command with explicit args, cleared environment, and timeout.
///
/// - `args`: The git subcommand and arguments (e.g., `["status", "--porcelain"]`).
/// - `repo_path`: Optional working directory for the git command.
/// - `timeout_secs`: Maximum seconds to wait before killing the process.
///
/// Returns `GitOutput` with success status, stdout, and stderr.
pub fn run_git_command(
    args: &[&str],
    repo_path: Option<&Path>,
    timeout_secs: u64,
) -> Result<GitOutput, String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());

    let mut cmd = std::process::Command::new("git");
    cmd.args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/local/bin")
        .env("HOME", &home)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if let Some(rp) = repo_path {
        cmd.current_dir(rp);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn git: {}", e))?;

    let timeout_dur = std::time::Duration::from_secs(timeout_secs);
    let start = std::time::Instant::now();

    // Read stdout/stderr in background threads to avoid pipe deadlock
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let stdout_handle = std::thread::spawn(move || {
        stdout_pipe
            .map(|mut s| {
                let mut buf = Vec::new();
                use std::io::Read;
                let _ = s.read_to_end(&mut buf);
                String::from_utf8_lossy(&buf).to_string()
            })
            .unwrap_or_default()
    });
    let stderr_handle = std::thread::spawn(move || {
        stderr_pipe
            .map(|mut s| {
                let mut buf = Vec::new();
                use std::io::Read;
                let _ = s.read_to_end(&mut buf);
                String::from_utf8_lossy(&buf).to_string()
            })
            .unwrap_or_default()
    });

    // Poll with sleep until timeout
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {
                if start.elapsed() > timeout_dur {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(format!(
                        "git command timed out after {}s: git {}",
                        timeout_secs,
                        args.join(" ")
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                break Err(format!("git wait failed: {}", e));
            }
        }
    };

    let stdout = stdout_handle.join().unwrap_or_default();
    let stderr = stderr_handle.join().unwrap_or_default();

    match status {
        Ok(exit_status) => {
            // Cap output at 1MB
            let stdout = truncate_str(&stdout, 1_048_576);
            let stderr = truncate_str(&stderr, 1_048_576);
            Ok(GitOutput {
                success: exit_status.success(),
                stdout,
                stderr,
            })
        }
        Err(e) => Err(e),
    }
}

/// Truncate a string at a UTF-8-safe boundary.
fn truncate_str(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        let truncate_at = s
            .char_indices()
            .take_while(|&(i, _)| i <= max_bytes)
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        format!("{}...[truncated]", &s[..truncate_at])
    }
}

// ---------------------------------------------------------------------------
// Public git operations
// ---------------------------------------------------------------------------

/// `git status --porcelain`
pub fn git_status(repo_path: Option<&str>) -> Result<GitOutput, String> {
    let rp = match repo_path {
        Some(p) => {
            sanitize_path(p)?;
            Some(std::path::PathBuf::from(p))
        }
        None => None,
    };
    run_git_command(&["status", "--porcelain"], rp.as_deref(), TIMEOUT_SHORT)
}

/// `git diff` (or `git diff --cached` if staged=true)
pub fn git_diff(repo_path: Option<&str>, staged: bool) -> Result<GitOutput, String> {
    let rp = match repo_path {
        Some(p) => {
            sanitize_path(p)?;
            Some(std::path::PathBuf::from(p))
        }
        None => None,
    };
    let args: Vec<&str> = if staged {
        vec!["diff", "--cached"]
    } else {
        vec!["diff"]
    };
    run_git_command(&args, rp.as_deref(), TIMEOUT_SHORT)
}

/// `git add <files> && git commit -m <message>`
/// If files is None, commits all staged changes (no add).
pub fn git_commit(
    repo_path: Option<&str>,
    message: &str,
    files: Option<Vec<&str>>,
) -> Result<GitOutput, String> {
    if message.is_empty() {
        return Err("git.commit: message must not be empty".into());
    }
    if message.len() > 10_000 {
        return Err("git.commit: message too long (max 10000 chars)".into());
    }

    let rp = match repo_path {
        Some(p) => {
            sanitize_path(p)?;
            Some(std::path::PathBuf::from(p))
        }
        None => None,
    };

    // Stage files if provided
    if let Some(ref file_list) = files {
        if file_list.is_empty() {
            return Err("git.commit: files list must not be empty if provided".into());
        }
        for f in file_list {
            sanitize_path(f)?;
        }
        let mut add_args: Vec<&str> = vec!["add", "--"];
        for f in file_list {
            add_args.push(f);
        }
        let add_result = run_git_command(&add_args, rp.as_deref(), TIMEOUT_SHORT)?;
        if !add_result.success {
            return Ok(GitOutput {
                success: false,
                stdout: add_result.stdout,
                stderr: format!("git add failed: {}", add_result.stderr),
            });
        }
    }

    // Commit
    run_git_command(&["commit", "-m", message], rp.as_deref(), TIMEOUT_LONG)
}

/// `git push <remote> <branch>` — requires explicit remote and branch.
pub fn git_push(repo_path: Option<&str>, remote: &str, branch: &str) -> Result<GitOutput, String> {
    if remote.is_empty() {
        return Err("git.push: remote must not be empty".into());
    }
    if branch.is_empty() {
        return Err("git.push: branch must not be empty".into());
    }
    sanitize_path(remote)?;
    sanitize_path(branch)?;

    let rp = match repo_path {
        Some(p) => {
            sanitize_path(p)?;
            Some(std::path::PathBuf::from(p))
        }
        None => None,
    };

    run_git_command(&["push", remote, branch], rp.as_deref(), TIMEOUT_LONG)
}

/// `git clone <url> [target_path]` — https-only, SSRF-checked.
pub fn git_clone(url: &str, target_path: Option<&str>) -> Result<GitOutput, String> {
    validate_clone_url(url)?;

    let mut args: Vec<&str> = vec!["clone", "--depth", "1", url];
    if let Some(tp) = target_path {
        sanitize_path(tp)?;
        args.push(tp);
    }

    run_git_command(&args, None, TIMEOUT_LONG)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- sanitize_path tests --

    #[test]
    fn test_sanitize_normal_path() {
        assert!(sanitize_path("src/main.rs").is_ok());
        assert!(sanitize_path("my-repo").is_ok());
        assert!(sanitize_path("README.md").is_ok());
    }

    #[test]
    fn test_sanitize_rejects_dash_prefix() {
        assert!(sanitize_path("-evil").is_err());
        assert!(sanitize_path("--upload-pack").is_err());
        assert!(sanitize_path("-c").is_err());
    }

    #[test]
    fn test_sanitize_rejects_null_bytes() {
        assert!(sanitize_path("foo\0bar").is_err());
    }

    #[test]
    fn test_sanitize_rejects_traversal() {
        assert!(sanitize_path("../etc/passwd").is_err());
        assert!(sanitize_path("foo/../bar").is_err());
    }

    #[test]
    fn test_sanitize_rejects_empty() {
        assert!(sanitize_path("").is_err());
    }

    // -- validate_clone_url tests --

    #[test]
    fn test_clone_url_https_valid() {
        assert!(validate_clone_url("https://github.com/user/repo.git").is_ok());
        assert!(validate_clone_url("https://gitlab.com/user/repo").is_ok());
    }

    #[test]
    fn test_clone_url_rejects_http() {
        assert!(validate_clone_url("http://github.com/user/repo.git").is_err());
    }

    #[test]
    fn test_clone_url_rejects_ssh() {
        assert!(validate_clone_url("git@github.com:user/repo.git").is_err());
    }

    #[test]
    fn test_clone_url_rejects_ftp() {
        assert!(validate_clone_url("ftp://example.com/repo.git").is_err());
    }

    #[test]
    fn test_clone_url_rejects_localhost() {
        assert!(validate_clone_url("https://localhost/repo.git").is_err());
        assert!(validate_clone_url("https://127.0.0.1/repo.git").is_err());
        assert!(validate_clone_url("https://0.0.0.0/repo.git").is_err());
    }

    #[test]
    fn test_clone_url_rejects_private_ips() {
        assert!(validate_clone_url("https://10.0.0.1/repo.git").is_err());
        assert!(validate_clone_url("https://192.168.1.1/repo.git").is_err());
        assert!(validate_clone_url("https://172.16.0.1/repo.git").is_err());
    }

    #[test]
    fn test_clone_url_rejects_local_domain() {
        assert!(validate_clone_url("https://evil.localhost/repo.git").is_err());
        assert!(validate_clone_url("https://myhost.local/repo.git").is_err());
    }

    #[test]
    fn test_clone_url_strips_userinfo() {
        // Should still validate the real host after stripping userinfo
        let result = validate_clone_url("https://evil@localhost/repo.git");
        assert!(result.is_err());
    }

    // -- git_push validation tests --

    #[test]
    fn test_push_requires_remote() {
        let result = git_push(None, "", "main");
        assert!(result.is_err());
    }

    #[test]
    fn test_push_requires_branch() {
        let result = git_push(None, "origin", "");
        assert!(result.is_err());
    }

    #[test]
    fn test_push_rejects_dash_remote() {
        let result = git_push(None, "--receive-pack=evil", "main");
        assert!(result.is_err());
    }

    // -- git_commit validation tests --

    #[test]
    fn test_commit_requires_message() {
        let result = git_commit(None, "", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_commit_rejects_long_message() {
        let long_msg = "a".repeat(10_001);
        let result = git_commit(None, &long_msg, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_commit_rejects_dash_files() {
        let result = git_commit(None, "test", Some(vec!["--exec=evil"]));
        assert!(result.is_err());
    }
}
