//! fal.ai gateway client — single API key for 600+ models.
//!
//! Queue pattern: POST /queue.fal.run/{model} → poll status → GET result.

use crate::core::error::{NyayaError, Result};
use crate::media::traits::*;
use async_trait::async_trait;
use serde::Deserialize;

const FAL_QUEUE_BASE: &str = "https://queue.fal.run";
const DEFAULT_IMAGE_MODEL: &str = "fal-ai/flux/dev";
const DEFAULT_VIDEO_MODEL: &str = "fal-ai/kling-video/v2/master";
const POLL_INTERVAL_MS: u64 = 3000;
const MAX_POLL_ATTEMPTS: u32 = 120;

pub struct FalClient {
    api_key: String,
    client: reqwest::Client,
    image_model: String,
    video_model: String,
}

#[derive(Debug, Deserialize)]
struct QueueResponse {
    request_id: String,
}

#[derive(Debug, Deserialize)]
struct StatusResponse {
    status: String,
}

#[derive(Debug, Deserialize)]
struct FalImageOutput {
    images: Vec<FalImage>,
}

#[derive(Debug, Deserialize)]
struct FalImage {
    url: String,
    width: u32,
    height: u32,
    content_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FalVideoOutput {
    video: FalVideo,
}

#[derive(Debug, Deserialize)]
struct FalVideo {
    url: String,
}

impl FalClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
            image_model: DEFAULT_IMAGE_MODEL.to_string(),
            video_model: DEFAULT_VIDEO_MODEL.to_string(),
        }
    }

    pub fn with_models(api_key: String, image_model: String, video_model: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
            image_model,
            video_model,
        }
    }

    async fn submit(&self, model: &str, body: serde_json::Value) -> Result<String> {
        let url = format!("{}/{}", FAL_QUEUE_BASE, model);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Key {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| NyayaError::Config(format!("fal.ai submit error: {e}")))?;
        let queue: QueueResponse = resp
            .json()
            .await
            .map_err(|e| NyayaError::Config(format!("fal.ai submit parse error: {e}")))?;
        Ok(queue.request_id)
    }

    async fn poll_until_done(&self, model: &str, request_id: &str) -> Result<serde_json::Value> {
        let status_url = format!(
            "{}/{}/requests/{}/status",
            FAL_QUEUE_BASE, model, request_id
        );
        let result_url = format!("{}/{}/requests/{}", FAL_QUEUE_BASE, model, request_id);
        for _ in 0..MAX_POLL_ATTEMPTS {
            tokio::time::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS)).await;
            let resp = self
                .client
                .get(&status_url)
                .header("Authorization", format!("Key {}", self.api_key))
                .send()
                .await
                .map_err(|e| NyayaError::Config(format!("fal.ai poll error: {e}")))?;
            let status: StatusResponse = resp
                .json()
                .await
                .map_err(|e| NyayaError::Config(format!("fal.ai status parse error: {e}")))?;
            match status.status.as_str() {
                "COMPLETED" => {
                    let result = self
                        .client
                        .get(&result_url)
                        .header("Authorization", format!("Key {}", self.api_key))
                        .send()
                        .await
                        .map_err(|e| NyayaError::Config(format!("fal.ai result error: {e}")))?;
                    let json: serde_json::Value = result.json().await.map_err(|e| {
                        NyayaError::Config(format!("fal.ai result parse error: {e}"))
                    })?;
                    return Ok(json);
                }
                "FAILED" | "CANCELLED" => {
                    return Err(NyayaError::Config(format!(
                        "fal.ai request {}: {}",
                        status.status, request_id
                    )));
                }
                _ => continue,
            }
        }
        Err(NyayaError::Config("fal.ai request timed out".to_string()))
    }

    async fn download_url(&self, url: &str) -> Result<Vec<u8>> {
        let bytes = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| NyayaError::Config(format!("fal.ai download error: {e}")))?
            .bytes()
            .await
            .map_err(|e| NyayaError::Config(format!("fal.ai download read error: {e}")))?;
        Ok(bytes.to_vec())
    }

    pub fn queue_url(model: &str) -> String {
        format!("{}/{}", FAL_QUEUE_BASE, model)
    }

    pub fn status_url(model: &str, request_id: &str) -> String {
        format!(
            "{}/{}/requests/{}/status",
            FAL_QUEUE_BASE, model, request_id
        )
    }
}

#[async_trait]
impl ImageGenerator for FalClient {
    fn provider_name(&self) -> &str {
        "fal.ai"
    }

    async fn generate(&self, prompt: &str, config: &ImageConfig) -> Result<ImageResult> {
        let body = serde_json::json!({
            "prompt": prompt,
            "image_size": {
                "width": config.width,
                "height": config.height,
            },
        });
        let request_id = self.submit(&self.image_model, body).await?;
        let result = self.poll_until_done(&self.image_model, &request_id).await?;
        let output: FalImageOutput = serde_json::from_value(result)
            .map_err(|e| NyayaError::Config(format!("fal.ai image parse error: {e}")))?;
        let image = output
            .images
            .first()
            .ok_or_else(|| NyayaError::Config("fal.ai returned no images".to_string()))?;
        let data = self.download_url(&image.url).await?;
        Ok(ImageResult {
            data,
            mime_type: image
                .content_type
                .clone()
                .unwrap_or_else(|| "image/png".to_string()),
            width: image.width,
            height: image.height,
            provider: "fal.ai".to_string(),
            cost_usd: 0.025,
        })
    }
}

#[async_trait]
impl VideoGenerator for FalClient {
    fn provider_name(&self) -> &str {
        "fal.ai"
    }

    async fn text_to_video(&self, prompt: &str, config: &VideoConfig) -> Result<VideoResult> {
        let body = serde_json::json!({
            "prompt": prompt,
            "duration": config.duration_secs.to_string(),
        });
        let request_id = self.submit(&self.video_model, body).await?;
        let result = self.poll_until_done(&self.video_model, &request_id).await?;
        let output: FalVideoOutput = serde_json::from_value(result)
            .map_err(|e| NyayaError::Config(format!("fal.ai video parse error: {e}")))?;
        let data = self.download_url(&output.video.url).await?;
        Ok(VideoResult {
            data,
            duration_secs: config.duration_secs as f32,
            provider: "fal.ai".to_string(),
            cost_usd: config.duration_secs as f64 * 0.10,
        })
    }

    async fn image_to_video(
        &self,
        image: &[u8],
        prompt: &str,
        config: &VideoConfig,
    ) -> Result<VideoResult> {
        let image_b64 = base64_encode(image);
        let body = serde_json::json!({
            "prompt": prompt,
            "image_url": format!("data:image/png;base64,{}", image_b64),
            "duration": config.duration_secs.to_string(),
        });
        let request_id = self.submit(&self.video_model, body).await?;
        let result = self.poll_until_done(&self.video_model, &request_id).await?;
        let output: FalVideoOutput = serde_json::from_value(result)
            .map_err(|e| NyayaError::Config(format!("fal.ai video parse error: {e}")))?;
        let data = self.download_url(&output.video.url).await?;
        Ok(VideoResult {
            data,
            duration_secs: config.duration_secs as f32,
            provider: "fal.ai".to_string(),
            cost_usd: config.duration_secs as f64 * 0.10,
        })
    }
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queue_url_image_model() {
        let url = FalClient::queue_url("fal-ai/flux/dev");
        assert_eq!(url, "https://queue.fal.run/fal-ai/flux/dev");
    }

    #[test]
    fn test_queue_url_video_model() {
        let url = FalClient::queue_url("fal-ai/kling-video/v2/master");
        assert_eq!(url, "https://queue.fal.run/fal-ai/kling-video/v2/master");
    }

    #[test]
    fn test_status_url() {
        let url = FalClient::status_url("fal-ai/flux/dev", "req-123");
        assert_eq!(
            url,
            "https://queue.fal.run/fal-ai/flux/dev/requests/req-123/status"
        );
    }

    #[test]
    fn test_poll_response_parse_completed() {
        let json = r#"{"status": "COMPLETED"}"#;
        let status: StatusResponse = serde_json::from_str(json).unwrap();
        assert_eq!(status.status, "COMPLETED");
    }
}
