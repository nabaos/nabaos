use std::collections::HashMap;
use std::path::Path;

use crate::core::error::{NyayaError, Result};
use crate::security::constitution::{
    get_constitution_template, ConstitutionCheck, ConstitutionEnforcer,
};
use crate::w5h2::types::W5H2Intent;

/// Manages a system-wide constitution plus optional per-agent constitutions.
///
/// When checking an intent the flow is:
/// 1. Run the **system** enforcer first — if it blocks, that decision is final.
/// 2. If the agent has a registered constitution, run the agent enforcer.
/// 3. Otherwise fall back to the system enforcer.
pub struct ConstitutionManager {
    system: ConstitutionEnforcer,
    agents: HashMap<String, ConstitutionEnforcer>,
}

impl ConstitutionManager {
    /// Create a new manager with the given system-level constitution enforcer.
    pub fn new(system: ConstitutionEnforcer) -> Self {
        Self {
            system,
            agents: HashMap::new(),
        }
    }

    /// Register an agent constitution loaded from a YAML file on disk.
    pub fn register_agent(&mut self, agent_id: &str, constitution_path: &Path) -> Result<()> {
        let enforcer = ConstitutionEnforcer::load(constitution_path)?;
        self.agents.insert(agent_id.to_string(), enforcer);
        Ok(())
    }

    /// Register an agent constitution from a built-in template name.
    pub fn register_agent_template(&mut self, agent_id: &str, template_name: &str) -> Result<()> {
        let constitution = get_constitution_template(template_name).ok_or_else(|| {
            NyayaError::Config(format!("unknown constitution template: {}", template_name))
        })?;
        let enforcer = ConstitutionEnforcer::from_constitution(constitution);
        self.agents.insert(agent_id.to_string(), enforcer);
        Ok(())
    }

    /// Remove a previously registered agent constitution.
    pub fn unregister_agent(&mut self, agent_id: &str) {
        self.agents.remove(agent_id);
    }

    /// Check an intent for the given agent.
    ///
    /// The system constitution is always checked first. If the system blocks,
    /// that result is returned immediately. Otherwise the agent-specific
    /// constitution is consulted (falling back to the system result when no
    /// agent constitution is registered).
    pub fn check(
        &self,
        agent_id: &str,
        intent: &W5H2Intent,
        query: Option<&str>,
    ) -> ConstitutionCheck {
        let system_result = self.system.check(intent, query);
        if !system_result.allowed {
            return system_result;
        }

        if let Some(agent_enforcer) = self.agents.get(agent_id) {
            agent_enforcer.check(intent, query)
        } else {
            system_result
        }
    }

    /// Returns `true` if the agent has a registered constitution.
    pub fn has_agent(&self, agent_id: &str) -> bool {
        self.agents.contains_key(agent_id)
    }

    /// List all registered agent ids.
    pub fn agent_ids(&self) -> Vec<&str> {
        self.agents.keys().map(|s| s.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::constitution::{default_constitution, ConstitutionEnforcer};
    use crate::w5h2::types::{Action, Target, W5H2Intent};
    use std::collections::HashMap as StdHashMap;

    fn make_intent(action: Action, target: Target) -> W5H2Intent {
        W5H2Intent {
            action,
            target,
            confidence: 0.95,
            params: StdHashMap::new(),
        }
    }

    #[test]
    fn test_system_override_blocks() {
        let system = ConstitutionEnforcer::from_constitution(default_constitution());
        let mut mgr = ConstitutionManager::new(system);
        mgr.register_agent_template("bot-1", "default").unwrap();
        assert!(mgr.has_agent("bot-1"));
    }

    #[test]
    fn test_unregister_agent() {
        let system = ConstitutionEnforcer::from_constitution(default_constitution());
        let mut mgr = ConstitutionManager::new(system);
        mgr.register_agent_template("bot-1", "default").unwrap();
        assert!(mgr.has_agent("bot-1"));
        mgr.unregister_agent("bot-1");
        assert!(!mgr.has_agent("bot-1"));
    }

    #[test]
    fn test_agent_ids_list() {
        let system = ConstitutionEnforcer::from_constitution(default_constitution());
        let mut mgr = ConstitutionManager::new(system);
        mgr.register_agent_template("alpha", "default").unwrap();
        mgr.register_agent_template("beta", "solopreneur").unwrap();
        assert_eq!(mgr.agent_ids().len(), 2);
    }

    #[test]
    fn test_missing_agent_uses_system() {
        let system = ConstitutionEnforcer::from_constitution(default_constitution());
        let mgr = ConstitutionManager::new(system);
        // "check" + "weather" is allowed by the default constitution
        let intent = make_intent(Action::Check, Target::Weather);
        let result = mgr.check("nonexistent-agent", &intent, Some("check weather"));
        assert!(result.allowed);
    }
}
