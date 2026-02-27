use std::collections::HashMap;

use crate::chain::circuit_breaker::BreakerRegistry;
use crate::chain::dsl::{ChainDef, ChainStep};
use crate::chain::executor::{ChainExecutionResult, ChainExecutor};
use crate::core::error::Result;
use crate::runtime::host_functions::AbilityRegistry;
use crate::runtime::manifest::AgentManifest;
use crate::security::constitution::ConstitutionEnforcer;

use serde::{Deserialize, Serialize};

/// Configuration for autonomous plan-execute-review loops.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomousConfig {
    /// Maximum number of plan-execute-review iterations (default: 5).
    pub max_iterations: u32,
    /// Total timeout in seconds (default: 300 = 5 minutes).
    pub timeout_secs: u64,
    /// Maximum estimated cost in cents (default: 500 = $5.00).
    pub max_cost_cents: u64,
}

impl Default for AutonomousConfig {
    fn default() -> Self {
        Self {
            max_iterations: 5,
            timeout_secs: 300,
            max_cost_cents: 500,
        }
    }
}

/// Result of an autonomous execution run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomousResult {
    /// Whether the goal was achieved.
    pub success: bool,
    /// Number of plan-execute-review iterations completed.
    pub iterations: u32,
    /// Total wall-clock time in milliseconds.
    pub total_ms: u64,
    /// Final output from the last successful chain execution.
    pub final_output: HashMap<String, String>,
    /// Total number of individual chain steps executed across all iterations.
    pub steps_executed: usize,
    /// Estimated cost in cents across all iterations.
    pub cost_estimate_cents: u64,
}

/// Autonomous executor that runs a plan-execute-review loop to achieve a goal.
///
/// Flow:
/// 1. **Plan**: Build a chain definition from the goal and available abilities.
/// 2. **Execute**: Run the planned chain via `ChainExecutor`.
/// 3. **Review**: Evaluate whether the goal has been met.
/// 4. Repeat up to `max_iterations` or until timeout / cost cap.
///
/// Security: Constitution checks are applied on each planned step. Iteration
/// limits, cost caps, and timeouts prevent runaway execution.
pub struct AutonomousExecutor {
    config: AutonomousConfig,
}

impl AutonomousExecutor {
    /// Create a new executor with the given configuration.
    pub fn new(config: AutonomousConfig) -> Self {
        Self { config }
    }

    /// Create a new executor with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(AutonomousConfig::default())
    }

    /// Plan a chain from a goal description and available abilities.
    ///
    /// Since we don't have direct LLM integration in this module, we build a
    /// chain with a single `deep.delegate` step that passes the goal. The LLM
    /// backend will interpret the goal and produce a result.
    pub fn plan_chain(&self, goal: &str, available_abilities: &[String]) -> ChainDef {
        let abilities_csv = available_abilities.join(", ");

        let mut args = HashMap::new();
        args.insert("task".into(), goal.to_string());
        args.insert(
            "context".into(),
            format!(
                "Autonomous execution. Available abilities: [{}]. Achieve the goal in a single step.",
                abilities_csv
            ),
        );
        args.insert("backend".into(), "auto".into());

        ChainDef {
            id: format!("auto_{}", now_ms()),
            name: format!("Autonomous: {}", truncate(goal, 60)),
            description: format!(
                "Auto-planned workflow for goal: {}. Available: [{}]",
                goal, abilities_csv
            ),
            params: vec![],
            steps: vec![ChainStep {
                id: "plan_and_execute".into(),
                ability: "deep.delegate".into(),
                args,
                output_key: Some("result".into()),
                condition: None,
                on_failure: None,
            }],
        }
    }

    /// Review whether the goal has been achieved based on chain execution results.
    ///
    /// Heuristic: goal is met if the chain succeeded and produced non-empty output.
    pub fn review_result(&self, _goal: &str, result: &ChainExecutionResult) -> bool {
        if !result.success {
            return false;
        }
        // Consider the goal met if there is at least one non-empty output value
        result
            .outputs
            .values()
            .any(|v| !v.is_empty() && v != "stopped" && v != "{}")
    }

    /// Execute the autonomous plan-execute-review loop.
    ///
    /// Returns an `AutonomousResult` summarizing all iterations.
    pub fn execute(
        &self,
        goal: &str,
        ability_registry: &AbilityRegistry,
        manifest: &AgentManifest,
        breakers: Option<&BreakerRegistry>,
        constitution: Option<&ConstitutionEnforcer>,
    ) -> Result<AutonomousResult> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(self.config.timeout_secs);

        let available_abilities: Vec<String> = ability_registry
            .list_abilities()
            .iter()
            .map(|spec| spec.name.clone())
            .collect();

        let mut total_steps = 0usize;
        let mut cost_estimate_cents = 0u64;
        let mut last_outputs = HashMap::new();
        let mut iteration = 0u32;
        let mut succeeded = false;

        while iteration < self.config.max_iterations {
            // Check timeout
            if start.elapsed() >= timeout {
                tracing::warn!(
                    goal = %goal,
                    iteration = iteration,
                    "Autonomous execution timed out after {}s",
                    self.config.timeout_secs
                );
                break;
            }

            // Check cost cap
            if cost_estimate_cents >= self.config.max_cost_cents {
                tracing::warn!(
                    goal = %goal,
                    iteration = iteration,
                    cost_cents = cost_estimate_cents,
                    "Autonomous execution hit cost cap ({}c)",
                    self.config.max_cost_cents
                );
                break;
            }

            iteration += 1;
            tracing::info!(
                goal = %goal,
                iteration = iteration,
                "Autonomous iteration {}/{}",
                iteration,
                self.config.max_iterations
            );

            // Step 1: Plan
            let chain = self.plan_chain(goal, &available_abilities);

            // Step 2: Execute
            let mut executor = ChainExecutor::new(ability_registry, manifest);
            if let Some(b) = breakers {
                executor = executor.with_breakers(b);
            }
            if let Some(c) = constitution {
                executor = executor.with_constitution(c);
            }

            let chain_result = match executor.run(&chain, &HashMap::new()) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        goal = %goal,
                        iteration = iteration,
                        error = %e,
                        "Autonomous workflow execution failed"
                    );
                    // Estimate cost for failed attempt (1 cent per step attempted)
                    cost_estimate_cents += 1;
                    continue;
                }
            };

            total_steps += chain_result.receipts.len();
            // Rough cost estimate: 5 cents per deep.delegate call
            cost_estimate_cents += 5 * chain_result.receipts.len() as u64;
            last_outputs = chain_result.outputs.clone();

            // Step 3: Review
            if self.review_result(goal, &chain_result) {
                succeeded = true;
                tracing::info!(
                    goal = %goal,
                    iteration = iteration,
                    "Autonomous goal achieved"
                );
                break;
            }

            tracing::info!(
                goal = %goal,
                iteration = iteration,
                "Goal not yet met, retrying..."
            );
        }

        Ok(AutonomousResult {
            success: succeeded,
            iterations: iteration,
            total_ms: start.elapsed().as_millis() as u64,
            final_output: last_outputs,
            steps_executed: total_steps,
            cost_estimate_cents,
        })
    }
}

/// Current time in milliseconds (for unique chain IDs).
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Truncate a string to at most `max_len` characters, appending "..." if truncated.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.min(s.len())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::receipt::ReceiptSigner;

    #[test]
    fn test_config_defaults() {
        let config = AutonomousConfig::default();
        assert_eq!(config.max_iterations, 5);
        assert_eq!(config.timeout_secs, 300);
        assert_eq!(config.max_cost_cents, 500);
    }

    #[test]
    fn test_config_custom() {
        let config = AutonomousConfig {
            max_iterations: 3,
            timeout_secs: 60,
            max_cost_cents: 100,
        };
        assert_eq!(config.max_iterations, 3);
        assert_eq!(config.timeout_secs, 60);
        assert_eq!(config.max_cost_cents, 100);
    }

    #[test]
    fn test_plan_chain_structure() {
        let executor = AutonomousExecutor::with_defaults();
        let abilities = vec!["data.fetch_url".into(), "nlp.summarize".into()];
        let chain = executor.plan_chain("Summarize the news", &abilities);

        assert!(chain.id.starts_with("auto_"));
        assert_eq!(chain.steps.len(), 1);
        assert_eq!(chain.steps[0].ability, "deep.delegate");
        assert_eq!(chain.steps[0].args["task"], "Summarize the news");
        assert!(chain.steps[0].args["context"].contains("data.fetch_url"));
        assert!(chain.steps[0].args["context"].contains("nlp.summarize"));
        assert_eq!(chain.steps[0].output_key, Some("result".into()));
    }

    #[test]
    fn test_review_result_success() {
        let executor = AutonomousExecutor::with_defaults();

        let result = ChainExecutionResult {
            success: true,
            receipts: vec![],
            outputs: HashMap::from([("result".into(), "Here is the summary.".into())]),
            skipped_steps: vec![],
            total_ms: 100,
        };
        assert!(executor.review_result("Summarize news", &result));
    }

    #[test]
    fn test_review_result_failure() {
        let executor = AutonomousExecutor::with_defaults();

        let result = ChainExecutionResult {
            success: false,
            receipts: vec![],
            outputs: HashMap::new(),
            skipped_steps: vec![],
            total_ms: 50,
        };
        assert!(!executor.review_result("Summarize news", &result));
    }

    #[test]
    fn test_review_result_empty_output() {
        let executor = AutonomousExecutor::with_defaults();

        let result = ChainExecutionResult {
            success: true,
            receipts: vec![],
            outputs: HashMap::from([("result".into(), "".into())]),
            skipped_steps: vec![],
            total_ms: 50,
        };
        assert!(!executor.review_result("Summarize news", &result));
    }

    #[test]
    fn test_iteration_limit_respected() {
        // Use a config with max_iterations = 1 and a manifest that has deep.delegate permission
        let config = AutonomousConfig {
            max_iterations: 1,
            timeout_secs: 10,
            max_cost_cents: 1000,
        };
        let executor = AutonomousExecutor::new(config);
        let signer = ReceiptSigner::generate();
        let registry = AbilityRegistry::new(signer);
        let manifest = AgentManifest {
            name: "test-agent".into(),
            version: "0.1.0".into(),
            description: "test".into(),
            permissions: vec!["deep.delegate".into()],
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
        };

        let result = executor
            .execute("test goal", &registry, &manifest, None, None)
            .unwrap();

        // Should complete in exactly 1 iteration (may or may not succeed, but limited)
        assert!(result.iterations <= 1);
    }

    #[test]
    fn test_timeout_zero_stops_immediately() {
        let config = AutonomousConfig {
            max_iterations: 10,
            timeout_secs: 0,
            max_cost_cents: 1000,
        };
        let executor = AutonomousExecutor::new(config);
        let signer = ReceiptSigner::generate();
        let registry = AbilityRegistry::new(signer);
        let manifest = AgentManifest {
            name: "test-agent".into(),
            version: "0.1.0".into(),
            description: "test".into(),
            permissions: vec!["deep.delegate".into()],
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
        };

        let result = executor
            .execute("test goal", &registry, &manifest, None, None)
            .unwrap();

        // With 0s timeout, should stop very quickly
        assert!(!result.success);
    }

    #[test]
    fn test_cost_cap_zero_stops_immediately() {
        let config = AutonomousConfig {
            max_iterations: 10,
            timeout_secs: 300,
            max_cost_cents: 0,
        };
        let executor = AutonomousExecutor::new(config);
        let signer = ReceiptSigner::generate();
        let registry = AbilityRegistry::new(signer);
        let manifest = AgentManifest {
            name: "test-agent".into(),
            version: "0.1.0".into(),
            description: "test".into(),
            permissions: vec!["deep.delegate".into()],
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
        };

        let result = executor
            .execute("test goal", &registry, &manifest, None, None)
            .unwrap();

        assert!(!result.success);
        assert_eq!(result.iterations, 0);
    }

    #[test]
    fn test_autonomous_result_serialization() {
        let result = AutonomousResult {
            success: true,
            iterations: 2,
            total_ms: 1500,
            final_output: HashMap::from([("result".into(), "done".into())]),
            steps_executed: 3,
            cost_estimate_cents: 15,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"iterations\":2"));
        assert!(json.contains("\"cost_estimate_cents\":15"));

        let parsed: AutonomousResult = serde_json::from_str(&json).unwrap();
        assert!(parsed.success);
        assert_eq!(parsed.iterations, 2);
    }
}
