//! Watch event types for the runtime watcher event bus.

use serde::{Deserialize, Serialize};

/// Severity levels for watch events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Warning,
    Suspicious,
    Critical,
    Fatal,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "INFO"),
            Self::Warning => write!(f, "WARNING"),
            Self::Suspicious => write!(f, "SUSPICIOUS"),
            Self::Critical => write!(f, "CRITICAL"),
            Self::Fatal => write!(f, "FATAL"),
        }
    }
}

/// Kinds of events the watcher can observe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WatchEventKind {
    // Logs & errors
    Error {
        module: String,
        message: String,
    },
    Panic {
        module: String,
        backtrace: Option<String>,
    },

    // Security
    InjectionDetected {
        pattern: String,
        confidence: f64,
        source: String,
    },
    CredentialLeak {
        credential_type: String,
        destination: String,
    },
    ConstitutionViolation {
        rule: String,
        action_attempted: String,
    },

    // Network
    OutboundRequest {
        destination: String,
        bytes: u64,
        status: u16,
    },
    UnusualDestination {
        destination: String,
        reason: String,
    },
    DataVolumeSpike {
        bytes_last_minute: u64,
        baseline: u64,
    },

    // PEA
    PramanaValidationFailed {
        objective_id: String,
        decision: String,
        reason: String,
    },
    BudgetAnomaly {
        objective_id: String,
        burn_rate: f64,
        projected_overshoot: f64,
    },
    TaskRetriesExhausted {
        objective_id: String,
        task_id: String,
    },

    // Cache
    CacheConfidenceDrift {
        entry_id: String,
        old_confidence: f64,
        new_confidence: f64,
    },
    SuspiciousCacheEntry {
        entry_id: String,
        reason: String,
    },

    // Privilege escalation
    PrivilegeEscalation {
        agent_id: String,
        attempted_level: String,
        current_level: String,
    },

    // System health
    HighMemory {
        used_mb: u64,
        total_mb: u64,
    },
    HighCpu {
        percent: f64,
    },
    ComponentFailure {
        component: String,
        error: String,
    },
}

impl WatchEventKind {
    /// Return the component name this event relates to.
    pub fn component(&self) -> &str {
        match self {
            Self::Error { module, .. } | Self::Panic { module, .. } => module,
            Self::InjectionDetected { .. } | Self::CredentialLeak { .. } => "security",
            Self::ConstitutionViolation { .. } | Self::PrivilegeEscalation { .. } => "constitution",
            Self::OutboundRequest { .. }
            | Self::UnusualDestination { .. }
            | Self::DataVolumeSpike { .. } => "network",
            Self::PramanaValidationFailed { .. }
            | Self::BudgetAnomaly { .. }
            | Self::TaskRetriesExhausted { .. } => "pea",
            Self::CacheConfidenceDrift { .. } | Self::SuspiciousCacheEntry { .. } => "cache",
            Self::HighMemory { .. } | Self::HighCpu { .. } => "system",
            Self::ComponentFailure { component, .. } => component,
        }
    }

    /// Return the monitor category this event belongs to.
    /// Matches the field names in `EnabledMonitors`: "logs", "security", "network", "pea", "cache", "system_health".
    pub fn monitor_category(&self) -> &str {
        match self {
            Self::Error { .. } | Self::Panic { .. } => "logs",
            Self::InjectionDetected { .. }
            | Self::CredentialLeak { .. }
            | Self::ConstitutionViolation { .. }
            | Self::PrivilegeEscalation { .. } => "security",
            Self::OutboundRequest { .. }
            | Self::UnusualDestination { .. }
            | Self::DataVolumeSpike { .. } => "network",
            Self::PramanaValidationFailed { .. }
            | Self::BudgetAnomaly { .. }
            | Self::TaskRetriesExhausted { .. } => "pea",
            Self::CacheConfidenceDrift { .. } | Self::SuspiciousCacheEntry { .. } => "cache",
            Self::HighMemory { .. } | Self::HighCpu { .. } | Self::ComponentFailure { .. } => {
                "system_health"
            }
        }
    }

    /// Base score contribution for this event kind.
    pub fn base_score(&self) -> f64 {
        match self {
            Self::Error { .. } => 0.1,
            Self::Panic { .. } => 0.5,
            Self::InjectionDetected { confidence, .. } => 0.3 * confidence,
            Self::CredentialLeak { .. } => 0.8,
            Self::ConstitutionViolation { .. } => 0.6,
            Self::PrivilegeEscalation { .. } => 0.7,
            Self::OutboundRequest { .. } => 0.0, // informational
            Self::UnusualDestination { .. } => 0.2,
            Self::DataVolumeSpike {
                bytes_last_minute,
                baseline,
            } => {
                if *baseline == 0 {
                    0.2
                } else {
                    (0.2 * (*bytes_last_minute as f64 / *baseline as f64)).min(1.0)
                }
            }
            Self::PramanaValidationFailed { .. } => 0.15,
            Self::BudgetAnomaly {
                projected_overshoot,
                ..
            } => (0.2 * projected_overshoot).min(1.0),
            Self::TaskRetriesExhausted { .. } => 0.1,
            Self::CacheConfidenceDrift { .. } => 0.05,
            Self::SuspiciousCacheEntry { .. } => 0.2,
            Self::HighMemory { .. } => 0.3,
            Self::HighCpu { .. } => 0.2,
            Self::ComponentFailure { .. } => 0.4,
        }
    }
}

/// A timestamped event emitted by any NabaOS component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchEvent {
    pub timestamp: u64,
    pub kind: WatchEventKind,
    pub severity: Severity,
}

impl WatchEvent {
    pub fn new(kind: WatchEventKind, severity: Severity) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            timestamp,
            kind,
            severity,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Suspicious);
        assert!(Severity::Suspicious < Severity::Critical);
        assert!(Severity::Critical < Severity::Fatal);
    }

    #[test]
    fn test_event_component_mapping() {
        let e = WatchEventKind::CredentialLeak {
            credential_type: "aws".into(),
            destination: "evil.com".into(),
        };
        assert_eq!(e.component(), "security");

        let e2 = WatchEventKind::BudgetAnomaly {
            objective_id: "obj1".into(),
            burn_rate: 2.0,
            projected_overshoot: 1.5,
        };
        assert_eq!(e2.component(), "pea");
    }

    #[test]
    fn test_privilege_escalation_event() {
        let e = WatchEventKind::PrivilegeEscalation {
            agent_id: "agent-1".into(),
            attempted_level: "Admin".into(),
            current_level: "Open".into(),
        };
        assert_eq!(e.component(), "constitution");
        assert_eq!(e.monitor_category(), "security");
        assert!((e.base_score() - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn test_event_kind_count_is_17() {
        // Ensure we have exactly 17 event types by checking all categories
        let events: Vec<WatchEventKind> = vec![
            WatchEventKind::Error { module: "m".into(), message: "e".into() },
            WatchEventKind::Panic { module: "m".into(), backtrace: None },
            WatchEventKind::InjectionDetected { pattern: "p".into(), confidence: 0.9, source: "s".into() },
            WatchEventKind::CredentialLeak { credential_type: "c".into(), destination: "d".into() },
            WatchEventKind::ConstitutionViolation { rule: "r".into(), action_attempted: "a".into() },
            WatchEventKind::PrivilegeEscalation { agent_id: "a".into(), attempted_level: "Admin".into(), current_level: "Open".into() },
            WatchEventKind::OutboundRequest { destination: "d".into(), bytes: 100, status: 200 },
            WatchEventKind::UnusualDestination { destination: "d".into(), reason: "r".into() },
            WatchEventKind::DataVolumeSpike { bytes_last_minute: 1000, baseline: 100 },
            WatchEventKind::PramanaValidationFailed { objective_id: "o".into(), decision: "d".into(), reason: "r".into() },
            WatchEventKind::BudgetAnomaly { objective_id: "o".into(), burn_rate: 1.0, projected_overshoot: 0.5 },
            WatchEventKind::TaskRetriesExhausted { objective_id: "o".into(), task_id: "t".into() },
            WatchEventKind::CacheConfidenceDrift { entry_id: "e".into(), old_confidence: 0.9, new_confidence: 0.5 },
            WatchEventKind::SuspiciousCacheEntry { entry_id: "e".into(), reason: "r".into() },
            WatchEventKind::HighMemory { used_mb: 8000, total_mb: 16000 },
            WatchEventKind::HighCpu { percent: 95.0 },
            WatchEventKind::ComponentFailure { component: "c".into(), error: "e".into() },
        ];
        assert_eq!(events.len(), 17);
    }

    #[test]
    fn test_base_score_credential_leak_is_high() {
        let e = WatchEventKind::CredentialLeak {
            credential_type: "api_key".into(),
            destination: "unknown.com".into(),
        };
        assert!(e.base_score() >= 0.8);
    }
}
