use serde::{Deserialize, Serialize};

/// How the council produces its final output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum OutputMode {
    #[default]
    Synthesis,
    Vote,
    AllResponses,
}

/// A participant in a council session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Participant {
    pub agent: String,
    pub role: String,
}

/// Configuration for a council collaboration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouncilConfig {
    pub name: String,
    pub mode: String,
    pub moderator: String,
    pub participants: Vec<Participant>,
    #[serde(default = "default_rounds")]
    pub rounds: u32,
    #[serde(default = "default_parallel")]
    pub parallel: bool,
    #[serde(default)]
    pub output: OutputMode,
}

fn default_rounds() -> u32 {
    1
}

fn default_parallel() -> bool {
    true
}

/// Top-level wrapper for YAML deserialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouncilFile {
    pub council: CouncilConfig,
}

/// A single agent's response in a council round.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub agent_id: String,
    pub role: String,
    pub text: String,
}

/// The result of running a council collaboration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouncilResult {
    pub synthesis: String,
    pub individual_responses: Vec<AgentResponse>,
    pub rounds_completed: u32,
    pub total_tokens: u32,
    pub estimated_cost_usd: f64,
}

/// Build a prompt for a council participant.
///
/// Combines the persona prompt, the participant's role, the user query,
/// and any responses from the previous round.
pub fn build_participant_prompt(
    user_query: &str,
    persona_prompt: &str,
    role: &str,
    previous_round: &[(String, String)],
) -> String {
    let mut prompt = String::new();

    prompt.push_str(persona_prompt);
    prompt.push_str("\n\n");
    prompt.push_str(&format!("Your role in this council: {}\n\n", role));
    prompt.push_str(&format!("User query: {}\n", user_query));

    if !previous_round.is_empty() {
        prompt.push_str("\n--- Previous Round Responses ---\n");
        for (agent, response) in previous_round {
            prompt.push_str(&format!("\n[{}]:\n{}\n", agent, response));
        }
        prompt.push_str("\n--- End Previous Round ---\n");
    }

    prompt.push_str("\nPlease provide your perspective based on your role.");
    prompt
}

/// Build a synthesis prompt for the moderator.
///
/// Asks the moderator to synthesize all participant perspectives into
/// a single coherent response.
pub fn build_synthesis_prompt(user_query: &str, responses: &[(String, String)]) -> String {
    let mut prompt = String::new();

    prompt.push_str("You are the council moderator. Your task is to synthesize ");
    prompt.push_str("the following perspectives into a single coherent response.\n\n");
    prompt.push_str(&format!("Original user query: {}\n\n", user_query));
    prompt.push_str("--- Participant Responses ---\n");

    for (agent, response) in responses {
        prompt.push_str(&format!("\n[{}]:\n{}\n", agent, response));
    }

    prompt.push_str("\n--- End Participant Responses ---\n\n");
    prompt.push_str("Please synthesize these perspectives into a comprehensive answer.");
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_council_config() {
        let yaml = r#"
council:
  name: "security-review"
  mode: "council"
  moderator: "lead-analyst"
  participants:
    - agent: "red-team"
      role: "attack surface analyst"
    - agent: "blue-team"
      role: "defense strategist"
  rounds: 2
  parallel: false
  output: vote
"#;
        let parsed: CouncilFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(parsed.council.name, "security-review");
        assert_eq!(parsed.council.moderator, "lead-analyst");
        assert_eq!(parsed.council.participants.len(), 2);
        assert_eq!(parsed.council.participants[0].agent, "red-team");
        assert_eq!(parsed.council.participants[1].role, "defense strategist");
        assert_eq!(parsed.council.rounds, 2);
        assert!(!parsed.council.parallel);
        assert_eq!(parsed.council.output, OutputMode::Vote);
    }

    #[test]
    fn test_parse_council_config_defaults() {
        let yaml = r#"
council:
  name: "test"
  mode: "council"
  moderator: "mod"
  participants: []
"#;
        let parsed: CouncilFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(parsed.council.rounds, 1);
        assert!(parsed.council.parallel);
        assert_eq!(parsed.council.output, OutputMode::Synthesis);
    }

    #[test]
    fn test_build_participant_prompt() {
        let prompt = build_participant_prompt(
            "How should we secure the API?",
            "You are a cybersecurity expert.",
            "penetration tester",
            &[],
        );
        assert!(prompt.contains("How should we secure the API?"));
        assert!(prompt.contains("penetration tester"));
        assert!(prompt.contains("cybersecurity expert"));
        assert!(!prompt.contains("Previous Round"));
    }

    #[test]
    fn test_build_participant_prompt_with_previous_round() {
        let previous = vec![
            ("alice".to_string(), "We need rate limiting.".to_string()),
            ("bob".to_string(), "Add input validation.".to_string()),
        ];
        let prompt = build_participant_prompt(
            "How should we secure the API?",
            "You are an expert.",
            "reviewer",
            &previous,
        );
        assert!(prompt.contains("Previous Round"));
        assert!(prompt.contains("[alice]"));
        assert!(prompt.contains("rate limiting"));
        assert!(prompt.contains("[bob]"));
    }

    #[test]
    fn test_build_synthesis_prompt() {
        let responses = vec![
            (
                "analyst".to_string(),
                "Focus on authentication.".to_string(),
            ),
            ("architect".to_string(), "Consider scalability.".to_string()),
        ];
        let prompt = build_synthesis_prompt("Design the system", &responses);
        assert!(prompt.contains("Design the system"));
        assert!(prompt.contains("[analyst]"));
        assert!(prompt.contains("Focus on authentication"));
        assert!(prompt.contains("[architect]"));
        assert!(prompt.contains("Consider scalability"));
        assert!(prompt.contains("synthesize"));
    }
}
