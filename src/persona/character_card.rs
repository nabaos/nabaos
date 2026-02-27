use serde::Deserialize;
use std::path::Path;

use super::style::{AgentConfig, AgentPersona};
use crate::core::error::{NyayaError, Result};

/// SillyTavern V2 character card format.
#[derive(Debug, Deserialize)]
struct CharacterCardV2 {
    #[allow(dead_code)]
    spec: String,
    data: CharacterCardV2Data,
}

#[derive(Debug, Deserialize)]
struct CharacterCardV2Data {
    #[serde(default)]
    name: String,
    #[serde(default)]
    personality: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    scenario: String,
    #[serde(default)]
    first_mes: String,
    #[serde(default)]
    system_prompt: String,
}

/// SillyTavern V1 character card format (flat).
#[derive(Debug, Deserialize)]
struct CharacterCardV1 {
    #[serde(default)]
    name: String,
    #[serde(default)]
    personality: String,
    #[serde(default)]
    description: String,
}

/// Parse a SillyTavern character card JSON (V2 or V1) into an AgentConfig.
pub fn parse_character_card_json(json_str: &str) -> Result<AgentConfig> {
    // Try V2 first
    if let Ok(v2) = serde_json::from_str::<CharacterCardV2>(json_str) {
        if v2.data.name.is_empty()
            && v2.data.personality.is_empty()
            && v2.data.description.is_empty()
        {
            return Err(NyayaError::Config(
                "Character card V2 has no usable fields".to_string(),
            ));
        }

        let mut voice_parts = Vec::new();
        if !v2.data.personality.is_empty() {
            voice_parts.push(v2.data.personality);
        }
        if !v2.data.description.is_empty() {
            voice_parts.push(v2.data.description);
        }
        if !v2.data.scenario.is_empty() {
            voice_parts.push(format!("Context: {}", v2.data.scenario));
        }

        return Ok(AgentConfig {
            persona: AgentPersona {
                name: v2.data.name,
                voice: voice_parts.join(". "),
                greeting: v2.data.first_mes,
                system_prompt_prefix: v2.data.system_prompt,
                ..AgentPersona::default()
            },
            ..AgentConfig::default()
        });
    }

    // Try V1
    if let Ok(v1) = serde_json::from_str::<CharacterCardV1>(json_str) {
        if v1.name.is_empty() && v1.personality.is_empty() && v1.description.is_empty() {
            return Err(NyayaError::Config(
                "Character card V1 has no usable fields".to_string(),
            ));
        }

        let mut voice_parts = Vec::new();
        if !v1.personality.is_empty() {
            voice_parts.push(v1.personality);
        }
        if !v1.description.is_empty() {
            voice_parts.push(v1.description);
        }

        return Ok(AgentConfig {
            persona: AgentPersona {
                name: v1.name,
                voice: voice_parts.join(". "),
                ..AgentPersona::default()
            },
            ..AgentConfig::default()
        });
    }

    Err(NyayaError::Config(
        "Invalid character card JSON: could not parse as V2 or V1 format".to_string(),
    ))
}

/// Save an AgentConfig as a YAML file in the given directory.
/// The file name is derived from the persona name (lowercased, spaces replaced with underscores).
pub fn save_as_agent_yaml(agent: &AgentConfig, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;

    let filename = if agent.persona.name.is_empty() {
        "unnamed_agent".to_string()
    } else {
        agent.persona.name.to_lowercase().replace(' ', "_")
    };

    let path = dir.join(format!("{}.yaml", filename));
    let yaml = serde_yaml::to_string(agent)?;
    std::fs::write(&path, yaml)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_v2_json() {
        let json = r#"{
            "spec": "chara_card_v2",
            "data": {
                "name": "Luna",
                "personality": "cheerful and witty",
                "description": "A helpful AI assistant",
                "scenario": "User is chatting casually",
                "first_mes": "Hey! What's up?",
                "system_prompt": "You are Luna, a cheerful assistant."
            }
        }"#;

        let config = parse_character_card_json(json).unwrap();
        assert_eq!(config.persona.name, "Luna");
        assert!(config.persona.voice.contains("cheerful and witty"));
        assert!(config.persona.voice.contains("A helpful AI assistant"));
        assert!(config
            .persona
            .voice
            .contains("Context: User is chatting casually"));
        assert_eq!(config.persona.greeting, "Hey! What's up?");
        assert_eq!(
            config.persona.system_prompt_prefix,
            "You are Luna, a cheerful assistant."
        );
    }

    #[test]
    fn test_parse_v1_json() {
        let json = r#"{
            "name": "Atlas",
            "personality": "stoic and analytical",
            "description": "A research-focused agent"
        }"#;

        let config = parse_character_card_json(json).unwrap();
        assert_eq!(config.persona.name, "Atlas");
        assert!(config.persona.voice.contains("stoic and analytical"));
        assert!(config.persona.voice.contains("A research-focused agent"));
        assert!(config.persona.greeting.is_empty());
        assert!(config.persona.system_prompt_prefix.is_empty());
    }

    #[test]
    fn test_invalid_json_returns_error() {
        let result = parse_character_card_json("not json at all");
        assert!(result.is_err());

        let result = parse_character_card_json("{}");
        assert!(result.is_err());
    }
}
