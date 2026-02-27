//! Watcher configuration with sensible defaults.

use serde::{Deserialize, Serialize};

/// Bitflags-style monitor selection (kept simple — just bools).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnabledMonitors {
    pub logs: bool,
    pub security: bool,
    pub network: bool,
    pub pea: bool,
    pub cache: bool,
    pub system_health: bool,
}

impl EnabledMonitors {
    /// Check whether a monitor category is enabled.
    /// `category` should be one of: "logs", "security", "network", "pea", "cache", "system_health".
    pub fn is_enabled(&self, category: &str) -> bool {
        match category {
            "logs" => self.logs,
            "security" => self.security,
            "network" => self.network,
            "pea" => self.pea,
            "cache" => self.cache,
            "system_health" => self.system_health,
            _ => true, // unknown categories are enabled by default
        }
    }
}

impl Default for EnabledMonitors {
    fn default() -> Self {
        Self {
            logs: true,
            security: true,
            network: true,
            pea: true,
            cache: true,
            system_health: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherConfig {
    /// Component score threshold to trigger LLM analysis.
    pub llm_threshold: f64,
    /// Component score threshold to auto-pause the component.
    pub pause_threshold: f64,
    /// Sliding window duration in seconds.
    pub window_secs: u64,
    /// If true, run periodic LLM review on schedule.
    pub periodic_review: bool,
    /// Interval between periodic reviews in seconds.
    pub review_interval_secs: u64,
    /// Channel names to send alerts to (e.g., "telegram", "slack").
    pub alert_channels: Vec<String>,
    /// Which monitor categories are active.
    pub enabled_monitors: EnabledMonitors,
    /// LLM cooldown per component in seconds (prevent flapping).
    pub llm_cooldown_secs: u64,
    /// Auto-prune alerts older than this many days.
    pub alert_retention_days: u64,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            llm_threshold: 0.7,
            pause_threshold: 0.9,
            window_secs: 300,
            periodic_review: false,
            review_interval_secs: 3600,
            alert_channels: vec!["telegram".to_string()],
            enabled_monitors: EnabledMonitors::default(),
            llm_cooldown_secs: 600,
            alert_retention_days: 30,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_thresholds() {
        let cfg = WatcherConfig::default();
        assert!((cfg.llm_threshold - 0.7).abs() < f64::EPSILON);
        assert!((cfg.pause_threshold - 0.9).abs() < f64::EPSILON);
        assert_eq!(cfg.window_secs, 300);
        assert!(!cfg.periodic_review);
    }

    #[test]
    fn test_default_monitors_all_enabled() {
        let m = EnabledMonitors::default();
        assert!(m.logs && m.security && m.network && m.pea && m.cache && m.system_health);
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let cfg = WatcherConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let cfg2: WatcherConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg.window_secs, cfg2.window_secs);
    }
}
