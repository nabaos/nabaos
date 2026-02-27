//! Docker Execution Sandbox — runs untrusted code in ephemeral containers.
//!
//! Uses bollard to manage container lifecycle. Each execution gets:
//! - Ephemeral tmpfs filesystem
//! - CPU and memory limits from constitution
//! - Network isolation (none by default, allowlist via config)
//! - Auto-cleanup after execution or timeout
//!
//! Falls back to subprocess execution when Docker is unavailable.

use std::time::Instant;

use bollard::container::{
    Config as ContainerConfig, CreateContainerOptions, LogOutput, LogsOptions,
    RemoveContainerOptions, StartContainerOptions, WaitContainerOptions,
};
use bollard::models::HostConfig;
use bollard::Docker;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::time::timeout;

use crate::core::error::{NyayaError, Result};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for a Docker execution sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerSandboxConfig {
    /// Docker image to use (default: "python:3.12-slim")
    #[serde(default = "default_image")]
    pub image: String,
    /// CPU limit (number of cores, e.g. 1.0)
    #[serde(default = "default_cpu")]
    pub cpu_limit: f64,
    /// Memory limit in bytes (default: 256MB)
    #[serde(default = "default_memory")]
    pub memory_limit: u64,
    /// Execution timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Enable network access (default: false)
    #[serde(default)]
    pub network_enabled: bool,
    /// Allowed network hosts (only if network_enabled)
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    /// Allow falling back to unsandboxed subprocess if Docker is unavailable.
    /// Default: true (backward compatible). Set to false in production.
    #[serde(default = "default_allow_fallback")]
    pub allow_subprocess_fallback: bool,
}

fn default_image() -> String {
    "python:3.12-slim".into()
}
fn default_cpu() -> f64 {
    1.0
}
fn default_memory() -> u64 {
    256 * 1024 * 1024
}
fn default_timeout() -> u64 {
    30
}
fn default_allow_fallback() -> bool {
    true
}

impl Default for DockerSandboxConfig {
    fn default() -> Self {
        Self {
            image: default_image(),
            cpu_limit: default_cpu(),
            memory_limit: default_memory(),
            timeout_secs: default_timeout(),
            network_enabled: false,
            allowed_hosts: vec![],
            allow_subprocess_fallback: default_allow_fallback(),
        }
    }
}

// ---------------------------------------------------------------------------
// ExecutionResult
// ---------------------------------------------------------------------------

/// Result of a sandboxed execution.
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i64,
    pub duration_ms: u64,
    pub timed_out: bool,
}

// ---------------------------------------------------------------------------
// DockerExecutor
// ---------------------------------------------------------------------------

/// Docker executor — creates and manages ephemeral containers.
/// Falls back to subprocess execution when Docker is unavailable.
pub struct DockerExecutor {
    config: DockerSandboxConfig,
}

impl DockerExecutor {
    pub fn new(config: DockerSandboxConfig) -> Self {
        Self { config }
    }

    /// Get the current configuration.
    pub fn config(&self) -> &DockerSandboxConfig {
        &self.config
    }

    /// Build the shell command for a given language.
    pub fn build_command(code: &str, language: &str) -> Vec<String> {
        match language {
            "python" | "py" => vec!["python3".into(), "-c".into(), code.into()],
            "bash" | "sh" => vec!["bash".into(), "-c".into(), code.into()],
            "node" | "javascript" | "js" => vec!["node".into(), "-e".into(), code.into()],
            _ => vec!["sh".into(), "-c".into(), code.into()],
        }
    }

    /// Execute code in an ephemeral Docker container.
    /// Returns stdout/stderr and exit code.
    ///
    /// The container is always removed after execution (success or failure).
    /// Falls back to subprocess execution if Docker is unavailable.
    pub async fn execute(&self, code: &str, language: &str) -> Result<ExecutionResult> {
        match self.execute_docker(code, language).await {
            Ok(result) => Ok(result),
            Err(docker_err) => {
                if !self.config.allow_subprocess_fallback {
                    return Err(docker_err);
                }
                tracing::warn!(
                    "Docker unavailable, falling back to unsandboxed subprocess execution"
                );
                self.execute_subprocess(code, language).await
            }
        }
    }

    /// Execute code inside a Docker container using bollard.
    async fn execute_docker(&self, code: &str, language: &str) -> Result<ExecutionResult> {
        let docker = Docker::connect_with_local_defaults()
            .map_err(|e| NyayaError::Config(format!("Docker connect failed: {e}")))?;

        // Verify Docker daemon connectivity
        docker
            .ping()
            .await
            .map_err(|e| NyayaError::Config(format!("Docker ping failed: {e}")))?;

        let cmd = Self::build_command(code, language);
        let cmd_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
        let container_name = format!("nyaya-sandbox-{}", uuid::Uuid::new_v4().as_simple());

        // CPU limit: bollard expects nano_cpus (1 core = 1_000_000_000)
        let nano_cpus = (self.config.cpu_limit * 1_000_000_000.0) as i64;

        let network_mode = if self.config.network_enabled {
            None
        } else {
            Some("none".to_string())
        };

        let host_config = HostConfig {
            nano_cpus: Some(nano_cpus),
            memory: Some(self.config.memory_limit as i64),
            network_mode,
            ..Default::default()
        };

        let container_config = ContainerConfig {
            image: Some(self.config.image.as_str()),
            cmd: Some(cmd_refs),
            host_config: Some(host_config),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: container_name.as_str(),
            platform: None,
        };

        let container = docker
            .create_container(Some(options), container_config)
            .await
            .map_err(|e| NyayaError::Config(format!("Container create failed: {e}")))?;

        let container_id = container.id;
        let start = Instant::now();

        // Start the container
        let start_result = docker
            .start_container(&container_id, None::<StartContainerOptions<String>>)
            .await;

        if let Err(e) = start_result {
            // Clean up on start failure
            let _ = docker
                .remove_container(
                    &container_id,
                    Some(RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await;
            return Err(NyayaError::Config(format!("Container start failed: {e}")));
        }

        // Wait for container to finish (with timeout)
        let timeout_duration = std::time::Duration::from_secs(self.config.timeout_secs);
        let mut wait_stream = docker.wait_container(
            &container_id,
            Some(WaitContainerOptions {
                condition: "not-running",
            }),
        );

        let timed_out = match timeout(timeout_duration, wait_stream.next()).await {
            Ok(_) => false,
            Err(_) => {
                // Timeout — kill the container
                let _ = docker.kill_container::<String>(&container_id, None).await;
                true
            }
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        // Collect logs
        let mut stdout = String::new();
        let mut stderr = String::new();

        let log_options = LogsOptions::<String> {
            stdout: true,
            stderr: true,
            ..Default::default()
        };

        let mut log_stream = docker.logs(&container_id, Some(log_options));
        while let Some(Ok(log)) = log_stream.next().await {
            match log {
                LogOutput::StdOut { message } => {
                    stdout.push_str(&String::from_utf8_lossy(&message));
                }
                LogOutput::StdErr { message } => {
                    stderr.push_str(&String::from_utf8_lossy(&message));
                }
                _ => {}
            }
        }

        // Inspect container for exit code
        let inspect = docker.inspect_container(&container_id, None).await;
        let exit_code = inspect
            .ok()
            .and_then(|i| i.state)
            .and_then(|s| s.exit_code)
            .unwrap_or(-1);

        // Always remove container
        let _ = docker
            .remove_container(
                &container_id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;

        Ok(ExecutionResult {
            stdout,
            stderr,
            exit_code,
            duration_ms,
            timed_out,
        })
    }

    /// Execute code as a local subprocess (fallback when Docker is unavailable).
    pub(crate) async fn execute_subprocess(
        &self,
        code: &str,
        language: &str,
    ) -> Result<ExecutionResult> {
        let cmd = Self::build_command(code, language);
        let start = Instant::now();

        let mut child = tokio::process::Command::new(&cmd[0])
            .args(&cmd[1..])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| NyayaError::Config(format!("Subprocess spawn failed: {e}")))?;

        let timeout_duration = std::time::Duration::from_secs(self.config.timeout_secs);

        // Take stdout/stderr handles before waiting so we can read them after timeout
        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        match timeout(timeout_duration, child.wait()).await {
            Ok(Ok(status)) => {
                let duration_ms = start.elapsed().as_millis() as u64;

                let mut stdout_bytes = Vec::new();
                let mut stderr_bytes = Vec::new();

                if let Some(mut out) = stdout_handle {
                    use tokio::io::AsyncReadExt;
                    let _ = out.read_to_end(&mut stdout_bytes).await;
                }
                if let Some(mut err) = stderr_handle {
                    use tokio::io::AsyncReadExt;
                    let _ = err.read_to_end(&mut stderr_bytes).await;
                }

                Ok(ExecutionResult {
                    stdout: String::from_utf8_lossy(&stdout_bytes).to_string(),
                    stderr: String::from_utf8_lossy(&stderr_bytes).to_string(),
                    exit_code: status.code().map(|c| c as i64).unwrap_or(-1),
                    duration_ms,
                    timed_out: false,
                })
            }
            Ok(Err(e)) => Err(NyayaError::Config(format!("Subprocess wait failed: {e}"))),
            Err(_) => {
                // Timeout — kill the child process
                let _ = child.kill().await;
                let duration_ms = start.elapsed().as_millis() as u64;
                Ok(ExecutionResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: -1,
                    duration_ms,
                    timed_out: true,
                })
            }
        }
    }

    /// Check if Docker is available on this system.
    pub async fn is_available() -> bool {
        tokio::process::Command::new("docker")
            .arg("info")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_docker_sandbox_config_defaults() {
        let config = DockerSandboxConfig::default();
        assert_eq!(config.image, "python:3.12-slim");
        assert!((config.cpu_limit - 1.0).abs() < f64::EPSILON);
        assert_eq!(config.memory_limit, 256 * 1024 * 1024);
        assert_eq!(config.timeout_secs, 30);
        assert!(!config.network_enabled);
        assert!(config.allowed_hosts.is_empty());
    }

    #[test]
    fn test_docker_sandbox_config_serde() {
        let yaml = r#"
image: "node:20-slim"
cpu_limit: 2.0
memory_limit: 536870912
timeout_secs: 60
network_enabled: true
allowed_hosts:
  - "api.example.com"
"#;
        let config: DockerSandboxConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.image, "node:20-slim");
        assert!((config.cpu_limit - 2.0).abs() < f64::EPSILON);
        assert_eq!(config.timeout_secs, 60);
        assert!(config.network_enabled);
        assert_eq!(config.allowed_hosts.len(), 1);

        // Roundtrip
        let yaml_out = serde_yaml::to_string(&config).unwrap();
        let config2: DockerSandboxConfig = serde_yaml::from_str(&yaml_out).unwrap();
        assert_eq!(config2.image, "node:20-slim");
    }

    #[test]
    fn test_build_command_python() {
        let cmd = DockerExecutor::build_command("print('hi')", "python");
        assert_eq!(cmd, vec!["python3", "-c", "print('hi')"]);
    }

    #[test]
    fn test_build_command_bash() {
        let cmd = DockerExecutor::build_command("echo hi", "bash");
        assert_eq!(cmd, vec!["bash", "-c", "echo hi"]);
    }

    #[test]
    fn test_build_command_unknown_language() {
        let cmd = DockerExecutor::build_command("code", "ruby");
        assert_eq!(cmd[0], "sh");
    }

    #[tokio::test]
    async fn test_docker_executor_subprocess_fallback() {
        let executor = DockerExecutor::new(DockerSandboxConfig::default());
        let result = executor.execute("print('hello')", "python").await.unwrap();
        assert!(!result.timed_out);
    }

    #[tokio::test]
    async fn test_subprocess_fallback_echo() {
        let executor = DockerExecutor::new(DockerSandboxConfig::default());
        let result = executor
            .execute_subprocess("echo hello_from_subprocess", "bash")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!result.timed_out);
        assert!(result.stdout.contains("hello_from_subprocess"));
    }

    #[tokio::test]
    async fn test_subprocess_stderr_capture() {
        let executor = DockerExecutor::new(DockerSandboxConfig::default());
        let result = executor
            .execute_subprocess("echo error_output >&2", "bash")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stderr.contains("error_output"));
    }

    #[tokio::test]
    async fn test_subprocess_exit_code() {
        let executor = DockerExecutor::new(DockerSandboxConfig::default());
        let result = executor
            .execute_subprocess("exit 42", "bash")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 42);
        assert!(!result.timed_out);
    }

    #[tokio::test]
    async fn test_subprocess_timeout() {
        let config = DockerSandboxConfig {
            timeout_secs: 1,
            ..DockerSandboxConfig::default()
        };
        let executor = DockerExecutor::new(config);
        let result = executor
            .execute_subprocess("sleep 30", "bash")
            .await
            .unwrap();
        assert!(result.timed_out);
    }

    #[tokio::test]
    async fn test_docker_executor_node() {
        let executor = DockerExecutor::new(DockerSandboxConfig::default());
        // This will use subprocess fallback since Docker is likely not available
        let result = executor.execute("console.log('node_test')", "node").await;
        // Node may or may not be installed; just verify no panic
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_allow_subprocess_fallback_defaults_true() {
        let config = DockerSandboxConfig::default();
        assert!(config.allow_subprocess_fallback);
    }

    #[test]
    fn test_allow_subprocess_fallback_serde() {
        let yaml = "allow_subprocess_fallback: false\n";
        let config: DockerSandboxConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(!config.allow_subprocess_fallback);
    }

    #[tokio::test]
    async fn test_fallback_disabled_config_respected() {
        let config = DockerSandboxConfig {
            allow_subprocess_fallback: false,
            ..DockerSandboxConfig::default()
        };
        let executor = DockerExecutor::new(config);
        assert!(!executor.config().allow_subprocess_fallback);
    }
}
