//! MCP server lifecycle manager.
//!
//! Tracks running MCP servers, manages agent-level allow-lists, and provides
//! configuration lookups. Actual process spawning is deferred to the transport
//! layer; this module handles the bookkeeping.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use serde_json::Value;

use super::config::{GlobalMcpConfig, McpAgentConfig, McpServerConfig};
use super::spawner::McpProcess;

/// Status of an MCP server process.
#[derive(Debug, Clone)]
pub enum ServerStatus {
    Stopped,
    Starting,
    Ready,
    Error(String),
}

impl fmt::Display for ServerStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServerStatus::Stopped => write!(f, "stopped"),
            ServerStatus::Starting => write!(f, "starting"),
            ServerStatus::Ready => write!(f, "ready"),
            ServerStatus::Error(msg) => write!(f, "error: {}", msg),
        }
    }
}

/// Bookkeeping for a running (or recently-running) MCP server.
#[derive(Debug)]
pub struct RunningServer {
    pub server_id: String,
    pub status: ServerStatus,
    pub tool_count: usize,
    pub call_count: u64,
    pub error_count: u32,
}

/// Manages MCP server lifecycle and configuration.
pub struct McpManager {
    global_config: GlobalMcpConfig,
    cache_dir: PathBuf,
    allowed: McpAgentConfig,
    running: HashMap<String, RunningServer>,
    processes: HashMap<String, McpProcess>,
}

impl McpManager {
    /// Create a new manager with the given global config and cache directory.
    /// Starts with empty agent allow-list and no running servers.
    pub fn new(global_config: GlobalMcpConfig, cache_dir: PathBuf) -> Self {
        Self {
            global_config,
            cache_dir,
            allowed: McpAgentConfig::default(),
            running: HashMap::new(),
            processes: HashMap::new(),
        }
    }

    /// Configure which servers and tools an agent is allowed to use.
    ///
    /// Servers listed in the agent config that are not present in the global
    /// config are silently filtered out.
    pub fn configure_for_agent(&mut self, agent_mcp: McpAgentConfig) {
        let known_servers: std::collections::HashSet<&String> =
            self.global_config.servers.keys().collect();

        let filtered_servers: Vec<String> = agent_mcp
            .servers
            .into_iter()
            .filter(|s| known_servers.contains(s))
            .collect();

        let filtered_tools: HashMap<String, Vec<String>> = agent_mcp
            .allowed_tools
            .into_iter()
            .filter(|(k, _)| known_servers.contains(k))
            .collect();

        self.allowed = McpAgentConfig {
            servers: filtered_servers,
            allowed_tools: filtered_tools,
        };
    }

    /// Shut down all running servers, clearing tracked state.
    pub fn shutdown_all(&mut self) {
        self.processes.clear();
        self.running.clear();
    }

    /// Return the list of server IDs the agent is allowed to use.
    pub fn allowed_servers(&self) -> Vec<String> {
        self.allowed.servers.clone()
    }

    /// Return the allowed tools for a specific server, if configured.
    pub fn allowed_tools_for(&self, server_id: &str) -> Option<&Vec<String>> {
        self.allowed.allowed_tools.get(server_id)
    }

    /// Return references to all currently running servers.
    pub fn running_servers(&self) -> Vec<&RunningServer> {
        self.running.values().collect()
    }

    /// Look up the global config for a server by ID.
    pub fn server_config(&self, server_id: &str) -> Option<&McpServerConfig> {
        self.global_config.servers.get(server_id)
    }

    /// Return a reference to the cache directory.
    pub fn cache_dir(&self) -> &PathBuf {
        &self.cache_dir
    }

    /// Start an MCP server, performing initialize + tools/list handshake.
    /// Returns the discovered tools.
    pub async fn start_server(
        &mut self,
        server_id: &str,
        resolved_env: HashMap<String, String>,
    ) -> crate::core::error::Result<Vec<super::transport::McpToolDef>> {
        let config = self.global_config.servers.get(server_id).ok_or_else(|| {
            crate::core::error::NyayaError::Config(format!("Unknown MCP server: {}", server_id))
        })?;
        let config_clone = config.clone();

        // Update status to Starting
        self.running.insert(
            server_id.to_string(),
            RunningServer {
                server_id: server_id.to_string(),
                status: ServerStatus::Starting,
                tool_count: 0,
                call_count: 0,
                error_count: 0,
            },
        );

        let mut process = McpProcess::spawn(&config_clone, resolved_env).await?;
        process.initialize().await?;
        let tools = process.list_tools().await?;

        // Update status to Ready
        if let Some(rs) = self.running.get_mut(server_id) {
            rs.status = ServerStatus::Ready;
            rs.tool_count = tools.len();
        }

        self.processes.insert(server_id.to_string(), process);
        Ok(tools)
    }

    /// Call a tool on a running MCP server.
    pub async fn call_tool(
        &mut self,
        server_id: &str,
        tool_name: &str,
        arguments: Value,
    ) -> crate::core::error::Result<String> {
        // Check allow-list
        if let Some(allowed) = self.allowed.allowed_tools.get(server_id) {
            if !allowed.contains(&tool_name.to_string()) {
                return Err(crate::core::error::NyayaError::PermissionDenied(format!(
                    "Tool '{}' not in allow-list for server '{}'",
                    tool_name, server_id
                )));
            }
        }

        let process = self.processes.get_mut(server_id).ok_or_else(|| {
            crate::core::error::NyayaError::Config(format!(
                "MCP server '{}' is not running",
                server_id
            ))
        })?;

        let result = process.call_tool(tool_name, arguments).await;

        // Update counters
        if let Some(rs) = self.running.get_mut(server_id) {
            rs.call_count += 1;
            if result.is_err() {
                rs.error_count += 1;
            }
        }

        result
    }

    /// Shut down a specific MCP server.
    pub fn shutdown_server(&mut self, server_id: &str) {
        self.processes.remove(server_id);
        if let Some(rs) = self.running.get_mut(server_id) {
            rs.status = ServerStatus::Stopped;
        }
    }
}

/// Resolve environment variables for a server, filtering out secret references.
///
/// Entries whose values start with `$SECRET:` are excluded; they must be
/// resolved through the credential store at connect time.
pub fn resolve_server_env_without_secrets(
    env: &HashMap<String, String>,
) -> HashMap<String, String> {
    env.iter()
        .filter(|(_, v)| !v.starts_with("$SECRET:"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_global_config() -> GlobalMcpConfig {
        let mut servers = HashMap::new();
        servers.insert(
            "filesystem".to_string(),
            McpServerConfig {
                command: "npx".to_string(),
                args: vec![
                    "-y".to_string(),
                    "@anthropic/mcp-server-filesystem".to_string(),
                ],
                env: HashMap::new(),
                trust_level: super::super::config::McpTrustLevel::Verified,
                idle_timeout_secs: 300,
                max_concurrent_calls: 5,
                call_timeout_secs: 30,
                max_output_bytes: 1_048_576,
            },
        );
        servers.insert(
            "github".to_string(),
            McpServerConfig {
                command: "npx".to_string(),
                args: vec!["-y".to_string(), "@anthropic/mcp-server-github".to_string()],
                env: {
                    let mut e = HashMap::new();
                    e.insert(
                        "GITHUB_TOKEN".to_string(),
                        "$SECRET:github_token".to_string(),
                    );
                    e
                },
                trust_level: super::super::config::McpTrustLevel::Community,
                idle_timeout_secs: 300,
                max_concurrent_calls: 3,
                call_timeout_secs: 30,
                max_output_bytes: 1_048_576,
            },
        );
        GlobalMcpConfig { servers }
    }

    #[test]
    fn test_new_manager_is_empty() {
        let mgr = McpManager::new(GlobalMcpConfig::default(), PathBuf::from("/tmp/cache"));
        assert!(mgr.allowed_servers().is_empty());
        assert!(mgr.running_servers().is_empty());
    }

    #[test]
    fn test_configure_for_agent_sets_allowed() {
        let mut mgr = McpManager::new(sample_global_config(), PathBuf::from("/tmp/cache"));
        let agent_cfg = McpAgentConfig {
            servers: vec!["filesystem".to_string(), "github".to_string()],
            allowed_tools: {
                let mut m = HashMap::new();
                m.insert("filesystem".to_string(), vec!["read_file".to_string()]);
                m
            },
        };
        mgr.configure_for_agent(agent_cfg);

        let allowed = mgr.allowed_servers();
        assert_eq!(allowed.len(), 2);
        assert!(allowed.contains(&"filesystem".to_string()));
        assert!(allowed.contains(&"github".to_string()));

        let tools = mgr.allowed_tools_for("filesystem").unwrap();
        assert_eq!(tools, &vec!["read_file".to_string()]);

        assert!(mgr.allowed_tools_for("github").is_none());
    }

    #[test]
    fn test_configure_for_agent_ignores_unknown_servers() {
        let mut mgr = McpManager::new(sample_global_config(), PathBuf::from("/tmp/cache"));
        let agent_cfg = McpAgentConfig {
            servers: vec!["filesystem".to_string(), "unknown_server".to_string()],
            allowed_tools: {
                let mut m = HashMap::new();
                m.insert("unknown_server".to_string(), vec!["foo".to_string()]);
                m
            },
        };
        mgr.configure_for_agent(agent_cfg);

        let allowed = mgr.allowed_servers();
        assert_eq!(allowed.len(), 1);
        assert_eq!(allowed[0], "filesystem");

        // unknown_server's tools should also be filtered
        assert!(mgr.allowed_tools_for("unknown_server").is_none());
    }

    #[test]
    fn test_shutdown_clears_running() {
        let mut mgr = McpManager::new(sample_global_config(), PathBuf::from("/tmp/cache"));
        mgr.running.insert(
            "filesystem".to_string(),
            RunningServer {
                server_id: "filesystem".to_string(),
                status: ServerStatus::Ready,
                tool_count: 5,
                call_count: 10,
                error_count: 0,
            },
        );
        assert_eq!(mgr.running_servers().len(), 1);

        mgr.shutdown_all();
        assert!(mgr.running_servers().is_empty());
    }

    #[test]
    fn test_resolve_env_plain_values() {
        let mut env = HashMap::new();
        env.insert("PATH".to_string(), "/usr/bin".to_string());
        env.insert("HOME".to_string(), "/home/user".to_string());

        let resolved = resolve_server_env_without_secrets(&env);
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved["PATH"], "/usr/bin");
        assert_eq!(resolved["HOME"], "/home/user");
    }

    #[test]
    fn test_resolve_env_skips_secrets() {
        let mut env = HashMap::new();
        env.insert("API_URL".to_string(), "https://api.example.com".to_string());
        env.insert("API_KEY".to_string(), "$SECRET:my_api_key".to_string());
        env.insert("ANOTHER_SECRET".to_string(), "$SECRET:another".to_string());

        let resolved = resolve_server_env_without_secrets(&env);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved["API_URL"], "https://api.example.com");
        assert!(!resolved.contains_key("API_KEY"));
        assert!(!resolved.contains_key("ANOTHER_SECRET"));
    }

    #[test]
    fn test_server_status_display() {
        assert_eq!(format!("{}", ServerStatus::Stopped), "stopped");
        assert_eq!(format!("{}", ServerStatus::Starting), "starting");
        assert_eq!(format!("{}", ServerStatus::Ready), "ready");
        assert_eq!(
            format!("{}", ServerStatus::Error("connection refused".to_string())),
            "error: connection refused"
        );
    }

    #[test]
    fn test_server_config_lookup() {
        let mgr = McpManager::new(sample_global_config(), PathBuf::from("/tmp/cache"));
        let cfg = mgr.server_config("filesystem").unwrap();
        assert_eq!(cfg.command, "npx");

        assert!(mgr.server_config("nonexistent").is_none());
    }

    #[test]
    fn test_cache_dir() {
        let mgr = McpManager::new(GlobalMcpConfig::default(), PathBuf::from("/tmp/my_cache"));
        assert_eq!(mgr.cache_dir(), &PathBuf::from("/tmp/my_cache"));
    }

    #[test]
    fn test_start_unknown_server_config_check() {
        let mgr = McpManager::new(GlobalMcpConfig::default(), PathBuf::from("/tmp/cache"));
        // Verify that server_config returns None for unknown server
        assert!(mgr.server_config("nonexistent").is_none());
    }

    #[test]
    fn test_shutdown_server_specific() {
        let mut mgr = McpManager::new(sample_global_config(), PathBuf::from("/tmp/cache"));
        mgr.running.insert(
            "filesystem".to_string(),
            RunningServer {
                server_id: "filesystem".to_string(),
                status: ServerStatus::Ready,
                tool_count: 5,
                call_count: 10,
                error_count: 0,
            },
        );
        mgr.running.insert(
            "github".to_string(),
            RunningServer {
                server_id: "github".to_string(),
                status: ServerStatus::Ready,
                tool_count: 3,
                call_count: 5,
                error_count: 0,
            },
        );

        mgr.shutdown_server("filesystem");
        assert_eq!(mgr.running_servers().len(), 2); // still in running map but Stopped
        let fs_server = mgr.running.get("filesystem").unwrap();
        assert!(matches!(fs_server.status, ServerStatus::Stopped));
        // github should still be Ready
        let gh_server = mgr.running.get("github").unwrap();
        assert!(matches!(gh_server.status, ServerStatus::Ready));
    }

    #[test]
    fn test_shutdown_all_clears_processes() {
        let mut mgr = McpManager::new(sample_global_config(), PathBuf::from("/tmp/cache"));
        mgr.running.insert(
            "filesystem".to_string(),
            RunningServer {
                server_id: "filesystem".to_string(),
                status: ServerStatus::Ready,
                tool_count: 5,
                call_count: 0,
                error_count: 0,
            },
        );
        mgr.shutdown_all();
        assert!(mgr.running_servers().is_empty());
        assert!(mgr.processes.is_empty());
    }
}
