//! LLM workflow generator for meta-agent system.
//!
//! Takes a natural language requirement, checks the template library first,
//! then falls back to LLM generation via Anthropic or OpenAI APIs.

use crate::chain::workflow::WorkflowDef;
use crate::meta_agent::capability_index::CapabilityIndex;
use crate::meta_agent::template_library::TemplateLibrary;

/// Generates workflow definitions from natural language requirements.
///
/// Uses a two-tier strategy:
/// 1. Check the template library for a keyword match (free, instant).
/// 2. Fall back to LLM generation using the capability digest as context.
pub struct WorkflowGenerator {
    capability_digest: String,
}

impl WorkflowGenerator {
    /// Create a new generator from a capability index.
    ///
    /// The index is rendered to a compact text digest that will be injected
    /// into the LLM prompt so the model knows which abilities are available.
    pub fn new(index: &CapabilityIndex) -> Self {
        Self {
            capability_digest: index.to_digest(),
        }
    }

    /// Generate a workflow definition for the given natural language requirement.
    ///
    /// First checks the template library for a keyword match. If a template
    /// scores above the 0.3 threshold, its `WorkflowDef` is returned directly
    /// without any LLM call. Otherwise, builds a prompt and calls the LLM.
    pub fn generate(
        &self,
        requirement: &str,
        templates: &TemplateLibrary,
    ) -> Result<WorkflowDef, String> {
        // Tier 1: check templates
        if let Some(tmpl) = templates.find_match(requirement) {
            return Ok(tmpl.def.clone());
        }

        // Tier 2: LLM generation
        let prompt = self.build_prompt(requirement);
        let yaml_text = call_llm_for_workflow(&prompt)?;
        let def: WorkflowDef = serde_yaml::from_str(&yaml_text)
            .map_err(|e| format!("Failed to parse LLM YAML response: {}", e))?;
        self.validate_workflow(&def)?;
        Ok(def)
    }

    /// Build the LLM prompt that includes the capability digest, rules,
    /// and the user requirement.
    pub fn build_prompt(&self, requirement: &str) -> String {
        format!(
            r#"You are a workflow architect for the Nyaya Agent system.

Given the available abilities and constraints below, generate a WorkflowDef in YAML format.

{}

RULES:
- Only use abilities from the ABILITIES list above
- Use WaitPoll for external status checks (polling APIs)
- Use WaitEvent for webhook-driven flows (external triggers)
- Use Parallel for independent subtasks that can run concurrently
- Use Branch for conditional logic
- Use Delay for fixed time waits
- Include on_failure handlers for critical steps
- Set reasonable timeouts (seconds)
- Give each node a clear, descriptive id
- Use {{{{param_name}}}} syntax for parameterized values

USER REQUIREMENT: {}

Generate ONLY the YAML (no markdown fences, no explanation):"#,
            self.capability_digest, requirement
        )
    }

    /// Validate that all ability references in the workflow exist in the
    /// capability digest.
    ///
    /// Checks Action nodes and WaitPoll nodes for their `ability` field.
    pub fn validate_workflow(&self, def: &WorkflowDef) -> Result<(), String> {
        let mut unknown = Vec::new();
        validate_nodes(&def.nodes, &self.capability_digest, &mut unknown);
        if unknown.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "Workflow references unknown abilities: {}",
                unknown.join(", ")
            ))
        }
    }
}

/// Recursively walk workflow nodes and collect abilities not present in the digest.
fn validate_nodes(
    nodes: &[crate::chain::workflow::WorkflowNode],
    digest: &str,
    unknown: &mut Vec<String>,
) {
    use crate::chain::workflow::WorkflowNode;
    for node in nodes {
        match node {
            WorkflowNode::Action(a) => {
                if !digest.contains(&a.ability) {
                    unknown.push(a.ability.clone());
                }
            }
            WorkflowNode::WaitPoll(wp) => {
                if !digest.contains(&wp.ability) {
                    unknown.push(wp.ability.clone());
                }
            }
            WorkflowNode::Parallel(p) => {
                for branch in &p.branches {
                    validate_nodes(&branch.nodes, digest, unknown);
                }
            }
            WorkflowNode::Branch(b) => {
                for arm in &b.conditions {
                    validate_nodes(&arm.nodes, digest, unknown);
                }
                validate_nodes(&b.otherwise, digest, unknown);
            }
            // Delay, WaitEvent, and Compensate do not reference abilities directly
            // (compensation ability validation is deferred to engine wiring).
            WorkflowNode::Delay(_) | WorkflowNode::WaitEvent(_) | WorkflowNode::Compensate(_) => {}
        }
    }
}

/// Strip leading/trailing markdown fences from LLM output.
///
/// Handles patterns like:
/// - ```yaml\n...\n```
/// - ```\n...\n```
fn strip_markdown_fences(text: &str) -> String {
    let trimmed = text.trim();
    let without_opening = if trimmed.starts_with("```yaml") {
        trimmed
            .strip_prefix("```yaml")
            .unwrap()
            .trim_start_matches('\n')
    } else if trimmed.starts_with("```yml") {
        trimmed
            .strip_prefix("```yml")
            .unwrap()
            .trim_start_matches('\n')
    } else if trimmed.starts_with("```") {
        trimmed
            .strip_prefix("```")
            .unwrap()
            .trim_start_matches('\n')
    } else {
        trimmed
    };
    let without_closing = if without_opening.ends_with("```") {
        without_opening.strip_suffix("```").unwrap().trim_end()
    } else {
        without_opening
    };
    without_closing.to_string()
}

/// Call an LLM API to generate a workflow YAML string from the given prompt.
///
/// Reads configuration from environment variables:
/// - `NABA_LLM_API_KEY` (required)
/// - `NABA_LLM_PROVIDER` (default: "anthropic")
/// - `NABA_CHEAP_LLM_MODEL` (default: "claude-haiku-4-5")
///
/// Supports "anthropic" and "openai" providers.
pub fn call_llm_for_workflow(prompt: &str) -> Result<String, String> {
    let api_key = std::env::var("NABA_LLM_API_KEY")
        .map_err(|_| "NABA_LLM_API_KEY environment variable not set".to_string())?;

    let provider = std::env::var("NABA_LLM_PROVIDER").unwrap_or_else(|_| "anthropic".to_string());
    let model =
        std::env::var("NABA_CHEAP_LLM_MODEL").unwrap_or_else(|_| "claude-haiku-4-5".to_string());

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let raw_text = match provider.as_str() {
        "anthropic" => call_anthropic(&client, &api_key, &model, prompt)?,
        "openai" => call_openai(&client, &api_key, &model, prompt)?,
        other => return Err(format!("Unsupported LLM provider: {}", other)),
    };

    Ok(strip_markdown_fences(&raw_text))
}

/// Call the Anthropic Messages API.
fn call_anthropic(
    client: &reqwest::blocking::Client,
    api_key: &str,
    model: &str,
    prompt: &str,
) -> Result<String, String> {
    let body = serde_json::json!({
        "model": model,
        "max_tokens": 4096,
        "messages": [{"role": "user", "content": prompt}]
    });

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| format!("Anthropic API request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().unwrap_or_default();
        return Err(format!("Anthropic API error {}: {}", status, body_text));
    }

    let json: serde_json::Value = resp
        .json()
        .map_err(|e| format!("Failed to parse Anthropic response: {}", e))?;

    json["content"][0]["text"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Anthropic response missing content[0].text".to_string())
}

/// Call the OpenAI Chat Completions API.
fn call_openai(
    client: &reqwest::blocking::Client,
    api_key: &str,
    model: &str,
    prompt: &str,
) -> Result<String, String> {
    let body = serde_json::json!({
        "model": model,
        "max_tokens": 4096,
        "messages": [{"role": "user", "content": prompt}]
    });

    let resp = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| format!("OpenAI API request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().unwrap_or_default();
        return Err(format!("OpenAI API error {}: {}", status, body_text));
    }

    let json: serde_json::Value = resp
        .json()
        .map_err(|e| format!("Failed to parse OpenAI response: {}", e))?;

    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "OpenAI response missing choices[0].message.content".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta_agent::capability_index::{CapabilityIndex, WorkflowSummary};
    use crate::runtime::host_functions::AbilitySpec;
    use crate::runtime::plugin::AbilitySource;

    fn sample_index() -> CapabilityIndex {
        let abilities = vec![
            AbilitySpec {
                name: "email.send".to_string(),
                description: "Send an email".to_string(),
                permission: "email.send".to_string(),
                source: AbilitySource::BuiltIn,
                input_schema: None,
            },
            AbilitySpec {
                name: "shopify.get_order".to_string(),
                description: "Fetch Shopify order details".to_string(),
                permission: "shopify.get_order".to_string(),
                source: AbilitySource::BuiltIn,
                input_schema: None,
            },
        ];

        let workflows = vec![WorkflowSummary {
            id: "test_wf".into(),
            name: "Test Workflow".into(),
            description: "A test workflow".into(),
            node_count: 3,
        }];

        CapabilityIndex::build(&abilities, workflows, &[])
    }

    #[test]
    fn test_build_prompt_contains_digest() {
        let index = sample_index();
        let wgen = WorkflowGenerator::new(&index);
        let prompt = wgen.build_prompt("process a new order");

        // The digest text should be embedded in the prompt.
        assert!(
            prompt.contains("email.send"),
            "Prompt should contain the email.send ability from the digest"
        );
        assert!(
            prompt.contains("shopify.get_order"),
            "Prompt should contain the shopify.get_order ability from the digest"
        );
        assert!(
            prompt.contains("process a new order"),
            "Prompt should contain the user requirement"
        );
        assert!(
            prompt.contains("ABILITIES (2):"),
            "Prompt should contain the ABILITIES header from the digest"
        );
    }

    #[test]
    fn test_build_prompt_contains_rules() {
        let index = sample_index();
        let wgen = WorkflowGenerator::new(&index);
        let prompt = wgen.build_prompt("anything");

        assert!(
            prompt.contains("RULES:"),
            "Prompt must contain RULES section"
        );
        assert!(
            prompt.contains("Only use abilities from the ABILITIES list above"),
            "Prompt must include the abilities-only rule"
        );
        assert!(
            prompt.contains("Use WaitPoll for external status checks"),
            "Prompt must include WaitPoll rule"
        );
        assert!(
            prompt.contains("Use WaitEvent for webhook-driven flows"),
            "Prompt must include WaitEvent rule"
        );
        assert!(
            prompt.contains("Use Parallel for independent subtasks"),
            "Prompt must include Parallel rule"
        );
        assert!(
            prompt.contains("Use Branch for conditional logic"),
            "Prompt must include Branch rule"
        );
        assert!(
            prompt.contains("Include on_failure handlers"),
            "Prompt must include on_failure rule"
        );
    }

    #[test]
    fn test_validate_workflow_valid() {
        let index = sample_index();
        let wgen = WorkflowGenerator::new(&index);

        // Build a workflow that references only known abilities.
        let def = WorkflowDef {
            id: "test".into(),
            name: "Test".into(),
            description: "Test workflow".into(),
            params: vec![],
            nodes: vec![
                crate::chain::workflow::WorkflowNode::Action(crate::chain::workflow::ActionNode {
                    id: "step1".into(),
                    ability: "email.send".into(),
                    args: std::collections::HashMap::new(),
                    output_key: None,
                    condition: None,
                    on_failure: None,
                }),
                crate::chain::workflow::WorkflowNode::Action(crate::chain::workflow::ActionNode {
                    id: "step2".into(),
                    ability: "shopify.get_order".into(),
                    args: std::collections::HashMap::new(),
                    output_key: None,
                    condition: None,
                    on_failure: None,
                }),
            ],
            global_timeout_secs: 0,
            max_instances: 0,
            correlation_key: None,
            style: None,
            kb_context: None,
            channel_permissions: None,
        };

        assert!(
            wgen.validate_workflow(&def).is_ok(),
            "Workflow referencing known abilities should pass validation"
        );
    }

    #[test]
    fn test_validate_workflow_unknown_ability() {
        let index = sample_index();
        let wgen = WorkflowGenerator::new(&index);

        let def = WorkflowDef {
            id: "bad".into(),
            name: "Bad".into(),
            description: "Bad workflow".into(),
            params: vec![],
            nodes: vec![crate::chain::workflow::WorkflowNode::Action(
                crate::chain::workflow::ActionNode {
                    id: "step1".into(),
                    ability: "nonexistent.ability".into(),
                    args: std::collections::HashMap::new(),
                    output_key: None,
                    condition: None,
                    on_failure: None,
                },
            )],
            global_timeout_secs: 0,
            max_instances: 0,
            correlation_key: None,
            style: None,
            kb_context: None,
            channel_permissions: None,
        };

        let result = wgen.validate_workflow(&def);
        assert!(result.is_err(), "Should fail for unknown ability");
        let err = result.unwrap_err();
        assert!(
            err.contains("nonexistent.ability"),
            "Error should mention the unknown ability, got: {}",
            err
        );
    }

    #[test]
    fn test_generate_uses_template_first() {
        // Build an index with a dummy ability (doesn't matter for template matching).
        let index = sample_index();
        let wgen = WorkflowGenerator::new(&index);

        // TemplateLibrary::new() loads the 5 builtin demo workflows.
        let templates = TemplateLibrary::new();

        // Use keywords that match the shopify_dropship template.
        let result = wgen.generate("shopify order fulfillment shipping dropship", &templates);
        assert!(
            result.is_ok(),
            "Template match should succeed without API key: {:?}",
            result.err()
        );
        let def = result.unwrap();
        assert_eq!(
            def.id, "shopify_dropship",
            "Should return the shopify_dropship template"
        );
    }

    #[test]
    fn test_strip_markdown_fences() {
        let yaml = "id: test\nname: Test";
        assert_eq!(
            strip_markdown_fences(&format!("```yaml\n{}\n```", yaml)),
            yaml
        );
        assert_eq!(
            strip_markdown_fences(&format!("```yml\n{}\n```", yaml)),
            yaml
        );
        assert_eq!(strip_markdown_fences(&format!("```\n{}\n```", yaml)), yaml);
        assert_eq!(strip_markdown_fences(yaml), yaml);
        assert_eq!(
            strip_markdown_fences(&format!("  ```yaml\n{}\n```  ", yaml)),
            yaml
        );
    }
}
