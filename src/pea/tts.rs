//! TTS dispatcher — open-source-first text-to-speech with paid fallback.
//!
//! Detection priority: Piper → espeak-ng → OpenAI TTS → ElevenLabs

use crate::core::error::{NyayaError, Result};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Global flag set by `--narrate` CLI. Checked alongside `NABA_PEA_NARRATE` env var.
static NARRATE_FLAG: AtomicBool = AtomicBool::new(false);

/// Enable narration via the CLI flag (called from main.rs).
pub fn enable_narrate() {
    NARRATE_FLAG.store(true, Ordering::Relaxed);
}

/// Check if narration is enabled (CLI flag OR env var).
pub fn is_narrate_enabled() -> bool {
    if NARRATE_FLAG.load(Ordering::Relaxed) {
        return true;
    }
    std::env::var("NABA_PEA_NARRATE")
        .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
        .unwrap_or(false)
}

/// Which TTS provider was detected.
#[derive(Debug, Clone, PartialEq)]
enum TtsProvider {
    Piper { model_path: String },
    EspeakNg,
    OpenAi { api_key: String },
    ElevenLabs { api_key: String },
    None,
}

/// Synchronous TTS dispatcher with automatic provider detection.
pub struct TtsDispatcher {
    provider: TtsProvider,
}

impl TtsDispatcher {
    /// Probe system for available TTS providers in priority order.
    pub fn detect() -> Self {
        // 1. Piper (local, open-source, highest quality offline)
        if let Ok(model) = std::env::var("NABA_PIPER_MODEL") {
            if !model.is_empty() && Path::new(&model).exists() {
                let has_piper = std::process::Command::new("piper")
                    .arg("--help")
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                if has_piper {
                    eprintln!("[pea/tts] detected Piper with model: {}", model);
                    return Self {
                        provider: TtsProvider::Piper { model_path: model },
                    };
                }
            }
        }

        // 2. espeak-ng (widely available, lower quality but free)
        let has_espeak = std::process::Command::new("espeak-ng")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if has_espeak {
            eprintln!("[pea/tts] detected espeak-ng");
            return Self {
                provider: TtsProvider::EspeakNg,
            };
        }

        // 3. OpenAI TTS (reuses existing LLM API key)
        if let Ok(key) = std::env::var("NABA_LLM_API_KEY") {
            if !key.is_empty() {
                eprintln!("[pea/tts] detected OpenAI TTS via NABA_LLM_API_KEY");
                return Self {
                    provider: TtsProvider::OpenAi { api_key: key },
                };
            }
        }

        // 4. ElevenLabs
        if let Ok(key) = std::env::var("NABA_ELEVENLABS_API_KEY") {
            if !key.is_empty() {
                eprintln!("[pea/tts] detected ElevenLabs TTS");
                return Self {
                    provider: TtsProvider::ElevenLabs { api_key: key },
                };
            }
        }

        eprintln!("[pea/tts] no TTS provider available");
        Self {
            provider: TtsProvider::None,
        }
    }

    /// Human-readable provider name.
    pub fn provider(&self) -> &str {
        match &self.provider {
            TtsProvider::Piper { .. } => "piper",
            TtsProvider::EspeakNg => "espeak-ng",
            TtsProvider::OpenAi { .. } => "openai",
            TtsProvider::ElevenLabs { .. } => "elevenlabs",
            TtsProvider::None => "none",
        }
    }

    /// Whether a usable provider was detected.
    pub fn is_available(&self) -> bool {
        self.provider != TtsProvider::None
    }

    /// Synthesize text to an MP3 file. Returns Ok(true) on success, Ok(false) if skipped.
    pub fn synthesize(&self, text: &str, output_path: &Path) -> Result<bool> {
        match &self.provider {
            TtsProvider::Piper { model_path } => synthesize_piper(text, model_path, output_path),
            TtsProvider::EspeakNg => synthesize_espeak(text, output_path),
            TtsProvider::OpenAi { api_key } => synthesize_openai(text, api_key, output_path),
            TtsProvider::ElevenLabs { api_key } => synthesize_elevenlabs(text, api_key, output_path),
            TtsProvider::None => Ok(false),
        }
    }

    /// Convert slide content to natural narration text.
    pub fn slide_to_narration(title: &str, bullets: &[String]) -> String {
        if bullets.is_empty() {
            return title.to_string();
        }
        let mut narration = format!("{}. ", title);
        for (i, bullet) in bullets.iter().enumerate() {
            if i > 0 {
                narration.push_str(" ");
            }
            narration.push_str(bullet);
            if !bullet.ends_with('.') {
                narration.push('.');
            }
        }
        narration
    }
}

/// Measure audio file duration in seconds using ffprobe.
pub fn measure_audio_duration(path: &Path) -> Option<f32> {
    let output = std::process::Command::new("ffprobe")
        .args([
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout);
        s.trim().parse::<f32>().ok()
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Provider implementations
// ---------------------------------------------------------------------------

fn synthesize_piper(text: &str, model_path: &str, output_path: &Path) -> Result<bool> {
    // Piper reads from stdin and writes WAV/MP3 to output file
    let mut child = std::process::Command::new("piper")
        .args(["--model", model_path, "--output_file"])
        .arg(output_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| NyayaError::Config(format!("piper spawn: {}", e)))?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(text.as_bytes());
    }

    let output = child
        .wait_with_output()
        .map_err(|e| NyayaError::Config(format!("piper wait: {}", e)))?;

    if output.status.success() && output_path.exists() {
        Ok(true)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("[pea/tts] piper failed: {}", &stderr[..stderr.len().min(200)]);
        Ok(false)
    }
}

fn synthesize_espeak(text: &str, output_path: &Path) -> Result<bool> {
    // espeak-ng outputs WAV; convert to MP3 with ffmpeg
    let wav_path = output_path.with_extension("wav");

    // Truncate text for espeak (it can struggle with very long input)
    let clamped = if text.len() > 2000 { &text[..2000] } else { text };

    let espeak = std::process::Command::new("espeak-ng")
        .arg("-w")
        .arg(&wav_path)
        .arg(clamped)
        .output()
        .map_err(|e| NyayaError::Config(format!("espeak-ng: {}", e)))?;

    if !espeak.status.success() || !wav_path.exists() {
        return Ok(false);
    }

    // Convert WAV → MP3
    let has_ffmpeg = std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if has_ffmpeg {
        let convert = std::process::Command::new("ffmpeg")
            .args(["-y", "-i"])
            .arg(&wav_path)
            .args(["-codec:a", "libmp3lame", "-qscale:a", "4"])
            .arg(output_path)
            .output()
            .map_err(|e| NyayaError::Config(format!("ffmpeg wav→mp3: {}", e)))?;

        let _ = std::fs::remove_file(&wav_path);

        if convert.status.success() && output_path.exists() {
            return Ok(true);
        }
    } else {
        // No ffmpeg — just rename WAV to the output path (Remotion can handle WAV)
        let _ = std::fs::rename(&wav_path, output_path);
        if output_path.exists() {
            return Ok(true);
        }
    }

    Ok(false)
}

fn synthesize_openai(text: &str, api_key: &str, output_path: &Path) -> Result<bool> {
    let client = reqwest::blocking::Client::new();
    let body = serde_json::json!({
        "model": "tts-1",
        "input": text,
        "voice": "alloy",
        "response_format": "mp3",
    });

    let resp = client
        .post("https://api.openai.com/v1/audio/speech")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| NyayaError::Config(format!("OpenAI TTS request: {}", e)))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        eprintln!(
            "[pea/tts] OpenAI TTS failed ({}): {}",
            status,
            &text[..text.len().min(200)]
        );
        return Ok(false);
    }

    let data = resp
        .bytes()
        .map_err(|e| NyayaError::Config(format!("OpenAI TTS read: {}", e)))?;

    std::fs::write(output_path, &data)
        .map_err(|e| NyayaError::Config(format!("write TTS output: {}", e)))?;

    Ok(output_path.exists())
}

fn synthesize_elevenlabs(text: &str, api_key: &str, output_path: &Path) -> Result<bool> {
    let voice_id = "21m00Tcm4TlvDq8ikWAM"; // Rachel (default)
    let client = reqwest::blocking::Client::new();
    let body = serde_json::json!({
        "text": text,
        "model_id": "eleven_flash_v2_5",
        "voice_settings": {
            "stability": 0.5,
            "similarity_boost": 0.75,
        },
    });

    let resp = client
        .post(format!(
            "https://api.elevenlabs.io/v1/text-to-speech/{}",
            voice_id
        ))
        .header("xi-api-key", api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| NyayaError::Config(format!("ElevenLabs TTS request: {}", e)))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        eprintln!(
            "[pea/tts] ElevenLabs TTS failed ({}): {}",
            status,
            &text[..text.len().min(200)]
        );
        return Ok(false);
    }

    let data = resp
        .bytes()
        .map_err(|e| NyayaError::Config(format!("ElevenLabs TTS read: {}", e)))?;

    std::fs::write(output_path, &data)
        .map_err(|e| NyayaError::Config(format!("write TTS output: {}", e)))?;

    Ok(output_path.exists())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slide_to_narration_basic() {
        let narration = TtsDispatcher::slide_to_narration(
            "Climate Change",
            &["Global temperatures rising".into(), "Ice caps melting rapidly".into()],
        );
        assert!(narration.starts_with("Climate Change. "));
        assert!(narration.contains("Global temperatures rising."));
        assert!(narration.contains("Ice caps melting rapidly."));
    }

    #[test]
    fn test_slide_to_narration_empty_bullets() {
        let narration = TtsDispatcher::slide_to_narration("Thank You", &[]);
        assert_eq!(narration, "Thank You");
    }

    #[test]
    fn test_slide_to_narration_preserves_existing_period() {
        let narration = TtsDispatcher::slide_to_narration(
            "Intro",
            &["Already has a period.".into()],
        );
        // Should not double-period
        assert!(!narration.contains(".."));
    }

    #[test]
    fn test_detect_no_providers() {
        // In test environment, no TTS providers should be available
        // (unless the test runner happens to have espeak-ng installed)
        let dispatcher = TtsDispatcher::detect();
        let prov = dispatcher.provider();
        // Just verify it returns a valid provider name
        assert!(
            ["piper", "espeak-ng", "openai", "elevenlabs", "none"].contains(&prov),
            "unexpected provider: {}",
            prov
        );
    }

    #[test]
    fn test_provider_name() {
        let d = TtsDispatcher {
            provider: TtsProvider::None,
        };
        assert_eq!(d.provider(), "none");
        assert!(!d.is_available());
    }
}
