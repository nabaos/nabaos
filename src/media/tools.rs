//! External tool detection — ffmpeg, pandoc, docker, ComfyUI.

use std::path::PathBuf;
use std::process::Command;

/// Detected external tools and their paths.
#[derive(Debug, Clone, Default)]
pub struct ExternalTools {
    pub ffmpeg: Option<PathBuf>,
    pub pandoc: Option<PathBuf>,
    pub docker: Option<PathBuf>,
}

impl ExternalTools {
    /// Detect all external tools on the system.
    pub fn detect() -> Self {
        Self {
            ffmpeg: detect_tool("ffmpeg"),
            pandoc: detect_tool("pandoc"),
            docker: detect_tool("docker"),
        }
    }

    /// Print status of all tools to stdout (for setup wizard).
    pub fn print_status(&self) {
        println!("=== Recommended Tools ===\n");
        println!("Checking system tools...");
        print_tool_status("ffmpeg", &self.ffmpeg, "video assembly, frame extraction");
        print_tool_status("pandoc", &self.pandoc, "slide export to PPTX/ODP/PDF");
        print_tool_status("docker", &self.docker, "sandbox execution");

        let missing: Vec<&str> = [
            self.ffmpeg.is_none().then_some("ffmpeg"),
            self.pandoc.is_none().then_some("pandoc"),
        ]
        .into_iter()
        .flatten()
        .collect();

        if !missing.is_empty() {
            println!("\nMissing tools — install with:");
            for tool in missing {
                println!("  {}: {}", tool, install_hint(tool));
            }
        }
        println!();
    }
}

/// Try to find a tool on PATH using `which` (Unix) or `where` (Windows).
fn detect_tool(name: &str) -> Option<PathBuf> {
    let cmd = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    let output = Command::new(cmd).arg(name).output().ok()?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if path.is_empty() {
            None
        } else {
            Some(PathBuf::from(path))
        }
    } else {
        None
    }
}

fn print_tool_status(name: &str, path: &Option<PathBuf>, purpose: &str) {
    match path {
        Some(_p) => println!("  [ok] {} ({})", name, purpose),
        None => println!("  [  ] {} ({}) — not found", name, purpose),
    }
}

/// Return a platform-appropriate install command for a tool.
pub fn install_hint(tool: &str) -> String {
    let os = std::env::consts::OS;
    match (os, tool) {
        ("linux", "ffmpeg") => "sudo apt install ffmpeg".to_string(),
        ("linux", "pandoc") => "sudo apt install pandoc".to_string(),
        ("macos", "ffmpeg") => "brew install ffmpeg".to_string(),
        ("macos", "pandoc") => "brew install pandoc".to_string(),
        ("windows", "ffmpeg") => "choco install ffmpeg".to_string(),
        ("windows", "pandoc") => "choco install pandoc".to_string(),
        _ => format!("Install {} from the official website", tool),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_install_hint_linux() {
        // Only meaningful on Linux, but the function itself is pure
        let hint = install_hint("pandoc");
        assert!(!hint.is_empty());
    }

    #[test]
    fn test_install_hint_unknown_tool() {
        let hint = install_hint("unknown_tool_xyz");
        assert!(hint.contains("official website"));
    }

    #[test]
    fn test_external_tools_default_all_none() {
        let tools = ExternalTools::default();
        assert!(tools.ffmpeg.is_none());
        assert!(tools.pandoc.is_none());
        assert!(tools.docker.is_none());
    }
}
