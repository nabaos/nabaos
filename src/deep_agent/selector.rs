use super::backend::{DeepAgentBackend, DeepAgentResult, DeepAgentStatus};
use super::spending_guard::{SpendingDecision, SpendingGuard};
use crate::core::error::{NyayaError, Result};
use crate::swarm::orchestrator::{SwarmConfig, SwarmOrchestrator};
use crate::swarm::worker::{ResearchPlan, SourcePlan, SourceTarget};
use std::collections::HashMap;

/// OllamaLocal backend — runs tasks via a local Ollama instance.
/// Zero cost, no network required, suitable for simple reasoning tasks.
pub struct OllamaLocalBackend;

impl DeepAgentBackend for OllamaLocalBackend {
    fn name(&self) -> &str {
        "ollama_local"
    }

    fn supported_task_types(&self) -> Vec<String> {
        vec![
            "chat".into(),
            "summarize".into(),
            "classify".into(),
            "simple_reasoning".into(),
        ]
    }

    fn estimated_cost(&self, _task: &str) -> f64 {
        0.0
    }

    fn execute(&self, _task: &str, _params: &HashMap<String, String>) -> Result<DeepAgentResult> {
        // Ollama execution is handled by the LLM router's Ollama provider,
        // not through the deep agent backend pathway. If the selector picks
        // this backend, the orchestrator should redirect to the LLM router.
        Err(crate::core::error::NyayaError::Config(
            "Ollama tasks should be routed through the LLM router, not the deep agent executor"
                .into(),
        ))
    }
}

/// NyayaSwarm backend — parallel research using NyayaBrowser workers.
/// Dramatically cheaper than cloud-hosted deep agents for research tasks.
pub struct SwarmBackend {
    orchestrator: SwarmOrchestrator,
}

impl SwarmBackend {
    pub fn new(config: SwarmConfig) -> Self {
        Self {
            orchestrator: SwarmOrchestrator::new(config),
        }
    }
}

impl Default for SwarmBackend {
    fn default() -> Self {
        Self::new(SwarmConfig::default())
    }
}

impl DeepAgentBackend for SwarmBackend {
    fn name(&self) -> &str {
        "nyaya_swarm"
    }

    fn supported_task_types(&self) -> Vec<String> {
        vec![
            "research".into(),
            "web_research".into(),
            "academic_research".into(),
        ]
    }

    fn estimated_cost(&self, _task: &str) -> f64 {
        0.01
    }

    fn execute(&self, task: &str, _params: &HashMap<String, String>) -> Result<DeepAgentResult> {
        // Build a simple research plan from the query
        let plan = ResearchPlan {
            query: task.to_string(),
            sources: vec![SourcePlan {
                worker_type: "search".into(),
                target: SourceTarget::DuckDuckGoQuery(task.to_string()),
                priority: 0,
                needs_auth: false,
                extraction_focus: Some("relevant results".into()),
            }],
            synthesis_instructions: format!("Synthesize research findings for: {}", task),
            max_workers: self.orchestrator.config().max_workers,
        };

        let report = tokio::runtime::Handle::try_current()
            .map(|handle| {
                // We're inside an async context — use block_in_place to run the future.
                tokio::task::block_in_place(|| {
                    handle.block_on(self.orchestrator.execute_plan(&plan))
                })
            })
            .unwrap_or_else(|_| {
                // No runtime — create a temporary one.
                let rt = tokio::runtime::Runtime::new().map_err(|e| {
                    crate::core::error::NyayaError::Config(format!(
                        "Failed to create runtime: {}",
                        e
                    ))
                })?;
                rt.block_on(self.orchestrator.execute_plan(&plan))
            })?;
        let output = format!(
            "# {}\n\n{}\n\nSources: {}/{}",
            report.query, report.summary, report.sources_used, report.sources_total
        );

        Ok(DeepAgentResult {
            backend_name: "nyaya_swarm".into(),
            status: DeepAgentStatus::Completed,
            output,
            cost_usd: 0.01,
            duration_secs: 0.0,
            metadata: HashMap::new(),
        })
    }
}

pub struct BackendSelector {
    backends: Vec<Box<dyn DeepAgentBackend>>,
    spending_guard: Option<SpendingGuard>,
}

impl BackendSelector {
    pub fn new(backends: Vec<Box<dyn DeepAgentBackend>>) -> Self {
        Self {
            backends,
            spending_guard: None,
        }
    }

    pub fn with_spending_guard(mut self, guard: SpendingGuard) -> Self {
        self.spending_guard = Some(guard);
        self
    }

    pub fn select_backend(&self, task_type: &str) -> Option<&dyn DeepAgentBackend> {
        self.backends
            .iter()
            .filter(|b| b.supported_task_types().iter().any(|t| t == task_type))
            .min_by(|a, b| {
                a.estimated_cost("")
                    .partial_cmp(&b.estimated_cost(""))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|b| b.as_ref())
    }

    pub fn execute(
        &self,
        task: &str,
        task_type: &str,
        params: &HashMap<String, String>,
    ) -> Result<DeepAgentResult> {
        let backend = self.select_backend(task_type).ok_or_else(|| {
            NyayaError::Config(format!("No backend supports task type: {}", task_type))
        })?;

        // Check spending limits
        if let Some(ref guard) = self.spending_guard {
            let estimated = backend.estimated_cost(task);
            match guard.check(estimated)? {
                SpendingDecision::Denied { reason } => {
                    return Err(NyayaError::Config(format!("Spending denied: {}", reason)));
                }
                SpendingDecision::NeedsApproval {
                    estimated_cost,
                    reason,
                } => {
                    tracing::warn!(cost = estimated_cost, reason = %reason, "Deep agent needs approval");
                    return Err(NyayaError::Config(format!(
                        "Task requires approval (est. ${:.2}): {}",
                        estimated_cost, reason
                    )));
                }
                SpendingDecision::Approved => {
                    tracing::info!(
                        backend = backend.name(),
                        estimated = estimated,
                        "Spending approved"
                    );
                }
            }
        }

        tracing::info!(
            backend = backend.name(),
            task_type = task_type,
            "Delegating to deep agent"
        );
        let result = backend.execute(task, params)?;

        // Record actual spend
        if let Some(ref guard) = self.spending_guard {
            if let Err(e) = guard.record_spend(backend.name(), task, result.cost_usd) {
                tracing::warn!(error = %e, "Failed to record deep agent spend");
            }
        }

        Ok(result)
    }

    pub fn list_backends(&self) -> Vec<(&str, Vec<String>, f64)> {
        self.backends
            .iter()
            .map(|b| (b.name(), b.supported_task_types(), b.estimated_cost("")))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::backend::DeepAgentStatus;
    use super::super::spending_guard::SpendingConfig;
    use super::*;

    // Mock backend for testing
    struct MockBackend {
        name: &'static str,
        task_types: Vec<String>,
        cost: f64,
    }

    impl DeepAgentBackend for MockBackend {
        fn name(&self) -> &str {
            self.name
        }
        fn supported_task_types(&self) -> Vec<String> {
            self.task_types.clone()
        }
        fn estimated_cost(&self, _task: &str) -> f64 {
            self.cost
        }
        fn execute(&self, task: &str, params: &HashMap<String, String>) -> Result<DeepAgentResult> {
            Ok(DeepAgentResult {
                backend_name: self.name.into(),
                status: DeepAgentStatus::Completed,
                output: format!("Mock result for: {}", task),
                cost_usd: self.cost,
                duration_secs: 0.1,
                metadata: params.clone(),
            })
        }
    }

    #[test]
    fn test_selector_picks_cheapest() {
        let selector = BackendSelector::new(vec![
            Box::new(MockBackend {
                name: "expensive",
                task_types: vec!["research".into()],
                cost: 5.0,
            }),
            Box::new(MockBackend {
                name: "cheap",
                task_types: vec!["research".into()],
                cost: 0.5,
            }),
        ]);
        let backend = selector.select_backend("research").unwrap();
        assert_eq!(backend.name(), "cheap");
    }

    #[test]
    fn test_selector_no_backend_for_type() {
        let selector = BackendSelector::new(vec![Box::new(MockBackend {
            name: "test",
            task_types: vec!["research".into()],
            cost: 1.0,
        })]);
        assert!(selector.select_backend("unknown_type").is_none());
    }

    #[test]
    fn test_selector_spending_denied() {
        let guard = SpendingGuard::new(
            ":memory:",
            SpendingConfig {
                max_per_task_usd: 0.1,
                max_daily_usd: 1.0,
                max_monthly_usd: 10.0,
                approval_threshold_usd: 0.05,
            },
        )
        .unwrap();

        let selector = BackendSelector::new(vec![Box::new(MockBackend {
            name: "test",
            task_types: vec!["research".into()],
            cost: 1.0,
        })])
        .with_spending_guard(guard);

        let result = selector.execute("test task", "research", &HashMap::new());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Spending denied"));
    }

    #[test]
    fn test_selector_spending_approved() {
        let guard = SpendingGuard::new(":memory:", SpendingConfig::default()).unwrap();

        let selector = BackendSelector::new(vec![Box::new(MockBackend {
            name: "test",
            task_types: vec!["research".into()],
            cost: 1.0,
        })])
        .with_spending_guard(guard);

        let result = selector.execute("test task", "research", &HashMap::new());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().backend_name, "test");
    }

    #[test]
    fn test_backend_selector_has_swarm() {
        let swarm = super::SwarmBackend::default();
        assert_eq!(swarm.name(), "nyaya_swarm");
        assert!(swarm
            .supported_task_types()
            .contains(&"research".to_string()));
        assert!(swarm
            .supported_task_types()
            .contains(&"web_research".to_string()));
        assert!(swarm
            .supported_task_types()
            .contains(&"academic_research".to_string()));
        assert!((swarm.estimated_cost("any task") - 0.01).abs() < f64::EPSILON);

        // SwarmBackend should be selectable and cheaper than other backends
        let selector = BackendSelector::new(vec![
            Box::new(MockBackend {
                name: "expensive",
                task_types: vec!["research".into()],
                cost: 1.50,
            }),
            Box::new(super::SwarmBackend::default()),
        ]);
        let backend = selector.select_backend("research").unwrap();
        assert_eq!(backend.name(), "nyaya_swarm");
    }

    #[test]
    fn test_manus_missing_key() {
        use super::super::manus::ManusBackend;
        let backend = ManusBackend {
            api_key: None,
            base_url: "http://localhost".into(),
        };
        let result = backend.execute("test", &HashMap::new());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("MANUS_API_KEY"));
    }

    #[test]
    fn test_claude_missing_key() {
        use super::super::claude_computer::ClaudeComputerBackend;
        let backend = ClaudeComputerBackend {
            api_key: None,
            base_url: "http://localhost".into(),
        };
        let result = backend.execute("test", &HashMap::new());
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn test_openai_missing_key() {
        use super::super::openai_agent::OpenAIAgentBackend;
        let backend = OpenAIAgentBackend {
            api_key: None,
            base_url: "http://localhost".into(),
        };
        let result = backend.execute("test", &HashMap::new());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("OPENAI_API_KEY"));
    }

    #[test]
    fn test_ollama_backend_info() {
        let ollama = super::OllamaLocalBackend;
        assert_eq!(ollama.name(), "ollama_local");
        assert!((ollama.estimated_cost("any task") - 0.0).abs() < f64::EPSILON);
        assert!(ollama.supported_task_types().contains(&"chat".to_string()));
        assert!(ollama
            .supported_task_types()
            .contains(&"summarize".to_string()));
        assert!(ollama
            .supported_task_types()
            .contains(&"classify".to_string()));
        assert!(ollama
            .supported_task_types()
            .contains(&"simple_reasoning".to_string()));

        // OllamaLocal should be selectable and cheaper than other backends
        let selector = BackendSelector::new(vec![
            Box::new(MockBackend {
                name: "expensive",
                task_types: vec!["chat".into()],
                cost: 1.0,
            }),
            Box::new(super::OllamaLocalBackend),
        ]);
        let backend = selector.select_backend("chat").unwrap();
        assert_eq!(backend.name(), "ollama_local");
    }
}
