use crate::core::error::Result;
use std::collections::HashMap;

/// Result from a deep agent execution
#[derive(Debug, Clone)]
pub struct DeepAgentResult {
    pub backend_name: String,
    pub status: DeepAgentStatus,
    pub output: String,
    pub cost_usd: f64,
    pub duration_secs: f64,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeepAgentStatus {
    Completed,
    Partial,
    Failed,
    TimedOut,
}

/// Trait for deep agent backends
pub trait DeepAgentBackend: Send + Sync {
    fn name(&self) -> &str;
    fn supported_task_types(&self) -> Vec<String>;
    fn estimated_cost(&self, task: &str) -> f64;
    fn execute(&self, task: &str, params: &HashMap<String, String>) -> Result<DeepAgentResult>;
}
