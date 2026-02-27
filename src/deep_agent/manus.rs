use super::backend::{DeepAgentBackend, DeepAgentResult, DeepAgentStatus};
use crate::core::error::{NyayaError, Result};
use std::collections::HashMap;

pub struct ManusBackend {
    pub(crate) api_key: Option<String>,
    pub(crate) base_url: String,
}

impl Default for ManusBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ManusBackend {
    pub fn new() -> Self {
        Self {
            api_key: std::env::var("MANUS_API_KEY").ok(),
            base_url: std::env::var("MANUS_BASE_URL")
                .unwrap_or_else(|_| "https://api.manus.im/v1".into()),
        }
    }
}

impl DeepAgentBackend for ManusBackend {
    fn name(&self) -> &str {
        "manus"
    }

    fn supported_task_types(&self) -> Vec<String> {
        vec![
            "research".into(),
            "web_browsing".into(),
            "data_collection".into(),
            "multi_step".into(),
        ]
    }

    fn estimated_cost(&self, _task: &str) -> f64 {
        1.50
    }

    fn execute(&self, task: &str, params: &HashMap<String, String>) -> Result<DeepAgentResult> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| NyayaError::Config("MANUS_API_KEY not set".into()))?;

        let start = std::time::Instant::now();

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| NyayaError::Config(format!("HTTP build: {}", e)))?;

        // Step 1: Create task
        let create_body = serde_json::json!({
            "task": task,
            "parameters": params,
        });

        let resp = client
            .post(format!("{}/tasks", self.base_url))
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&create_body)
            .send()
            .map_err(|e| NyayaError::Config(format!("Manus create task: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(NyayaError::Config(format!(
                "Manus API error {}: {}",
                status, body
            )));
        }

        let created: serde_json::Value = resp
            .json()
            .map_err(|e| NyayaError::Config(format!("Manus parse: {}", e)))?;

        let task_id = created["id"]
            .as_str()
            .ok_or_else(|| NyayaError::Config("No task ID in Manus response".into()))?
            .to_string();

        tracing::info!(task_id = %task_id, "Manus task created, polling for completion");

        // Step 2: Poll for completion
        let poll_deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);

        loop {
            if std::time::Instant::now() >= poll_deadline {
                return Ok(DeepAgentResult {
                    backend_name: "manus".into(),
                    status: DeepAgentStatus::TimedOut,
                    output: format!("Task {} timed out after 5 minutes", task_id),
                    cost_usd: 0.0,
                    duration_secs: start.elapsed().as_secs_f64(),
                    metadata: params.clone(),
                });
            }

            std::thread::sleep(std::time::Duration::from_secs(5));

            let poll_resp = client
                .get(format!("{}/tasks/{}", self.base_url, task_id))
                .header("Authorization", format!("Bearer {}", api_key))
                .send()
                .map_err(|e| NyayaError::Config(format!("Manus poll: {}", e)))?;

            if !poll_resp.status().is_success() {
                continue; // Retry on transient errors
            }

            let poll_data: serde_json::Value = poll_resp
                .json()
                .map_err(|e| NyayaError::Config(format!("Manus poll parse: {}", e)))?;

            match poll_data["status"].as_str().unwrap_or("unknown") {
                "completed" => {
                    return Ok(DeepAgentResult {
                        backend_name: "manus".into(),
                        status: DeepAgentStatus::Completed,
                        output: poll_data["output"].as_str().unwrap_or("").to_string(),
                        cost_usd: poll_data["cost_usd"].as_f64().unwrap_or(0.0),
                        duration_secs: start.elapsed().as_secs_f64(),
                        metadata: params.clone(),
                    });
                }
                "failed" => {
                    return Ok(DeepAgentResult {
                        backend_name: "manus".into(),
                        status: DeepAgentStatus::Failed,
                        output: poll_data["error"]
                            .as_str()
                            .unwrap_or("Unknown error")
                            .to_string(),
                        cost_usd: poll_data["cost_usd"].as_f64().unwrap_or(0.0),
                        duration_secs: start.elapsed().as_secs_f64(),
                        metadata: params.clone(),
                    });
                }
                _ => continue,
            }
        }
    }
}
