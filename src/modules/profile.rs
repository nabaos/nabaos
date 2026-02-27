use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::core::error::{NyayaError, Result};

/// Voice processing mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum VoiceMode {
    #[default]
    Disabled,
    Local,
    Api,
}

impl std::fmt::Display for VoiceMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VoiceMode::Disabled => write!(f, "disabled"),
            VoiceMode::Local => write!(f, "local"),
            VoiceMode::Api => write!(f, "api"),
        }
    }
}

/// Describes which optional modules are enabled for this installation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleProfile {
    /// Core module is always enabled.
    pub core: bool,
    /// Enable web dashboard.
    pub web: bool,
    /// Voice input mode.
    pub voice: VoiceMode,
    /// Enable headless browser automation.
    pub browser: bool,
    /// OAuth providers configured.
    pub oauth: Vec<String>,
    /// Enable Telegram channel.
    pub telegram: bool,
    /// Enable mobile push notifications.
    pub mobile: bool,
    /// Enable LaTeX document generation.
    pub latex: bool,
    /// Human-readable profile name.
    pub name: String,
    /// ISO-8601 timestamp when this profile was generated.
    pub generated_at: String,
}

impl Default for ModuleProfile {
    fn default() -> Self {
        Self {
            core: true,
            web: false,
            voice: VoiceMode::Disabled,
            browser: false,
            oauth: Vec::new(),
            telegram: false,
            mobile: false,
            latex: false,
            name: "default".to_string(),
            generated_at: String::new(),
        }
    }
}

impl ModuleProfile {
    /// Returns true if any voice mode other than Disabled is selected.
    pub fn voice_enabled(&self) -> bool {
        self.voice != VoiceMode::Disabled
    }

    /// Load a profile from a TOML file at the given path.
    pub fn load_from(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let profile: ModuleProfile = toml::from_str(&contents)
            .map_err(|e| NyayaError::Config(format!("Failed to parse profile TOML: {}", e)))?;
        Ok(profile)
    }

    /// Load a profile from the given path, or return a default profile if the
    /// file does not exist.
    pub fn load_or_default(path: &Path) -> ModuleProfile {
        Self::load_from(path).unwrap_or_default()
    }

    /// Serialize this profile to TOML and write it to the given path.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        let contents = toml::to_string_pretty(self)
            .map_err(|e| NyayaError::Config(format!("Failed to serialize profile: {}", e)))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, contents)?;
        Ok(())
    }

    /// Canonical path for profile.toml inside a data directory.
    pub fn profile_path(data_dir: &Path) -> PathBuf {
        data_dir.join("profile.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_profile_has_core_enabled() {
        let p = ModuleProfile::default();
        assert!(p.core);
        assert!(!p.web);
        assert_eq!(p.voice, VoiceMode::Disabled);
        assert!(!p.voice_enabled());
        assert!(p.oauth.is_empty());
    }

    #[test]
    fn test_profile_roundtrip_toml() {
        let mut p = ModuleProfile::default();
        p.name = "test-roundtrip".to_string();
        p.web = true;
        p.voice = VoiceMode::Local;
        p.oauth = vec!["google".to_string(), "github".to_string()];

        let toml_str = toml::to_string_pretty(&p).expect("serialize");
        let p2: ModuleProfile = toml::from_str(&toml_str).expect("deserialize");

        assert_eq!(p2.name, "test-roundtrip");
        assert!(p2.web);
        assert_eq!(p2.voice, VoiceMode::Local);
        assert!(p2.voice_enabled());
        assert_eq!(p2.oauth, vec!["google".to_string(), "github".to_string()]);
        assert!(p2.core);
    }

    #[test]
    fn test_profile_load_from_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("profile.toml");

        let mut p = ModuleProfile::default();
        p.name = "file-test".to_string();
        p.browser = true;
        p.save_to(&path).expect("save");

        let loaded = ModuleProfile::load_from(&path).expect("load");
        assert_eq!(loaded.name, "file-test");
        assert!(loaded.browser);
        assert!(loaded.core);
    }

    #[test]
    fn test_profile_load_returns_default_when_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("does-not-exist.toml");

        let p = ModuleProfile::load_or_default(&path);
        assert!(p.core);
        assert_eq!(p.name, "default");
    }
}
