use serde::{Deserialize, Serialize};

use crate::core::error::{NyayaError, Result};

/// LLM provider configuration.
#[derive(Debug, Clone)]
pub struct LlmProvider {
    pub provider: ProviderType,
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
pub enum ProviderType {
    Anthropic,
    OpenAI,
    DeepSeek,
    Ollama,
}

/// LLM request.
#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

/// LLM response (Anthropic format).
#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlock {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Result of an LLM call.
#[derive(Debug)]
pub struct LlmResponse {
    pub text: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub latency_ms: u64,
}

/// A tool definition for LLM function calling.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// A tool use request from the LLM.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolUseBlock {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

/// Response from `complete_with_tools()`.
#[derive(Debug)]
pub struct LlmToolResponse {
    pub text: Option<String>,
    pub tool_calls: Vec<ToolUseBlock>,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub latency_ms: u64,
}

/// A content block for vision requests (text or image).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: ImageSource },
}

/// Image source for vision requests.
#[derive(Debug, Clone, Serialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: String,
    pub data: String,
}

/// A streaming chunk from the LLM.
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// A text delta (partial token)
    Delta(String),
    /// Final usage stats
    Done {
        input_tokens: u32,
        output_tokens: u32,
    },
    /// Error during streaming
    Error(String),
}

impl LlmProvider {
    pub fn anthropic(api_key: &str, model: &str) -> Self {
        Self {
            provider: ProviderType::Anthropic,
            api_key: api_key.to_string(),
            model: model.to_string(),
            base_url: "https://api.anthropic.com/v1/messages".to_string(),
            timeout_secs: None,
        }
    }

    pub fn openai(api_key: &str, model: &str) -> Self {
        Self {
            provider: ProviderType::OpenAI,
            api_key: api_key.to_string(),
            model: model.to_string(),
            base_url: "https://api.openai.com/v1/chat/completions".to_string(),
            timeout_secs: None,
        }
    }

    pub fn openai_with_url(api_key: &str, model: &str, base_url: &str) -> Self {
        let base = base_url.trim_end_matches('/');
        let base = base.strip_suffix("/v1").unwrap_or(base);
        Self {
            provider: ProviderType::OpenAI,
            api_key: api_key.to_string(),
            model: model.to_string(),
            base_url: format!("{}/v1/chat/completions", base),
            timeout_secs: None,
        }
    }

    pub fn deepseek(api_key: &str, model: &str) -> Self {
        Self {
            provider: ProviderType::DeepSeek,
            api_key: api_key.to_string(),
            model: model.to_string(),
            base_url: "https://api.deepseek.com/v1/chat/completions".to_string(),
            timeout_secs: None,
        }
    }

    pub fn ollama(model: &str) -> Self {
        Self {
            provider: ProviderType::Ollama,
            api_key: String::new(),
            model: model.to_string(),
            base_url: "http://localhost:11434/v1/chat/completions".to_string(),
            timeout_secs: Some(120),
        }
    }

    pub fn ollama_with_url(model: &str, base_url: &str) -> Self {
        Self {
            provider: ProviderType::Ollama,
            api_key: String::new(),
            model: model.to_string(),
            base_url: base_url.to_string(),
            timeout_secs: Some(120),
        }
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }

    /// Send a message to the LLM and get a response.
    /// This is a blocking call — use from a tokio::spawn context.
    pub fn complete(&self, system_prompt: &str, user_message: &str) -> Result<LlmResponse> {
        let start = std::time::Instant::now();

        let mut builder = reqwest::blocking::Client::builder();
        if let Some(secs) = self.timeout_secs {
            builder = builder.timeout(std::time::Duration::from_secs(secs));
        }
        let client = builder
            .build()
            .map_err(|e| NyayaError::Config(format!("HTTP client build failed: {}", e)))?;

        let response = match self.provider {
            ProviderType::Anthropic => {
                let body = AnthropicRequest {
                    model: self.model.clone(),
                    max_tokens: 4096,
                    messages: vec![Message {
                        role: "user".into(),
                        content: user_message.into(),
                    }],
                    system: Some(system_prompt.into()),
                    temperature: Some(0.2),
                };

                let resp = client
                    .post(&self.base_url)
                    .header("x-api-key", &self.api_key)
                    .header("anthropic-version", "2023-06-01")
                    .header("content-type", "application/json")
                    .json(&body)
                    .send()
                    .map_err(|e| NyayaError::Config(format!("LLM request failed: {}", e)))?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let body = resp.text().unwrap_or_default();
                    // Redact API key from error body to prevent leaks
                    let safe_body = body.replace(&self.api_key, "[REDACTED]");
                    return Err(NyayaError::Config(format!(
                        "LLM API error {}: {}",
                        status, safe_body
                    )));
                }

                let parsed: AnthropicResponse = resp
                    .json()
                    .map_err(|e| NyayaError::Config(format!("LLM response parse failed: {}", e)))?;

                let text = parsed
                    .content
                    .into_iter()
                    .filter_map(|c| c.text)
                    .collect::<Vec<_>>()
                    .join("");

                if text.is_empty() {
                    return Err(NyayaError::Config(
                        "LLM returned empty response (Anthropic: no text content blocks)".into(),
                    ));
                }

                let usage = parsed.usage.unwrap_or(Usage {
                    input_tokens: 0,
                    output_tokens: 0,
                });

                LlmResponse {
                    text,
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    latency_ms: start.elapsed().as_millis() as u64,
                }
            }
            ProviderType::OpenAI | ProviderType::DeepSeek | ProviderType::Ollama => {
                let body = serde_json::json!({
                    "model": self.model,
                    "messages": [
                        {"role": "system", "content": system_prompt},
                        {"role": "user", "content": user_message}
                    ],
                    "max_tokens": 4096,
                    "temperature": 0.2
                });

                let mut req = client
                    .post(&self.base_url)
                    .header("content-type", "application/json");
                if !self.api_key.is_empty() {
                    req = req.header("Authorization", format!("Bearer {}", self.api_key));
                }
                let resp = req
                    .json(&body)
                    .send()
                    .map_err(|e| NyayaError::Config(format!("LLM request failed: {}", e)))?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let body = resp.text().unwrap_or_default();
                    // Redact API key from error body to prevent leaks
                    let safe_body = body.replace(&self.api_key, "[REDACTED]");
                    return Err(NyayaError::Config(format!(
                        "LLM API error {}: {}",
                        status, safe_body
                    )));
                }

                let parsed: serde_json::Value = resp
                    .json()
                    .map_err(|e| NyayaError::Config(format!("LLM response parse failed: {}", e)))?;

                let text = parsed["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                if text.is_empty() {
                    // Surface the API response so failures aren't silent
                    let error_hint = parsed["error"]["message"]
                        .as_str()
                        .or_else(|| parsed["error"].as_str())
                        .unwrap_or("LLM returned empty response");
                    return Err(NyayaError::Config(format!(
                        "LLM returned empty response: {}",
                        error_hint
                    )));
                }

                let input_tokens = parsed["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32;
                let output_tokens =
                    parsed["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32;

                LlmResponse {
                    text,
                    input_tokens,
                    output_tokens,
                    latency_ms: start.elapsed().as_millis() as u64,
                }
            }
        };

        Ok(response)
    }

    /// Send a message with tool definitions and get back text and/or tool calls.
    /// This is a blocking call.
    pub fn complete_with_tools(
        &self,
        system_prompt: &str,
        user_message: &str,
        tools: &[ToolDefinition],
    ) -> Result<LlmToolResponse> {
        let start = std::time::Instant::now();

        let mut builder = reqwest::blocking::Client::builder();
        if let Some(secs) = self.timeout_secs {
            builder = builder.timeout(std::time::Duration::from_secs(secs));
        }
        let client = builder
            .build()
            .map_err(|e| NyayaError::Config(format!("HTTP client build failed: {}", e)))?;

        let (text, tool_calls, input_tokens, output_tokens) = match self.provider {
            ProviderType::Anthropic => {
                let tools_json: Vec<serde_json::Value> = tools
                    .iter()
                    .map(|t| {
                        serde_json::json!({
                            "name": t.name,
                            "description": t.description,
                            "input_schema": t.input_schema,
                        })
                    })
                    .collect();

                let body = serde_json::json!({
                    "model": self.model,
                    "max_tokens": 4096,
                    "system": system_prompt,
                    "messages": [{"role": "user", "content": user_message}],
                    "tools": tools_json,
                    "temperature": 0.2,
                });

                let resp = client
                    .post(&self.base_url)
                    .header("x-api-key", &self.api_key)
                    .header("anthropic-version", "2023-06-01")
                    .header("content-type", "application/json")
                    .json(&body)
                    .send()
                    .map_err(|e| NyayaError::Config(format!("LLM request failed: {}", e)))?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let body = resp.text().unwrap_or_default();
                    let safe_body = body.replace(&self.api_key, "[REDACTED]");
                    return Err(NyayaError::Config(format!(
                        "LLM API error {}: {}",
                        status, safe_body
                    )));
                }

                let json: serde_json::Value = resp
                    .json()
                    .map_err(|e| NyayaError::Config(format!("LLM response parse failed: {}", e)))?;

                let mut text = None;
                let mut tool_calls = Vec::new();
                if let Some(content) = json.get("content").and_then(|c| c.as_array()) {
                    for block in content {
                        match block.get("type").and_then(|t| t.as_str()) {
                            Some("text") => {
                                text = block.get("text").and_then(|t| t.as_str()).map(String::from);
                            }
                            Some("tool_use") => {
                                if let (Some(id), Some(name), Some(input)) = (
                                    block.get("id").and_then(|v| v.as_str()),
                                    block.get("name").and_then(|v| v.as_str()),
                                    block.get("input"),
                                ) {
                                    tool_calls.push(ToolUseBlock {
                                        id: id.to_string(),
                                        name: name.to_string(),
                                        input: input.clone(),
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }

                let input_tokens = json["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32;
                let output_tokens = json["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32;
                (text, tool_calls, input_tokens, output_tokens)
            }
            ProviderType::OpenAI | ProviderType::DeepSeek | ProviderType::Ollama => {
                let tools_json: Vec<serde_json::Value> = tools
                    .iter()
                    .map(|t| {
                        serde_json::json!({
                            "type": "function",
                            "function": {
                                "name": t.name,
                                "description": t.description,
                                "parameters": t.input_schema,
                            }
                        })
                    })
                    .collect();

                let body = serde_json::json!({
                    "model": self.model,
                    "max_tokens": 4096,
                    "messages": [
                        {"role": "system", "content": system_prompt},
                        {"role": "user", "content": user_message}
                    ],
                    "tools": tools_json,
                    "temperature": 0.2,
                });

                let mut req = client
                    .post(&self.base_url)
                    .header("content-type", "application/json");
                if !self.api_key.is_empty() {
                    req = req.header("Authorization", format!("Bearer {}", self.api_key));
                }
                let resp = req
                    .json(&body)
                    .send()
                    .map_err(|e| NyayaError::Config(format!("LLM request failed: {}", e)))?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let body = resp.text().unwrap_or_default();
                    let safe_body = body.replace(&self.api_key, "[REDACTED]");
                    return Err(NyayaError::Config(format!(
                        "LLM API error {}: {}",
                        status, safe_body
                    )));
                }

                let json: serde_json::Value = resp
                    .json()
                    .map_err(|e| NyayaError::Config(format!("LLM response parse failed: {}", e)))?;

                let text = json["choices"][0]["message"]["content"]
                    .as_str()
                    .map(String::from);
                let mut tool_calls = Vec::new();
                if let Some(calls) = json["choices"][0]["message"]["tool_calls"].as_array() {
                    for call in calls {
                        if let (Some(id), Some(name), Some(args)) = (
                            call.get("id").and_then(|v| v.as_str()),
                            call["function"].get("name").and_then(|v| v.as_str()),
                            call["function"].get("arguments").and_then(|v| v.as_str()),
                        ) {
                            let input =
                                serde_json::from_str(args).unwrap_or(serde_json::Value::Null);
                            tool_calls.push(ToolUseBlock {
                                id: id.to_string(),
                                name: name.to_string(),
                                input,
                            });
                        }
                    }
                }

                let input_tokens = json["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32;
                let output_tokens = json["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32;
                (text, tool_calls, input_tokens, output_tokens)
            }
        };

        Ok(LlmToolResponse {
            text,
            tool_calls,
            input_tokens,
            output_tokens,
            latency_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// Send a message with mixed text and image content blocks.
    /// This is a blocking call for vision-capable models.
    pub fn complete_with_images(
        &self,
        system_prompt: &str,
        content: Vec<ContentBlock>,
    ) -> Result<LlmResponse> {
        let start = std::time::Instant::now();

        let mut builder = reqwest::blocking::Client::builder();
        if let Some(secs) = self.timeout_secs {
            builder = builder.timeout(std::time::Duration::from_secs(secs));
        }
        let client = builder
            .build()
            .map_err(|e| NyayaError::Config(format!("HTTP client build failed: {}", e)))?;

        let response = match self.provider {
            ProviderType::Anthropic => {
                let content_json: Vec<serde_json::Value> = content
                    .iter()
                    .map(|block| match block {
                        ContentBlock::Text { text } => {
                            serde_json::json!({"type": "text", "text": text})
                        }
                        ContentBlock::Image { source } => serde_json::json!({
                            "type": "image",
                            "source": {
                                "type": source.source_type,
                                "media_type": source.media_type,
                                "data": source.data,
                            }
                        }),
                    })
                    .collect();

                let body = serde_json::json!({
                    "model": self.model,
                    "max_tokens": 4096,
                    "system": system_prompt,
                    "messages": [{"role": "user", "content": content_json}],
                    "temperature": 0.2,
                });

                let resp = client
                    .post(&self.base_url)
                    .header("x-api-key", &self.api_key)
                    .header("anthropic-version", "2023-06-01")
                    .header("content-type", "application/json")
                    .json(&body)
                    .send()
                    .map_err(|e| NyayaError::Config(format!("LLM request failed: {}", e)))?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let body = resp.text().unwrap_or_default();
                    let safe_body = body.replace(&self.api_key, "[REDACTED]");
                    return Err(NyayaError::Config(format!(
                        "LLM API error {}: {}",
                        status, safe_body
                    )));
                }

                let json: serde_json::Value = resp
                    .json()
                    .map_err(|e| NyayaError::Config(format!("LLM response parse failed: {}", e)))?;

                let text = json["content"][0]["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let input_tokens = json["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32;
                let output_tokens = json["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32;

                LlmResponse {
                    text,
                    input_tokens,
                    output_tokens,
                    latency_ms: start.elapsed().as_millis() as u64,
                }
            }
            ProviderType::OpenAI | ProviderType::DeepSeek | ProviderType::Ollama => {
                let content_json: Vec<serde_json::Value> = content
                    .iter()
                    .map(|block| match block {
                        ContentBlock::Text { text } => {
                            serde_json::json!({"type": "text", "text": text})
                        }
                        ContentBlock::Image { source } => serde_json::json!({
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:{};base64,{}", source.media_type, source.data)
                            }
                        }),
                    })
                    .collect();

                let body = serde_json::json!({
                    "model": self.model,
                    "max_tokens": 4096,
                    "messages": [
                        {"role": "system", "content": system_prompt},
                        {"role": "user", "content": content_json}
                    ],
                    "temperature": 0.2,
                });

                let mut req = client
                    .post(&self.base_url)
                    .header("content-type", "application/json");
                if !self.api_key.is_empty() {
                    req = req.header("Authorization", format!("Bearer {}", self.api_key));
                }
                let resp = req
                    .json(&body)
                    .send()
                    .map_err(|e| NyayaError::Config(format!("LLM request failed: {}", e)))?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let body = resp.text().unwrap_or_default();
                    let safe_body = body.replace(&self.api_key, "[REDACTED]");
                    return Err(NyayaError::Config(format!(
                        "LLM API error {}: {}",
                        status, safe_body
                    )));
                }

                let json: serde_json::Value = resp
                    .json()
                    .map_err(|e| NyayaError::Config(format!("LLM response parse failed: {}", e)))?;

                let text = json["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let input_tokens = json["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32;
                let output_tokens = json["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32;

                LlmResponse {
                    text,
                    input_tokens,
                    output_tokens,
                    latency_ms: start.elapsed().as_millis() as u64,
                }
            }
        };

        Ok(response)
    }

    /// Stream LLM response chunks to a channel.
    /// Async version of complete() that yields deltas as they arrive.
    pub async fn complete_streaming(
        &self,
        system_prompt: &str,
        user_message: &str,
        tx: tokio::sync::mpsc::Sender<StreamChunk>,
    ) -> Result<()> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(
                self.timeout_secs.unwrap_or(60),
            ))
            .build()
            .map_err(|e| NyayaError::Config(format!("HTTP build: {}", e)))?;

        match self.provider {
            ProviderType::Anthropic => {
                let body = serde_json::json!({
                    "model": self.model,
                    "max_tokens": 4096,
                    "stream": true,
                    "messages": [{"role": "user", "content": user_message}],
                    "system": system_prompt,
                    "temperature": 0.2,
                });

                let resp = client
                    .post(&self.base_url)
                    .header("x-api-key", &self.api_key)
                    .header("anthropic-version", "2023-06-01")
                    .header("content-type", "application/json")
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| NyayaError::Config(format!("Stream request: {}", e)))?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let body_text = resp.text().await.unwrap_or_default();
                    let safe = body_text.replace(&self.api_key, "[REDACTED]");
                    let _ = tx
                        .send(StreamChunk::Error(format!(
                            "API error {}: {}",
                            status, safe
                        )))
                        .await;
                    return Ok(());
                }

                let mut stream = resp.bytes_stream();
                let mut buffer = String::new();
                let mut input_tokens = 0u32;
                let mut output_tokens = 0u32;

                use futures_util::StreamExt;
                while let Some(chunk) = stream.next().await {
                    let bytes = match chunk {
                        Ok(b) => b,
                        Err(e) => {
                            let _ = tx
                                .send(StreamChunk::Error(format!("Stream read: {}", e)))
                                .await;
                            break;
                        }
                    };
                    buffer.push_str(&String::from_utf8_lossy(&bytes));

                    while let Some(line_end) = buffer.find('\n') {
                        let line = buffer[..line_end].trim().to_string();
                        buffer = buffer[line_end + 1..].to_string();

                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" {
                                continue;
                            }
                            if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                                match event["type"].as_str() {
                                    Some("content_block_delta") => {
                                        if let Some(text) = event["delta"]["text"].as_str() {
                                            let _ =
                                                tx.send(StreamChunk::Delta(text.to_string())).await;
                                        }
                                    }
                                    Some("message_delta") => {
                                        output_tokens =
                                            event["usage"]["output_tokens"].as_u64().unwrap_or(0)
                                                as u32;
                                    }
                                    Some("message_start") => {
                                        input_tokens = event["message"]["usage"]["input_tokens"]
                                            .as_u64()
                                            .unwrap_or(0)
                                            as u32;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }

                let _ = tx
                    .send(StreamChunk::Done {
                        input_tokens,
                        output_tokens,
                    })
                    .await;
            }
            ProviderType::OpenAI | ProviderType::DeepSeek | ProviderType::Ollama => {
                let mut body = serde_json::json!({
                    "model": self.model,
                    "stream": true,
                    "messages": [
                        {"role": "system", "content": system_prompt},
                        {"role": "user", "content": user_message}
                    ],
                    "max_tokens": 4096,
                    "temperature": 0.2,
                });
                // stream_options is OpenAI-specific; Ollama silently ignores it
                // but token counts would report as 0. Only add for OpenAI/DeepSeek.
                if !matches!(self.provider, ProviderType::Ollama) {
                    body["stream_options"] = serde_json::json!({"include_usage": true});
                }

                let mut req = client
                    .post(&self.base_url)
                    .header("content-type", "application/json");
                if !self.api_key.is_empty() {
                    req = req.header("Authorization", format!("Bearer {}", self.api_key));
                }
                let resp = req
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| NyayaError::Config(format!("Stream request: {}", e)))?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let body_text = resp.text().await.unwrap_or_default();
                    let safe = body_text.replace(&self.api_key, "[REDACTED]");
                    let _ = tx
                        .send(StreamChunk::Error(format!(
                            "API error {}: {}",
                            status, safe
                        )))
                        .await;
                    return Ok(());
                }

                let mut stream = resp.bytes_stream();
                let mut buffer = String::new();
                let mut input_tokens = 0u32;
                let mut output_tokens = 0u32;

                use futures_util::StreamExt;
                while let Some(chunk) = stream.next().await {
                    let bytes = match chunk {
                        Ok(b) => b,
                        Err(e) => {
                            let _ = tx
                                .send(StreamChunk::Error(format!("Stream read: {}", e)))
                                .await;
                            break;
                        }
                    };
                    buffer.push_str(&String::from_utf8_lossy(&bytes));

                    while let Some(line_end) = buffer.find('\n') {
                        let line = buffer[..line_end].trim().to_string();
                        buffer = buffer[line_end + 1..].to_string();

                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" {
                                continue;
                            }
                            if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                                if let Some(delta) =
                                    event["choices"][0]["delta"]["content"].as_str()
                                {
                                    if !delta.is_empty() {
                                        let _ =
                                            tx.send(StreamChunk::Delta(delta.to_string())).await;
                                    }
                                }
                                if let Some(usage) = event["usage"].as_object() {
                                    input_tokens = usage
                                        .get("prompt_tokens")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0)
                                        as u32;
                                    output_tokens = usage
                                        .get("completion_tokens")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0)
                                        as u32;
                                }
                            }
                        }
                    }
                }

                let _ = tx
                    .send(StreamChunk::Done {
                        input_tokens,
                        output_tokens,
                    })
                    .await;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_chunk_variants() {
        let delta = StreamChunk::Delta("hello".into());
        assert!(format!("{:?}", delta).contains("Delta"));

        let done = StreamChunk::Done {
            input_tokens: 10,
            output_tokens: 20,
        };
        assert!(format!("{:?}", done).contains("20"));

        let err = StreamChunk::Error("fail".into());
        assert!(format!("{:?}", err).contains("fail"));
    }

    #[test]
    fn test_provider_construction() {
        let p = LlmProvider::anthropic("test-key", "claude-haiku-4-5-20251001");
        assert_eq!(p.model, "claude-haiku-4-5-20251001");
        assert!(matches!(p.provider, ProviderType::Anthropic));

        let p2 = LlmProvider::openai("test-key", "gpt-4o-mini");
        assert!(matches!(p2.provider, ProviderType::OpenAI));

        let p3 = LlmProvider::deepseek("test-key", "deepseek-v3");
        assert!(matches!(p3.provider, ProviderType::DeepSeek));
    }

    #[test]
    fn test_openai_with_url() {
        let p = LlmProvider::openai_with_url("key", "qwen3.5-32b", "https://nano-gpt.com/api/v1");
        assert_eq!(p.model, "qwen3.5-32b");
        assert_eq!(
            p.base_url,
            "https://nano-gpt.com/api/v1/chat/completions"
        );
        assert!(matches!(p.provider, ProviderType::OpenAI));

        // trailing slash + v1 normalization
        let p2 =
            LlmProvider::openai_with_url("key", "gpt-4o", "https://openrouter.ai/api/v1/");
        assert_eq!(
            p2.base_url,
            "https://openrouter.ai/api/v1/chat/completions"
        );
    }

    #[test]
    fn test_ollama_provider_construction() {
        let p = LlmProvider::ollama("llama3.2");
        assert_eq!(p.model, "llama3.2");
        assert!(matches!(p.provider, ProviderType::Ollama));
        assert_eq!(p.api_key, "");
        assert_eq!(p.base_url, "http://localhost:11434/v1/chat/completions");
        assert_eq!(p.timeout_secs, Some(120));
    }

    #[test]
    fn test_ollama_custom_url() {
        let p =
            LlmProvider::ollama_with_url("mistral", "http://gpu-server:11434/v1/chat/completions");
        assert_eq!(p.model, "mistral");
        assert_eq!(p.base_url, "http://gpu-server:11434/v1/chat/completions");
        assert!(matches!(p.provider, ProviderType::Ollama));
    }

    #[test]
    fn test_provider_with_timeout() {
        let p = LlmProvider::anthropic("key", "model").with_timeout(120);
        assert_eq!(p.timeout_secs, Some(120));
    }

    #[tokio::test]
    async fn test_delta_accumulation_via_channel() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);

        // Simulate sending chunks
        tx.send(StreamChunk::Delta("Hello".into())).await.unwrap();
        tx.send(StreamChunk::Delta(" world".into())).await.unwrap();
        tx.send(StreamChunk::Done {
            input_tokens: 5,
            output_tokens: 2,
        })
        .await
        .unwrap();
        drop(tx);

        let mut accumulated = String::new();
        let mut final_tokens = (0u32, 0u32);

        while let Some(chunk) = rx.recv().await {
            match chunk {
                StreamChunk::Delta(text) => accumulated.push_str(&text),
                StreamChunk::Done {
                    input_tokens,
                    output_tokens,
                } => {
                    final_tokens = (input_tokens, output_tokens);
                }
                StreamChunk::Error(_) => panic!("Unexpected error"),
            }
        }

        assert_eq!(accumulated, "Hello world");
        assert_eq!(final_tokens, (5, 2));
    }

    // --- Function calling tests ---

    #[test]
    fn test_tool_definition_serialize() {
        let tool = ToolDefinition {
            name: "get_weather".to_string(),
            description: "Get current weather for a location".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "location": {"type": "string", "description": "City name"}
                },
                "required": ["location"]
            }),
        };
        let json = serde_json::to_value(&tool).unwrap();
        assert_eq!(json["name"], "get_weather");
        assert_eq!(json["description"], "Get current weather for a location");
        assert_eq!(json["input_schema"]["type"], "object");
        assert!(json["input_schema"]["properties"]["location"].is_object());
    }

    #[test]
    fn test_anthropic_tool_format() {
        let tools = [ToolDefinition {
            name: "search".to_string(),
            description: "Search the web".to_string(),
            input_schema: serde_json::json!({"type": "object", "properties": {"query": {"type": "string"}}}),
        }];

        let tools_json: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            })
            .collect();

        assert_eq!(tools_json.len(), 1);
        assert_eq!(tools_json[0]["name"], "search");
        assert_eq!(tools_json[0]["input_schema"]["type"], "object");
        // Anthropic format has input_schema at top level (no "function" wrapper)
        assert!(tools_json[0].get("function").is_none());
    }

    #[test]
    fn test_openai_tool_format() {
        let tools = [ToolDefinition {
            name: "search".to_string(),
            description: "Search the web".to_string(),
            input_schema: serde_json::json!({"type": "object", "properties": {"query": {"type": "string"}}}),
        }];

        let tools_json: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    }
                })
            })
            .collect();

        assert_eq!(tools_json.len(), 1);
        assert_eq!(tools_json[0]["type"], "function");
        assert_eq!(tools_json[0]["function"]["name"], "search");
        assert_eq!(tools_json[0]["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn test_parse_tool_use_block() {
        let json = r#"{"id": "toolu_123", "name": "get_weather", "input": {"location": "Tokyo"}}"#;
        let block: ToolUseBlock = serde_json::from_str(json).unwrap();
        assert_eq!(block.id, "toolu_123");
        assert_eq!(block.name, "get_weather");
        assert_eq!(block.input["location"], "Tokyo");
    }

    // --- Vision tests ---

    #[test]
    fn test_content_block_text_serialize() {
        let block = ContentBlock::Text {
            text: "Describe this image".to_string(),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "text");
        assert_eq!(json["text"], "Describe this image");
    }

    #[test]
    fn test_content_block_image_serialize() {
        let block = ContentBlock::Image {
            source: ImageSource {
                source_type: "base64".to_string(),
                media_type: "image/png".to_string(),
                data: "iVBORw0KGgo=".to_string(),
            },
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "image");
        assert_eq!(json["source"]["type"], "base64");
        assert_eq!(json["source"]["media_type"], "image/png");
        assert_eq!(json["source"]["data"], "iVBORw0KGgo=");
    }

    #[test]
    fn test_anthropic_image_format() {
        let source = ImageSource {
            source_type: "base64".to_string(),
            media_type: "image/jpeg".to_string(),
            data: "/9j/4AAQ".to_string(),
        };
        let content_json = serde_json::json!({
            "type": "image",
            "source": {
                "type": source.source_type,
                "media_type": source.media_type,
                "data": source.data,
            }
        });
        assert_eq!(content_json["type"], "image");
        assert_eq!(content_json["source"]["type"], "base64");
        assert_eq!(content_json["source"]["media_type"], "image/jpeg");
        assert_eq!(content_json["source"]["data"], "/9j/4AAQ");
    }
}
