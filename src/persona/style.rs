use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::core::error::{NyayaError, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum EmojiUsage {
    None,
    #[default]
    Minimal,
    Moderate,
    Heavy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum Formality {
    Formal,
    #[default]
    Balanced,
    Casual,
    Chaotic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum VocabularyLevel {
    Simple,
    #[default]
    Adaptive,
    Technical,
    DomainExpert,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AgentPersona {
    pub name: String,
    pub voice: String,
    pub tone: String,
    pub emoji_usage: EmojiUsage,
    pub formality: Formality,
    pub vocabulary_level: VocabularyLevel,
    pub quirks: Vec<String>,
    pub greeting: String,
    pub system_prompt_prefix: String,
}

impl Default for AgentPersona {
    fn default() -> Self {
        AgentPersona {
            name: "Nyaya".to_string(),
            voice: String::new(),
            tone: String::new(),
            emoji_usage: EmojiUsage::default(),
            formality: Formality::default(),
            vocabulary_level: VocabularyLevel::default(),
            quirks: Vec::new(),
            greeting: String::new(),
            system_prompt_prefix: String::new(),
        }
    }
}

impl AgentPersona {
    /// Merge with an overlay persona. Non-empty overlay fields override base fields.
    pub fn merge_with(&self, overlay: &AgentPersona) -> AgentPersona {
        let default = AgentPersona::default();
        AgentPersona {
            name: if !overlay.name.is_empty() && overlay.name != default.name {
                overlay.name.clone()
            } else {
                self.name.clone()
            },
            voice: if !overlay.voice.is_empty() {
                overlay.voice.clone()
            } else {
                self.voice.clone()
            },
            tone: if !overlay.tone.is_empty() {
                overlay.tone.clone()
            } else {
                self.tone.clone()
            },
            emoji_usage: if overlay.emoji_usage != default.emoji_usage {
                overlay.emoji_usage.clone()
            } else {
                self.emoji_usage.clone()
            },
            formality: if overlay.formality != default.formality {
                overlay.formality.clone()
            } else {
                self.formality.clone()
            },
            vocabulary_level: if overlay.vocabulary_level != default.vocabulary_level {
                overlay.vocabulary_level.clone()
            } else {
                self.vocabulary_level.clone()
            },
            quirks: if !overlay.quirks.is_empty() {
                overlay.quirks.clone()
            } else {
                self.quirks.clone()
            },
            greeting: if !overlay.greeting.is_empty() {
                overlay.greeting.clone()
            } else {
                self.greeting.clone()
            },
            system_prompt_prefix: if !overlay.system_prompt_prefix.is_empty() {
                overlay.system_prompt_prefix.clone()
            } else {
                self.system_prompt_prefix.clone()
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct AgentProviderPreference {
    pub preferred: Option<String>,
    pub fallback: Vec<String>,
    pub model_override: Option<String>,
    pub max_cost_per_call: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct AgentConfig {
    pub persona: AgentPersona,
    pub provider: AgentProviderPreference,
    pub constitution: Option<String>,
    pub mcp: Option<crate::mcp::config::McpAgentConfig>,
    pub conditional_styles: HashMap<String, crate::persona::conditional::StyleProfile>,
    pub knowledge_base: Option<String>,
    pub resources: Vec<crate::resource::ResourceConfig>,
}

/// Load all agent config files from a directory. Each .yaml/.yml file's stem is the agent ID.
pub fn load_agents_dir(dir: &Path) -> Result<HashMap<String, AgentConfig>> {
    let mut agents = HashMap::new();

    let entries = std::fs::read_dir(dir).map_err(|e| {
        NyayaError::Config(format!(
            "Cannot read agents directory {}: {}",
            dir.display(),
            e
        ))
    })?;

    for entry in entries {
        let entry = entry
            .map_err(|e| NyayaError::Config(format!("Error reading directory entry: {}", e)))?;
        let path = entry.path();

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "yaml" && ext != "yml" {
            continue;
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        if stem.is_empty() {
            continue;
        }

        let content = std::fs::read_to_string(&path)?;
        let config: AgentConfig = serde_yaml::from_str(&content)?;
        agents.insert(stem, config);
    }

    Ok(agents)
}

/// Resolve an agent persona by merging _default with the agent-specific config.
pub fn resolve_agent(agents: &HashMap<String, AgentConfig>, agent_id: &str) -> AgentPersona {
    let default_persona = agents
        .get("_default")
        .map(|c| c.persona.clone())
        .unwrap_or_default();

    let agent_persona = agents.get(agent_id).map(|c| c.persona.clone());

    match agent_persona {
        Some(overlay) => default_persona.merge_with(&overlay),
        None => default_persona,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_parse_persona_yaml() {
        let yaml = r#"
persona:
  name: "TestBot"
  voice: "friendly and helpful"
  tone: "warm"
  emoji_usage: "heavy"
  formality: "casual"
  vocabulary_level: "simple"
  quirks:
    - "says 'yo' a lot"
  greeting: "Hey there!"
  system_prompt_prefix: ""
"#;
        let config: AgentConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.persona.name, "TestBot");
        assert_eq!(config.persona.voice, "friendly and helpful");
        assert_eq!(config.persona.emoji_usage, EmojiUsage::Heavy);
        assert_eq!(config.persona.formality, Formality::Casual);
        assert_eq!(config.persona.vocabulary_level, VocabularyLevel::Simple);
        assert_eq!(config.persona.quirks.len(), 1);
    }

    #[test]
    fn test_layered_merge() {
        let base = AgentPersona {
            name: "Nyaya".to_string(),
            voice: "helpful, concise".to_string(),
            tone: "warm".to_string(),
            emoji_usage: EmojiUsage::Minimal,
            formality: Formality::Balanced,
            vocabulary_level: VocabularyLevel::Adaptive,
            quirks: vec!["base quirk".to_string()],
            greeting: "Hello!".to_string(),
            system_prompt_prefix: String::new(),
        };

        let overlay = AgentPersona {
            name: "CustomBot".to_string(),
            voice: String::new(), // empty -> should keep base
            tone: "sarcastic".to_string(),
            emoji_usage: EmojiUsage::Minimal, // default -> keep base
            formality: Formality::Chaotic,
            vocabulary_level: VocabularyLevel::Adaptive, // default -> keep base
            quirks: vec!["overlay quirk".to_string()],
            greeting: String::new(), // empty -> keep base
            system_prompt_prefix: String::new(),
        };

        let merged = base.merge_with(&overlay);
        assert_eq!(merged.name, "CustomBot");
        assert_eq!(merged.voice, "helpful, concise"); // kept from base
        assert_eq!(merged.tone, "sarcastic"); // overridden
        assert_eq!(merged.formality, Formality::Chaotic); // overridden
        assert_eq!(merged.vocabulary_level, VocabularyLevel::Adaptive); // kept (default)
        assert_eq!(merged.quirks, vec!["overlay quirk".to_string()]); // overridden
        assert_eq!(merged.greeting, "Hello!"); // kept from base
    }

    #[test]
    fn test_default_persona_has_sane_values() {
        let p = AgentPersona::default();
        assert_eq!(p.name, "Nyaya");
        assert_eq!(p.emoji_usage, EmojiUsage::Minimal);
        assert_eq!(p.formality, Formality::Balanced);
        assert_eq!(p.vocabulary_level, VocabularyLevel::Adaptive);
        assert!(p.quirks.is_empty());
    }

    #[test]
    fn test_load_agent_from_yaml_string() {
        let yaml = r#"
persona:
  name: "Aria"
  voice: "poetic and thoughtful"
provider:
  preferred: "anthropic"
  fallback:
    - "openai"
  model_override: "claude-opus-4-6"
  max_cost_per_call: 0.05
constitution: "general"
"#;
        let config: AgentConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.persona.name, "Aria");
        assert_eq!(config.persona.voice, "poetic and thoughtful");
        assert_eq!(config.provider.preferred, Some("anthropic".to_string()));
        assert_eq!(config.provider.fallback, vec!["openai".to_string()]);
        assert_eq!(config.constitution, Some("general".to_string()));
    }

    #[test]
    fn test_load_all_agents_from_dir() {
        let dir = TempDir::new().unwrap();

        let default_yaml = r#"
persona:
  name: "Nyaya"
  voice: "helpful"
"#;
        let mut f = std::fs::File::create(dir.path().join("_default.yaml")).unwrap();
        f.write_all(default_yaml.as_bytes()).unwrap();

        let custom_yaml = r#"
persona:
  name: "CustomBot"
  tone: "witty"
"#;
        let mut f = std::fs::File::create(dir.path().join("custom.yml")).unwrap();
        f.write_all(custom_yaml.as_bytes()).unwrap();

        // Non-yaml file should be ignored
        let mut f = std::fs::File::create(dir.path().join("notes.txt")).unwrap();
        f.write_all(b"not a yaml file").unwrap();

        let agents = load_agents_dir(dir.path()).unwrap();
        assert_eq!(agents.len(), 2);
        assert!(agents.contains_key("_default"));
        assert!(agents.contains_key("custom"));
        assert_eq!(agents["_default"].persona.name, "Nyaya");
        assert_eq!(agents["custom"].persona.name, "CustomBot");
    }

    #[test]
    fn test_resolve_agent_with_layered_merge() {
        let dir = TempDir::new().unwrap();

        let default_yaml = r#"
persona:
  name: "Nyaya"
  voice: "helpful, concise"
  tone: "warm"
  formality: "formal"
"#;
        let mut f = std::fs::File::create(dir.path().join("_default.yaml")).unwrap();
        f.write_all(default_yaml.as_bytes()).unwrap();

        let custom_yaml = r#"
persona:
  name: "TradeBot"
  tone: "serious"
"#;
        let mut f = std::fs::File::create(dir.path().join("tradebot.yaml")).unwrap();
        f.write_all(custom_yaml.as_bytes()).unwrap();

        let agents = load_agents_dir(dir.path()).unwrap();
        let resolved = resolve_agent(&agents, "tradebot");

        assert_eq!(resolved.name, "TradeBot"); // from agent-specific
        assert_eq!(resolved.voice, "helpful, concise"); // from _default
        assert_eq!(resolved.tone, "serious"); // overridden
        assert_eq!(resolved.formality, Formality::Formal); // from _default

        // Non-existent agent should return default
        let fallback = resolve_agent(&agents, "nonexistent");
        assert_eq!(fallback.name, "Nyaya");
    }

    #[test]
    fn test_agent_config_with_conditional_styles() {
        let yaml = r#"
persona:
  name: "TestBot"
conditional_styles:
  children:
    name: "children"
    persona_overlay:
      emoji_usage: heavy
      formality: casual
    max_sentence_length: 15
    style_prompt_suffix: ""
"#;
        let config: AgentConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.persona.name, "TestBot");
        assert_eq!(config.conditional_styles.len(), 1);
        let children = config.conditional_styles.get("children").unwrap();
        assert_eq!(children.name, "children");
        assert_eq!(children.persona_overlay.emoji_usage, EmojiUsage::Heavy);
        assert_eq!(children.persona_overlay.formality, Formality::Casual);
        assert_eq!(children.max_sentence_length, Some(15));
    }

    #[test]
    fn test_agent_config_without_conditional_styles_backwards_compat() {
        let yaml = r#"
persona:
  name: "LegacyBot"
"#;
        let config: AgentConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.persona.name, "LegacyBot");
        assert!(config.conditional_styles.is_empty());
    }

    #[test]
    fn test_agent_config_with_knowledge_base() {
        let yaml = r#"
persona:
  name: "TradeBot"
knowledge_base: "trading"
"#;
        let config: AgentConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.persona.name, "TradeBot");
        assert_eq!(config.knowledge_base, Some("trading".to_string()));
    }

    #[test]
    fn test_agent_config_without_knowledge_base() {
        let yaml = r#"
persona:
  name: "LegacyBot"
"#;
        let config: AgentConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.persona.name, "LegacyBot");
        assert!(config.knowledge_base.is_none());
    }

    #[test]
    fn test_agent_config_with_resources() {
        let yaml = r#"
persona:
  name: "RobotBot"
resources:
  - id: arm_camera
    type: device
    device_type: camera
    driver: http
    endpoint: "http://192.168.1.50/snapshot"
"#;
        let config: AgentConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.persona.name, "RobotBot");
        assert_eq!(config.resources.len(), 1);
        assert_eq!(config.resources[0].id, "arm_camera");
    }

    #[test]
    fn test_agent_config_without_resources() {
        let yaml = r#"
persona:
  name: "LegacyBot"
"#;
        let config: AgentConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.persona.name, "LegacyBot");
        assert!(config.resources.is_empty());
    }
}
