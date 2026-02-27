//! Trigger engine — manages scheduled, event, and webhook triggers for agents.

use super::types::*;
use std::collections::HashMap;

/// Registered trigger with its owning agent.
#[derive(Debug, Clone)]
pub struct RegisteredTrigger {
    pub agent_id: String,
    pub trigger: TriggerDef,
}

/// Manages all triggers across all agents.
pub struct TriggerEngine {
    /// All registered triggers keyed by agent_id.
    triggers: HashMap<String, Vec<TriggerDef>>,
    /// Webhook path → (agent_id, chain, secret).
    webhook_routes: HashMap<String, WebhookRoute>,
    /// Channel triggers: (agent_id, ChannelTrigger).
    channel_triggers: Vec<(String, ChannelTrigger)>,
}

#[derive(Debug, Clone)]
pub struct WebhookRoute {
    pub agent_id: String,
    pub chain: String,
    pub secret: Option<String>,
    pub params: HashMap<String, String>,
}

impl TriggerEngine {
    pub fn new() -> Self {
        Self {
            triggers: HashMap::new(),
            webhook_routes: HashMap::new(),
            channel_triggers: Vec::new(),
        }
    }

    /// Register all triggers for an agent from its manifest.
    pub fn register_agent(&mut self, agent_id: &str, agent_triggers: &AgentTriggers) {
        let mut defs = Vec::new();

        for s in &agent_triggers.scheduled {
            defs.push(TriggerDef::Scheduled(s.clone()));
        }
        for e in &agent_triggers.events {
            defs.push(TriggerDef::Event(e.clone()));
        }
        for w in &agent_triggers.webhooks {
            let route_path = if w.path.starts_with('/') {
                w.path.clone()
            } else {
                format!("/hooks/{}/{}", agent_id, w.path)
            };
            self.webhook_routes.insert(
                route_path,
                WebhookRoute {
                    agent_id: agent_id.to_string(),
                    chain: w.chain.clone(),
                    secret: w.secret.clone(),
                    params: w.params.clone(),
                },
            );
            defs.push(TriggerDef::Webhook(w.clone()));
        }
        for c in &agent_triggers.channels {
            self.channel_triggers
                .push((agent_id.to_string(), c.clone()));
            defs.push(TriggerDef::Channel(c.clone()));
        }

        self.triggers.insert(agent_id.to_string(), defs);
    }

    /// Unregister all triggers for an agent.
    pub fn unregister_agent(&mut self, agent_id: &str) {
        self.triggers.remove(agent_id);
        self.webhook_routes.retain(|_, r| r.agent_id != agent_id);
        self.channel_triggers.retain(|(id, _)| id != agent_id);
    }

    /// Get all scheduled triggers across all agents.
    pub fn scheduled_triggers(&self) -> Vec<(String, ScheduledTrigger)> {
        let mut result = Vec::new();
        for (agent_id, defs) in &self.triggers {
            for def in defs {
                if let TriggerDef::Scheduled(s) = def {
                    result.push((agent_id.clone(), s.clone()));
                }
            }
        }
        result
    }

    /// Get all event triggers across all agents.
    pub fn event_triggers(&self) -> Vec<(String, EventTrigger)> {
        let mut result = Vec::new();
        for (agent_id, defs) in &self.triggers {
            for def in defs {
                if let TriggerDef::Event(e) = def {
                    result.push((agent_id.clone(), e.clone()));
                }
            }
        }
        result
    }

    /// Look up a webhook route by path.
    pub fn resolve_webhook(&self, path: &str) -> Option<&WebhookRoute> {
        self.webhook_routes.get(path)
    }

    /// Get all triggers for an agent.
    pub fn agent_triggers(&self, agent_id: &str) -> Option<&Vec<TriggerDef>> {
        self.triggers.get(agent_id)
    }

    /// Get total trigger count.
    pub fn total_count(&self) -> usize {
        self.triggers.values().map(|v| v.len()).sum()
    }

    /// Match an incoming channel message against registered channel triggers.
    /// Returns Vec of (agent_id, workflow_id, resolved_params).
    pub fn match_channel_message(
        &self,
        channel: &str,
        sender: &str,
        group: Option<&str>,
        message: &str,
        subject: Option<&str>,
    ) -> Vec<(String, String, HashMap<String, String>)> {
        let mut results = Vec::new();
        for (agent_id, trigger) in &self.channel_triggers {
            if trigger.channel != channel {
                continue;
            }
            if let Some(ref from) = trigger.from {
                if from != sender {
                    continue;
                }
            }
            if let Some(ref domain) = trigger.from_domain {
                // Dot-boundary check: ensure the domain match occurs at a proper
                // boundary (start of string, after '.', or after '@') to prevent
                // "evilbank.com" from matching "bank.com".
                if !sender.ends_with(domain) {
                    continue;
                }
                if sender.len() > domain.len() {
                    let boundary_char = sender.as_bytes()[sender.len() - domain.len() - 1];
                    if boundary_char != b'.' && boundary_char != b'@' {
                        continue;
                    }
                }
            }
            if let Some(ref tg) = trigger.group {
                match group {
                    Some(g) if g == tg => {}
                    _ => continue,
                }
            }
            if let Some(ref pat) = trigger.pattern {
                match regex::Regex::new(pat) {
                    Ok(re) => {
                        if !re.is_match(message) {
                            continue;
                        }
                    }
                    Err(_) => continue,
                }
            }
            if let Some(ref spat) = trigger.subject_pattern {
                match (subject, regex::Regex::new(spat)) {
                    (Some(subj), Ok(re)) => {
                        if !re.is_match(subj) {
                            continue;
                        }
                    }
                    _ => continue,
                }
            }
            // Resolve template params
            let mut resolved = HashMap::new();
            for (k, v) in &trigger.params {
                let val = v
                    .replace("{{message}}", message)
                    .replace("{{sender}}", sender)
                    .replace("{{group}}", group.unwrap_or(""))
                    .replace("{{subject}}", subject.unwrap_or(""));
                resolved.insert(k.clone(), val);
            }
            results.push((agent_id.clone(), trigger.workflow.clone(), resolved));
        }
        results
    }

    /// Check if an event matches any event trigger.
    pub fn match_event(
        &self,
        event_type: &str,
        properties: &HashMap<String, String>,
    ) -> Vec<(String, EventTrigger)> {
        let mut matches = Vec::new();
        for (agent_id, defs) in &self.triggers {
            for def in defs {
                if let TriggerDef::Event(e) = def {
                    if e.on.eq_ignore_ascii_case(event_type) {
                        let filter_match =
                            e.filter.iter().all(|(k, v)| properties.get(k) == Some(v));
                        if filter_match {
                            matches.push((agent_id.clone(), e.clone()));
                        }
                    }
                }
            }
        }
        matches
    }
}

impl Default for TriggerEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_scheduled_triggers() {
        let mut engine = TriggerEngine::new();
        let triggers = AgentTriggers {
            scheduled: vec![ScheduledTrigger {
                chain: "email-triage".into(),
                interval: "30m".into(),
                at: None,
                params: HashMap::new(),
            }],
            events: vec![],
            webhooks: vec![],
            channels: vec![],
        };
        engine.register_agent("email-bot", &triggers);
        assert_eq!(engine.scheduled_triggers().len(), 1);
        assert_eq!(engine.total_count(), 1);
    }

    #[test]
    fn test_register_event_triggers() {
        let mut engine = TriggerEngine::new();
        let triggers = AgentTriggers {
            scheduled: vec![],
            events: vec![EventTrigger {
                on: "DataChanged".into(),
                filter: [("namespace".into(), "gmail".into())].into(),
                chain: "process-email".into(),
                params: HashMap::new(),
            }],
            webhooks: vec![],
            channels: vec![],
        };
        engine.register_agent("email-bot", &triggers);
        assert_eq!(engine.event_triggers().len(), 1);
    }

    #[test]
    fn test_register_webhook_triggers() {
        let mut engine = TriggerEngine::new();
        let triggers = AgentTriggers {
            scheduled: vec![],
            events: vec![],
            webhooks: vec![WebhookTrigger {
                path: "new-mail".into(),
                chain: "process".into(),
                secret: Some("abc123".into()),
                params: HashMap::new(),
            }],
            channels: vec![],
        };
        engine.register_agent("email-bot", &triggers);
        let route = engine.resolve_webhook("/hooks/email-bot/new-mail");
        assert!(route.is_some());
        assert_eq!(route.unwrap().chain, "process");
    }

    #[test]
    fn test_unregister_agent() {
        let mut engine = TriggerEngine::new();
        let triggers = AgentTriggers {
            scheduled: vec![ScheduledTrigger {
                chain: "test".into(),
                interval: "1h".into(),
                at: None,
                params: HashMap::new(),
            }],
            events: vec![],
            webhooks: vec![WebhookTrigger {
                path: "hook".into(),
                chain: "test".into(),
                secret: None,
                params: HashMap::new(),
            }],
            channels: vec![],
        };
        engine.register_agent("bot", &triggers);
        assert_eq!(engine.total_count(), 2);
        engine.unregister_agent("bot");
        assert_eq!(engine.total_count(), 0);
        assert!(engine.resolve_webhook("/hooks/bot/hook").is_none());
    }

    #[test]
    fn test_match_event_with_filter() {
        let mut engine = TriggerEngine::new();
        let triggers = AgentTriggers {
            scheduled: vec![],
            events: vec![EventTrigger {
                on: "DataChanged".into(),
                filter: [("namespace".into(), "gmail".into())].into(),
                chain: "notify".into(),
                params: HashMap::new(),
            }],
            webhooks: vec![],
            channels: vec![],
        };
        engine.register_agent("bot", &triggers);

        let props = [("namespace".into(), "gmail".into())].into();
        let matches = engine.match_event("DataChanged", &props);
        assert_eq!(matches.len(), 1);

        let wrong_props = [("namespace".into(), "slack".into())].into();
        let no_match = engine.match_event("DataChanged", &wrong_props);
        assert_eq!(no_match.len(), 0);
    }

    #[test]
    fn test_match_event_case_insensitive() {
        let mut engine = TriggerEngine::new();
        let triggers = AgentTriggers {
            scheduled: vec![],
            events: vec![EventTrigger {
                on: "AlertRaised".into(),
                filter: HashMap::new(),
                chain: "handle".into(),
                params: HashMap::new(),
            }],
            webhooks: vec![],
            channels: vec![],
        };
        engine.register_agent("bot", &triggers);
        let matches = engine.match_event("alertraised", &HashMap::new());
        assert_eq!(matches.len(), 1);
    }

    fn make_channel_trigger(
        channel: &str,
        workflow: &str,
        from: Option<&str>,
        from_domain: Option<&str>,
        group: Option<&str>,
        pattern: Option<&str>,
        subject_pattern: Option<&str>,
        params: HashMap<String, String>,
    ) -> ChannelTrigger {
        ChannelTrigger {
            channel: channel.into(),
            from: from.map(|s| s.into()),
            from_domain: from_domain.map(|s| s.into()),
            group: group.map(|s| s.into()),
            pattern: pattern.map(|s| s.into()),
            subject_pattern: subject_pattern.map(|s| s.into()),
            workflow: workflow.into(),
            params,
            mode: TriggerMode::Realtime,
            poll_interval: None,
        }
    }

    #[test]
    fn test_match_channel_exact_contact() {
        let mut engine = TriggerEngine::new();
        let triggers = AgentTriggers {
            scheduled: vec![],
            events: vec![],
            webhooks: vec![],
            channels: vec![make_channel_trigger(
                "telegram",
                "handle-alice",
                Some("alice"),
                None,
                None,
                None,
                None,
                HashMap::new(),
            )],
        };
        engine.register_agent("bot", &triggers);

        let matches = engine.match_channel_message("telegram", "alice", None, "hello", None);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "bot");
        assert_eq!(matches[0].1, "handle-alice");

        // Different sender should not match
        let no = engine.match_channel_message("telegram", "bob", None, "hello", None);
        assert_eq!(no.len(), 0);
    }

    #[test]
    fn test_match_channel_pattern() {
        let mut engine = TriggerEngine::new();
        let triggers = AgentTriggers {
            scheduled: vec![],
            events: vec![],
            webhooks: vec![],
            channels: vec![make_channel_trigger(
                "email",
                "urgent-handler",
                None,
                None,
                None,
                Some("urgent.*deploy"),
                None,
                HashMap::new(),
            )],
        };
        engine.register_agent("bot", &triggers);

        let matches = engine.match_channel_message(
            "email",
            "anyone",
            None,
            "urgent need to deploy now",
            None,
        );
        assert_eq!(matches.len(), 1);

        let no = engine.match_channel_message("email", "anyone", None, "normal message", None);
        assert_eq!(no.len(), 0);
    }

    #[test]
    fn test_match_channel_group() {
        let mut engine = TriggerEngine::new();
        let triggers = AgentTriggers {
            scheduled: vec![],
            events: vec![],
            webhooks: vec![],
            channels: vec![make_channel_trigger(
                "telegram",
                "dev-workflow",
                None,
                None,
                Some("dev-team"),
                None,
                None,
                HashMap::new(),
            )],
        };
        engine.register_agent("bot", &triggers);

        let matches =
            engine.match_channel_message("telegram", "alice", Some("dev-team"), "msg", None);
        assert_eq!(matches.len(), 1);

        let no = engine.match_channel_message("telegram", "alice", Some("other-team"), "msg", None);
        assert_eq!(no.len(), 0);

        // No group provided should also not match
        let no2 = engine.match_channel_message("telegram", "alice", None, "msg", None);
        assert_eq!(no2.len(), 0);
    }

    #[test]
    fn test_match_channel_no_match() {
        let mut engine = TriggerEngine::new();
        let triggers = AgentTriggers {
            scheduled: vec![],
            events: vec![],
            webhooks: vec![],
            channels: vec![make_channel_trigger(
                "telegram",
                "wf",
                Some("alice"),
                Some("@example.com"),
                Some("dev"),
                Some("^urgent"),
                None,
                HashMap::new(),
            )],
        };
        engine.register_agent("bot", &triggers);

        // Wrong channel
        assert_eq!(
            engine
                .match_channel_message("email", "alice", Some("dev"), "urgent stuff", None)
                .len(),
            0
        );
        // Wrong sender
        assert_eq!(
            engine
                .match_channel_message("telegram", "bob", Some("dev"), "urgent stuff", None)
                .len(),
            0
        );
        // Wrong group
        assert_eq!(
            engine
                .match_channel_message("telegram", "alice", Some("ops"), "urgent stuff", None)
                .len(),
            0
        );
        // Pattern doesn't match
        assert_eq!(
            engine
                .match_channel_message("telegram", "alice", Some("dev"), "normal stuff", None)
                .len(),
            0
        );
    }

    #[test]
    fn test_template_resolution() {
        let mut engine = TriggerEngine::new();
        let mut params = HashMap::new();
        params.insert("body".into(), "{{message}}".into());
        params.insert("from".into(), "{{sender}}".into());
        params.insert("grp".into(), "{{group}}".into());
        params.insert("subj".into(), "{{subject}}".into());

        let triggers = AgentTriggers {
            scheduled: vec![],
            events: vec![],
            webhooks: vec![],
            channels: vec![make_channel_trigger(
                "email", "process", None, None, None, None, None, params,
            )],
        };
        engine.register_agent("bot", &triggers);

        let matches = engine.match_channel_message(
            "email",
            "alice@co.com",
            Some("eng"),
            "hello world",
            Some("Re: test"),
        );
        assert_eq!(matches.len(), 1);
        let resolved = &matches[0].2;
        assert_eq!(resolved.get("body").unwrap(), "hello world");
        assert_eq!(resolved.get("from").unwrap(), "alice@co.com");
        assert_eq!(resolved.get("grp").unwrap(), "eng");
        assert_eq!(resolved.get("subj").unwrap(), "Re: test");
    }

    #[test]
    fn test_from_domain_boundary_check() {
        let mut engine = TriggerEngine::new();
        let triggers = AgentTriggers {
            scheduled: vec![],
            events: vec![],
            webhooks: vec![],
            channels: vec![make_channel_trigger(
                "email",
                "handle-bank",
                None,
                Some("bank.com"),
                None,
                None,
                None,
                HashMap::new(),
            )],
        };
        engine.register_agent("bot", &triggers);

        // Legitimate subdomain match: "user@sub.bank.com" should match "bank.com"
        let matches =
            engine.match_channel_message("email", "user@sub.bank.com", None, "hello", None);
        assert_eq!(matches.len(), 1);

        // Exact domain match: "user@bank.com" should match "bank.com"
        let matches = engine.match_channel_message("email", "user@bank.com", None, "hello", None);
        assert_eq!(matches.len(), 1);

        // Spoofed domain: "evilbank.com" must NOT match "bank.com"
        let no = engine.match_channel_message("email", "user@evilbank.com", None, "hello", None);
        assert_eq!(
            no.len(),
            0,
            "evilbank.com must not match from_domain bank.com"
        );

        // Spoofed domain without @: "evilbank.com" must NOT match "bank.com"
        let no = engine.match_channel_message("email", "evilbank.com", None, "hello", None);
        assert_eq!(
            no.len(),
            0,
            "evilbank.com (bare) must not match from_domain bank.com"
        );

        // Exact domain as sender should match
        let matches = engine.match_channel_message("email", "bank.com", None, "hello", None);
        assert_eq!(matches.len(), 1);
    }
}
