//! MCP tool discovery cache and ability registration.
//!
//! Provides functions to:
//! - Cache discovered MCP tools to disk as JSON
//! - Load cached tools
//! - Convert MCP tool definitions into AbilitySpecs
//! - Validate tool allow-lists against discovered tools
//! - Validate server IDs

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::transport::McpToolDef;
use crate::core::error::{NyayaError, Result};
use crate::runtime::host_functions::AbilitySpec;
use crate::runtime::plugin::AbilitySource;

/// Internal cache structure written to disk.
#[derive(Debug, Serialize, Deserialize)]
struct ToolsCache {
    server_id: String,
    discovered_at: String,
    tools: Vec<McpToolDef>,
}

/// Save discovered tools to a JSON cache file at `<cache_dir>/<server_id>.json`.
pub fn save_tools_cache(cache_dir: &Path, server_id: &str, tools: &[McpToolDef]) -> Result<()> {
    validate_server_id(server_id)?;
    std::fs::create_dir_all(cache_dir)?;
    let cache = ToolsCache {
        server_id: server_id.to_string(),
        discovered_at: chrono::Utc::now().to_rfc3339(),
        tools: tools.to_vec(),
    };
    let json = serde_json::to_string_pretty(&cache)?;
    let path = cache_dir.join(format!("{}.json", server_id));
    std::fs::write(&path, json)?;
    Ok(())
}

/// Load cached tools from `<cache_dir>/<server_id>.json`.
/// Returns `Ok(None)` if the cache file does not exist.
pub fn load_tools_cache(cache_dir: &Path, server_id: &str) -> Result<Option<Vec<McpToolDef>>> {
    let path = cache_dir.join(format!("{}.json", server_id));
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let cache: ToolsCache = serde_json::from_str(&content)?;
    Ok(Some(cache.tools))
}

/// Convert MCP tool definitions into AbilitySpecs.
///
/// Each tool is registered as `mcp.<server_id>.<tool_name>` with
/// `AbilitySource::Cloud` and the tool's input schema attached.
pub fn tools_to_ability_specs(server_id: &str, tools: &[McpToolDef]) -> Vec<AbilitySpec> {
    tools
        .iter()
        .map(|tool| {
            let name = format!("mcp.{}.{}", server_id, tool.name);
            AbilitySpec {
                name: name.clone(),
                description: tool.description.clone(),
                permission: name,
                source: AbilitySource::Cloud,
                input_schema: Some(tool.input_schema.clone()),
            }
        })
        .collect()
}

/// Validate an `allowed_tools` list against discovered tools.
///
/// Returns warnings for each tool name in `allowed` that does not appear
/// in the `discovered` set.
pub fn validate_allowed_tools(
    server_id: &str,
    discovered: &[McpToolDef],
    allowed: &[String],
) -> Vec<String> {
    let discovered_names: std::collections::HashSet<&str> =
        discovered.iter().map(|t| t.name.as_str()).collect();
    allowed
        .iter()
        .filter(|name| !discovered_names.contains(name.as_str()))
        .map(|name| {
            format!(
                "Server '{}': allowed tool '{}' was not found in discovered tools",
                server_id, name
            )
        })
        .collect()
}

/// Validate a server ID.
///
/// Rules: non-empty, contains only alphanumeric characters, hyphens, and underscores.
pub fn validate_server_id(server_id: &str) -> Result<()> {
    if server_id.is_empty() {
        return Err(NyayaError::Config(
            "MCP server ID must not be empty".to_string(),
        ));
    }
    if !server_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(NyayaError::Config(format!(
            "MCP server ID '{}' contains invalid characters (only alphanumeric, hyphens, underscores allowed)",
            server_id
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn sample_tools() -> Vec<McpToolDef> {
        vec![
            McpToolDef {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    },
                    "required": ["path"]
                }),
            },
            McpToolDef {
                name: "list_directory".to_string(),
                description: "List a directory".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    }
                }),
            },
        ]
    }

    #[test]
    fn test_save_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path().join("mcp_cache");
        let tools = sample_tools();

        save_tools_cache(&cache_dir, "filesystem", &tools).unwrap();
        let loaded = load_tools_cache(&cache_dir, "filesystem").unwrap();

        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "read_file");
        assert_eq!(loaded[1].name, "list_directory");
        assert_eq!(loaded[0].input_schema["required"], json!(["path"]));
    }

    #[test]
    fn test_load_missing_returns_none() {
        let dir = TempDir::new().unwrap();
        let result = load_tools_cache(dir.path(), "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_tools_to_ability_specs_naming_and_schema() {
        let tools = sample_tools();
        let specs = tools_to_ability_specs("filesystem", &tools);

        assert_eq!(specs.len(), 2);

        assert_eq!(specs[0].name, "mcp.filesystem.read_file");
        assert_eq!(specs[0].permission, "mcp.filesystem.read_file");
        assert_eq!(specs[0].description, "Read a file");
        assert_eq!(specs[0].source, AbilitySource::Cloud);
        assert!(specs[0].input_schema.is_some());
        assert_eq!(
            specs[0].input_schema.as_ref().unwrap()["required"],
            json!(["path"])
        );

        assert_eq!(specs[1].name, "mcp.filesystem.list_directory");
        assert_eq!(specs[1].source, AbilitySource::Cloud);
    }

    #[test]
    fn test_validate_allowed_tools_warns_on_unknown() {
        let tools = sample_tools();
        let allowed = vec![
            "read_file".to_string(),
            "write_file".to_string(),
            "delete_file".to_string(),
        ];

        let warnings = validate_allowed_tools("filesystem", &tools, &allowed);
        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].contains("write_file"));
        assert!(warnings[1].contains("delete_file"));
    }

    #[test]
    fn test_validate_allowed_tools_no_warnings_when_all_exist() {
        let tools = sample_tools();
        let allowed = vec!["read_file".to_string(), "list_directory".to_string()];

        let warnings = validate_allowed_tools("filesystem", &tools, &allowed);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_validate_server_id_accepts_valid() {
        assert!(validate_server_id("filesystem").is_ok());
        assert!(validate_server_id("my-server").is_ok());
        assert!(validate_server_id("my_server_2").is_ok());
        assert!(validate_server_id("MCP-Server-01").is_ok());
    }

    #[test]
    fn test_validate_server_id_rejects_empty() {
        let err = validate_server_id("").unwrap_err();
        assert!(format!("{}", err).contains("must not be empty"));
    }

    #[test]
    fn test_validate_server_id_rejects_invalid_chars() {
        assert!(validate_server_id("my server").is_err());
        assert!(validate_server_id("my.server").is_err());
        assert!(validate_server_id("my/server").is_err());
        assert!(validate_server_id("server@home").is_err());
    }
}
