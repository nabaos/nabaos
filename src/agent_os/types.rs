use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Unique identifier for an agent instance.
pub type AgentId = String;

/// Lifecycle state of an installed agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentState {
    Running,
    Paused,
    Stopped,
    Disabled,
}

impl fmt::Display for AgentState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentState::Running => write!(f, "running"),
            AgentState::Paused => write!(f, "paused"),
            AgentState::Stopped => write!(f, "stopped"),
            AgentState::Disabled => write!(f, "disabled"),
        }
    }
}

/// Tracks resource consumption for a single agent.
#[derive(Debug, Default)]
pub struct ResourceUsage {
    pub fuel_consumed: u64,
    pub api_calls_this_hour: u32,
    pub peak_memory_bytes: u64,
    pub hour_reset_ts: u64,
}

impl ResourceUsage {
    /// Resets `api_calls_this_hour` if at least 3600 seconds have elapsed
    /// since `hour_reset_ts`.
    pub fn maybe_reset_hourly(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now.saturating_sub(self.hour_reset_ts) >= 3600 {
            self.api_calls_this_hour = 0;
            self.hour_reset_ts = now;
        }
    }
}

/// Configurable resource limits for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    #[serde(default = "default_max_memory_mb")]
    pub max_memory_mb: u32,
    #[serde(default = "default_max_fuel")]
    pub max_fuel: u64,
    #[serde(default = "default_max_api_calls_per_hour")]
    pub max_api_calls_per_hour: u32,
}

fn default_max_memory_mb() -> u32 {
    128
}
fn default_max_fuel() -> u64 {
    1_000_000
}
fn default_max_api_calls_per_hour() -> u32 {
    100
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory_mb: 128,
            max_fuel: 1_000_000,
            max_api_calls_per_hour: 100,
        }
    }
}

/// Filter that determines which intents an agent subscribes to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentFilter {
    pub actions: Vec<String>,
    pub targets: Vec<String>,
    #[serde(default)]
    pub priority: i32,
}

impl IntentFilter {
    /// Returns `true` if this filter matches the given action and target.
    /// An empty `actions` or `targets` vec means "match all".
    pub fn matches(&self, action: &str, target: &str) -> bool {
        let action_ok =
            self.actions.is_empty() || self.actions.iter().any(|a| a.eq_ignore_ascii_case(action));
        let target_ok =
            self.targets.is_empty() || self.targets.iter().any(|t| t.eq_ignore_ascii_case(target));
        action_ok && target_ok
    }
}

/// Metadata for an installed agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledAgent {
    pub id: AgentId,
    pub version: String,
    pub state: AgentState,
    pub data_dir: PathBuf,
    pub installed_at: u64,
    pub updated_at: u64,
}

/// Trigger definition — when an agent wakes up.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerDef {
    Scheduled(ScheduledTrigger),
    Event(EventTrigger),
    Webhook(WebhookTrigger),
    Channel(ChannelTrigger),
}

/// Scheduled trigger — fires on a time interval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTrigger {
    pub chain: String,
    pub interval: String,
    #[serde(default)]
    pub at: Option<String>,
    #[serde(default)]
    pub params: std::collections::HashMap<String, String>,
}

/// Event trigger — fires when a MessageBus event matches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTrigger {
    pub on: String,
    #[serde(default)]
    pub filter: std::collections::HashMap<String, String>,
    pub chain: String,
    #[serde(default)]
    pub params: std::collections::HashMap<String, String>,
}

/// Webhook trigger — fires when an external HTTP POST arrives.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookTrigger {
    pub path: String,
    pub chain: String,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub params: std::collections::HashMap<String, String>,
}

/// Trigger mode — realtime (push) or poll (pull).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TriggerMode {
    Realtime,
    Poll,
}

/// Channel trigger — fires when a message arrives on a messaging channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelTrigger {
    pub channel: String,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub from_domain: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub pattern: Option<String>,
    #[serde(default)]
    pub subject_pattern: Option<String>,
    pub workflow: String,
    #[serde(default)]
    pub params: std::collections::HashMap<String, String>,
    #[serde(default = "default_realtime")]
    pub mode: TriggerMode,
    #[serde(default)]
    pub poll_interval: Option<String>,
}

fn default_realtime() -> TriggerMode {
    TriggerMode::Realtime
}

/// Extended manifest triggers section.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentTriggers {
    #[serde(default)]
    pub scheduled: Vec<ScheduledTrigger>,
    #[serde(default)]
    pub events: Vec<EventTrigger>,
    #[serde(default)]
    pub webhooks: Vec<WebhookTrigger>,
    #[serde(default)]
    pub channels: Vec<ChannelTrigger>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_state_display() {
        assert_eq!(AgentState::Running.to_string(), "running");
        assert_eq!(AgentState::Paused.to_string(), "paused");
        assert_eq!(AgentState::Stopped.to_string(), "stopped");
        assert_eq!(AgentState::Disabled.to_string(), "disabled");
    }

    #[test]
    fn test_resource_usage_hourly_reset() {
        let mut usage = ResourceUsage {
            fuel_consumed: 500,
            api_calls_this_hour: 42,
            peak_memory_bytes: 1024,
            hour_reset_ts: 0, // epoch — guaranteed >3600s ago
        };
        usage.maybe_reset_hourly();
        assert_eq!(usage.api_calls_this_hour, 0);
        // fuel_consumed and peak_memory_bytes are untouched
        assert_eq!(usage.fuel_consumed, 500);
        assert_eq!(usage.peak_memory_bytes, 1024);
    }

    #[test]
    fn test_intent_filter_matches() {
        let filter = IntentFilter {
            actions: vec!["check".to_string(), "search".to_string()],
            targets: vec!["email".to_string()],
            priority: 0,
        };
        assert!(filter.matches("check", "email"));
        assert!(filter.matches("CHECK", "EMAIL")); // case-insensitive
        assert!(filter.matches("search", "email"));
        assert!(!filter.matches("send", "email"));
        assert!(!filter.matches("check", "weather"));
    }

    #[test]
    fn test_intent_filter_empty_matches_all() {
        let filter = IntentFilter {
            actions: vec![],
            targets: vec![],
            priority: 0,
        };
        assert!(filter.matches("anything", "everything"));
        assert!(filter.matches("check", "email"));
    }

    #[test]
    fn test_resource_limits_default() {
        let limits = ResourceLimits::default();
        assert_eq!(limits.max_memory_mb, 128);
        assert_eq!(limits.max_fuel, 1_000_000);
        assert_eq!(limits.max_api_calls_per_hour, 100);
    }

    #[test]
    fn test_channel_trigger_yaml_parse() {
        let yaml = r#"
channel: telegram
from: alice@example.com
from_domain: example.com
group: dev-team
pattern: "urgent.*deploy"
subject_pattern: "^\\[PROD\\]"
workflow: triage-urgent
params:
  priority: high
mode: realtime
poll_interval: null
"#;
        let trigger: ChannelTrigger = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(trigger.channel, "telegram");
        assert_eq!(trigger.from.as_deref(), Some("alice@example.com"));
        assert_eq!(trigger.from_domain.as_deref(), Some("example.com"));
        assert_eq!(trigger.group.as_deref(), Some("dev-team"));
        assert_eq!(trigger.pattern.as_deref(), Some("urgent.*deploy"));
        assert_eq!(trigger.subject_pattern.as_deref(), Some("^\\[PROD\\]"));
        assert_eq!(trigger.workflow, "triage-urgent");
        assert_eq!(trigger.params.get("priority").unwrap(), "high");
        assert!(matches!(trigger.mode, TriggerMode::Realtime));
    }

    #[test]
    fn test_agent_triggers_with_channels() {
        let yaml = r#"
scheduled: []
events: []
webhooks: []
channels:
  - channel: email
    workflow: process-email
    params: {}
  - channel: telegram
    from: bob
    workflow: handle-telegram
    mode: poll
    poll_interval: "5m"
    params: {}
"#;
        let triggers: AgentTriggers = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(triggers.channels.len(), 2);
        assert_eq!(triggers.channels[0].channel, "email");
        assert_eq!(triggers.channels[0].workflow, "process-email");
        assert_eq!(triggers.channels[1].channel, "telegram");
        assert_eq!(triggers.channels[1].from.as_deref(), Some("bob"));
        assert!(matches!(triggers.channels[1].mode, TriggerMode::Poll));
        assert_eq!(triggers.channels[1].poll_interval.as_deref(), Some("5m"));
    }
}
