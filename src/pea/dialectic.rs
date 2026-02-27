use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// DialecticReview — result of a Hegelian strategic review
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialecticReview {
    pub thesis: String,
    pub antithesis: String,
    pub synthesis: String,
    pub action_items: Vec<String>,
    pub should_pivot: bool,
}

// ---------------------------------------------------------------------------
// Prompt builder
// ---------------------------------------------------------------------------

/// Build an LLM prompt asking for Hegelian dialectic analysis of the current
/// strategy, incorporating progress data and belief state.
pub fn build_dialectic_prompt(
    objective_description: &str,
    current_strategy: &str,
    progress_score: f64,
    completed_tasks: &[String],
    failed_tasks: &[String],
    beliefs_summary: &str,
) -> String {
    let completed_list = if completed_tasks.is_empty() {
        "(none)".to_string()
    } else {
        completed_tasks
            .iter()
            .map(|t| format!("- {t}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let failed_list = if failed_tasks.is_empty() {
        "(none)".to_string()
    } else {
        failed_tasks
            .iter()
            .map(|t| format!("- {t}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        r#"You are a strategic advisor performing a Hegelian dialectic review.

## Objective
{objective_description}

## Current Strategy
{current_strategy}

## Progress
Score: {progress_score:.2}

### Completed Tasks
{completed_list}

### Failed Tasks
{failed_list}

## Current Beliefs
{beliefs_summary}

## Instructions

Perform a Hegelian dialectic analysis:

1. **THESIS**: State the current strategy and its underlying assumptions.
2. **ANTITHESIS**: Identify contradictions, failures, and evidence against the current approach.
3. **SYNTHESIS**: Propose a refined strategy that resolves the contradictions and incorporates lessons learned.

Respond with a JSON object (no extra text outside the JSON):
{{
  "thesis": "...",
  "antithesis": "...",
  "synthesis": "...",
  "action_items": ["item1", "item2", ...],
  "should_pivot": true/false
}}"#
    )
}

// ---------------------------------------------------------------------------
// Response parser
// ---------------------------------------------------------------------------

/// Extract JSON from an LLM response (handling optional markdown code fences)
/// and deserialize into a `DialecticReview`.
pub fn parse_dialectic_response(response: &str) -> Option<DialecticReview> {
    let first_brace = response.find('{')?;
    let last_brace = response.rfind('}')?;
    if last_brace < first_brace {
        return None;
    }
    let json_str = &response[first_brace..=last_brace];
    serde_json::from_str(json_str).ok()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_dialectic_prompt_contains_objective() {
        let prompt = build_dialectic_prompt(
            "Launch the product by March",
            "Incremental development",
            0.45,
            &["Design complete".to_string()],
            &["CI pipeline broken".to_string()],
            "Team morale is moderate",
        );

        assert!(prompt.contains("Launch the product by March"));
        assert!(prompt.contains("THESIS"));
        assert!(prompt.contains("ANTITHESIS"));
        assert!(prompt.contains("SYNTHESIS"));
    }

    #[test]
    fn test_parse_dialectic_response_valid_json() {
        let raw = r#"{"thesis":"current approach works","antithesis":"but it is slow","synthesis":"combine both","action_items":["speed up","add tests"],"should_pivot":false}"#;

        let review = parse_dialectic_response(raw).expect("should parse valid JSON");
        assert_eq!(review.thesis, "current approach works");
        assert_eq!(review.antithesis, "but it is slow");
        assert_eq!(review.synthesis, "combine both");
        assert_eq!(review.action_items, vec!["speed up", "add tests"]);
        assert!(!review.should_pivot);
    }

    #[test]
    fn test_parse_dialectic_response_json_in_markdown() {
        let wrapped = r#"Here is my analysis:
```json
{
  "thesis": "we are on track",
  "antithesis": "deadlines slipping",
  "synthesis": "re-prioritize deliverables",
  "action_items": ["cut scope", "add resources"],
  "should_pivot": true
}
```
Hope this helps!"#;

        let review = parse_dialectic_response(wrapped).expect("should extract JSON from markdown");
        assert_eq!(review.thesis, "we are on track");
        assert_eq!(review.antithesis, "deadlines slipping");
        assert_eq!(review.synthesis, "re-prioritize deliverables");
        assert_eq!(review.action_items, vec!["cut scope", "add resources"]);
        assert!(review.should_pivot);
    }
}
