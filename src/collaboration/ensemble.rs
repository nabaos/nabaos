use serde::{Deserialize, Serialize};

/// How ensemble output sections are organized.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum OutputFormat {
    #[default]
    Chapters,
    Sections,
    Interleaved,
}

/// An agent in an ensemble collaboration with an assigned section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnsembleAgent {
    pub agent: String,
    pub assignment: String,
    pub order: u32,
}

/// Configuration for an ensemble collaboration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnsembleConfig {
    pub name: String,
    pub mode: String,
    #[serde(default)]
    pub output_format: OutputFormat,
    pub agents: Vec<EnsembleAgent>,
    #[serde(default)]
    pub context_passing: bool,
    pub editor: Option<String>,
}

/// Top-level wrapper for YAML deserialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnsembleFile {
    pub ensemble: EnsembleConfig,
}

/// The result of running an ensemble collaboration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnsembleResult {
    pub sections: Vec<(String, String)>,
    pub edited_output: Option<String>,
    pub total_tokens: u32,
    pub estimated_cost_usd: f64,
}

/// Build a prompt for an ensemble agent.
///
/// Combines the persona prompt, assignment, user query, and optionally
/// the sections produced by previous agents (if context_passing is enabled).
pub fn build_ensemble_prompt(
    user_query: &str,
    persona_prompt: &str,
    assignment: &str,
    previous_sections: &[(String, String)],
) -> String {
    let mut prompt = String::new();

    prompt.push_str(persona_prompt);
    prompt.push_str("\n\n");
    prompt.push_str(&format!("User query: {}\n\n", user_query));
    prompt.push_str(&format!("Your assignment: {}\n", assignment));

    if !previous_sections.is_empty() {
        prompt.push_str("\n--- Previously Completed Sections ---\n");
        for (agent, section) in previous_sections {
            prompt.push_str(&format!("\n[{}]:\n{}\n", agent, section));
        }
        prompt.push_str("\n--- End Previous Sections ---\n");
        prompt.push_str("\nBuild on the context above while completing your assignment.");
    }

    prompt.push_str("\nPlease complete your assigned section.");
    prompt
}

/// Build a prompt for the ensemble editor.
///
/// Asks the editor to review and polish all sections into a cohesive output.
pub fn build_editor_prompt(user_query: &str, sections: &[(String, String)]) -> String {
    let mut prompt = String::new();

    prompt.push_str("You are the editor for this ensemble collaboration. ");
    prompt.push_str("Your task is to review all sections and produce a cohesive, ");
    prompt.push_str("well-structured final output.\n\n");
    prompt.push_str(&format!("Original user query: {}\n\n", user_query));
    prompt.push_str("--- Sections to Edit ---\n");

    for (agent, section) in sections {
        prompt.push_str(&format!("\n[{}]:\n{}\n", agent, section));
    }

    prompt.push_str("\n--- End Sections ---\n\n");
    prompt.push_str("Please edit these sections into a unified, polished document.");
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ensemble_config() {
        let yaml = r#"
ensemble:
  name: "book-chapter"
  mode: "ensemble"
  output_format: sections
  context_passing: true
  editor: "senior-editor"
  agents:
    - agent: "historian"
      assignment: "Write the historical background"
      order: 1
    - agent: "analyst"
      assignment: "Write the analysis section"
      order: 2
    - agent: "futurist"
      assignment: "Write the future outlook"
      order: 3
"#;
        let parsed: EnsembleFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(parsed.ensemble.name, "book-chapter");
        assert_eq!(parsed.ensemble.output_format, OutputFormat::Sections);
        assert!(parsed.ensemble.context_passing);
        assert_eq!(parsed.ensemble.editor, Some("senior-editor".to_string()));
        assert_eq!(parsed.ensemble.agents.len(), 3);
        assert_eq!(parsed.ensemble.agents[0].agent, "historian");
        assert_eq!(
            parsed.ensemble.agents[1].assignment,
            "Write the analysis section"
        );
        assert_eq!(parsed.ensemble.agents[2].order, 3);
    }

    #[test]
    fn test_parse_ensemble_config_defaults() {
        let yaml = r#"
ensemble:
  name: "test"
  mode: "ensemble"
  agents: []
"#;
        let parsed: EnsembleFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(parsed.ensemble.output_format, OutputFormat::Chapters);
        assert!(!parsed.ensemble.context_passing);
        assert!(parsed.ensemble.editor.is_none());
    }

    #[test]
    fn test_build_ensemble_prompt_with_context() {
        let previous = vec![(
            "historian".to_string(),
            "The Roman Empire fell in 476 AD.".to_string(),
        )];
        let prompt = build_ensemble_prompt(
            "Write about the fall of Rome",
            "You are a political analyst.",
            "Analyze the political causes",
            &previous,
        );
        assert!(prompt.contains("Write about the fall of Rome"));
        assert!(prompt.contains("political analyst"));
        assert!(prompt.contains("Analyze the political causes"));
        assert!(prompt.contains("Previously Completed Sections"));
        assert!(prompt.contains("[historian]"));
        assert!(prompt.contains("Roman Empire"));
        assert!(prompt.contains("Build on the context"));
    }

    #[test]
    fn test_build_ensemble_prompt_without_context() {
        let prompt = build_ensemble_prompt(
            "Write about Rust",
            "You are a technical writer.",
            "Write the introduction",
            &[],
        );
        assert!(prompt.contains("Write about Rust"));
        assert!(prompt.contains("technical writer"));
        assert!(prompt.contains("Write the introduction"));
        assert!(!prompt.contains("Previously Completed Sections"));
        assert!(!prompt.contains("Build on the context"));
    }

    #[test]
    fn test_build_editor_prompt() {
        let sections = vec![
            (
                "intro-writer".to_string(),
                "Rust is a systems language.".to_string(),
            ),
            (
                "deep-dive".to_string(),
                "The borrow checker ensures safety.".to_string(),
            ),
        ];
        let prompt = build_editor_prompt("Write about Rust", &sections);
        assert!(prompt.contains("Write about Rust"));
        assert!(prompt.contains("[intro-writer]"));
        assert!(prompt.contains("systems language"));
        assert!(prompt.contains("[deep-dive]"));
        assert!(prompt.contains("borrow checker"));
        assert!(prompt.contains("editor"));
        assert!(prompt.contains("unified"));
    }
}
