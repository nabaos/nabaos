//! LLM analyzer — builds prompts from event windows, parses verdicts, tracks cooldowns.

use crate::watcher::events::{WatchEvent, WatchEventKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The LLM's assessment of a component's anomaly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmVerdict {
    pub severity: String,
    pub diagnosis: String,
    pub recommended_action: String,
    pub confidence: f64,
}

/// Action the watcher should take based on LLM verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecommendedAction {
    Ignore,
    Alert { message: String },
    Pause { component: String },
}

/// Tracks cooldowns per component to prevent repeated LLM calls.
pub struct Analyzer {
    /// component → timestamp of last LLM call
    last_llm_call: HashMap<String, u64>,
    cooldown_secs: u64,
}

impl Analyzer {
    pub fn new(cooldown_secs: u64) -> Self {
        Self {
            last_llm_call: HashMap::new(),
            cooldown_secs,
        }
    }

    /// Check if a component is still in cooldown.
    pub fn in_cooldown(&self, component: &str, now: u64) -> bool {
        self.last_llm_call
            .get(component)
            .is_some_and(|&last| now.saturating_sub(last) < self.cooldown_secs)
    }

    /// Record that we called the LLM for this component.
    pub fn record_call(&mut self, component: &str, now: u64) {
        self.last_llm_call.insert(component.to_string(), now);
    }

    /// Build a prompt from component events for LLM analysis.
    /// Redacts credential values — only includes types and destinations.
    pub fn build_prompt(component: &str, events: &[&WatchEvent], score: f64) -> String {
        let mut lines = Vec::new();
        lines.push(
            "You are a security and reliability monitor for NabaOS, a personal agent runtime."
                .to_string(),
        );
        lines.push(format!(
            "Component '{}' has anomaly score {:.2}/1.0. Analyze these recent events:",
            component, score
        ));
        lines.push(String::new());

        for (i, ev) in events.iter().enumerate().take(20) {
            let summary = summarize_event_redacted(&ev.kind);
            lines.push(format!(
                "{}. [{}] t={}: {}",
                i + 1,
                ev.severity,
                ev.timestamp,
                summary
            ));
        }

        lines.push(String::new());
        lines.push("Respond with JSON only:".to_string());
        lines.push(r#"{"severity":"INFO|WARNING|SUSPICIOUS|CRITICAL|FATAL","diagnosis":"...","recommended_action":"ignore|alert|pause","confidence":0.0-1.0}"#.to_string());

        lines.join("\n")
    }

    /// Parse LLM response JSON into a verdict.
    pub fn parse_verdict(response: &str) -> Option<LlmVerdict> {
        // Try direct JSON parse first
        if let Ok(v) = serde_json::from_str::<LlmVerdict>(response) {
            return Some(v);
        }
        // Try extracting JSON from markdown code fences
        let trimmed = response.trim();
        if let Some(start) = trimmed.find('{') {
            if let Some(end) = trimmed.rfind('}') {
                if let Ok(v) = serde_json::from_str::<LlmVerdict>(&trimmed[start..=end]) {
                    return Some(v);
                }
            }
        }
        None
    }

    /// Convert LlmVerdict into a RecommendedAction.
    pub fn verdict_to_action(verdict: &LlmVerdict, component: &str) -> RecommendedAction {
        match verdict.recommended_action.as_str() {
            "pause" => RecommendedAction::Pause {
                component: component.to_string(),
            },
            "alert" => RecommendedAction::Alert {
                message: verdict.diagnosis.clone(),
            },
            _ => RecommendedAction::Ignore,
        }
    }
}

/// Redact common secret patterns from a string.
/// Replaces API keys (sk-..., ghp_..., gho_..., glpat-..., xoxb-..., xoxp-...),
/// bearer tokens, and password-like assignments with [REDACTED].
fn redact_secrets(s: &str) -> String {
    use regex::Regex;
    // Lazy-init regexes — compiled once per process.
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(concat!(
            r"(?i)",
            r"(?:",
            // OpenAI / Anthropic style keys: sk-... (at least 10 chars after prefix)
            r"sk-[A-Za-z0-9_\-]{10,}",
            r"|",
            // GitHub personal access tokens
            r"ghp_[A-Za-z0-9]{30,}",
            r"|",
            // GitHub OAuth tokens
            r"gho_[A-Za-z0-9]{30,}",
            r"|",
            // GitLab tokens
            r"glpat-[A-Za-z0-9\-]{20,}",
            r"|",
            // Slack tokens
            r"xox[bp]-[A-Za-z0-9\-]{20,}",
            r"|",
            // AWS access key IDs
            r"AKIA[A-Z0-9]{16}",
            r"|",
            // Bearer tokens
            r"[Bb]earer\s+[A-Za-z0-9\-._~+/]+=*",
            r"|",
            // password= or password: assignments
            r"(?:password|passwd|pwd)\s*[=:]\s*\S+",
            r")",
        ))
        .expect("redact_secrets regex")
    });
    re.replace_all(s, "[REDACTED]").into_owned()
}

/// Summarize an event kind without exposing sensitive data.
fn summarize_event_redacted(kind: &WatchEventKind) -> String {
    match kind {
        WatchEventKind::Error { module, message } => {
            format!(
                "Error in {}: {}",
                module,
                redact_secrets(truncate(message, 100))
            )
        }
        WatchEventKind::Panic { module, .. } => format!("PANIC in {}", module),
        WatchEventKind::InjectionDetected {
            pattern,
            confidence,
            source,
        } => {
            format!(
                "Injection pattern '{}' (conf={:.2}) from {}",
                redact_secrets(truncate(pattern, 30)),
                confidence,
                source
            )
        }
        WatchEventKind::CredentialLeak {
            credential_type,
            destination,
        } => {
            format!(
                "Credential leak: type={} dest={}",
                credential_type, destination
            )
        }
        WatchEventKind::ConstitutionViolation {
            rule,
            action_attempted,
        } => {
            format!(
                "Constitution violation: {} (attempted: {})",
                rule,
                truncate(action_attempted, 50)
            )
        }
        WatchEventKind::OutboundRequest {
            destination,
            bytes,
            status,
        } => {
            format!(
                "Outbound {} → {} bytes, HTTP {}",
                destination, bytes, status
            )
        }
        WatchEventKind::UnusualDestination {
            destination,
            reason,
        } => {
            format!("Unusual destination: {} ({})", destination, reason)
        }
        WatchEventKind::DataVolumeSpike {
            bytes_last_minute,
            baseline,
        } => {
            format!(
                "Data spike: {}B/min vs {}B baseline",
                bytes_last_minute, baseline
            )
        }
        WatchEventKind::PramanaValidationFailed {
            objective_id,
            decision,
            reason,
        } => {
            format!(
                "Pramana failed for {}: {} ({})",
                objective_id,
                truncate(decision, 40),
                reason
            )
        }
        WatchEventKind::BudgetAnomaly {
            objective_id,
            burn_rate,
            projected_overshoot,
        } => {
            format!(
                "Budget anomaly {}: burn={:.2}x overshoot={:.1}x",
                objective_id, burn_rate, projected_overshoot
            )
        }
        WatchEventKind::TaskRetriesExhausted {
            objective_id,
            task_id,
        } => {
            format!("Task retries exhausted: {}/{}", objective_id, task_id)
        }
        WatchEventKind::CacheConfidenceDrift {
            entry_id,
            old_confidence,
            new_confidence,
        } => {
            format!(
                "Cache drift {}: {:.2} → {:.2}",
                entry_id, old_confidence, new_confidence
            )
        }
        WatchEventKind::SuspiciousCacheEntry { entry_id, reason } => {
            format!("Suspicious cache entry {}: {}", entry_id, reason)
        }
        WatchEventKind::HighMemory { used_mb, total_mb } => {
            format!("High memory: {}MB / {}MB", used_mb, total_mb)
        }
        WatchEventKind::HighCpu { percent } => format!("High CPU: {:.1}%", percent),
        WatchEventKind::ComponentFailure { component, error } => {
            format!(
                "Component failure {}: {}",
                component,
                redact_secrets(truncate(error, 80))
            )
        }
    }
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        let end = s.floor_char_boundary(max);
        &s[..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watcher::events::{Severity, WatchEvent, WatchEventKind};

    #[test]
    fn test_cooldown_prevents_repeat() {
        let mut analyzer = Analyzer::new(600);
        analyzer.record_call("pea", 1000);
        assert!(analyzer.in_cooldown("pea", 1500)); // 500s < 600s
        assert!(!analyzer.in_cooldown("pea", 1700)); // 700s > 600s
    }

    #[test]
    fn test_build_prompt_redacts_and_contains_component() {
        let events = [WatchEvent::new(
            WatchEventKind::CredentialLeak {
                credential_type: "aws_key".into(),
                destination: "evil.com".into(),
            },
            Severity::Critical,
        )];
        let refs: Vec<&WatchEvent> = events.iter().collect();
        let prompt = Analyzer::build_prompt("security", &refs, 0.85);
        assert!(prompt.contains("security"));
        assert!(prompt.contains("0.85"));
        assert!(prompt.contains("Credential leak"));
        // Should NOT contain actual credential values
        assert!(!prompt.contains("sk-"));
    }

    #[test]
    fn test_redact_secrets_replaces_api_keys() {
        let input = "Error: API call failed with key sk-1234567890abcdef and token ghp_abcdefghijklmnopqrstuvwxyz0123456789";
        let redacted = super::redact_secrets(input);
        assert!(
            !redacted.contains("sk-1234567890"),
            "sk- key should be redacted"
        );
        assert!(!redacted.contains("ghp_"), "ghp_ token should be redacted");
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn test_redact_secrets_replaces_bearer_tokens() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.test";
        let redacted = super::redact_secrets(input);
        assert!(
            !redacted.contains("eyJhbGci"),
            "Bearer token should be redacted"
        );
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn test_redact_secrets_replaces_passwords() {
        let input = "config: password=supersecret123 host=localhost";
        let redacted = super::redact_secrets(input);
        assert!(
            !redacted.contains("supersecret123"),
            "password should be redacted"
        );
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn test_summarize_event_redacts_error_message() {
        let kind = WatchEventKind::Error {
            module: "auth".into(),
            message: "Failed with key sk-1234567890abcdef".into(),
        };
        let summary = super::summarize_event_redacted(&kind);
        assert!(
            !summary.contains("sk-1234567890"),
            "secret in error message should be redacted"
        );
        assert!(summary.contains("[REDACTED]"));
        assert!(summary.contains("auth"));
    }

    #[test]
    fn test_parse_verdict_direct_json() {
        let json = r#"{"severity":"CRITICAL","diagnosis":"credential leak detected","recommended_action":"pause","confidence":0.95}"#;
        let v = Analyzer::parse_verdict(json).unwrap();
        assert_eq!(v.recommended_action, "pause");
        assert!(v.confidence > 0.9);
    }

    #[test]
    fn test_parse_verdict_from_markdown() {
        let response = "Here's my analysis:\n```json\n{\"severity\":\"WARNING\",\"diagnosis\":\"minor issue\",\"recommended_action\":\"alert\",\"confidence\":0.6}\n```";
        let v = Analyzer::parse_verdict(response).unwrap();
        assert_eq!(v.recommended_action, "alert");
    }
}
