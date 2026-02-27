use std::collections::{HashMap, HashSet};

use crate::chain::circuit_breaker::{BreakerAction, BreakerRegistry};
use crate::chain::dsl::{ChainDef, ChainStep};
use crate::core::error::{NyayaError, Result};
use crate::runtime::host_functions::{AbilityRegistry, AbilityResult};
use crate::runtime::manifest::AgentManifest;
use crate::runtime::receipt::ToolReceipt;
use crate::security::constitution::ConstitutionEnforcer;

/// Result of executing a complete chain.
#[derive(Debug)]
pub struct ChainExecutionResult {
    /// Whether the chain completed successfully
    pub success: bool,
    /// Receipts from all executed steps
    pub receipts: Vec<ToolReceipt>,
    /// Final outputs from each step (keyed by output_key)
    pub outputs: HashMap<String, String>,
    /// Steps that were skipped (condition was false)
    pub skipped_steps: Vec<String>,
    /// Total execution time in milliseconds
    pub total_ms: u64,
}

/// Chain executor — runs chain definitions step by step,
/// calling abilities and collecting receipts.
pub struct ChainExecutor<'a> {
    ability_registry: &'a AbilityRegistry,
    manifest: &'a AgentManifest,
    breaker_registry: Option<&'a BreakerRegistry>,
    constitution: Option<&'a ConstitutionEnforcer>,
}

/// Validate that all required chain parameters are provided.
fn validate_params(chain: &ChainDef, params: &HashMap<String, String>) -> Result<()> {
    for param_def in &chain.params {
        if param_def.required
            && !params.contains_key(&param_def.name)
            && param_def.default.is_none()
        {
            return Err(NyayaError::Config(format!(
                "Missing required parameter: '{}'",
                param_def.name
            )));
        }
    }
    Ok(())
}

impl<'a> ChainExecutor<'a> {
    pub fn new(ability_registry: &'a AbilityRegistry, manifest: &'a AgentManifest) -> Self {
        Self {
            ability_registry,
            manifest,
            breaker_registry: None,
            constitution: None,
        }
    }

    /// Attach circuit breaker registry for safety checks before each step.
    pub fn with_breakers(mut self, breakers: &'a BreakerRegistry) -> Self {
        self.breaker_registry = Some(breakers);
        self
    }

    /// Attach constitution enforcer for ability-level checks before each step.
    pub fn with_constitution(mut self, constitution: &'a ConstitutionEnforcer) -> Self {
        self.constitution = Some(constitution);
        self
    }

    /// Execute a chain with the given parameters.
    pub fn run(
        &self,
        chain: &ChainDef,
        params: &HashMap<String, String>,
    ) -> Result<ChainExecutionResult> {
        let start = std::time::Instant::now();

        // Fix 4: Validate required params before execution
        validate_params(chain, params)?;

        // H7: Record execution event for frequency breaker tracking
        if let Some(breakers) = self.breaker_registry {
            breakers.record_execution(&chain.id);
        }

        let mut receipts = Vec::new();
        let mut outputs: HashMap<String, String> = HashMap::new();
        let mut skipped_steps = Vec::new();
        let mut had_failure = false;
        // Track on_failure jumps to detect cycles (step_idx -> set of target_idxs already jumped to)
        let mut failure_visited: HashSet<usize> = HashSet::new();

        // Fix 3: Use while loop with manual index for on_failure jumps
        let mut idx = 0;
        while idx < chain.steps.len() {
            let step = &chain.steps[idx];

            // Check condition if present
            if let Some(ref condition) = step.condition {
                if !condition.test(&outputs) {
                    skipped_steps.push(step.id.clone());
                    idx += 1;
                    continue;
                }
            }

            // C7: Constitution check — verify ability is allowed before execution
            if let Some(constitution) = self.constitution {
                let check = constitution.check_ability(&step.ability);
                if !check.allowed {
                    return Err(NyayaError::PermissionDenied(format!(
                        "Constitution blocked ability '{}' at step '{}': {}",
                        step.ability,
                        step.id,
                        check.reason.unwrap_or_else(|| "not allowed".to_string())
                    )));
                }
            }

            // C6: Circuit breaker check — evaluate safety rules before each step
            if let Some(breakers) = self.breaker_registry {
                let check = breakers.evaluate(&chain.id, &outputs, &step.ability);
                if !check.proceed {
                    let reasons: Vec<String> = check
                        .fired
                        .iter()
                        .filter(|f| f.action == BreakerAction::Abort)
                        .map(|f| f.reason.clone())
                        .collect();
                    return Err(NyayaError::Wasm(format!(
                        "Circuit breaker halted workflow at step '{}': {}",
                        step.id,
                        reasons.join("; ")
                    )));
                }
                // Log confirmed breakers (non-abort) as warnings
                for fired in &check.fired {
                    if fired.action == BreakerAction::Confirm {
                        tracing::warn!(
                            step = %step.id,
                            breaker = %fired.breaker_id,
                            "Circuit breaker CONFIRM: {}",
                            fired.reason
                        );
                    }
                }
            }

            // Resolve template arguments
            let resolved_args = ChainDef::resolve_args(&step.args, params, &outputs);

            // Execute the ability
            let input_json = serde_json::to_string(&resolved_args)
                .map_err(|e| NyayaError::Config(format!("Failed to serialize args: {}", e)))?;

            match self.execute_step(step, &input_json) {
                Ok(result) => {
                    // Store output if output_key is specified
                    if let Some(ref key) = step.output_key {
                        let output_str = String::from_utf8_lossy(&result.output).to_string();
                        outputs.insert(key.clone(), output_str);
                    }
                    receipts.push(result.receipt);
                }
                Err(e) => {
                    // Fix 3: Check for on_failure handler and jump to target step
                    if let Some(ref target_id) = step.on_failure {
                        outputs.insert(format!("{}_error", step.id), e.to_string());
                        had_failure = true;
                        // Find the target step index and jump to it
                        if let Some(target_idx) =
                            chain.steps.iter().position(|s| s.id == *target_id)
                        {
                            // Cycle detection: prevent infinite on_failure loops
                            if !failure_visited.insert(target_idx) {
                                return Err(NyayaError::Wasm(format!(
                                    "Workflow step '{}' on_failure cycle detected: step '{}' already visited in failure path",
                                    step.id, target_id
                                )));
                            }
                            idx = target_idx;
                            continue;
                        }
                        // Target not found — fall through to error
                    }
                    return Err(NyayaError::Wasm(format!(
                        "Workflow step '{}' failed: {}",
                        step.id, e
                    )));
                }
            }

            idx += 1;
        }

        // Fix 5: Success is false if any step triggered on_failure
        Ok(ChainExecutionResult {
            success: !had_failure,
            receipts,
            outputs,
            skipped_steps,
            total_ms: start.elapsed().as_millis() as u64,
        })
    }

    fn execute_step(&self, step: &ChainStep, input_json: &str) -> Result<AbilityResult> {
        self.ability_registry
            .execute_ability(self.manifest, &step.ability, input_json)
            .map_err(NyayaError::PermissionDenied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::dsl::ChainDef;
    use crate::runtime::receipt::ReceiptSigner;

    fn test_manifest() -> AgentManifest {
        AgentManifest {
            name: "test-agent".into(),
            version: "0.1.0".into(),
            description: "test".into(),
            permissions: vec![
                "data.fetch_url".into(),
                "notify.user".into(),
                "flow.stop".into(),
            ],
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

    #[test]
    fn test_execute_simple_chain() {
        let yaml = r#"
id: test_chain
name: Test Chain
description: A simple test chain
params:
  - name: city
    param_type: text
    description: City name
    required: true
steps:
  - id: fetch
    ability: data.fetch_url
    args:
      url: "https://api.weather.com/{{city}}"
    output_key: weather
  - id: notify
    ability: notify.user
    args:
      message: "Weather: {{weather}}"
"#;
        let chain = ChainDef::from_yaml(yaml).unwrap();
        let manifest = test_manifest();
        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let executor = ChainExecutor::new(&registry, &manifest);

        let params = HashMap::from([("city".into(), "NYC".into())]);
        let result = executor.run(&chain, &params).unwrap();

        assert!(result.success);
        assert_eq!(result.receipts.len(), 2);
        assert!(result.outputs.contains_key("weather"));
    }

    #[test]
    fn test_conditional_step_skipped() {
        let yaml = r#"
id: conditional_chain
name: Conditional Chain
description: Chain with a conditional step
params: []
steps:
  - id: always
    ability: flow.stop
    args: {}
    output_key: status
  - id: conditional
    ability: notify.user
    args:
      message: "should not run"
    condition:
      ref_key: missing_key
      op: is_not_empty
      value: ""
"#;
        let chain = ChainDef::from_yaml(yaml).unwrap();
        let manifest = test_manifest();
        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let executor = ChainExecutor::new(&registry, &manifest);

        let result = executor.run(&chain, &HashMap::new()).unwrap();
        assert!(result.success);
        assert_eq!(result.receipts.len(), 1); // Only the first step ran
        assert_eq!(result.skipped_steps, vec!["conditional"]);
    }

    #[test]
    fn test_permission_denied_aborts() {
        let yaml = r#"
id: denied_chain
name: Denied Chain
description: Chain that needs email permission
params: []
steps:
  - id: send
    ability: email.send
    args:
      to: "test@example.com"
"#;
        let chain = ChainDef::from_yaml(yaml).unwrap();
        let manifest = test_manifest(); // No email.send permission
        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let executor = ChainExecutor::new(&registry, &manifest);

        let result = executor.run(&chain, &HashMap::new());
        assert!(result.is_err());
    }
}
