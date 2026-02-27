//! Runway Gen-4 client — best multi-shot video looping support.
//!
//! REST API: POST submit → GET poll → download result.
//! Supports keyframe_position for first/last frame pinning.

use crate::core::error::{NyayaError, Result};
use crate::media::traits::*;
use async_trait::async_trait;
use serde::Deserialize;

const RUNWAY_API_BASE: &str = "https://api.dev.runwayml.com/v1";
const RUNWAY_API_VERSION: &str = "2024-11-06";
const POLL_INTERVAL_MS: u64 = 5000;
const MAX_POLL_ATTEMPTS: u32 = 120;

pub struct RunwayClient {
    api_key: String,
    client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct RunwayTaskResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RunwayTaskStatus {
    id: String,
    status: String,
    output: Option<Vec<String>>,
}

impl RunwayClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    async fn submit_task(&self, endpoint: &str, body: serde_json::Value) -> Result<String> {
        let url = format!("{}/{}", RUNWAY_API_BASE, endpoint);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("X-Runway-Version", RUNWAY_API_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(|e| NyayaError::Config(format!("Runway submit error: {e}")))?;
        let task: RunwayTaskResponse = resp
            .json()
            .await
            .map_err(|e| NyayaError::Config(format!("Runway submit parse error: {e}")))?;
        Ok(task.id)
    }

    async fn poll_until_done(&self, task_id: &str) -> Result<Vec<String>> {
        let url = format!("{}/tasks/{}", RUNWAY_API_BASE, task_id);
        for _ in 0..MAX_POLL_ATTEMPTS {
            tokio::time::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS)).await;
            let resp = self
                .client
                .get(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("X-Runway-Version", RUNWAY_API_VERSION)
                .send()
                .await
                .map_err(|e| NyayaError::Config(format!("Runway poll error: {e}")))?;
            let status: RunwayTaskStatus = resp
                .json()
                .await
                .map_err(|e| NyayaError::Config(format!("Runway status parse error: {e}")))?;
            match status.status.as_str() {
                "SUCCEEDED" => {
                    return status.output.ok_or_else(|| {
                        NyayaError::Config("Runway succeeded but no output".to_string())
                    });
                }
                "FAILED" => {
                    return Err(NyayaError::Config("Runway task failed".to_string()));
                }
                _ => continue,
            }
        }
        Err(NyayaError::Config("Runway task timed out".to_string()))
    }

    async fn download_url(&self, url: &str) -> Result<Vec<u8>> {
        let bytes = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| NyayaError::Config(format!("Runway download error: {e}")))?
            .bytes()
            .await
            .map_err(|e| NyayaError::Config(format!("Runway download read error: {e}")))?;
        Ok(bytes.to_vec())
    }

    pub fn build_text_to_video_body(prompt: &str, duration_secs: u8) -> serde_json::Value {
        serde_json::json!({
            "promptText": prompt,
            "duration": duration_secs,
        })
    }

    pub fn build_image_to_video_body(
        image_url: &str,
        prompt: &str,
        duration_secs: u8,
        keyframe_position: &str,
    ) -> serde_json::Value {
        serde_json::json!({
            "promptImage": image_url,
            "promptText": prompt,
            "duration": duration_secs,
            "keyframe_position": keyframe_position,
        })
    }
}

#[async_trait]
impl VideoGenerator for RunwayClient {
    fn provider_name(&self) -> &str {
        "runway"
    }

    async fn text_to_video(&self, prompt: &str, config: &VideoConfig) -> Result<VideoResult> {
        let body = Self::build_text_to_video_body(prompt, config.duration_secs);
        let task_id = self.submit_task("text-to-video", body).await?;
        let outputs = self.poll_until_done(&task_id).await?;
        let video_url = outputs
            .first()
            .ok_or_else(|| NyayaError::Config("Runway returned no output URLs".to_string()))?;
        let data = self.download_url(video_url).await?;
        Ok(VideoResult {
            data,
            duration_secs: config.duration_secs as f32,
            provider: "runway".to_string(),
            cost_usd: config.duration_secs as f64 * 0.12,
        })
    }

    async fn image_to_video(
        &self,
        image: &[u8],
        prompt: &str,
        config: &VideoConfig,
    ) -> Result<VideoResult> {
        let image_b64 = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(image)
        };
        let image_url = format!("data:image/png;base64,{}", image_b64);
        let body =
            Self::build_image_to_video_body(&image_url, prompt, config.duration_secs, "first");
        let task_id = self.submit_task("image-to-video", body).await?;
        let outputs = self.poll_until_done(&task_id).await?;
        let video_url = outputs
            .first()
            .ok_or_else(|| NyayaError::Config("Runway returned no output URLs".to_string()))?;
        let data = self.download_url(video_url).await?;
        Ok(VideoResult {
            data,
            duration_secs: config.duration_secs as f32,
            provider: "runway".to_string(),
            cost_usd: config.duration_secs as f64 * 0.12,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runway_api_version_header() {
        assert_eq!(RUNWAY_API_VERSION, "2024-11-06");
    }

    #[test]
    fn test_text_to_video_body() {
        let body = RunwayClient::build_text_to_video_body("a sunset over mountains", 10);
        assert_eq!(body["promptText"], "a sunset over mountains");
        assert_eq!(body["duration"], 10);
    }

    #[test]
    fn test_image_to_video_body_includes_keyframe() {
        let body = RunwayClient::build_image_to_video_body(
            "https://example.com/img.png",
            "continue the scene",
            5,
            "last",
        );
        assert_eq!(body["keyframe_position"], "last");
        assert_eq!(body["promptImage"], "https://example.com/img.png");
    }

    #[test]
    fn test_task_status_parse_succeeded() {
        let json = r#"{"id": "task-1", "status": "SUCCEEDED", "output": ["https://cdn.runway.com/video.mp4"]}"#;
        let status: RunwayTaskStatus = serde_json::from_str(json).unwrap();
        assert_eq!(status.status, "SUCCEEDED");
        assert_eq!(status.output.unwrap().len(), 1);
    }
}
