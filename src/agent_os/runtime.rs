use std::collections::HashMap;
use std::path::PathBuf;

use super::agent_constitution::ConstitutionManager;
use super::message_bus::{AgentEvent, MessageBus};
use super::package::PackageMetadata;
use super::types::{AgentId, AgentState, ResourceUsage};
use crate::core::error::{NyayaError, Result};

/// A running (or registered) agent instance in the runtime.
#[derive(Debug)]
pub struct AgentInstance {
    pub id: AgentId,
    pub metadata: PackageMetadata,
    pub state: AgentState,
    pub data_dir: PathBuf,
    pub resource_usage: ResourceUsage,
}

/// The agent runtime — manages agent instances, intent routing, and lifecycle.
pub struct AgentRuntime {
    instances: HashMap<AgentId, AgentInstance>,
    bus: MessageBus,
    #[allow(dead_code)]
    constitutions: ConstitutionManager,
}

impl AgentRuntime {
    /// Create a new runtime with the given constitution manager and message bus.
    pub fn new(constitutions: ConstitutionManager, bus: MessageBus) -> Self {
        Self {
            instances: HashMap::new(),
            bus,
            constitutions,
        }
    }

    /// Register an agent instance (initially in Stopped state).
    pub fn register(
        &mut self,
        id: AgentId,
        metadata: PackageMetadata,
        data_dir: PathBuf,
    ) -> Result<()> {
        if self.instances.contains_key(&id) {
            return Err(NyayaError::Config(format!(
                "Agent '{}' is already registered",
                id
            )));
        }
        let instance = AgentInstance {
            id: id.clone(),
            metadata,
            state: AgentState::Stopped,
            data_dir,
            resource_usage: ResourceUsage::default(),
        };
        self.instances.insert(id, instance);
        Ok(())
    }

    /// Start an agent. Errors if the agent is disabled or not found.
    pub fn start(&mut self, agent_id: &str) -> Result<()> {
        let instance = self
            .instances
            .get_mut(agent_id)
            .ok_or_else(|| NyayaError::Config(format!("Agent '{}' not found", agent_id)))?;

        if instance.state == AgentState::Disabled {
            return Err(NyayaError::Config(format!(
                "Agent '{}' is disabled and cannot be started",
                agent_id
            )));
        }

        instance.state = AgentState::Running;
        self.bus.publish(AgentEvent::AgentStarted {
            agent_id: agent_id.to_string(),
        });
        Ok(())
    }

    /// Stop a running agent.
    pub fn stop(&mut self, agent_id: &str) -> Result<()> {
        let instance = self
            .instances
            .get_mut(agent_id)
            .ok_or_else(|| NyayaError::Config(format!("Agent '{}' not found", agent_id)))?;

        instance.state = AgentState::Stopped;
        self.bus.publish(AgentEvent::AgentStopped {
            agent_id: agent_id.to_string(),
        });
        Ok(())
    }

    /// Resolve an intent to the best matching running agent.
    /// Picks the running agent whose IntentFilter matches and has the highest priority.
    pub fn resolve_intent(&self, action: &str, target: &str) -> Option<&AgentInstance> {
        let mut best: Option<(&AgentInstance, i32)> = None;

        for instance in self.instances.values() {
            if instance.state != AgentState::Running {
                continue;
            }
            for filter in &instance.metadata.intent_filters {
                if filter.matches(action, target) {
                    let priority = filter.priority;
                    if best.is_none() || priority > best.unwrap().1 {
                        best = Some((instance, priority));
                    }
                }
            }
        }

        best.map(|(inst, _)| inst)
    }

    /// Get a reference to an agent instance by ID.
    pub fn get(&self, agent_id: &str) -> Option<&AgentInstance> {
        self.instances.get(agent_id)
    }

    /// List all agents with their current states.
    pub fn list(&self) -> Vec<(&AgentId, AgentState)> {
        self.instances
            .iter()
            .map(|(id, inst)| (id, inst.state))
            .collect()
    }

    /// Get a reference to the message bus.
    pub fn bus(&self) -> &MessageBus {
        &self.bus
    }

    /// Count running agents.
    pub fn running_count(&self) -> usize {
        self.instances
            .values()
            .filter(|inst| inst.state == AgentState::Running)
            .count()
    }

    /// Check if the agent is within its resource quota.
    /// Returns Ok(()) if within limits, Err if exceeded.
    pub fn check_quota(&self, agent_id: &str) -> Result<()> {
        let instance = self
            .instances
            .get(agent_id)
            .ok_or_else(|| NyayaError::Config(format!("Agent '{}' not found", agent_id)))?;

        let limits = instance
            .metadata
            .resources
            .as_ref()
            .cloned()
            .unwrap_or_default();

        let usage = &instance.resource_usage;

        if usage.fuel_consumed > limits.max_fuel {
            return Err(NyayaError::PermissionDenied(format!(
                "Agent '{}' exceeded fuel limit ({} > {})",
                agent_id, usage.fuel_consumed, limits.max_fuel
            )));
        }

        if usage.api_calls_this_hour > limits.max_api_calls_per_hour {
            return Err(NyayaError::PermissionDenied(format!(
                "Agent '{}' exceeded API call limit ({} > {})",
                agent_id, usage.api_calls_this_hour, limits.max_api_calls_per_hour
            )));
        }

        let peak_mb = (usage.peak_memory_bytes / (1024 * 1024)) as u32;
        if peak_mb > limits.max_memory_mb {
            return Err(NyayaError::PermissionDenied(format!(
                "Agent '{}' exceeded memory limit ({}MB > {}MB)",
                agent_id, peak_mb, limits.max_memory_mb
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_os::types::{IntentFilter, ResourceLimits};
    use crate::security::constitution::{default_constitution, ConstitutionEnforcer};

    fn test_bus() -> MessageBus {
        MessageBus::new()
    }

    fn test_constitutions() -> ConstitutionManager {
        ConstitutionManager::new(ConstitutionEnforcer::from_constitution(
            default_constitution(),
        ))
    }

    fn test_metadata(name: &str, filters: Vec<IntentFilter>) -> PackageMetadata {
        PackageMetadata {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: String::new(),
            author: String::new(),
            signature: String::new(),
            intent_filters: filters,
            permissions: vec![],
            resources: None,
            background: false,
            subscriptions: vec![],
            data_namespace: None,
            triggers: Default::default(),
        }
    }

    #[test]
    fn test_register_and_start() {
        let mut rt = AgentRuntime::new(test_constitutions(), test_bus());
        let meta = test_metadata("agent-a", vec![]);
        rt.register("agent-a".into(), meta, PathBuf::from("/tmp/a"))
            .unwrap();

        assert_eq!(rt.get("agent-a").unwrap().state, AgentState::Stopped);
        rt.start("agent-a").unwrap();
        assert_eq!(rt.get("agent-a").unwrap().state, AgentState::Running);
        assert_eq!(rt.running_count(), 1);
    }

    #[test]
    fn test_stop() {
        let mut rt = AgentRuntime::new(test_constitutions(), test_bus());
        let meta = test_metadata("agent-a", vec![]);
        rt.register("agent-a".into(), meta, PathBuf::from("/tmp/a"))
            .unwrap();
        rt.start("agent-a").unwrap();
        rt.stop("agent-a").unwrap();
        assert_eq!(rt.get("agent-a").unwrap().state, AgentState::Stopped);
        assert_eq!(rt.running_count(), 0);
    }

    #[test]
    fn test_intent_routing_by_priority() {
        let mut rt = AgentRuntime::new(test_constitutions(), test_bus());

        let low = test_metadata(
            "low-priority",
            vec![IntentFilter {
                actions: vec!["check".into()],
                targets: vec!["email".into()],
                priority: 1,
            }],
        );
        let high = test_metadata(
            "high-priority",
            vec![IntentFilter {
                actions: vec!["check".into()],
                targets: vec!["email".into()],
                priority: 10,
            }],
        );

        rt.register("low-priority".into(), low, PathBuf::from("/tmp/l"))
            .unwrap();
        rt.register("high-priority".into(), high, PathBuf::from("/tmp/h"))
            .unwrap();
        rt.start("low-priority").unwrap();
        rt.start("high-priority").unwrap();

        let resolved = rt.resolve_intent("check", "email").unwrap();
        assert_eq!(resolved.id, "high-priority");
    }

    #[test]
    fn test_stopped_agent_skipped_in_routing() {
        let mut rt = AgentRuntime::new(test_constitutions(), test_bus());

        let meta = test_metadata(
            "agent-a",
            vec![IntentFilter {
                actions: vec!["check".into()],
                targets: vec!["weather".into()],
                priority: 5,
            }],
        );
        rt.register("agent-a".into(), meta, PathBuf::from("/tmp/a"))
            .unwrap();
        // Not started — should not be resolved
        assert!(rt.resolve_intent("check", "weather").is_none());
    }

    #[test]
    fn test_disabled_cannot_start() {
        let mut rt = AgentRuntime::new(test_constitutions(), test_bus());
        let meta = test_metadata("agent-a", vec![]);
        rt.register("agent-a".into(), meta, PathBuf::from("/tmp/a"))
            .unwrap();

        // Manually disable
        rt.instances.get_mut("agent-a").unwrap().state = AgentState::Disabled;

        let result = rt.start("agent-a");
        assert!(result.is_err());
    }

    #[test]
    fn test_list() {
        let mut rt = AgentRuntime::new(test_constitutions(), test_bus());
        let meta_a = test_metadata("agent-a", vec![]);
        let meta_b = test_metadata("agent-b", vec![]);
        rt.register("agent-a".into(), meta_a, PathBuf::from("/tmp/a"))
            .unwrap();
        rt.register("agent-b".into(), meta_b, PathBuf::from("/tmp/b"))
            .unwrap();

        let all = rt.list();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_check_quota_ok() {
        let mut rt = AgentRuntime::new(test_constitutions(), test_bus());
        let meta = test_metadata("agent-a", vec![]);
        rt.register("agent-a".into(), meta, PathBuf::from("/tmp/a"))
            .unwrap();
        assert!(rt.check_quota("agent-a").is_ok());
    }

    #[test]
    fn test_check_quota_exceeded() {
        let mut rt = AgentRuntime::new(test_constitutions(), test_bus());
        let mut meta = test_metadata("agent-a", vec![]);
        meta.resources = Some(ResourceLimits {
            max_fuel: 100,
            max_memory_mb: 128,
            max_api_calls_per_hour: 10,
        });
        rt.register("agent-a".into(), meta, PathBuf::from("/tmp/a"))
            .unwrap();

        // Exceed fuel
        rt.instances
            .get_mut("agent-a")
            .unwrap()
            .resource_usage
            .fuel_consumed = 200;
        assert!(rt.check_quota("agent-a").is_err());
    }
}
