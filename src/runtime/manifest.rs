use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

use crate::core::error::{NyayaError, Result};

/// Agent manifest — declares identity, permissions, and resource limits.
/// Analogous to Android's AndroidManifest.xml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentManifest {
    /// Human-readable agent name
    pub name: String,
    /// Semantic version
    pub version: String,
    /// Short description of what this agent does
    pub description: String,
    /// Permissions this agent requests (list of ability names)
    pub permissions: Vec<String>,
    /// Maximum memory in MB the WASM module may use
    #[serde(default = "default_memory_limit")]
    pub memory_limit_mb: u32,
    /// Fuel limit for execution (prevents infinite loops)
    #[serde(default = "default_fuel_limit")]
    pub fuel_limit: u64,
    /// Namespace for the agent's scoped key-value store
    #[serde(default)]
    pub kv_namespace: Option<String>,
    /// Author
    #[serde(default)]
    pub author: Option<String>,
    /// Intent filters for Agent OS routing
    #[serde(default)]
    pub intent_filters: Vec<crate::agent_os::types::IntentFilter>,
    /// Resource limits for Agent OS sandbox
    #[serde(default)]
    pub resources: Option<crate::agent_os::types::ResourceLimits>,
    /// Whether this agent runs as a background service
    #[serde(default)]
    pub background: bool,
    /// Event subscriptions for background wake
    #[serde(default)]
    pub subscriptions: Vec<String>,
    /// Data namespace override
    #[serde(default)]
    pub data_namespace: Option<String>,
    /// Cryptographic signature for verification
    #[serde(default)]
    pub signature: Option<String>,
}

fn default_memory_limit() -> u32 {
    64
}

fn default_fuel_limit() -> u64 {
    1_000_000
}

/// Known permissions that can be granted to agents.
pub const KNOWN_PERMISSIONS: &[&str] = &[
    "kv.read",
    "kv.write",
    "http.fetch",
    "log.info",
    "log.error",
    "notify.user",
    "data.fetch_url",
    "data.download",
    "data.analyze",
    "nlp.sentiment",
    "nlp.summarize",
    "storage.get",
    "storage.set",
    "flow.branch",
    "flow.stop",
    "schedule.delay",
    "email.send",
    "email.list",
    "email.read",
    "email.reply",
    "trading.get_price",
    "files.read",
    "files.list",
    "files.write",
    "shell.exec",
    "browser.fetch",
    "browser.screenshot",
    "browser.set_cookies",
    "browser.click",
    "browser.fill_form",
    "calendar.list",
    "calendar.add",
    "memory.store",
    "memory.search",
    "channel.send",
    "sms.send",
    "deep.delegate",
    "llm.summarize",
    "llm.chat",
    "script.run",
    "docs.generate",
];

impl AgentManifest {
    /// Load manifest from a JSON file.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| NyayaError::Config(format!("Failed to read manifest: {}", e)))?;
        let manifest: Self = serde_json::from_str(&content)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validate the manifest fields.
    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty() {
            return Err(NyayaError::Config("Agent name cannot be empty".into()));
        }
        if self.version.is_empty() {
            return Err(NyayaError::Config("Agent version cannot be empty".into()));
        }
        if self.memory_limit_mb == 0 || self.memory_limit_mb > 512 {
            return Err(NyayaError::Config(
                "memory_limit_mb must be between 1 and 512".into(),
            ));
        }
        if self.fuel_limit == 0 {
            return Err(NyayaError::Config("fuel_limit must be > 0".into()));
        }
        Ok(())
    }

    /// Check if this agent has a specific permission.
    pub fn has_permission(&self, perm: &str) -> bool {
        self.permissions.iter().any(|p| p == perm)
    }

    /// Get the KV namespace, defaulting to the agent name.
    pub fn namespace(&self) -> &str {
        self.kv_namespace.as_deref().unwrap_or(&self.name)
    }

    /// Return the set of granted permissions.
    pub fn permission_set(&self) -> HashSet<&str> {
        self.permissions.iter().map(|s| s.as_str()).collect()
    }

    /// Create a default manifest for workflow engine execution.
    /// Grants all known permissions so workflow actions can execute.
    pub fn workflow_manifest() -> Self {
        Self {
            name: "workflow-engine".into(),
            version: "0.1.0".into(),
            description: "Internal manifest for workflow engine execution".into(),
            permissions: KNOWN_PERMISSIONS.iter().map(|s| s.to_string()).collect(),
            memory_limit_mb: 64,
            fuel_limit: 1_000_000,
            kv_namespace: None,
            author: None,
            intent_filters: vec![],
            resources: None,
            background: false,
            subscriptions: vec![],
            data_namespace: None,
            signature: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_manifest() {
        let json = r#"{
            "name": "weather-agent",
            "version": "0.1.0",
            "description": "Fetches weather data",
            "permissions": ["kv.read", "kv.write", "http.fetch"],
            "memory_limit_mb": 32,
            "fuel_limit": 500000
        }"#;
        let m: AgentManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.name, "weather-agent");
        assert!(m.has_permission("kv.read"));
        assert!(!m.has_permission("email.send"));
        assert_eq!(m.namespace(), "weather-agent");
    }

    #[test]
    fn test_empty_name_rejected() {
        let m = AgentManifest {
            name: String::new(),
            version: "0.1.0".into(),
            description: "test".into(),
            permissions: vec![],
            memory_limit_mb: 64,
            fuel_limit: 1_000_000,
            kv_namespace: None,
            author: None,
            intent_filters: vec![],
            resources: None,
            background: false,
            subscriptions: vec![],
            data_namespace: None,
            signature: None,
        };
        assert!(m.validate().is_err());
    }
}
