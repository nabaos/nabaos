use std::collections::HashMap;

use super::types::AgentId;
use crate::core::error::{NyayaError, Result};

/// Configuration for a background agent.
#[derive(Debug, Clone)]
pub struct BackgroundConfig {
    pub agent_id: AgentId,
    pub subscriptions: Vec<String>,
    pub max_cpu_secs_per_wake: u64,
    pub wake_interval_secs: u64,
}

/// Manages background agents — agents that wake periodically or on events.
pub struct BackgroundManager {
    configs: HashMap<AgentId, BackgroundConfig>,
}

impl BackgroundManager {
    /// Create a new empty background manager.
    pub fn new() -> Self {
        Self {
            configs: HashMap::new(),
        }
    }

    /// Register a background agent configuration.
    pub fn register(&mut self, config: BackgroundConfig) -> Result<()> {
        if self.configs.contains_key(&config.agent_id) {
            return Err(NyayaError::Config(format!(
                "Background agent '{}' is already registered",
                config.agent_id
            )));
        }
        self.configs.insert(config.agent_id.clone(), config);
        Ok(())
    }

    /// Unregister a background agent.
    pub fn unregister(&mut self, agent_id: &str) -> Result<()> {
        self.configs.remove(agent_id).ok_or_else(|| {
            NyayaError::Config(format!("Background agent '{}' not found", agent_id))
        })?;
        Ok(())
    }

    /// Get a background agent's configuration.
    pub fn get(&self, agent_id: &str) -> Option<&BackgroundConfig> {
        self.configs.get(agent_id)
    }

    /// List all registered background agents.
    pub fn list(&self) -> Vec<&BackgroundConfig> {
        self.configs.values().collect()
    }

    /// Check if an agent is registered as a background agent.
    pub fn is_background(&self, agent_id: &str) -> bool {
        self.configs.contains_key(agent_id)
    }
}

impl Default for BackgroundManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config(id: &str) -> BackgroundConfig {
        BackgroundConfig {
            agent_id: id.to_string(),
            subscriptions: vec!["data.changed".to_string()],
            max_cpu_secs_per_wake: 5,
            wake_interval_secs: 300,
        }
    }

    #[test]
    fn test_register() {
        let mut mgr = BackgroundManager::new();
        mgr.register(sample_config("bg-agent")).unwrap();
        assert!(mgr.is_background("bg-agent"));
        assert!(!mgr.is_background("other"));
    }

    #[test]
    fn test_register_duplicate_rejected() {
        let mut mgr = BackgroundManager::new();
        mgr.register(sample_config("bg-agent")).unwrap();
        let result = mgr.register(sample_config("bg-agent"));
        assert!(result.is_err());
    }

    #[test]
    fn test_unregister() {
        let mut mgr = BackgroundManager::new();
        mgr.register(sample_config("bg-agent")).unwrap();
        mgr.unregister("bg-agent").unwrap();
        assert!(!mgr.is_background("bg-agent"));
    }

    #[test]
    fn test_unregister_nonexistent() {
        let mut mgr = BackgroundManager::new();
        let result = mgr.unregister("nope");
        assert!(result.is_err());
    }

    #[test]
    fn test_list() {
        let mut mgr = BackgroundManager::new();
        mgr.register(sample_config("bg-a")).unwrap();
        mgr.register(sample_config("bg-b")).unwrap();
        assert_eq!(mgr.list().len(), 2);
    }

    #[test]
    fn test_get() {
        let mut mgr = BackgroundManager::new();
        mgr.register(sample_config("bg-agent")).unwrap();
        let cfg = mgr.get("bg-agent").unwrap();
        assert_eq!(cfg.wake_interval_secs, 300);
        assert!(mgr.get("nonexistent").is_none());
    }
}
