//! MediaEngine — routes media generation requests to the best available provider.

use crate::core::error::{NyayaError, Result};
use crate::media::providers::comfyui::ComfyUiClient;
use crate::media::providers::elevenlabs::ElevenLabsClient;
use crate::media::providers::fal::FalClient;
use crate::media::providers::openai_media::OpenAiMediaClient;
use crate::media::providers::runway::RunwayClient;
use crate::media::traits::*;
use std::sync::Arc;

/// The unified media engine. Routes to the best available provider.
pub struct MediaEngine {
    pub image_providers: Vec<Arc<dyn ImageGenerator>>,
    pub video_providers: Vec<Arc<dyn VideoGenerator>>,
    pub audio_providers: Vec<Arc<dyn AudioGenerator>>,
    preferences: MediaPreferences,
}

impl MediaEngine {
    /// Build a MediaEngine from environment variables.
    /// Providers are registered in priority order: local > cheapest > quality.
    pub fn from_env() -> Self {
        let mut image_providers: Vec<Arc<dyn ImageGenerator>> = Vec::new();
        let mut video_providers: Vec<Arc<dyn VideoGenerator>> = Vec::new();
        let mut audio_providers: Vec<Arc<dyn AudioGenerator>> = Vec::new();

        // ComfyUI (local, free) — highest priority
        if let Some(client) = ComfyUiClient::from_env() {
            image_providers.push(Arc::new(client));
        }

        // fal.ai (gateway, cheap) — second priority
        if let Ok(key) = std::env::var("NABA_FAL_API_KEY") {
            if !key.is_empty() {
                let fal = Arc::new(FalClient::new(key));
                image_providers.push(fal.clone());
                video_providers.push(fal);
            }
        }

        // Runway (direct, quality video)
        if let Ok(key) = std::env::var("NABA_RUNWAY_API_KEY") {
            if !key.is_empty() {
                video_providers.push(Arc::new(RunwayClient::new(key)));
            }
        }

        // OpenAI (reuse existing key)
        if let Some(client) = OpenAiMediaClient::from_env() {
            let client = Arc::new(client);
            image_providers.push(client.clone());
            audio_providers.push(client);
        }

        // ElevenLabs (quality TTS)
        if let Some(client) = ElevenLabsClient::from_env() {
            audio_providers.push(Arc::new(client));
        }

        Self {
            image_providers,
            video_providers,
            audio_providers,
            preferences: MediaPreferences::default(),
        }
    }

    /// Set provider preferences (loaded from SQLite or user choice).
    pub fn with_preferences(mut self, prefs: MediaPreferences) -> Self {
        self.preferences = prefs;
        self
    }

    /// Get the preferred or first available image generator.
    pub fn image(&self) -> Result<&dyn ImageGenerator> {
        if let Some(ref name) = self.preferences.image_provider {
            if let Some(p) = self
                .image_providers
                .iter()
                .find(|p| p.provider_name() == name)
            {
                return Ok(p.as_ref());
            }
        }
        self.image_providers
            .first()
            .map(|p| p.as_ref())
            .ok_or_else(|| {
                NyayaError::Config(
                    "No image provider configured. Add NABA_FAL_API_KEY or NABA_LLM_API_KEY via 'nabaos init'.".to_string(),
                )
            })
    }

    /// Get the preferred or first available video generator.
    pub fn video(&self) -> Result<&dyn VideoGenerator> {
        if let Some(ref name) = self.preferences.video_provider {
            if let Some(p) = self
                .video_providers
                .iter()
                .find(|p| p.provider_name() == name)
            {
                return Ok(p.as_ref());
            }
        }
        self.video_providers
            .first()
            .map(|p| p.as_ref())
            .ok_or_else(|| {
                NyayaError::Config(
                    "No video provider configured. Add NABA_FAL_API_KEY or NABA_RUNWAY_API_KEY via 'nabaos init'.".to_string(),
                )
            })
    }

    /// Get the preferred or first available audio generator.
    pub fn audio(&self) -> Result<&dyn AudioGenerator> {
        if let Some(ref name) = self.preferences.audio_provider {
            if let Some(p) = self
                .audio_providers
                .iter()
                .find(|p| p.provider_name() == name)
            {
                return Ok(p.as_ref());
            }
        }
        self.audio_providers
            .first()
            .map(|p| p.as_ref())
            .ok_or_else(|| {
                NyayaError::Config(
                    "No audio provider configured. Add NABA_LLM_API_KEY or NABA_ELEVENLABS_API_KEY via 'nabaos init'.".to_string(),
                )
            })
    }

    /// List all configured provider names by media type.
    pub fn list_providers(&self) -> MediaProviderList {
        MediaProviderList {
            image: self
                .image_providers
                .iter()
                .map(|p| p.provider_name().to_string())
                .collect(),
            video: self
                .video_providers
                .iter()
                .map(|p| p.provider_name().to_string())
                .collect(),
            audio: self
                .audio_providers
                .iter()
                .map(|p| p.provider_name().to_string())
                .collect(),
        }
    }

    /// Check if any media provider is configured.
    pub fn has_any_provider(&self) -> bool {
        !self.image_providers.is_empty()
            || !self.video_providers.is_empty()
            || !self.audio_providers.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct MediaProviderList {
    pub image: Vec<String>,
    pub video: Vec<String>,
    pub audio: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_no_providers_returns_error() {
        let engine = MediaEngine {
            image_providers: vec![],
            video_providers: vec![],
            audio_providers: vec![],
            preferences: MediaPreferences::default(),
        };
        assert!(engine.image().is_err());
        assert!(engine.video().is_err());
        assert!(engine.audio().is_err());
    }

    #[test]
    fn test_engine_has_any_provider_empty() {
        let engine = MediaEngine {
            image_providers: vec![],
            video_providers: vec![],
            audio_providers: vec![],
            preferences: MediaPreferences::default(),
        };
        assert!(!engine.has_any_provider());
    }

    #[test]
    fn test_provider_list_empty() {
        let engine = MediaEngine {
            image_providers: vec![],
            video_providers: vec![],
            audio_providers: vec![],
            preferences: MediaPreferences::default(),
        };
        let list = engine.list_providers();
        assert!(list.image.is_empty());
        assert!(list.video.is_empty());
        assert!(list.audio.is_empty());
    }
}
