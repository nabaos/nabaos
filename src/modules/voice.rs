// Voice module — Whisper-based audio transcription (STT) and TTS synthesis via API or local binary.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::core::error::{NyayaError, Result};
use crate::modules::profile::VoiceMode;

/// Supported audio file extensions.
const SUPPORTED_EXTENSIONS: &[&str] = &["wav", "ogg", "mp3", "m4a", "webm", "flac", "oga"];

/// Configuration for voice transcription.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    /// Processing mode — disabled, local binary, or cloud API.
    pub mode: VoiceMode,
    /// API key for OpenAI Whisper (required for Api mode).
    pub api_key: Option<String>,
    /// Whisper model name.
    pub model: String,
    /// Language hint (ISO 639-1 code).
    pub language: Option<String>,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            mode: VoiceMode::Disabled,
            api_key: None,
            model: "whisper-1".into(),
            language: None,
        }
    }
}

impl VoiceConfig {
    /// Returns true if voice input is enabled (mode is not Disabled).
    pub fn is_enabled(&self) -> bool {
        self.mode != VoiceMode::Disabled
    }

    /// Build a VoiceConfig from environment variables for the given mode.
    ///
    /// Checks NABA_OPENAI_API_KEY then OPENAI_API_KEY for the API key.
    pub fn from_env(mode: &VoiceMode) -> Self {
        let api_key = std::env::var("NABA_OPENAI_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .ok();

        VoiceConfig {
            mode: mode.clone(),
            api_key,
            model: "whisper-1".into(),
            language: None,
        }
    }
}

/// Result of a transcription operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    /// The transcribed text.
    pub text: String,
    /// Detected or specified language.
    pub language: Option<String>,
    /// Duration of the audio in seconds.
    pub duration_secs: f64,
    /// Confidence score (0.0 to 1.0) if available.
    pub confidence: Option<f64>,
}

/// Check whether a filename has a supported audio extension.
pub fn is_supported_format(filename: &str) -> bool {
    if let Some(ext) = Path::new(filename).extension() {
        let ext_lower = ext.to_string_lossy().to_lowercase();
        SUPPORTED_EXTENSIONS.contains(&ext_lower.as_str())
    } else {
        false
    }
}

/// Transcribe an audio file using the OpenAI Whisper API.
pub fn transcribe_api(audio_path: &Path, config: &VoiceConfig) -> Result<TranscriptionResult> {
    let api_key = config
        .api_key
        .as_ref()
        .ok_or_else(|| NyayaError::Config("No API key configured for Whisper API".into()))?;

    if !audio_path.exists() {
        return Err(NyayaError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Audio file not found: {}", audio_path.display()),
        )));
    }

    let filename = audio_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();

    if !is_supported_format(&filename) {
        return Err(NyayaError::Config(format!(
            "Unsupported audio format: {}",
            filename
        )));
    }

    // Read the audio file bytes.
    let file_bytes = std::fs::read(audio_path)?;

    // Determine MIME type from extension.
    let ext = audio_path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let mime = match ext.as_str() {
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "ogg" | "oga" => "audio/ogg",
        "m4a" => "audio/mp4",
        "webm" => "audio/webm",
        "flac" => "audio/flac",
        _ => "application/octet-stream",
    };

    // Build a multipart/form-data body manually since the reqwest multipart
    // feature is not enabled.
    let boundary = "----NyayaVoiceBoundary9876543210";
    let mut body = Vec::new();

    // model field
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"model\"\r\n\r\n");
    body.extend_from_slice(config.model.as_bytes());
    body.extend_from_slice(b"\r\n");

    // language field (optional)
    if let Some(ref lang) = config.language {
        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        body.extend_from_slice(b"Content-Disposition: form-data; name=\"language\"\r\n\r\n");
        body.extend_from_slice(lang.as_bytes());
        body.extend_from_slice(b"\r\n");
    }

    // file field
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\n",
            filename
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: {}\r\n\r\n", mime).as_bytes());
    body.extend_from_slice(&file_bytes);
    body.extend_from_slice(b"\r\n");

    // closing boundary
    body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

    let client = reqwest::blocking::Client::new();
    let response = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={}", boundary),
        )
        .body(body)
        .send()
        .map_err(|e| NyayaError::Config(format!("Whisper API request failed: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().unwrap_or_else(|_| "unknown error".into());
        return Err(NyayaError::Config(format!(
            "Whisper API returned {}: {}",
            status, text
        )));
    }

    let json: serde_json::Value = response
        .json()
        .map_err(|e| NyayaError::Config(format!("Failed to parse Whisper response: {}", e)))?;

    let text = json["text"].as_str().unwrap_or("").to_string();
    let language = json["language"].as_str().map(|s| s.to_string());
    let duration_secs = json["duration"].as_f64().unwrap_or(0.0);

    Ok(TranscriptionResult {
        text,
        language,
        duration_secs,
        confidence: None,
    })
}

/// Transcribe an audio file using a local whisper/whisper-cpp binary.
pub fn transcribe_local(audio_path: &Path, config: &VoiceConfig) -> Result<TranscriptionResult> {
    if !audio_path.exists() {
        return Err(NyayaError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Audio file not found: {}", audio_path.display()),
        )));
    }

    let whisper_bin = super::hardware::detect_tool(&["whisper", "whisper-cpp", "whisper.cpp"])
        .ok_or_else(|| {
            NyayaError::Config(
                "No local whisper binary found. Install whisper or whisper-cpp.".into(),
            )
        })?;

    let mut cmd = std::process::Command::new(&whisper_bin);
    cmd.arg(audio_path);
    cmd.args(["--model", &config.model]);
    cmd.arg("--output-json");

    if let Some(ref lang) = config.language {
        cmd.args(["--language", lang]);
    }

    let output = cmd
        .output()
        .map_err(|e| NyayaError::Config(format!("Failed to run whisper binary: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(NyayaError::Config(format!(
            "Whisper transcription failed: {}",
            stderr
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Try to parse as JSON first.
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
        let text = json["text"].as_str().unwrap_or("").trim().to_string();
        let language = json["language"].as_str().map(|s| s.to_string());
        let duration_secs = json["duration"].as_f64().unwrap_or(0.0);

        return Ok(TranscriptionResult {
            text,
            language,
            duration_secs,
            confidence: None,
        });
    }

    // Fallback: treat stdout as plain text transcription.
    Ok(TranscriptionResult {
        text: stdout.trim().to_string(),
        language: config.language.clone(),
        duration_secs: 0.0,
        confidence: None,
    })
}

/// Transcribe an audio file, dispatching to the appropriate backend based on mode.
pub fn transcribe(audio_path: &Path, config: &VoiceConfig) -> Result<TranscriptionResult> {
    match config.mode {
        VoiceMode::Api => transcribe_api(audio_path, config),
        VoiceMode::Local => transcribe_local(audio_path, config),
        VoiceMode::Disabled => Err(NyayaError::Config("Voice transcription is disabled".into())),
    }
}

// ---------------------------------------------------------------------------
// Text-to-Speech (TTS) — voice.speak
// ---------------------------------------------------------------------------

/// Maximum text length for TTS synthesis (OpenAI API limit).
pub const TTS_TEXT_CAP: usize = 4096;

/// Valid voice options for OpenAI TTS.
pub const VALID_VOICES: &[&str] = &["alloy", "echo", "fable", "onyx", "nova", "shimmer"];

/// Default voice when none is specified.
const DEFAULT_VOICE: &str = "alloy";

/// Default speech speed (1.0 = normal).
const DEFAULT_SPEED: f64 = 1.0;

/// Default output format.
const DEFAULT_FORMAT: &str = "mp3";

/// Valid output formats for TTS.
const VALID_FORMATS: &[&str] = &["mp3", "opus", "aac", "flac", "wav", "pcm"];

/// Configuration for text-to-speech synthesis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsConfig {
    /// Processing mode — disabled, local binary, or cloud API.
    pub mode: VoiceMode,
    /// API key for OpenAI TTS (required for Api mode).
    pub api_key: Option<String>,
    /// Voice name (alloy, echo, fable, onyx, nova, shimmer).
    pub voice: String,
    /// Speech speed (0.25 to 4.0, default 1.0).
    pub speed: f64,
    /// Output format (mp3, opus, aac, flac, wav, pcm).
    pub format: String,
    /// Model name for local TTS (piper model path).
    pub model: Option<String>,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            mode: VoiceMode::Disabled,
            api_key: None,
            voice: DEFAULT_VOICE.into(),
            speed: DEFAULT_SPEED,
            format: DEFAULT_FORMAT.into(),
            model: None,
        }
    }
}

impl TtsConfig {
    /// Returns true if TTS is enabled (mode is not Disabled).
    pub fn is_enabled(&self) -> bool {
        self.mode != VoiceMode::Disabled
    }

    /// Build a TtsConfig from environment variables for the given mode.
    pub fn from_env(mode: &VoiceMode) -> Self {
        let api_key = std::env::var("NABA_OPENAI_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .ok();

        TtsConfig {
            mode: mode.clone(),
            api_key,
            voice: DEFAULT_VOICE.into(),
            speed: DEFAULT_SPEED,
            format: DEFAULT_FORMAT.into(),
            model: None,
        }
    }

    /// Validate voice name against allowed list.
    pub fn validate_voice(voice: &str) -> std::result::Result<(), String> {
        if VALID_VOICES.contains(&voice) {
            Ok(())
        } else {
            Err(format!(
                "Invalid voice '{}'. Valid voices: {}",
                voice,
                VALID_VOICES.join(", ")
            ))
        }
    }

    /// Validate output format against allowed list.
    pub fn validate_format(format: &str) -> std::result::Result<(), String> {
        if VALID_FORMATS.contains(&format) {
            Ok(())
        } else {
            Err(format!(
                "Invalid format '{}'. Valid formats: {}",
                format,
                VALID_FORMATS.join(", ")
            ))
        }
    }
}

/// Result of a TTS synthesis operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsResult {
    /// Path to the generated audio file.
    pub path: String,
    /// Output format (mp3, wav, etc.).
    pub format: String,
    /// Size of the output file in bytes.
    pub size_bytes: u64,
    /// Estimated duration in seconds (based on text length heuristic).
    pub duration_estimate_secs: f64,
}

/// Estimate speech duration from text length.
/// Average speaking rate is ~150 words per minute, ~5 chars per word.
/// So ~750 chars/minute = ~12.5 chars/second.
fn estimate_duration(text: &str, speed: f64) -> f64 {
    let chars = text.len() as f64;
    let base_duration = chars / 12.5;
    // Speed factor: higher speed = shorter duration
    let speed = if speed > 0.0 { speed } else { 1.0 };
    base_duration / speed
}

/// Synthesize speech using the OpenAI TTS API.
///
/// POST to `https://api.openai.com/v1/audio/speech` with JSON body.
/// Response is raw audio bytes in the requested format.
pub fn synthesize_api(text: &str, config: &TtsConfig, output_path: &Path) -> Result<TtsResult> {
    let api_key = config
        .api_key
        .as_ref()
        .ok_or_else(|| NyayaError::Config("No API key configured for TTS API".into()))?;

    if text.is_empty() {
        return Err(NyayaError::Config("TTS text cannot be empty".into()));
    }

    if text.len() > TTS_TEXT_CAP {
        return Err(NyayaError::Config(format!(
            "TTS text exceeds {} char limit ({} chars provided)",
            TTS_TEXT_CAP,
            text.len()
        )));
    }

    TtsConfig::validate_voice(&config.voice).map_err(NyayaError::Config)?;

    let request_body = serde_json::json!({
        "model": "tts-1",
        "input": text,
        "voice": config.voice,
        "speed": config.speed,
        "response_format": config.format,
    });

    let client = reqwest::blocking::Client::new();
    let response = client
        .post("https://api.openai.com/v1/audio/speech")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .body(request_body.to_string())
        .send()
        .map_err(|e| NyayaError::Config(format!("TTS API request failed: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_else(|_| "unknown error".into());
        return Err(NyayaError::Config(format!(
            "TTS API returned {}: {}",
            status, body
        )));
    }

    let audio_bytes = response
        .bytes()
        .map_err(|e| NyayaError::Config(format!("Failed to read TTS response body: {}", e)))?;

    // Write audio bytes to the output file
    std::fs::write(output_path, &audio_bytes)?;

    let size_bytes = audio_bytes.len() as u64;
    let duration_estimate = estimate_duration(text, config.speed);

    Ok(TtsResult {
        path: output_path.to_string_lossy().to_string(),
        format: config.format.clone(),
        size_bytes,
        duration_estimate_secs: duration_estimate,
    })
}

/// Synthesize speech using a local piper binary.
///
/// Pipes text to stdin and writes output WAV to the specified path.
/// Uses env_clear + scoped PATH for security.
pub fn synthesize_local(text: &str, config: &TtsConfig, output_path: &Path) -> Result<TtsResult> {
    if text.is_empty() {
        return Err(NyayaError::Config("TTS text cannot be empty".into()));
    }

    if text.len() > TTS_TEXT_CAP {
        return Err(NyayaError::Config(format!(
            "TTS text exceeds {} char limit ({} chars provided)",
            TTS_TEXT_CAP,
            text.len()
        )));
    }

    let piper_bin = super::hardware::detect_tool(&["piper", "piper-tts"]).ok_or_else(|| {
        NyayaError::Config(
            "No local piper binary found. Install piper: https://github.com/rhasspy/piper".into(),
        )
    })?;

    let output_str = output_path.to_string_lossy().to_string();

    let mut cmd = std::process::Command::new(&piper_bin);

    // Security: clear environment, set only a scoped PATH
    cmd.env_clear();
    if let Some(parent) = piper_bin.parent() {
        cmd.env("PATH", parent);
    } else {
        cmd.env("PATH", "/usr/bin:/usr/local/bin");
    }

    // Pass model if configured
    if let Some(ref model) = config.model {
        cmd.args(["--model", model]);
    }

    cmd.args(["--output_file", &output_str]);

    // Pipe text to stdin
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| NyayaError::Config(format!("Failed to spawn piper binary: {}", e)))?;

    // Write text to stdin
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| NyayaError::Config(format!("Failed to write to piper stdin: {}", e)))?;
        // Drop stdin to close it, signaling EOF
    }

    let output = child
        .wait_with_output()
        .map_err(|e| NyayaError::Config(format!("Failed to wait for piper process: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(NyayaError::Config(format!("Piper TTS failed: {}", stderr)));
    }

    // Get file size
    let size_bytes = std::fs::metadata(output_path).map(|m| m.len()).unwrap_or(0);

    let duration_estimate = estimate_duration(text, config.speed);

    Ok(TtsResult {
        path: output_str,
        format: "wav".into(), // piper outputs WAV by default
        size_bytes,
        duration_estimate_secs: duration_estimate,
    })
}

/// Synthesize speech, dispatching to the appropriate backend based on mode.
pub fn synthesize(text: &str, config: &TtsConfig, output_path: &Path) -> Result<TtsResult> {
    match config.mode {
        VoiceMode::Api => synthesize_api(text, config, output_path),
        VoiceMode::Local => synthesize_local(text, config, output_path),
        VoiceMode::Disabled => Err(NyayaError::Config("Text-to-speech is disabled".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_voice_config_default_disabled() {
        let config = VoiceConfig::default();
        assert!(!config.is_enabled());
    }

    #[test]
    fn test_voice_config_api_mode() {
        let config = VoiceConfig {
            mode: super::super::profile::VoiceMode::Api,
            api_key: Some("sk-test".into()),
            model: "whisper-1".into(),
            language: None,
        };
        assert!(config.is_enabled());
        assert!(config.api_key.is_some());
    }

    #[test]
    fn test_transcription_result_structure() {
        let result = TranscriptionResult {
            text: "Hello world".into(),
            language: Some("en".into()),
            duration_secs: 1.5,
            confidence: Some(0.95),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("Hello world"));
        assert!(json.contains("0.95"));
    }

    #[test]
    fn test_supported_audio_formats() {
        assert!(is_supported_format("audio.wav"));
        assert!(is_supported_format("voice.ogg"));
        assert!(is_supported_format("recording.mp3"));
        assert!(is_supported_format("clip.m4a"));
        assert!(!is_supported_format("document.pdf"));
        assert!(!is_supported_format("image.png"));
    }

    // --- TTS Tests ---

    #[test]
    fn test_tts_config_default_disabled() {
        let config = TtsConfig::default();
        assert!(!config.is_enabled());
        assert_eq!(config.voice, "alloy");
        assert_eq!(config.speed, 1.0);
        assert_eq!(config.format, "mp3");
    }

    #[test]
    fn test_tts_config_api_mode() {
        let config = TtsConfig {
            mode: VoiceMode::Api,
            api_key: Some("sk-test".into()),
            voice: "nova".into(),
            speed: 1.5,
            format: "mp3".into(),
            model: None,
        };
        assert!(config.is_enabled());
        assert!(config.api_key.is_some());
    }

    #[test]
    fn test_tts_valid_voices() {
        for voice in VALID_VOICES {
            assert!(
                TtsConfig::validate_voice(voice).is_ok(),
                "Voice '{}' should be valid",
                voice
            );
        }
    }

    #[test]
    fn test_tts_invalid_voice_rejected() {
        assert!(TtsConfig::validate_voice("robot").is_err());
        assert!(TtsConfig::validate_voice("").is_err());
        assert!(TtsConfig::validate_voice("ALLOY").is_err()); // case-sensitive
    }

    #[test]
    fn test_tts_valid_formats() {
        for fmt in VALID_FORMATS {
            assert!(
                TtsConfig::validate_format(fmt).is_ok(),
                "Format '{}' should be valid",
                fmt
            );
        }
    }

    #[test]
    fn test_tts_invalid_format_rejected() {
        assert!(TtsConfig::validate_format("ogg").is_err());
        assert!(TtsConfig::validate_format("").is_err());
        assert!(TtsConfig::validate_format("MP3").is_err());
    }

    #[test]
    fn test_tts_text_cap_enforcement() {
        let config = TtsConfig {
            mode: VoiceMode::Api,
            api_key: Some("sk-test".into()),
            ..TtsConfig::default()
        };
        let long_text = "a".repeat(TTS_TEXT_CAP + 1);
        let path = PathBuf::from("/tmp/test_tts.mp3");
        let result = synthesize_api(&long_text, &config, &path);
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("4096"),
            "Error should mention char limit: {}",
            err
        );
    }

    #[test]
    fn test_tts_empty_text_rejected() {
        let config = TtsConfig {
            mode: VoiceMode::Api,
            api_key: Some("sk-test".into()),
            ..TtsConfig::default()
        };
        let path = PathBuf::from("/tmp/test_tts.mp3");
        let result = synthesize_api("", &config, &path);
        assert!(result.is_err());
    }

    #[test]
    fn test_tts_missing_api_key() {
        let config = TtsConfig {
            mode: VoiceMode::Api,
            api_key: None,
            ..TtsConfig::default()
        };
        let path = PathBuf::from("/tmp/test_tts.mp3");
        let result = synthesize_api("Hello", &config, &path);
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("API key"),
            "Error should mention API key: {}",
            err
        );
    }

    #[test]
    fn test_tts_disabled_mode_rejected() {
        let config = TtsConfig::default(); // mode = Disabled
        let path = PathBuf::from("/tmp/test_tts.mp3");
        let result = synthesize("Hello", &config, &path);
        assert!(result.is_err());
    }

    #[test]
    fn test_tts_result_structure() {
        let result = TtsResult {
            path: "/tmp/speech.mp3".into(),
            format: "mp3".into(),
            size_bytes: 12345,
            duration_estimate_secs: 3.2,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("speech.mp3"));
        assert!(json.contains("12345"));
    }

    #[test]
    fn test_tts_duration_estimate() {
        // ~12.5 chars/sec at speed 1.0
        let dur = estimate_duration("Hello world!", 1.0);
        assert!(
            dur > 0.5 && dur < 2.0,
            "Duration {} should be reasonable for 12 chars",
            dur
        );

        // Double speed = half duration
        let dur_fast = estimate_duration("Hello world!", 2.0);
        assert!((dur_fast - dur / 2.0).abs() < 0.01);
    }

    #[test]
    fn test_tts_text_at_exact_cap() {
        let config = TtsConfig {
            mode: VoiceMode::Api,
            api_key: Some("sk-test".into()),
            ..TtsConfig::default()
        };
        let text = "a".repeat(TTS_TEXT_CAP);
        let path = PathBuf::from("/tmp/test_tts_cap.mp3");
        // This should NOT fail on text length (will fail on network, which is expected)
        let result = synthesize_api(&text, &config, &path);
        // It should get past validation and fail on the HTTP request, not on text length
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(
            !err.contains("char limit"),
            "Should not fail on text cap: {}",
            err
        );
    }
}
