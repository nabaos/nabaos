use serde::{Deserialize, Serialize};

/// How errors are handled during a relay pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ErrorHandling {
    #[default]
    RetryStage,
    SkipStage,
    Abort,
}

/// A single stage in a relay pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayStage {
    pub agent: String,
    pub task: String,
    pub pass_to_next: String,
}

/// Configuration for a relay collaboration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayConfig {
    pub name: String,
    pub mode: String,
    pub stages: Vec<RelayStage>,
    #[serde(default)]
    pub error_handling: ErrorHandling,
}

/// Top-level wrapper for YAML deserialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayFile {
    pub relay: RelayConfig,
}

/// The result of running a relay pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayResult {
    pub final_output: String,
    pub stage_outputs: Vec<(String, String)>,
    pub total_tokens: u32,
    pub estimated_cost_usd: f64,
}

/// Build a prompt for a relay stage.
///
/// Combines the persona prompt, task description, the original query,
/// and optionally the output from the previous stage.
pub fn build_stage_prompt(
    original_query: &str,
    persona_prompt: &str,
    task_description: &str,
    previous_output: Option<&str>,
) -> String {
    let mut prompt = String::new();

    prompt.push_str(persona_prompt);
    prompt.push_str("\n\n");
    prompt.push_str(&format!("Original query: {}\n\n", original_query));
    prompt.push_str(&format!("Your task: {}\n", task_description));

    if let Some(prev) = previous_output {
        prompt.push_str("\n--- Output from Previous Stage ---\n");
        prompt.push_str(prev);
        prompt.push_str("\n--- End Previous Stage Output ---\n");
    }

    prompt.push_str("\nPlease complete your assigned task.");
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_relay_config() {
        let yaml = r#"
relay:
  name: "content-pipeline"
  mode: "relay"
  stages:
    - agent: "researcher"
      task: "Research the topic"
      pass_to_next: "research findings"
    - agent: "writer"
      task: "Write a draft based on research"
      pass_to_next: "draft article"
    - agent: "editor"
      task: "Polish the draft"
      pass_to_next: "final article"
  error_handling: skip_stage
"#;
        let parsed: RelayFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(parsed.relay.name, "content-pipeline");
        assert_eq!(parsed.relay.stages.len(), 3);
        assert_eq!(parsed.relay.stages[0].agent, "researcher");
        assert_eq!(
            parsed.relay.stages[1].task,
            "Write a draft based on research"
        );
        assert_eq!(parsed.relay.stages[2].pass_to_next, "final article");
        assert_eq!(parsed.relay.error_handling, ErrorHandling::SkipStage);
    }

    #[test]
    fn test_parse_relay_config_default_error_handling() {
        let yaml = r#"
relay:
  name: "test"
  mode: "relay"
  stages: []
"#;
        let parsed: RelayFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(parsed.relay.error_handling, ErrorHandling::RetryStage);
    }

    #[test]
    fn test_build_stage_prompt_without_previous() {
        let prompt = build_stage_prompt(
            "Write about Rust",
            "You are a technical writer.",
            "Research the topic thoroughly",
            None,
        );
        assert!(prompt.contains("Write about Rust"));
        assert!(prompt.contains("technical writer"));
        assert!(prompt.contains("Research the topic thoroughly"));
        assert!(!prompt.contains("Previous Stage"));
    }

    #[test]
    fn test_build_stage_prompt_with_previous() {
        let prompt = build_stage_prompt(
            "Write about Rust",
            "You are an editor.",
            "Polish the draft",
            Some("Rust is a systems programming language known for safety."),
        );
        assert!(prompt.contains("Write about Rust"));
        assert!(prompt.contains("editor"));
        assert!(prompt.contains("Polish the draft"));
        assert!(prompt.contains("Previous Stage"));
        assert!(prompt.contains("systems programming language"));
    }
}
