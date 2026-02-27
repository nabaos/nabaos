use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// Severity level for alerts published on the bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

/// Events that flow through the agent message bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentEvent {
    AgentStarted {
        agent_id: String,
    },
    AgentStopped {
        agent_id: String,
    },
    IntentResolved {
        agent_id: String,
        action: String,
        target: String,
    },
    AlertRaised {
        source: String,
        severity: AlertSeverity,
        message: String,
    },
    DataChanged {
        agent_id: String,
        namespace: String,
        key: String,
    },
    PermissionRequested {
        agent_id: String,
        permission: String,
    },
    Custom {
        event_type: String,
        agent_id: String,
        payload: String,
    },
}

impl AgentEvent {
    /// Return the event type as a string for trigger matching.
    pub fn event_type(&self) -> String {
        match self {
            AgentEvent::AgentStarted { .. } => "agent.started".to_string(),
            AgentEvent::AgentStopped { .. } => "agent.stopped".to_string(),
            AgentEvent::IntentResolved { .. } => "intent.resolved".to_string(),
            AgentEvent::AlertRaised { .. } => "alert.raised".to_string(),
            AgentEvent::DataChanged { .. } => "data.changed".to_string(),
            AgentEvent::PermissionRequested { .. } => "permission.requested".to_string(),
            AgentEvent::Custom { event_type, .. } => event_type.clone(),
        }
    }

    /// Extract event properties as key-value pairs for trigger filter matching.
    pub fn properties(&self) -> std::collections::HashMap<String, String> {
        let mut props = std::collections::HashMap::new();
        match self {
            AgentEvent::AgentStarted { agent_id } | AgentEvent::AgentStopped { agent_id } => {
                props.insert("agent_id".to_string(), agent_id.clone());
            }
            AgentEvent::IntentResolved {
                agent_id,
                action,
                target,
            } => {
                props.insert("agent_id".to_string(), agent_id.clone());
                props.insert("action".to_string(), action.clone());
                props.insert("target".to_string(), target.clone());
            }
            AgentEvent::AlertRaised {
                source,
                severity,
                message,
            } => {
                props.insert("source".to_string(), source.clone());
                props.insert("severity".to_string(), format!("{:?}", severity));
                props.insert("message".to_string(), message.clone());
            }
            AgentEvent::DataChanged {
                agent_id,
                namespace,
                key,
            } => {
                props.insert("agent_id".to_string(), agent_id.clone());
                props.insert("namespace".to_string(), namespace.clone());
                props.insert("key".to_string(), key.clone());
            }
            AgentEvent::PermissionRequested {
                agent_id,
                permission,
            } => {
                props.insert("agent_id".to_string(), agent_id.clone());
                props.insert("permission".to_string(), permission.clone());
            }
            AgentEvent::Custom {
                event_type,
                agent_id,
                payload,
            } => {
                props.insert("event_type".to_string(), event_type.clone());
                props.insert("agent_id".to_string(), agent_id.clone());
                props.insert("payload".to_string(), payload.clone());
            }
        }
        props
    }
}

/// A broadcast-based message bus for agent events.
///
/// Cloning a `MessageBus` gives another handle to the *same* underlying
/// channel, so all clones share subscribers.
#[derive(Clone)]
pub struct MessageBus {
    sender: broadcast::Sender<AgentEvent>,
    /// Buffer of events published since last drain, for trigger dispatch.
    pending_events: Vec<AgentEvent>,
}

impl MessageBus {
    /// Create a new message bus with the default capacity (256).
    pub fn new() -> Self {
        Self::with_capacity(256)
    }

    /// Create a new message bus with the given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            pending_events: Vec::new(),
        }
    }

    /// Publish an event to all current subscribers.
    /// Returns the number of receivers that will receive the event.
    pub fn publish(&mut self, event: AgentEvent) -> usize {
        self.pending_events.push(event.clone());
        // send returns Err only when there are zero receivers — that is fine.
        self.sender.send(event).unwrap_or(0)
    }

    /// Drain all pending events, returning them and clearing the buffer.
    pub fn drain_events(&mut self) -> Vec<AgentEvent> {
        std::mem::take(&mut self.pending_events)
    }

    /// Subscribe to the bus. Returns a receiver that will get all future events.
    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.sender.subscribe()
    }

    /// Number of active subscribers (receivers).
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for MessageBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_publish_subscribe() {
        let mut bus = MessageBus::new();
        let mut rx = bus.subscribe();

        let event = AgentEvent::AgentStarted {
            agent_id: "agent-1".into(),
        };
        let count = bus.publish(event);
        assert_eq!(count, 1);

        let received = rx.recv().await.unwrap();
        match received {
            AgentEvent::AgentStarted { agent_id } => assert_eq!(agent_id, "agent-1"),
            _ => panic!("unexpected event variant"),
        }
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let mut bus = MessageBus::new();
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();

        assert_eq!(bus.subscriber_count(), 2);

        let event = AgentEvent::AlertRaised {
            source: "system".into(),
            severity: AlertSeverity::Warning,
            message: "test".into(),
        };
        let count = bus.publish(event);
        assert_eq!(count, 2);

        let e1 = rx1.recv().await.unwrap();
        let e2 = rx2.recv().await.unwrap();
        match (&e1, &e2) {
            (
                AgentEvent::AlertRaised { message: m1, .. },
                AgentEvent::AlertRaised { message: m2, .. },
            ) => {
                assert_eq!(m1, "test");
                assert_eq!(m2, "test");
            }
            _ => panic!("unexpected event variants"),
        }
    }

    #[test]
    fn test_publish_no_subscribers() {
        let mut bus = MessageBus::new();
        // No subscribers — publish should return 0
        let count = bus.publish(AgentEvent::AgentStopped {
            agent_id: "gone".into(),
        });
        assert_eq!(count, 0);
    }

    #[test]
    fn test_event_serialization() {
        let event = AgentEvent::IntentResolved {
            agent_id: "a1".into(),
            action: "check".into(),
            target: "weather".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: AgentEvent = serde_json::from_str(&json).unwrap();
        match deserialized {
            AgentEvent::IntentResolved {
                agent_id,
                action,
                target,
            } => {
                assert_eq!(agent_id, "a1");
                assert_eq!(action, "check");
                assert_eq!(target, "weather");
            }
            _ => panic!("roundtrip produced wrong variant"),
        }
    }

    #[test]
    fn test_drain_events_empty() {
        let mut bus = MessageBus::new();
        let events = bus.drain_events();
        assert!(events.is_empty());
    }

    #[test]
    fn test_drain_events_returns_and_clears() {
        let mut bus = MessageBus::new();
        bus.publish(AgentEvent::AgentStarted {
            agent_id: "test".to_string(),
        });
        bus.publish(AgentEvent::AgentStopped {
            agent_id: "test".to_string(),
        });

        let events = bus.drain_events();
        assert_eq!(events.len(), 2);

        // Second drain should be empty
        let events2 = bus.drain_events();
        assert!(events2.is_empty());
    }

    #[test]
    fn test_event_type_strings() {
        assert_eq!(
            AgentEvent::AgentStarted {
                agent_id: "a".to_string()
            }
            .event_type(),
            "agent.started"
        );
        assert_eq!(
            AgentEvent::AgentStopped {
                agent_id: "a".to_string()
            }
            .event_type(),
            "agent.stopped"
        );
        assert_eq!(
            AgentEvent::IntentResolved {
                agent_id: "a".to_string(),
                action: "act".to_string(),
                target: "t".to_string()
            }
            .event_type(),
            "intent.resolved"
        );
        assert_eq!(
            AgentEvent::AlertRaised {
                source: "s".to_string(),
                severity: AlertSeverity::Info,
                message: "m".to_string()
            }
            .event_type(),
            "alert.raised"
        );
        assert_eq!(
            AgentEvent::DataChanged {
                agent_id: "a".to_string(),
                namespace: "ns".to_string(),
                key: "k".to_string()
            }
            .event_type(),
            "data.changed"
        );
        assert_eq!(
            AgentEvent::PermissionRequested {
                agent_id: "a".to_string(),
                permission: "p".to_string()
            }
            .event_type(),
            "permission.requested"
        );
        assert_eq!(
            AgentEvent::Custom {
                event_type: "custom.test".to_string(),
                agent_id: "a".to_string(),
                payload: "{}".to_string()
            }
            .event_type(),
            "custom.test"
        );
    }

    #[test]
    fn test_event_properties() {
        let event = AgentEvent::AlertRaised {
            source: "bot1".to_string(),
            severity: AlertSeverity::Critical,
            message: "timeout".to_string(),
        };
        let props = event.properties();
        assert_eq!(props.get("source").unwrap(), "bot1");
        assert_eq!(props.get("message").unwrap(), "timeout");

        let event2 = AgentEvent::DataChanged {
            agent_id: "a1".to_string(),
            namespace: "ns".to_string(),
            key: "k1".to_string(),
        };
        let props2 = event2.properties();
        assert_eq!(props2.get("agent_id").unwrap(), "a1");
        assert_eq!(props2.get("namespace").unwrap(), "ns");
        assert_eq!(props2.get("key").unwrap(), "k1");
    }
}
