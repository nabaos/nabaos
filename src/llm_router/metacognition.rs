// src/llm_router/metacognition.rs
// Parses the metacognition block that expensive LLMs append to their responses.
// This is how the system learns what to cache and what to delegate.

use serde::{Deserialize, Serialize};

use crate::cache::semantic_cache::{Parameter, ToolCall};

/// The metacognition prompt appended to expensive LLM calls
pub const METACOGNITION_PROMPT: &str = r#"
---METACOGNITION---
You have just completed a task. Now evaluate your own work:

1. APPROACH_RATIONALE: In 1-2 sentences, why did you choose this approach?

2. ALTERNATIVES: Is there a simpler way to accomplish this? If yes, describe it.
   If the simpler way is almost as good, prefer it for future caching.

3. CACHEABILITY: Should this solution be cached for reuse?
   Answer with a JSON block between ```cache and ```:
   ```cache
   {
     "cacheable": true,
     "reason": "this is a recurring pattern with variable parameters",
     "function_name": "check_email_by_sender",
     "description": "Check email inbox filtered by sender name",
     "parameters": [
       {"name": "sender", "type": "text", "description": "sender name or email to filter by", "required": true}
     ],
     "tool_sequence": [
       {"tool": "email_fetch", "args": {"filter": "from:{{sender}}"}}
     ],
     "edge_cases": ["sender name is ambiguous", "multiple matching senders"]
   }
   ```

4. DELEGATION: Could a cheaper/smaller model have handled this?
   Answer: "fully" | "partially" | "no"
   If partially, which subtasks?

5. CONFIDENCE: 0.0-1.0
---END METACOGNITION---
"#;

/// Parsed metacognition response from the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetacognitionResult {
    pub rationale: String,
    pub alternatives: Option<String>,
    pub cache_decision: Option<CacheDecision>,
    pub delegation: DelegationAssessment,
    pub confidence: f64,
}

/// The LLM's cache decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheDecision {
    pub cacheable: bool,
    pub reason: String,
    pub function_name: String,
    pub description: String,
    pub parameters: Vec<Parameter>,
    pub tool_sequence: Vec<ToolCall>,
    pub edge_cases: Vec<String>,
}

/// Whether a cheaper model could handle this task
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DelegationAssessment {
    Fully,
    Partially { subtasks: Vec<String> },
    No,
}

/// Parse the metacognition block from an LLM response.
/// Returns the clean response (without metacognition) and the parsed result.
pub fn parse_metacognition(full_response: &str) -> (String, Option<MetacognitionResult>) {
    // Find the metacognition block
    let Some(start) = full_response.find("---METACOGNITION---") else {
        return (full_response.to_string(), None);
    };
    let end = full_response
        .find("---END METACOGNITION---")
        .unwrap_or(full_response.len());

    // Extract clean response (everything before the metacognition block)
    let clean_response = full_response[..start].trim().to_string();
    let meta_block = &full_response[start..end];

    // Parse the metacognition block
    let result = parse_meta_block(meta_block);

    (clean_response, result)
}

fn parse_meta_block(block: &str) -> Option<MetacognitionResult> {
    let rationale = extract_section(block, "APPROACH_RATIONALE")?;
    let alternatives = extract_section(block, "ALTERNATIVES");
    let cache_json = extract_cache_json(block);
    let delegation = extract_delegation(block);
    let confidence = extract_confidence(block);

    let cache_decision = cache_json.and_then(|json| {
        serde_json::from_str::<CacheDecision>(&json)
            .map_err(|e| {
                tracing::warn!(error = %e, "Failed to parse cache decision JSON");
                e
            })
            .ok()
    });

    Some(MetacognitionResult {
        rationale,
        alternatives,
        cache_decision,
        delegation: delegation.unwrap_or(DelegationAssessment::No),
        confidence: confidence.unwrap_or(0.5),
    })
}

/// Extract a named section from the metacognition block
fn extract_section(block: &str, section_name: &str) -> Option<String> {
    let pattern = format!("{}:", section_name);
    let start = block.find(&pattern)?;
    let content_start = start + pattern.len();

    // Find the next section (numbered item) or end of block
    let rest = &block[content_start..];
    let end = rest
        .find("\n\n")
        .or_else(|| rest.find("\n2."))
        .or_else(|| rest.find("\n3."))
        .or_else(|| rest.find("\n4."))
        .or_else(|| rest.find("\n5."))
        .unwrap_or(rest.len());

    let content = rest[..end].trim().to_string();
    if content.is_empty() {
        None
    } else {
        Some(content)
    }
}

/// Extract the JSON cache block
fn extract_cache_json(block: &str) -> Option<String> {
    // Look for ```cache ... ``` or just { ... } after CACHEABILITY
    if let Some(start) = block.find("```cache") {
        let json_start = start + "```cache".len();
        let json_end = block[json_start..].find("```")?;
        return Some(block[json_start..json_start + json_end].trim().to_string());
    }

    // Fallback: find JSON object after CACHEABILITY
    if let Some(cache_section) = extract_section(block, "CACHEABILITY") {
        if let Some(brace_start) = cache_section.find('{') {
            // Find matching closing brace
            let mut depth = 0;
            for (i, ch) in cache_section[brace_start..].chars().enumerate() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(
                                cache_section[brace_start..brace_start + i + 1].to_string(),
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    None
}

/// Extract delegation assessment
fn extract_delegation(block: &str) -> Option<DelegationAssessment> {
    let section = extract_section(block, "DELEGATION")?;
    let lower = section.to_lowercase();

    if lower.starts_with("fully") || lower.contains("\"fully\"") {
        Some(DelegationAssessment::Fully)
    } else if lower.starts_with("no") || lower.contains("\"no\"") {
        Some(DelegationAssessment::No)
    } else if lower.starts_with("partially") || lower.contains("\"partially\"") {
        // Try to extract subtask list
        let subtasks: Vec<String> = section
            .lines()
            .skip(1)
            .filter(|l| l.trim().starts_with('-') || l.trim().starts_with('*'))
            .map(|l| l.trim().trim_start_matches(['-', '*', ' ']).to_string())
            .collect();
        Some(DelegationAssessment::Partially { subtasks })
    } else {
        None
    }
}

/// Extract confidence score
fn extract_confidence(block: &str) -> Option<f64> {
    let section = extract_section(block, "CONFIDENCE")?;
    // Find a float in the section
    section
        .split_whitespace()
        .find_map(|word| {
            word.trim_matches(|c: char| !c.is_ascii_digit() && c != '.')
                .parse::<f64>()
                .ok()
        })
        .filter(|&v| (0.0..=1.0).contains(&v))
}

/// Append the metacognition prompt to a user message for expensive LLM calls
pub fn append_metacognition(original_prompt: &str) -> String {
    format!("{original_prompt}\n\n{METACOGNITION_PROMPT}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_metacognition_extracts_clean_response() {
        let response = "Here is the email summary.\n\n---METACOGNITION---\nAPPROACH_RATIONALE: Used email API.\n---END METACOGNITION---";
        let (clean, meta) = parse_metacognition(response);
        assert_eq!(clean, "Here is the email summary.");
        assert!(meta.is_some());
        assert!(meta.unwrap().rationale.contains("email API"));
    }

    #[test]
    fn test_parse_metacognition_no_block() {
        let response = "Just a normal response.";
        let (clean, meta) = parse_metacognition(response);
        assert_eq!(clean, "Just a normal response.");
        assert!(meta.is_none());
    }

    #[test]
    fn test_extract_confidence() {
        assert_eq!(extract_confidence("CONFIDENCE: 0.85"), Some(0.85));
        assert_eq!(
            extract_confidence("CONFIDENCE: 0.95 — very confident"),
            Some(0.95)
        );
        assert_eq!(extract_confidence("CONFIDENCE: not sure"), None);
    }
}
