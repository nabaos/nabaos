//! ElevenLabs TTS client — voice cloning and streaming speech synthesis.

use crate::core::error::{NyayaError, Result};
use crate::media::traits::*;
use async_trait::async_trait;
use serde::Deserialize;

const ELEVENLABS_API_BASE: &str = "https://api.elevenlabs.io/v1";
const DEFAULT_MODEL: &str = "eleven_flash_v2_5";
const DEFAULT_VOICE_ID: &str = "21m00Tcm4TlvDq8ikWAM";

pub struct ElevenLabsClient {
    api_key: String,
    client: reqwest::Client,
    default_voice_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ElevenLabsVoice {
    pub voice_id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
struct VoiceListResponse {
    voices: Vec<ElevenLabsVoice>,
}

impl ElevenLabsClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
            default_voice_id: DEFAULT_VOICE_ID.to_string(),
        }
    }

    pub fn with_voice(api_key: String, voice_id: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
            default_voice_id: voice_id,
        }
    }

    pub fn from_env() -> Option<Self> {
        std::env::var("NABA_ELEVENLABS_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())
            .map(Self::new)
    }

    pub fn build_tts_body(text: &str, _speed: f32) -> serde_json::Value {
        serde_json::json!({
            "text": text,
            "model_id": DEFAULT_MODEL,
            "voice_settings": {
                "stability": 0.5,
                "similarity_boost": 0.75,
            },
        })
    }

    pub async fn list_voices(&self) -> Result<Vec<ElevenLabsVoice>> {
        let resp = self
            .client
            .get(format!("{}/voices", ELEVENLABS_API_BASE))
            .header("xi-api-key", &self.api_key)
            .send()
            .await
            .map_err(|e| NyayaError::Config(format!("ElevenLabs voices error: {e}")))?;
        let list: VoiceListResponse = resp
            .json()
            .await
            .map_err(|e| NyayaError::Config(format!("ElevenLabs voices parse error: {e}")))?;
        Ok(list.voices)
    }
}

#[async_trait]
impl AudioGenerator for ElevenLabsClient {
    fn provider_name(&self) -> &str {
        "elevenlabs"
    }

    async fn text_to_speech(&self, text: &str, config: &AudioConfig) -> Result<AudioResult> {
        let voice_id = if config.voice == "alloy" || config.voice.is_empty() {
            &self.default_voice_id
        } else {
            &config.voice
        };
        let body = Self::build_tts_body(text, config.speed);
        let resp = self
            .client
            .post(format!(
                "{}/text-to-speech/{}",
                ELEVENLABS_API_BASE, voice_id
            ))
            .header("xi-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| NyayaError::Config(format!("ElevenLabs TTS error: {e}")))?;
        let data = resp
            .bytes()
            .await
            .map_err(|e| NyayaError::Config(format!("ElevenLabs TTS read error: {e}")))?
            .to_vec();
        let chars = text.len() as f64;
        let cost = chars * 0.30 / 1000.0;
        let duration_estimate = chars / 15.0;
        Ok(AudioResult {
            data,
            format: AudioFormat::Mp3,
            duration_secs: duration_estimate as f32,
            provider: "elevenlabs".to_string(),
            cost_usd: cost,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tts_body_structure() {
        let body = ElevenLabsClient::build_tts_body("Hello world", 1.0);
        assert_eq!(body["model_id"], "eleven_flash_v2_5");
        assert!(body["voice_settings"]["stability"].as_f64().is_some());
    }

    #[test]
    fn test_voice_list_parse() {
        let json = r#"{"voices": [{"voice_id": "abc", "name": "Rachel"}]}"#;
        let list: VoiceListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(list.voices.len(), 1);
        assert_eq!(list.voices[0].name, "Rachel");
    }

    #[test]
    fn test_from_env_missing_key() {
        std::env::remove_var("NABA_ELEVENLABS_API_KEY");
        assert!(ElevenLabsClient::from_env().is_none());
    }
}
