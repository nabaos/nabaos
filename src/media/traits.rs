//! Media generation traits and shared types.

use crate::core::error::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Image
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageConfig {
    pub width: u32,
    pub height: u32,
    pub quality: ImageQuality,
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 1024,
            quality: ImageQuality::Standard,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageQuality {
    Standard,
    Hd,
}

#[derive(Debug, Clone)]
pub struct ImageResult {
    pub data: Vec<u8>,
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
    pub provider: String,
    pub cost_usd: f64,
}

#[async_trait]
pub trait ImageGenerator: Send + Sync {
    fn provider_name(&self) -> &str;
    async fn generate(&self, prompt: &str, config: &ImageConfig) -> Result<ImageResult>;
}

// ---------------------------------------------------------------------------
// Video
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoConfig {
    pub duration_secs: u8,
    pub resolution: VideoResolution,
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            duration_secs: 10,
            resolution: VideoResolution::Hd1080,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoResolution {
    Hd720,
    Hd1080,
    Uhd4k,
}

impl VideoResolution {
    pub fn label(&self) -> &str {
        match self {
            Self::Hd720 => "720p",
            Self::Hd1080 => "1080p",
            Self::Uhd4k => "4K",
        }
    }
}

#[derive(Debug, Clone)]
pub struct VideoResult {
    pub data: Vec<u8>,
    pub duration_secs: f32,
    pub provider: String,
    pub cost_usd: f64,
}

#[async_trait]
pub trait VideoGenerator: Send + Sync {
    fn provider_name(&self) -> &str;
    async fn text_to_video(&self, prompt: &str, config: &VideoConfig) -> Result<VideoResult>;
    async fn image_to_video(
        &self,
        image: &[u8],
        prompt: &str,
        config: &VideoConfig,
    ) -> Result<VideoResult>;
}

// ---------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub voice: String,
    pub speed: f32,
    pub format: AudioFormat,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            voice: "alloy".to_string(),
            speed: 1.0,
            format: AudioFormat::Mp3,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioFormat {
    Mp3,
    Wav,
    Opus,
    Flac,
}

impl AudioFormat {
    pub fn extension(&self) -> &str {
        match self {
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
            Self::Opus => "opus",
            Self::Flac => "flac",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioResult {
    pub data: Vec<u8>,
    pub format: AudioFormat,
    pub duration_secs: f32,
    pub provider: String,
    pub cost_usd: f64,
}

#[async_trait]
pub trait AudioGenerator: Send + Sync {
    fn provider_name(&self) -> &str;
    async fn text_to_speech(&self, text: &str, config: &AudioConfig) -> Result<AudioResult>;
}

// ---------------------------------------------------------------------------
// Media provider preference (persisted)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MediaPreferences {
    pub image_provider: Option<String>,
    pub video_provider: Option<String>,
    pub audio_provider: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_config_defaults() {
        let config = ImageConfig::default();
        assert_eq!(config.width, 1024);
        assert_eq!(config.height, 1024);
    }

    #[test]
    fn test_video_config_defaults() {
        let config = VideoConfig::default();
        assert_eq!(config.duration_secs, 10);
    }

    #[test]
    fn test_audio_format_extension() {
        assert_eq!(AudioFormat::Mp3.extension(), "mp3");
        assert_eq!(AudioFormat::Wav.extension(), "wav");
        assert_eq!(AudioFormat::Opus.extension(), "opus");
        assert_eq!(AudioFormat::Flac.extension(), "flac");
    }
}
