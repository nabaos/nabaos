use super::style::AgentPersona;

/// Compile an AgentPersona into a system prompt string.
///
/// If `system_prompt_prefix` is non-empty, it is returned directly (power user override).
/// Otherwise, a prompt is built from the persona fields, skipping empty fields.
pub fn compile_persona(persona: &AgentPersona) -> String {
    if !persona.system_prompt_prefix.is_empty() {
        return persona.system_prompt_prefix.clone();
    }

    let mut parts: Vec<String> = Vec::new();

    if !persona.name.is_empty() {
        parts.push(format!("You are {}.", persona.name));
    }

    if !persona.voice.is_empty() {
        parts.push(format!("Communication style: {}.", persona.voice));
    }

    if !persona.tone.is_empty() {
        parts.push(format!("Tone: {}.", persona.tone));
    }

    let formality_str = format!("{:?}", persona.formality).to_lowercase();
    let emoji_str = format!("{:?}", persona.emoji_usage).to_lowercase();
    parts.push(format!(
        "Formality: {}. Emoji usage: {}.",
        formality_str, emoji_str
    ));

    if !persona.quirks.is_empty() {
        let quirk_lines: Vec<String> = persona.quirks.iter().map(|q| format!("- {}", q)).collect();
        parts.push(format!("Personality quirks:\n{}", quirk_lines.join("\n")));
    }

    parts.join("\n")
}

/// Compile a persona with an optional style profile overlay.
///
/// If `style` is `None`, delegates to [`compile_persona`]. Otherwise the style's
/// persona overlay is merged first, and branding / sentence-length / suffix hints
/// are appended to the compiled prompt.
pub fn compile_persona_with_style(
    persona: &AgentPersona,
    style: Option<&crate::persona::conditional::StyleProfile>,
) -> String {
    let style = match style {
        Some(s) => s,
        None => return compile_persona(persona),
    };

    let merged = style.apply_to(persona);
    let mut prompt = compile_persona(&merged);

    if let Some(ref branding) = style.branding {
        let branding_text = compile_branding(branding);
        if !branding_text.is_empty() {
            prompt.push('\n');
            prompt.push_str(&branding_text);
        }
    }

    if let Some(max) = style.max_sentence_length {
        prompt.push_str(&format!("\nKeep sentences under {} words.", max));
    }

    if !style.style_prompt_suffix.is_empty() {
        prompt.push('\n');
        prompt.push_str(&style.style_prompt_suffix);
    }

    prompt
}

/// Compile a [`BrandingProfile`](crate::persona::conditional::BrandingProfile) into
/// prompt-ready text describing the brand voice and (optionally) visual identity.
pub fn compile_branding(branding: &crate::persona::conditional::BrandingProfile) -> String {
    use crate::persona::conditional::BrandingMode;

    let mut lines: Vec<String> = Vec::new();

    // Conversation fields (always included)
    if !branding.conversation.brand_voice.is_empty() {
        lines.push(format!(
            "Brand voice: {}.",
            branding.conversation.brand_voice
        ));
    }
    if let Some(ref mascot) = branding.conversation.mascot_personality {
        lines.push(format!("Mascot personality: {}.", mascot));
    }
    if !branding.conversation.catchphrases.is_empty() {
        lines.push(format!(
            "Catchphrases: {}.",
            branding.conversation.catchphrases.join(", ")
        ));
    }
    if let Some(ref tone) = branding.conversation.tone_override {
        lines.push(format!("Tone: {}.", tone));
    }

    // Visual fields (Full mode only)
    if branding.mode == BrandingMode::Full {
        if let Some(ref c) = branding.visual.primary_color {
            lines.push(format!("Primary color: {}.", c));
        }
        if let Some(ref c) = branding.visual.secondary_color {
            lines.push(format!("Secondary color: {}.", c));
        }
        if let Some(ref f) = branding.visual.font_style {
            lines.push(format!("Font style: {}.", f));
        }
        if let Some(ref m) = branding.visual.mood {
            lines.push(format!("Mood: {}.", m));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persona::conditional::{
        AudiencePreset, BrandingMode, BrandingProfile, ConversationBranding, StyleProfile,
        VisualBranding,
    };
    use crate::persona::style::{EmojiUsage, Formality, VocabularyLevel};

    #[test]
    fn test_compile_default_persona() {
        let persona = AgentPersona::default();
        let prompt = compile_persona(&persona);
        assert!(prompt.contains("You are Nyaya."));
        assert!(prompt.contains("Formality: balanced."));
        assert!(prompt.contains("Emoji usage: minimal."));
        // voice and tone are empty, so no "Communication style:" or "Tone:" lines
        assert!(!prompt.contains("Communication style:"));
        assert!(!prompt.contains("Tone:"));
    }

    #[test]
    fn test_compile_with_quirks() {
        let persona = AgentPersona {
            name: "TestBot".to_string(),
            voice: "friendly".to_string(),
            tone: "warm".to_string(),
            emoji_usage: EmojiUsage::Heavy,
            formality: Formality::Casual,
            vocabulary_level: VocabularyLevel::Simple,
            quirks: vec![
                "uses puns frequently".to_string(),
                "ends messages with a fun fact".to_string(),
            ],
            greeting: String::new(),
            system_prompt_prefix: String::new(),
        };

        let prompt = compile_persona(&persona);
        assert!(prompt.contains("You are TestBot."));
        assert!(prompt.contains("Communication style: friendly."));
        assert!(prompt.contains("Tone: warm."));
        assert!(prompt.contains("Formality: casual. Emoji usage: heavy."));
        assert!(prompt.contains("Personality quirks:"));
        assert!(prompt.contains("- uses puns frequently"));
        assert!(prompt.contains("- ends messages with a fun fact"));
    }

    #[test]
    fn test_raw_prefix_overrides_everything() {
        let persona = AgentPersona {
            name: "ShouldNotAppear".to_string(),
            voice: "also ignored".to_string(),
            system_prompt_prefix: "You are a custom system prompt.".to_string(),
            ..AgentPersona::default()
        };

        let prompt = compile_persona(&persona);
        assert_eq!(prompt, "You are a custom system prompt.");
        assert!(!prompt.contains("ShouldNotAppear"));
    }

    #[test]
    fn test_empty_fields_produce_no_artifacts() {
        let persona = AgentPersona {
            name: String::new(),
            voice: String::new(),
            tone: String::new(),
            emoji_usage: EmojiUsage::Minimal,
            formality: Formality::Balanced,
            vocabulary_level: VocabularyLevel::Adaptive,
            quirks: Vec::new(),
            greeting: String::new(),
            system_prompt_prefix: String::new(),
        };

        let prompt = compile_persona(&persona);
        // Should not contain "You are ." or "Communication style:" etc.
        assert!(!prompt.contains("You are"));
        assert!(!prompt.contains("Communication style:"));
        assert!(!prompt.contains("Tone:"));
        assert!(!prompt.contains("Personality quirks:"));
        // Should still contain formality/emoji line
        assert!(prompt.contains("Formality: balanced. Emoji usage: minimal."));
    }

    #[test]
    fn test_with_style_none_equals_compile_persona() {
        let persona = AgentPersona::default();
        let without_style = compile_persona(&persona);
        let with_none = compile_persona_with_style(&persona, None);
        assert_eq!(without_style, with_none);
    }

    #[test]
    fn test_children_style_adds_sentence_hint() {
        let persona = AgentPersona::default();
        let style = StyleProfile::from_audience(&AudiencePreset::Children);
        let prompt = compile_persona_with_style(&persona, Some(&style));
        assert!(
            prompt.contains("Keep sentences under 15 words"),
            "Expected sentence-length hint in prompt: {}",
            prompt
        );
    }

    #[test]
    fn test_full_branding_includes_colors() {
        let branding = BrandingProfile {
            mode: BrandingMode::Full,
            visual: VisualBranding {
                primary_color: Some("#FF6600".to_string()),
                secondary_color: Some("#003366".to_string()),
                font_style: Some("comic-sans".to_string()),
                mood: Some("cheerful".to_string()),
                ..VisualBranding::default()
            },
            conversation: ConversationBranding {
                brand_voice: "fun and friendly".to_string(),
                mascot_personality: Some("playful fox".to_string()),
                catchphrases: vec!["let's go!".to_string()],
                tone_override: Some("enthusiastic".to_string()),
            },
        };

        let text = compile_branding(&branding);
        assert!(
            text.contains("Primary color: #FF6600."),
            "missing primary color"
        );
        assert!(
            text.contains("Secondary color: #003366."),
            "missing secondary color"
        );
        assert!(
            text.contains("Font style: comic-sans."),
            "missing font style"
        );
        assert!(text.contains("Mood: cheerful."), "missing mood");
        assert!(
            text.contains("Brand voice: fun and friendly."),
            "missing brand voice"
        );
        assert!(
            text.contains("Mascot personality: playful fox."),
            "missing mascot"
        );
        assert!(
            text.contains("Catchphrases: let's go!."),
            "missing catchphrases"
        );
        assert!(text.contains("Tone: enthusiastic."), "missing tone");
    }

    #[test]
    fn test_conversation_only_excludes_visual() {
        let branding = BrandingProfile {
            mode: BrandingMode::ConversationOnly,
            visual: VisualBranding {
                primary_color: Some("#FF6600".to_string()),
                secondary_color: Some("#003366".to_string()),
                font_style: Some("comic-sans".to_string()),
                mood: Some("cheerful".to_string()),
                ..VisualBranding::default()
            },
            conversation: ConversationBranding {
                brand_voice: "fun and friendly".to_string(),
                ..ConversationBranding::default()
            },
        };

        let text = compile_branding(&branding);
        assert!(
            !text.contains("Primary color:"),
            "ConversationOnly should exclude visual fields"
        );
        assert!(
            text.contains("Brand voice: fun and friendly."),
            "ConversationOnly should include brand voice"
        );
    }
}
