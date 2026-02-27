//! ComfyUI client — local image/video generation via ComfyUI REST API.
//!
//! Submits workflow JSON to POST /prompt, polls GET /history/{id},
//! downloads output from GET /view.

use crate::core::error::{NyayaError, Result};
use crate::media::traits::*;
use async_trait::async_trait;
use serde::Deserialize;

const DEFAULT_COMFYUI_URL: &str = "http://localhost:8188";
const POLL_INTERVAL_MS: u64 = 2000;
const MAX_POLL_ATTEMPTS: u32 = 300;

pub struct ComfyUiClient {
    base_url: String,
    client: reqwest::Client,
    client_id: String,
}

#[derive(Debug, Deserialize)]
struct PromptResponse {
    prompt_id: String,
}

impl ComfyUiClient {
    pub fn new(base_url: Option<String>) -> Self {
        Self {
            base_url: base_url.unwrap_or_else(|| DEFAULT_COMFYUI_URL.to_string()),
            client: reqwest::Client::new(),
            client_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    pub fn from_env() -> Option<Self> {
        std::env::var("NABA_COMFYUI_URL")
            .ok()
            .filter(|u| !u.is_empty())
            .map(|url| Self::new(Some(url)))
    }

    pub async fn is_available(&self) -> bool {
        self.client
            .get(format!("{}/system_stats", self.base_url))
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .is_ok()
    }

    pub async fn submit_workflow(&self, workflow: serde_json::Value) -> Result<String> {
        let body = serde_json::json!({
            "client_id": self.client_id,
            "prompt": workflow,
        });
        let resp = self
            .client
            .post(format!("{}/prompt", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| NyayaError::Config(format!("ComfyUI submit error: {e}")))?;
        let pr: PromptResponse = resp
            .json()
            .await
            .map_err(|e| NyayaError::Config(format!("ComfyUI submit parse error: {e}")))?;
        Ok(pr.prompt_id)
    }

    pub async fn poll_until_done(&self, prompt_id: &str) -> Result<Vec<String>> {
        for _ in 0..MAX_POLL_ATTEMPTS {
            tokio::time::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS)).await;
            let resp = self
                .client
                .get(format!("{}/history/{}", self.base_url, prompt_id))
                .send()
                .await
                .map_err(|e| NyayaError::Config(format!("ComfyUI poll error: {e}")))?;
            let json: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| NyayaError::Config(format!("ComfyUI poll parse error: {e}")))?;
            if let Some(entry) = json.get(prompt_id) {
                let mut filenames = Vec::new();
                if let Some(outputs) = entry.get("outputs") {
                    if let Some(obj) = outputs.as_object() {
                        for (_node_id, node_output) in obj {
                            if let Some(images) = node_output.get("images") {
                                if let Some(arr) = images.as_array() {
                                    for img in arr {
                                        if let Some(filename) =
                                            img.get("filename").and_then(|f| f.as_str())
                                        {
                                            filenames.push(filename.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if !filenames.is_empty() {
                    return Ok(filenames);
                }
            }
        }
        Err(NyayaError::Config("ComfyUI prompt timed out".to_string()))
    }

    pub async fn download_output(&self, filename: &str) -> Result<Vec<u8>> {
        let bytes = self
            .client
            .get(format!(
                "{}/view?filename={}&type=output",
                self.base_url, filename
            ))
            .send()
            .await
            .map_err(|e| NyayaError::Config(format!("ComfyUI download error: {e}")))?
            .bytes()
            .await
            .map_err(|e| NyayaError::Config(format!("ComfyUI download read error: {e}")))?;
        Ok(bytes.to_vec())
    }

    pub fn build_prompt_body(client_id: &str, workflow: &serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "client_id": client_id,
            "prompt": workflow,
        })
    }

    pub fn output_url(base_url: &str, filename: &str) -> String {
        format!("{}/view?filename={}&type=output", base_url, filename)
    }
}

#[async_trait]
impl ImageGenerator for ComfyUiClient {
    fn provider_name(&self) -> &str {
        "comfyui"
    }

    async fn generate(&self, prompt: &str, config: &ImageConfig) -> Result<ImageResult> {
        let workflow = serde_json::json!({
            "4": {"class_type": "CheckpointLoaderSimple", "inputs": {"ckpt_name": "v1-5-pruned-emaonly.ckpt"}},
            "5": {"class_type": "EmptyLatentImage", "inputs": {"width": config.width, "height": config.height, "batch_size": 1}},
            "6": {"class_type": "CLIPTextEncode", "inputs": {"text": prompt, "clip": ["4", 1]}},
            "7": {"class_type": "CLIPTextEncode", "inputs": {"text": "ugly, blurry, low quality", "clip": ["4", 1]}},
            "3": {"class_type": "KSampler", "inputs": {
                "model": ["4", 0], "positive": ["6", 0], "negative": ["7", 0],
                "latent_image": ["5", 0], "seed": 42, "steps": 20, "cfg": 7.5,
                "sampler_name": "euler", "scheduler": "normal", "denoise": 1.0
            }},
            "8": {"class_type": "VAEDecode", "inputs": {"samples": ["3", 0], "vae": ["4", 2]}},
            "9": {"class_type": "SaveImage", "inputs": {"images": ["8", 0], "filename_prefix": "nyaya"}}
        });
        let prompt_id = self.submit_workflow(workflow).await?;
        let filenames = self.poll_until_done(&prompt_id).await?;
        let filename = filenames
            .first()
            .ok_or_else(|| NyayaError::Config("ComfyUI returned no output files".to_string()))?;
        let data = self.download_output(filename).await?;
        Ok(ImageResult {
            data,
            mime_type: "image/png".to_string(),
            width: config.width,
            height: config.height,
            provider: "comfyui".to_string(),
            cost_usd: 0.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_body_structure() {
        let workflow = serde_json::json!({"4": {"class_type": "Test"}});
        let body = ComfyUiClient::build_prompt_body("client-123", &workflow);
        assert_eq!(body["client_id"], "client-123");
        assert!(body["prompt"]["4"].is_object());
    }

    #[test]
    fn test_output_url() {
        let url = ComfyUiClient::output_url("http://localhost:8188", "output_00001.png");
        assert_eq!(
            url,
            "http://localhost:8188/view?filename=output_00001.png&type=output"
        );
    }

    #[test]
    fn test_history_parse_filenames() {
        let json: serde_json::Value = serde_json::json!({
            "abc123": {
                "outputs": {
                    "9": {
                        "images": [
                            {"filename": "nyaya_00001.png", "subfolder": "", "type": "output"}
                        ]
                    }
                }
            }
        });
        let entry = json.get("abc123").unwrap();
        let outputs = entry.get("outputs").unwrap().as_object().unwrap();
        let mut filenames = Vec::new();
        for (_node_id, node_output) in outputs {
            if let Some(images) = node_output.get("images").and_then(|v| v.as_array()) {
                for img in images {
                    if let Some(f) = img.get("filename").and_then(|v| v.as_str()) {
                        filenames.push(f.to_string());
                    }
                }
            }
        }
        assert_eq!(filenames, vec!["nyaya_00001.png"]);
    }
}
