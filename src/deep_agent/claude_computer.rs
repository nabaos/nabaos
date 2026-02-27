use super::backend::{DeepAgentBackend, DeepAgentResult, DeepAgentStatus};
use crate::core::error::{NyayaError, Result};
use std::collections::HashMap;

pub struct ClaudeComputerBackend {
    pub(crate) api_key: Option<String>,
    pub(crate) base_url: String,
}

impl Default for ClaudeComputerBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeComputerBackend {
    pub fn new() -> Self {
        Self {
            api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            base_url: "https://api.anthropic.com/v1/messages".into(),
        }
    }
}

fn estimate_anthropic_cost(input: u64, output: u64) -> f64 {
    // Claude Sonnet pricing: $3/MTok in, $15/MTok out
    (input as f64 * 3.0 / 1_000_000.0) + (output as f64 * 15.0 / 1_000_000.0)
}

impl DeepAgentBackend for ClaudeComputerBackend {
    fn name(&self) -> &str {
        "claude"
    }

    fn supported_task_types(&self) -> Vec<String> {
        vec![
            "code_review".into(),
            "code_generation".into(),
            "analysis".into(),
            "document_processing".into(),
        ]
    }

    fn estimated_cost(&self, _task: &str) -> f64 {
        2.0
    }

    fn execute(&self, task: &str, params: &HashMap<String, String>) -> Result<DeepAgentResult> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| NyayaError::Config("ANTHROPIC_API_KEY not set".into()))?;

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

        let body = serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 8192,
            "messages": [{"role": "user", "content": full_prompt}],
            "system": "You are a deep analysis agent. Provide thorough, well-structured responses. Focus on accuracy and completeness."
        });

        tracing::info!("Claude deep agent: sending request");

        let resp = client
            .post(&self.base_url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| NyayaError::Config(format!("Claude request: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().unwrap_or_default();
            let safe = body_text.replace(api_key, "[REDACTED]");
            return Err(NyayaError::Config(format!(
                "Claude API error {}: {}",
                status, safe
            )));
        }

        let parsed: serde_json::Value = resp
            .json()
            .map_err(|e| NyayaError::Config(format!("Claude parse: {}", e)))?;

        let text = parsed["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();

        let input_tokens = parsed["usage"]["input_tokens"].as_u64().unwrap_or(0);
        let output_tokens = parsed["usage"]["output_tokens"].as_u64().unwrap_or(0);
        let cost = estimate_anthropic_cost(input_tokens, output_tokens);

        tracing::info!(
            input_tokens = input_tokens,
            output_tokens = output_tokens,
            cost_usd = cost,
            "Claude deep agent: completed"
        );

        Ok(DeepAgentResult {
            backend_name: "claude".into(),
            status: DeepAgentStatus::Completed,
            output: text,
            cost_usd: cost,
            duration_secs: start.elapsed().as_secs_f64(),
            metadata: params.clone(),
        })
    }
}
