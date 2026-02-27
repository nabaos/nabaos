use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::core::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum McpTrustLevel {
    Builtin,
    Verified,
    #[default]
    Community,
    Untrusted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub trust_level: McpTrustLevel,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_secs: u64,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_calls: u32,
    #[serde(default = "default_call_timeout")]
    pub call_timeout_secs: u64,
    #[serde(default = "default_max_output")]
    pub max_output_bytes: usize,
}

fn default_idle_timeout() -> u64 {
    300
}
fn default_max_concurrent() -> u32 {
    5
}
fn default_call_timeout() -> u64 {
    30
}
fn default_max_output() -> usize {
    1_048_576
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalMcpConfig {
    #[serde(default)]
    pub servers: HashMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct McpAgentConfig {
    #[serde(default)]
    pub servers: Vec<String>,
    #[serde(default)]
    pub allowed_tools: HashMap<String, Vec<String>>,
}

/// Load the global MCP configuration from `<mcp_dir>/global.yaml`.
/// Returns an empty default config if the file does not exist.
pub fn load_global_mcp_config(mcp_dir: &Path) -> Result<GlobalMcpConfig> {
    let path = mcp_dir.join("global.yaml");
    if !path.exists() {
        return Ok(GlobalMcpConfig::default());
    }
    let content = std::fs::read_to_string(&path)?;
    let config: GlobalMcpConfig = serde_yaml::from_str(&content)?;
    Ok(config)
}

/// If `value` has the form `$SECRET:<name>`, return the `<name>` portion.
pub fn resolve_secret_ref(value: &str) -> Option<&str> {
    value.strip_prefix("$SECRET:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_parse_global_config_yaml() {
        let yaml = r#"
servers:
  filesystem:
    command: "npx"
    args: ["-y", "@anthropic/mcp-server-filesystem"]
    trust_level: verified
    idle_timeout_secs: 600
  github:
    command: "npx"
    args: ["-y", "@anthropic/mcp-server-github"]
    env:
      GITHUB_TOKEN: "$SECRET:github_token"
    trust_level: community
    max_concurrent_calls: 3
"#;
        let config: GlobalMcpConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.servers.len(), 2);

        let fs = &config.servers["filesystem"];
        assert_eq!(fs.command, "npx");
        assert_eq!(fs.args, vec!["-y", "@anthropic/mcp-server-filesystem"]);
        assert_eq!(fs.trust_level, McpTrustLevel::Verified);
        assert_eq!(fs.idle_timeout_secs, 600);
        // defaults
        assert_eq!(fs.max_concurrent_calls, default_max_concurrent());
        assert_eq!(fs.call_timeout_secs, default_call_timeout());
        assert_eq!(fs.max_output_bytes, default_max_output());

        let gh = &config.servers["github"];
        assert_eq!(gh.trust_level, McpTrustLevel::Community);
        assert_eq!(gh.max_concurrent_calls, 3);
        assert_eq!(gh.env.get("GITHUB_TOKEN").unwrap(), "$SECRET:github_token");
    }

    #[test]
    fn test_parse_agent_mcp_config() {
        let yaml = r#"
servers:
  - filesystem
  - github
allowed_tools:
  filesystem:
    - read_file
    - list_directory
  github:
    - search_repos
"#;
        let config: McpAgentConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.servers, vec!["filesystem", "github"]);
        assert_eq!(config.allowed_tools.len(), 2);
        assert_eq!(
            config.allowed_tools["filesystem"],
            vec!["read_file", "list_directory"]
        );
        assert_eq!(config.allowed_tools["github"], vec!["search_repos"]);
    }

    #[test]
    fn test_default_global_config_is_empty() {
        let config = GlobalMcpConfig::default();
        assert!(config.servers.is_empty());
    }

    #[test]
    fn test_default_agent_config_is_empty() {
        let config = McpAgentConfig::default();
        assert!(config.servers.is_empty());
        assert!(config.allowed_tools.is_empty());
    }

    #[test]
    fn test_default_trust_level_is_community() {
        assert_eq!(McpTrustLevel::default(), McpTrustLevel::Community);
    }

    #[test]
    fn test_load_global_config_from_file() {
        let dir = TempDir::new().unwrap();
        let yaml = r#"
servers:
  myserver:
    command: "my-mcp-server"
    args: ["--port", "8080"]
    trust_level: builtin
"#;
        let mut f = std::fs::File::create(dir.path().join("global.yaml")).unwrap();
        f.write_all(yaml.as_bytes()).unwrap();

        let config = load_global_mcp_config(dir.path()).unwrap();
        assert_eq!(config.servers.len(), 1);
        let srv = &config.servers["myserver"];
        assert_eq!(srv.command, "my-mcp-server");
        assert_eq!(srv.args, vec!["--port", "8080"]);
        assert_eq!(srv.trust_level, McpTrustLevel::Builtin);
    }

    #[test]
    fn test_load_missing_global_config_returns_empty() {
        let dir = TempDir::new().unwrap();
        // No global.yaml file exists
        let config = load_global_mcp_config(dir.path()).unwrap();
        assert!(config.servers.is_empty());
    }

    #[test]
    fn test_secret_ref_detection() {
        assert_eq!(resolve_secret_ref("$SECRET:my_token"), Some("my_token"));
        assert_eq!(
            resolve_secret_ref("$SECRET:github_token"),
            Some("github_token")
        );
        assert_eq!(resolve_secret_ref("plain_value"), None);
        assert_eq!(resolve_secret_ref("$SECRET:"), Some(""));
        assert_eq!(resolve_secret_ref("$secret:lower"), None); // case-sensitive
        assert_eq!(resolve_secret_ref(""), None);
    }

    #[test]
    fn test_server_config_defaults() {
        let yaml = r#"
servers:
  minimal:
    command: "some-server"
"#;
        let config: GlobalMcpConfig = serde_yaml::from_str(yaml).unwrap();
        let srv = &config.servers["minimal"];
        assert_eq!(srv.command, "some-server");
        assert!(srv.args.is_empty());
        assert!(srv.env.is_empty());
        assert_eq!(srv.trust_level, McpTrustLevel::Community);
        assert_eq!(srv.idle_timeout_secs, 300);
        assert_eq!(srv.max_concurrent_calls, 5);
        assert_eq!(srv.call_timeout_secs, 30);
        assert_eq!(srv.max_output_bytes, 1_048_576);
    }
}
