//! OpenAI media client — DALL-E 3, Sora 2, TTS.
//! Reuses existing NABA_LLM_API_KEY.

use crate::core::error::{NyayaError, Result};
use crate::media::traits::*;
use async_trait::async_trait;
use serde::Deserialize;

const OPENAI_API_BASE: &str = "https://api.openai.com/v1";

pub struct OpenAiMediaClient {
    api_key: String,
    client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct DalleResponse {
    data: Vec<DalleImage>,
}

#[derive(Debug, Deserialize)]
struct DalleImage {
    #[allow(dead_code)]
    url: Option<String>,
    b64_json: Option<String>,
}

impl OpenAiMediaClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    pub fn from_env() -> Option<Self> {
        std::env::var("NABA_LLM_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())
            .map(Self::new)
    }

    pub fn build_dalle_body(prompt: &str, config: &ImageConfig) -> serde_json::Value {
        let size = match (config.width, config.height) {
            (w, h) if w == h => format!("{}x{}", w, h),
            (w, h) if w > h => "1792x1024".to_string(),
            _ => "1024x1792".to_string(),
        };
        let quality = match config.quality {
            ImageQuality::Standard => "standard",
            ImageQuality::Hd => "hd",
        };
        serde_json::json!({
            "model": "dall-e-3",
            "prompt": prompt,
            "n": 1,
            "size": size,
            "quality": quality,
            "response_format": "b64_json",
        })
    }

    pub fn build_tts_body(text: &str, config: &AudioConfig) -> serde_json::Value {
        let format = match config.format {
            AudioFormat::Mp3 => "mp3",
            AudioFormat::Wav => "wav",
            AudioFormat::Opus => "opus",
            AudioFormat::Flac => "flac",
        };
        serde_json::json!({
            "model": "tts-1",
            "input": text,
            "voice": config.voice,
            "speed": config.speed,
            "response_format": format,
        })
    }
}

#[async_trait]
impl ImageGenerator for OpenAiMediaClient {
    fn provider_name(&self) -> &str {
        "openai"
    }

    async fn generate(&self, prompt: &str, config: &ImageConfig) -> Result<ImageResult> {
        let body = Self::build_dalle_body(prompt, config);
        let resp = self
            .client
            .post(format!("{}/images/generations", OPENAI_API_BASE))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| NyayaError::Config(format!("OpenAI image error: {e}")))?;
        let dalle: DalleResponse = resp
            .json()
            .await
            .map_err(|e| NyayaError::Config(format!("OpenAI image parse error: {e}")))?;
        let image = dalle
            .data
            .first()
            .ok_or_else(|| NyayaError::Config("OpenAI returned no images".to_string()))?;
        let data = if let Some(ref b64) = image.b64_json {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| NyayaError::Config(format!("base64 decode error: {e}")))?
        } else {
            return Err(NyayaError::Config(
                "OpenAI image: no b64_json in response".to_string(),
            ));
        };
        let cost = match config.quality {
            ImageQuality::Standard => 0.04,
            ImageQuality::Hd => 0.08,
        };
        Ok(ImageResult {
            data,
            mime_type: "image/png".to_string(),
            width: config.width,
            height: config.height,
            provider: "openai".to_string(),
            cost_usd: cost,
        })
    }
}

#[async_trait]
impl AudioGenerator for OpenAiMediaClient {
    fn provider_name(&self) -> &str {
        "openai"
    }

    async fn text_to_speech(&self, text: &str, config: &AudioConfig) -> Result<AudioResult> {
        let body = Self::build_tts_body(text, config);
        let resp = self
            .client
            .post(format!("{}/audio/speech", OPENAI_API_BASE))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| NyayaError::Config(format!("OpenAI TTS error: {e}")))?;
        let data = resp
            .bytes()
            .await
            .map_err(|e| NyayaError::Config(format!("OpenAI TTS read error: {e}")))?
            .to_vec();
        let chars = text.len() as f64;
        let cost = chars * 15.0 / 1_000_000.0;
        let duration_estimate = chars / 15.0;
        Ok(AudioResult {
            data,
            format: config.format,
            duration_secs: duration_estimate as f32,
            provider: "openai".to_string(),
            cost_usd: cost,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dalle_body_standard_square() {
        let config = ImageConfig::default();
        let body = OpenAiMediaClient::build_dalle_body("a cat", &config);
        assert_eq!(body["model"], "dall-e-3");
        assert_eq!(body["size"], "1024x1024");
        assert_eq!(body["quality"], "standard");
        assert_eq!(body["response_format"], "b64_json");
    }

    #[test]
    fn test_tts_body_defaults() {
        let config = AudioConfig::default();
        let body = OpenAiMediaClient::build_tts_body("Hello world", &config);
        assert_eq!(body["model"], "tts-1");
        assert_eq!(body["voice"], "alloy");
        assert_eq!(body["speed"], 1.0);
        assert_eq!(body["response_format"], "mp3");
    }

    #[test]
    fn test_from_env_missing_key() {
        unsafe { std::env::remove_var("NABA_LLM_API_KEY"); }
        assert!(OpenAiMediaClient::from_env().is_none());
    }
}
