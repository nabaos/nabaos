use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::style::{AgentPersona, EmojiUsage, Formality, VocabularyLevel};

// ---------------------------------------------------------------------------
// AudiencePreset
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AudiencePreset {
    Children,
    YoungAdults,
    Seniors,
    Technical,
    Custom(String),
}

impl fmt::Display for AudiencePreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudiencePreset::Children => write!(f, "children"),
            AudiencePreset::YoungAdults => write!(f, "young_adults"),
            AudiencePreset::Seniors => write!(f, "seniors"),
            AudiencePreset::Technical => write!(f, "technical"),
            AudiencePreset::Custom(name) => write!(f, "{}", name),
        }
    }
}

// ---------------------------------------------------------------------------
// BrandingMode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BrandingMode {
    Full,
    #[default]
    ConversationOnly,
}

// ---------------------------------------------------------------------------
// VisualBranding
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct VisualBranding {
    pub primary_color: Option<String>,
    pub secondary_color: Option<String>,
    pub font_style: Option<String>,
    pub mood: Option<String>,
    pub logo_url: Option<String>,
}

// ---------------------------------------------------------------------------
// ConversationBranding
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ConversationBranding {
    pub brand_voice: String,
    pub mascot_personality: Option<String>,
    pub catchphrases: Vec<String>,
    pub tone_override: Option<String>,
}

// ---------------------------------------------------------------------------
// BrandingProfile
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct BrandingProfile {
    pub mode: BrandingMode,
    pub visual: VisualBranding,
    pub conversation: ConversationBranding,
}

// ---------------------------------------------------------------------------
// StyleProfile
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct StyleProfile {
    pub name: String,
    pub audience: Option<AudiencePreset>,
    pub persona_overlay: AgentPersona,
    pub branding: Option<BrandingProfile>,
    pub max_sentence_length: Option<usize>,
    pub style_prompt_suffix: String,
}

impl StyleProfile {
    /// Factory method that produces a built-in style profile for a known audience preset.
    pub fn from_audience(preset: &AudiencePreset) -> Self {
        match preset {
            AudiencePreset::Children => StyleProfile {
                name: "children".to_string(),
                audience: Some(AudiencePreset::Children),
                persona_overlay: AgentPersona {
                    emoji_usage: EmojiUsage::Heavy,
                    formality: Formality::Casual,
                    vocabulary_level: VocabularyLevel::Simple,
                    ..AgentPersona::default()
                },
                max_sentence_length: Some(15),
                ..StyleProfile::default()
            },
            AudiencePreset::YoungAdults => StyleProfile {
                name: "young_adults".to_string(),
                audience: Some(AudiencePreset::YoungAdults),
                persona_overlay: AgentPersona {
                    emoji_usage: EmojiUsage::Moderate,
                    formality: Formality::Casual,
                    vocabulary_level: VocabularyLevel::Adaptive,
                    ..AgentPersona::default()
                },
                ..StyleProfile::default()
            },
            AudiencePreset::Seniors => StyleProfile {
                name: "seniors".to_string(),
                audience: Some(AudiencePreset::Seniors),
                persona_overlay: AgentPersona {
                    emoji_usage: EmojiUsage::None,
                    formality: Formality::Formal,
                    vocabulary_level: VocabularyLevel::Simple,
                    ..AgentPersona::default()
                },
                max_sentence_length: Some(20),
                ..StyleProfile::default()
            },
            AudiencePreset::Technical => StyleProfile {
                name: "technical".to_string(),
                audience: Some(AudiencePreset::Technical),
                persona_overlay: AgentPersona {
                    emoji_usage: EmojiUsage::None,
                    formality: Formality::Formal,
                    vocabulary_level: VocabularyLevel::DomainExpert,
                    ..AgentPersona::default()
                },
                ..StyleProfile::default()
            },
            AudiencePreset::Custom(name) => StyleProfile {
                name: name.clone(),
                audience: Some(AudiencePreset::Custom(name.clone())),
                ..StyleProfile::default()
            },
        }
    }

    /// Merge this style's persona overlay onto a base persona.
    pub fn apply_to(&self, base: &AgentPersona) -> AgentPersona {
        base.merge_with(&self.persona_overlay)
    }

    /// Produce template variables for prompt rendering.
    pub fn to_template_vars(&self) -> HashMap<String, String> {
        let mut vars = HashMap::new();

        vars.insert("_style_name".to_string(), self.name.clone());

        vars.insert(
            "_style_audience".to_string(),
            self.audience
                .as_ref()
                .map(|a| a.to_string())
                .unwrap_or_default(),
        );

        vars.insert(
            "_style_formality".to_string(),
            format!("{:?}", self.persona_overlay.formality),
        );

        vars.insert(
            "_style_emoji".to_string(),
            format!("{:?}", self.persona_overlay.emoji_usage),
        );

        vars.insert(
            "_style_vocab".to_string(),
            format!("{:?}", self.persona_overlay.vocabulary_level),
        );

        vars.insert("_style_tone".to_string(), self.persona_overlay.tone.clone());

        vars.insert(
            "_style_max_sentence_length".to_string(),
            self.max_sentence_length
                .map(|n| n.to_string())
                .unwrap_or_default(),
        );

        // Branding variables
        let (brand_voice, mascot, primary_color, mood, font) = if let Some(ref bp) = self.branding {
            (
                bp.conversation.brand_voice.clone(),
                bp.conversation
                    .mascot_personality
                    .clone()
                    .unwrap_or_default(),
                bp.visual.primary_color.clone().unwrap_or_default(),
                bp.visual.mood.clone().unwrap_or_default(),
                bp.visual.font_style.clone().unwrap_or_default(),
            )
        } else {
            (
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            )
        };

        vars.insert("_style_brand_voice".to_string(), brand_voice);
        vars.insert("_style_mascot".to_string(), mascot);
        vars.insert("_style_primary_color".to_string(), primary_color);
        vars.insert("_style_mood".to_string(), mood);
        vars.insert("_style_font".to_string(), font);

        vars
    }
}

/// Parse a built-in audience preset name (with common aliases) into an `AudiencePreset`.
pub fn parse_builtin_preset(name: &str) -> Option<AudiencePreset> {
    match name {
        "children" | "kids" => Some(AudiencePreset::Children),
        "young_adults" | "teens" => Some(AudiencePreset::YoungAdults),
        "seniors" | "elderly" => Some(AudiencePreset::Seniors),
        "technical" | "tech" => Some(AudiencePreset::Technical),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_children_preset_values() {
        let style = StyleProfile::from_audience(&AudiencePreset::Children);
        assert_eq!(style.name, "children");
        assert_eq!(style.persona_overlay.emoji_usage, EmojiUsage::Heavy);
        assert_eq!(style.persona_overlay.formality, Formality::Casual);
        assert_eq!(
            style.persona_overlay.vocabulary_level,
            VocabularyLevel::Simple
        );
        assert_eq!(style.max_sentence_length, Some(15));
    }

    #[test]
    fn test_young_adults_preset_values() {
        let style = StyleProfile::from_audience(&AudiencePreset::YoungAdults);
        assert_eq!(style.name, "young_adults");
        assert_eq!(style.persona_overlay.emoji_usage, EmojiUsage::Moderate);
        assert_eq!(style.persona_overlay.formality, Formality::Casual);
        assert_eq!(
            style.persona_overlay.vocabulary_level,
            VocabularyLevel::Adaptive
        );
        assert_eq!(style.max_sentence_length, None);
    }

    #[test]
    fn test_seniors_preset_values() {
        let style = StyleProfile::from_audience(&AudiencePreset::Seniors);
        assert_eq!(style.name, "seniors");
        assert_eq!(style.persona_overlay.emoji_usage, EmojiUsage::None);
        assert_eq!(style.persona_overlay.formality, Formality::Formal);
        assert_eq!(
            style.persona_overlay.vocabulary_level,
            VocabularyLevel::Simple
        );
        assert_eq!(style.max_sentence_length, Some(20));
    }

    #[test]
    fn test_technical_preset_values() {
        let style = StyleProfile::from_audience(&AudiencePreset::Technical);
        assert_eq!(style.name, "technical");
        assert_eq!(style.persona_overlay.emoji_usage, EmojiUsage::None);
        assert_eq!(style.persona_overlay.formality, Formality::Formal);
        assert_eq!(
            style.persona_overlay.vocabulary_level,
            VocabularyLevel::DomainExpert
        );
        assert_eq!(style.max_sentence_length, None);
    }

    #[test]
    fn test_apply_to_merges_persona() {
        let base = AgentPersona {
            name: "BaseBot".to_string(),
            voice: "calm and steady".to_string(),
            tone: "neutral".to_string(),
            emoji_usage: EmojiUsage::Minimal,
            formality: Formality::Balanced,
            vocabulary_level: VocabularyLevel::Adaptive,
            quirks: vec!["says hmm".to_string()],
            greeting: "Hello!".to_string(),
            system_prompt_prefix: "You are helpful.".to_string(),
        };

        let style = StyleProfile::from_audience(&AudiencePreset::Children);
        let merged = style.apply_to(&base);

        // Overlay fields should take effect
        assert_eq!(merged.emoji_usage, EmojiUsage::Heavy);
        assert_eq!(merged.formality, Formality::Casual);
        assert_eq!(merged.vocabulary_level, VocabularyLevel::Simple);
        // Base fields should be preserved where overlay is default/empty
        assert_eq!(merged.voice, "calm and steady");
        assert_eq!(merged.greeting, "Hello!");
    }

    #[test]
    fn test_template_vars_populated() {
        let style = StyleProfile {
            name: "branded_style".to_string(),
            audience: Some(AudiencePreset::Children),
            persona_overlay: AgentPersona {
                formality: Formality::Casual,
                emoji_usage: EmojiUsage::Heavy,
                vocabulary_level: VocabularyLevel::Simple,
                tone: "playful".to_string(),
                ..AgentPersona::default()
            },
            branding: Some(BrandingProfile {
                mode: BrandingMode::Full,
                visual: VisualBranding {
                    primary_color: Some("#FF6600".to_string()),
                    mood: Some("cheerful".to_string()),
                    font_style: Some("comic-sans".to_string()),
                    ..VisualBranding::default()
                },
                conversation: ConversationBranding {
                    brand_voice: "fun and friendly".to_string(),
                    mascot_personality: Some("playful fox".to_string()),
                    ..ConversationBranding::default()
                },
            }),
            max_sentence_length: Some(15),
            style_prompt_suffix: String::new(),
        };

        let vars = style.to_template_vars();

        assert_eq!(vars.get("_style_name").unwrap(), "branded_style");
        assert_eq!(vars.get("_style_audience").unwrap(), "children");
        assert_eq!(vars.get("_style_formality").unwrap(), "Casual");
        assert_eq!(vars.get("_style_emoji").unwrap(), "Heavy");
        assert_eq!(vars.get("_style_vocab").unwrap(), "Simple");
        assert_eq!(vars.get("_style_tone").unwrap(), "playful");
        assert_eq!(vars.get("_style_max_sentence_length").unwrap(), "15");
        assert_eq!(vars.get("_style_brand_voice").unwrap(), "fun and friendly");
        assert_eq!(vars.get("_style_mascot").unwrap(), "playful fox");
        assert_eq!(vars.get("_style_primary_color").unwrap(), "#FF6600");
        assert_eq!(vars.get("_style_mood").unwrap(), "cheerful");
        assert_eq!(vars.get("_style_font").unwrap(), "comic-sans");
    }

    #[test]
    fn test_yaml_roundtrip() {
        let style = StyleProfile {
            name: "roundtrip_test".to_string(),
            audience: Some(AudiencePreset::Technical),
            persona_overlay: AgentPersona {
                formality: Formality::Formal,
                emoji_usage: EmojiUsage::None,
                vocabulary_level: VocabularyLevel::DomainExpert,
                ..AgentPersona::default()
            },
            branding: Some(BrandingProfile {
                mode: BrandingMode::Full,
                visual: VisualBranding {
                    primary_color: Some("#000000".to_string()),
                    ..VisualBranding::default()
                },
                conversation: ConversationBranding {
                    brand_voice: "authoritative".to_string(),
                    catchphrases: vec!["let me explain".to_string()],
                    ..ConversationBranding::default()
                },
            }),
            max_sentence_length: Some(25),
            style_prompt_suffix: "Be precise.".to_string(),
        };

        let yaml = serde_yaml::to_string(&style).expect("serialize to YAML");
        let parsed: StyleProfile = serde_yaml::from_str(&yaml).expect("parse from YAML");

        assert_eq!(style, parsed);
    }

    #[test]
    fn test_branding_modes() {
        let full = BrandingMode::Full;
        let conv = BrandingMode::ConversationOnly;
        let default_mode = BrandingMode::default();

        assert_eq!(full, BrandingMode::Full);
        assert_eq!(conv, BrandingMode::ConversationOnly);
        assert_eq!(default_mode, BrandingMode::ConversationOnly);
        assert_ne!(full, conv);
    }

    #[test]
    fn test_parse_builtin_preset() {
        assert_eq!(
            parse_builtin_preset("children"),
            Some(AudiencePreset::Children)
        );
        assert_eq!(parse_builtin_preset("kids"), Some(AudiencePreset::Children));
        assert_eq!(
            parse_builtin_preset("young_adults"),
            Some(AudiencePreset::YoungAdults)
        );
        assert_eq!(
            parse_builtin_preset("teens"),
            Some(AudiencePreset::YoungAdults)
        );
        assert_eq!(
            parse_builtin_preset("seniors"),
            Some(AudiencePreset::Seniors)
        );
        assert_eq!(
            parse_builtin_preset("elderly"),
            Some(AudiencePreset::Seniors)
        );
        assert_eq!(
            parse_builtin_preset("technical"),
            Some(AudiencePreset::Technical)
        );
        assert_eq!(
            parse_builtin_preset("tech"),
            Some(AudiencePreset::Technical)
        );
        assert_eq!(parse_builtin_preset("unknown"), None);
        assert_eq!(parse_builtin_preset(""), None);
    }
}
