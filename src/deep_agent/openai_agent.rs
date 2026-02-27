use super::backend::{DeepAgentBackend, DeepAgentResult, DeepAgentStatus};
use crate::core::error::{NyayaError, Result};
use std::collections::HashMap;

pub struct OpenAIAgentBackend {
    pub(crate) api_key: Option<String>,
    pub(crate) base_url: String,
}

impl Default for OpenAIAgentBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenAIAgentBackend {
    pub fn new() -> Self {
        Self {
            api_key: std::env::var("OPENAI_API_KEY").ok(),
            base_url: "https://api.openai.com/v1".into(),
        }
    }
}

fn estimate_openai_cost(input: u64, output: u64) -> f64 {
    // GPT-4o pricing: $2.50/MTok in, $10/MTok out
    (input as f64 * 2.5 / 1_000_000.0) + (output as f64 * 10.0 / 1_000_000.0)
}

impl DeepAgentBackend for OpenAIAgentBackend {
    fn name(&self) -> &str {
        "openai"
    }

    fn supported_task_types(&self) -> Vec<String> {
        vec![
            "structured_data".into(),
            "function_calling".into(),
            "classification".into(),
            "summarization".into(),
        ]
    }

    fn estimated_cost(&self, _task: &str) -> f64 {
        0.80
    }

    fn execute(&self, task: &str, params: &HashMap<String, String>) -> Result<DeepAgentResult> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| NyayaError::Config("OPENAI_API_KEY not set".into()))?;

        let start = std::time::Instant::now();

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| NyayaError::Config(format!("HTTP build: {}", e)))?;

        let context = params
            .iter()
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect::<Vec<_>>()
            .join("\n");
        let full_prompt = if context.is_empty() {
            task.to_string()
        } else {
            format!("{}\n\nContext:\n{}", task, context)
        };

        // Use chat completions API (widely supported, stable)
        let body = serde_json::json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "You are a deep agent. Execute the task thoroughly and return structured results."},
                {"role": "user", "content": full_prompt}
            ],
            "max_tokens": 8192,
        });

        tracing::info!("OpenAI deep agent: sending request");

        let resp = client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| NyayaError::Config(format!("OpenAI request: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().unwrap_or_default();
            let safe = body_text.replace(api_key, "[REDACTED]");
            return Err(NyayaError::Config(format!(
                "OpenAI API error {}: {}",
                status, safe
            )));
        }

        let parsed: serde_json::Value = resp
            .json()
            .map_err(|e| NyayaError::Config(format!("OpenAI parse: {}", e)))?;

        let text = parsed["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let input_tokens = parsed["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
        let output_tokens = parsed["usage"]["completion_tokens"].as_u64().unwrap_or(0);
        let cost = estimate_openai_cost(input_tokens, output_tokens);

        tracing::info!(
            input_tokens = input_tokens,
            output_tokens = output_tokens,
            cost_usd = cost,
            "OpenAI deep agent: completed"
        );

        Ok(DeepAgentResult {
            backend_name: "openai".into(),
            status: DeepAgentStatus::Completed,
            output: text,
            cost_usd: cost,
            duration_secs: start.elapsed().as_secs_f64(),
            metadata: params.clone(),
        })
    }
}
