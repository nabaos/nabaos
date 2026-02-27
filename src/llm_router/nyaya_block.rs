// src/llm_router/nyaya_block.rs
// Parser for the <nyaya> self-annotation blocks that the LLM emits after each response.
// Handles MODE 1-5: C (template ref), NEW (novel chain), PATCH, CACHE, NOCACHE.
//
// The LLM is the compiler. The <nyaya> block is like HTTP Cache-Control headers —
// the server (LLM) tells the CDN (orchestrator) how to cache its own output.

use crate::core::error::{NyayaError, Result};

/// Parsed <nyaya> block from an LLM response.
#[derive(Debug, Clone)]
pub enum NyayaBlock {
    /// MODE 1: Known template reference — C:template_name|param1|param2|...
    TemplateRef {
        template_name: String,
        params: Vec<String>,
    },

    /// MODE 2: Novel chain definition — NEW:chain_name with steps, params, etc.
    NewChain {
        chain_name: String,
        params: Vec<ParamSpec>,
        steps: Vec<StepSpec>,
        trigger: Option<String>,
        circuit_breakers: Vec<String>,
        intent_label: Option<String>,
        rephrasings: Vec<String>,
    },

    /// MODE 3: Patch an existing template — add params, steps, or modify behavior.
    Patch {
        base_template: String,
        base_params: Vec<String>,
        add_params: Vec<ParamSpec>,
        add_steps: Vec<PatchStep>,
        remove_steps: Vec<String>,
        intent_label: Option<String>,
        rephrasings: Vec<String>,
    },

    /// MODE 4: Simple cacheable answer (not a workflow) — CACHE:ttl
    Cache {
        ttl: String,
        intent_label: Option<String>,
        rephrasings: Vec<String>,
    },

    /// MODE 5: Non-cacheable — NOCACHE (still teaches SetFit)
    NoCache {
        intent_label: Option<String>,
        rephrasings: Vec<String>,
    },

    /// MODE 6: Propose a new function for the function library
    ProposeFunc {
        func_name: String,
        description: String,
        category: String,
        security_tier: String,
        params: Vec<ProposedParam>,
        returns: Option<ProposedReturn>,
        return_fields: Vec<ProposedReturnField>,
        examples: Vec<ProposedExample>,
    },
}

/// Parameter specification for a proposed function (MODE 6).
#[derive(Debug, Clone)]
pub struct ProposedParam {
    pub name: String,
    pub schema_type: String,
    pub required: bool,
    pub default: Option<String>,
    pub description: String,
}

/// Return schema for a proposed function (MODE 6).
#[derive(Debug, Clone)]
pub struct ProposedReturn {
    pub schema_type: String,
    pub description: String,
}

/// Return field for a proposed function (MODE 6).
#[derive(Debug, Clone)]
pub struct ProposedReturnField {
    pub name: String,
    pub schema_type: String,
    pub description: String,
}

/// Example invocation for a proposed function (MODE 6).
#[derive(Debug, Clone)]
pub struct ProposedExample {
    pub input: String,
    pub output: String,
}

/// Parameter specification from P: lines.
#[derive(Debug, Clone)]
pub struct ParamSpec {
    pub name: String,
    pub param_type: String,
    pub default_value: Option<String>,
}

/// Step specification from S: lines.
#[derive(Debug, Clone)]
pub struct StepSpec {
    pub ability: String,
    pub params: String,
    pub output_var: Option<String>,
    pub confirm: bool,
}

/// A step addition for PATCH mode.
#[derive(Debug, Clone)]
pub struct PatchStep {
    pub position: String, // "after:step_id"
    pub step_id: String,
    pub ability: String,
    pub params: String,
}

/// Result of parsing an LLM response: the user-facing text and optional nyaya metadata.
#[derive(Debug)]
pub struct ParsedResponse {
    pub user_text: String,
    pub nyaya: Option<NyayaBlock>,
}

/// Parse an LLM response, extracting the <nyaya> block if present.
/// Returns the clean user text and the parsed block.
///
/// SECURITY:
/// - Only parses the LAST <nyaya>...</nyaya> pair to prevent tag injection
///   where the LLM emits a fake </nyaya> early to inject content.
/// - If </nyaya> is missing, the block is DISCARDED (not consumed to EOF)
///   to prevent a missing close tag from swallowing the entire response.
/// - Nested <nyaya> tags inside the block content are stripped.
pub fn parse_response(full_response: &str) -> ParsedResponse {
    // Find the LAST <nyaya> tag to prevent injection via early fake tags
    let Some(start_tag) = full_response.rfind("<nyaya>") else {
        return ParsedResponse {
            user_text: full_response.to_string(),
            nyaya: None,
        };
    };

    // H22: If </nyaya> is missing, discard the block entirely instead of consuming to EOF
    let after_start = start_tag + 7;
    let end_tag = match full_response[after_start..].find("</nyaya>") {
        Some(offset) => after_start + offset,
        None => {
            // Missing close tag — return everything before <nyaya> as user text, discard block
            tracing::warn!("Missing </nyaya> close tag — discarding malformed block");
            return ParsedResponse {
                user_text: full_response[..start_tag].trim().to_string(),
                nyaya: None,
            };
        }
    };

    // User text is everything before the <nyaya> block
    let user_text = full_response[..start_tag].trim().to_string();

    // Extract the block content (between tags)
    let block_content = if end_tag > after_start {
        let raw = full_response[after_start..end_tag].trim();
        // C11: Strip any nested <nyaya> or </nyaya> tags inside the content
        // to prevent tag injection attacks
        raw.replace("<nyaya>", "").replace("</nyaya>", "")
    } else {
        String::new()
    };

    let nyaya = parse_block(&block_content).ok();

    ParsedResponse { user_text, nyaya }
}

/// Parse the content inside <nyaya>...</nyaya> tags.
fn parse_block(content: &str) -> Result<NyayaBlock> {
    let content = content.trim();

    if content.is_empty() {
        return Err(NyayaError::Config("Empty nyaya block".into()));
    }

    let first_line = content.lines().next().unwrap_or("");

    // MODE 1: C:template_name|param1|param2
    if first_line.starts_with("C:") {
        return parse_template_ref(first_line);
    }

    // MODE 2: NEW:chain_name
    if first_line.starts_with("NEW:") {
        return parse_new_chain(content);
    }

    // MODE 3: PATCH:template_name|params
    if first_line.starts_with("PATCH:") {
        return parse_patch(content);
    }

    // MODE 4: CACHE:ttl
    if first_line.starts_with("CACHE:") {
        return parse_cache(content);
    }

    // MODE 5: NOCACHE
    if first_line.starts_with("NOCACHE") {
        return parse_nocache(content);
    }

    // MODE 6: PROPOSE_FUNC:name
    if first_line.starts_with("PROPOSE_FUNC:") {
        return parse_propose_func(content);
    }

    Err(NyayaError::Config(format!(
        "Unknown nyaya block mode: {}",
        first_line
    )))
}

/// MODE 1: C:template_name|param1|param2|...
fn parse_template_ref(line: &str) -> Result<NyayaBlock> {
    let rest = line.strip_prefix("C:").unwrap_or(line);
    let parts: Vec<&str> = rest.split('|').collect();

    if parts.is_empty() {
        return Err(NyayaError::Config("Template ref missing name".into()));
    }

    Ok(NyayaBlock::TemplateRef {
        template_name: parts[0].trim().to_string(),
        params: parts[1..].iter().map(|p| p.trim().to_string()).collect(),
    })
}

/// MODE 2: NEW:chain_name with P:, S:, T:, B:, L:, R: lines
fn parse_new_chain(content: &str) -> Result<NyayaBlock> {
    let mut lines = content.lines();
    let first = lines.next().unwrap_or("");
    let chain_name = first
        .strip_prefix("NEW:")
        .unwrap_or(first)
        .trim()
        .to_string();

    let mut params = Vec::new();
    let mut steps = Vec::new();
    let mut trigger = None;
    let mut circuit_breakers = Vec::new();
    let mut intent_label = None;
    let mut rephrasings = Vec::new();

    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("P:") {
            params = parse_params(rest);
        } else if let Some(rest) = line.strip_prefix("S:") {
            steps.push(parse_step(rest));
        } else if let Some(rest) = line.strip_prefix("T:") {
            trigger = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("B:") {
            circuit_breakers.push(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("L:") {
            intent_label = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("R:") {
            rephrasings = rest
                .split('|')
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty())
                .collect();
        }
    }

    // SECURITY: Limit chain steps to prevent DoS via LLM response
    if steps.len() > 50 {
        return Err(NyayaError::Config(format!(
            "Workflow '{}' exceeds 50 step limit ({} steps)",
            chain_name,
            steps.len()
        )));
    }

    Ok(NyayaBlock::NewChain {
        chain_name,
        params,
        steps,
        trigger,
        circuit_breakers,
        intent_label,
        rephrasings,
    })
}

/// MODE 3: PATCH:template_name|params
fn parse_patch(content: &str) -> Result<NyayaBlock> {
    let mut lines = content.lines();
    let first = lines.next().unwrap_or("");
    let rest = first.strip_prefix("PATCH:").unwrap_or(first);
    let parts: Vec<&str> = rest.split('|').collect();

    let base_template = parts
        .first()
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let base_params: Vec<String> = parts[1..].iter().map(|p| p.trim().to_string()).collect();

    let mut add_params = Vec::new();
    let mut add_steps = Vec::new();
    let mut remove_steps = Vec::new();
    let mut intent_label = None;
    let mut rephrasings = Vec::new();

    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("ADD_PARAM:") {
            add_params.extend(parse_params(rest));
        } else if let Some(rest) = line.strip_prefix("ADD_STEP:") {
            if let Some(ps) = parse_patch_step(rest) {
                add_steps.push(ps);
            }
        } else if let Some(rest) = line.strip_prefix("REMOVE_STEP:") {
            remove_steps.push(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("L:") {
            intent_label = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("R:") {
            rephrasings = rest
                .split('|')
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty())
                .collect();
        }
    }

    Ok(NyayaBlock::Patch {
        base_template,
        base_params,
        add_params,
        add_steps,
        remove_steps,
        intent_label,
        rephrasings,
    })
}

/// MODE 4: CACHE:ttl
fn parse_cache(content: &str) -> Result<NyayaBlock> {
    let mut lines = content.lines();
    let first = lines.next().unwrap_or("");
    let ttl = first
        .strip_prefix("CACHE:")
        .unwrap_or(first)
        .trim()
        .to_string();

    let mut intent_label = None;
    let mut rephrasings = Vec::new();

    for line in lines {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("L:") {
            intent_label = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("R:") {
            rephrasings = rest
                .split('|')
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty())
                .collect();
        }
    }

    Ok(NyayaBlock::Cache {
        ttl,
        intent_label,
        rephrasings,
    })
}

/// MODE 5: NOCACHE
fn parse_nocache(content: &str) -> Result<NyayaBlock> {
    let mut intent_label = None;
    let mut rephrasings = Vec::new();

    for line in content.lines().skip(1) {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("L:") {
            intent_label = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("R:") {
            rephrasings = rest
                .split('|')
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty())
                .collect();
        }
    }

    Ok(NyayaBlock::NoCache {
        intent_label,
        rephrasings,
    })
}

/// Parse P: param specifications.
/// Format: name:type:default|name:type:default
fn parse_params(s: &str) -> Vec<ParamSpec> {
    s.split('|')
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            let parts: Vec<&str> = p.trim().splitn(3, ':').collect();
            ParamSpec {
                name: parts.first().map(|s| s.to_string()).unwrap_or_default(),
                param_type: parts
                    .get(1)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "str".into()),
                default_value: parts.get(2).map(|s| s.to_string()),
            }
        })
        .collect()
}

/// Parse S: step specification.
/// Format: ability.name:params>output_var or ability.name:params>output_var!confirm
fn parse_step(s: &str) -> StepSpec {
    let s = s.trim();
    let confirm = s.contains("!confirm");
    let s = s.replace("!confirm", "");

    let (main, output_var) = if let Some(idx) = s.find('>') {
        (s[..idx].to_string(), Some(s[idx + 1..].trim().to_string()))
    } else {
        (s.to_string(), None)
    };

    // Split ability:params (first colon separates ability from params)
    let (ability, params) = if let Some(idx) = main.find(':') {
        (
            main[..idx].trim().to_string(),
            main[idx + 1..].trim().to_string(),
        )
    } else {
        (main.trim().to_string(), String::new())
    };

    StepSpec {
        ability,
        params,
        output_var,
        confirm,
    }
}

/// Parse ADD_STEP: for PATCH mode.
/// Format: after:step_id|new_id|ability|params
fn parse_patch_step(s: &str) -> Option<PatchStep> {
    let parts: Vec<&str> = s.split('|').collect();
    if parts.len() < 3 {
        return None;
    }

    Some(PatchStep {
        position: parts[0].trim().to_string(),
        step_id: parts[1].trim().to_string(),
        ability: parts[2].trim().to_string(),
        params: parts
            .get(3)
            .map(|s| s.trim().to_string())
            .unwrap_or_default(),
    })
}

/// MODE 6: PROPOSE_FUNC:name
/// Lines: D:description, CAT:category, SEC:security_tier,
///        P:name:type:required:default:description
///        RET:type:description, RF:name:type:description
///        EX:input->output
fn parse_propose_func(content: &str) -> Result<NyayaBlock> {
    let mut lines = content.lines();
    let first = lines.next().unwrap_or("");
    let func_name = first
        .strip_prefix("PROPOSE_FUNC:")
        .unwrap_or("")
        .trim()
        .to_string();

    if func_name.is_empty() {
        return Err(NyayaError::Config(
            "PROPOSE_FUNC missing function name".into(),
        ));
    }

    if !validate_identifier(&func_name) {
        return Err(NyayaError::Config(format!(
            "PROPOSE_FUNC invalid function name: '{}'",
            func_name
        )));
    }

    let mut description = String::new();
    let mut category = String::new();
    let mut security_tier = String::new();
    let mut params = Vec::new();
    let mut returns: Option<ProposedReturn> = None;
    let mut return_fields = Vec::new();
    let mut examples = Vec::new();

    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("D:") {
            description = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("CAT:") {
            category = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("SEC:") {
            security_tier = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("P:") {
            // P:name:type:required:default:description
            let parts: Vec<&str> = rest.splitn(5, ':').collect();
            if parts.len() >= 2 {
                params.push(ProposedParam {
                    name: parts[0].trim().to_string(),
                    schema_type: parts[1].trim().to_string(),
                    required: parts.get(2).is_some_and(|s| s.trim() == "true"),
                    default: parts.get(3).and_then(|s| {
                        let s = s.trim();
                        if s.is_empty() {
                            None
                        } else {
                            Some(s.to_string())
                        }
                    }),
                    description: parts
                        .get(4)
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default(),
                });
            }
        } else if let Some(rest) = line.strip_prefix("RET:") {
            // RET:type:description
            let parts: Vec<&str> = rest.splitn(2, ':').collect();
            returns = Some(ProposedReturn {
                schema_type: parts[0].trim().to_string(),
                description: parts
                    .get(1)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default(),
            });
        } else if let Some(rest) = line.strip_prefix("RF:") {
            // RF:name:type:description
            let parts: Vec<&str> = rest.splitn(3, ':').collect();
            if parts.len() >= 2 {
                return_fields.push(ProposedReturnField {
                    name: parts[0].trim().to_string(),
                    schema_type: parts[1].trim().to_string(),
                    description: parts
                        .get(2)
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default(),
                });
            }
        } else if let Some(rest) = line.strip_prefix("EX:") {
            // EX:input_json->output_json
            if let Some(arrow) = rest.find("->") {
                examples.push(ProposedExample {
                    input: rest[..arrow].trim().to_string(),
                    output: rest[arrow + 2..].trim().to_string(),
                });
            }
        }
    }

    Ok(NyayaBlock::ProposeFunc {
        func_name,
        description,
        category,
        security_tier,
        params,
        returns,
        return_fields,
        examples,
    })
}

/// Extract intent label and rephrasings from any NyayaBlock variant.
impl NyayaBlock {
    pub fn intent_label(&self) -> Option<&str> {
        match self {
            NyayaBlock::TemplateRef { .. } => None,
            NyayaBlock::NewChain { intent_label, .. } => intent_label.as_deref(),
            NyayaBlock::Patch { intent_label, .. } => intent_label.as_deref(),
            NyayaBlock::Cache { intent_label, .. } => intent_label.as_deref(),
            NyayaBlock::NoCache { intent_label, .. } => intent_label.as_deref(),
            NyayaBlock::ProposeFunc { .. } => None,
        }
    }

    pub fn rephrasings(&self) -> &[String] {
        match self {
            NyayaBlock::TemplateRef { .. } => &[],
            NyayaBlock::NewChain { rephrasings, .. } => rephrasings,
            NyayaBlock::Patch { rephrasings, .. } => rephrasings,
            NyayaBlock::Cache { rephrasings, .. } => rephrasings,
            NyayaBlock::NoCache { rephrasings, .. } => rephrasings,
            NyayaBlock::ProposeFunc { .. } => &[],
        }
    }

    pub fn mode_name(&self) -> &'static str {
        match self {
            NyayaBlock::TemplateRef { .. } => "C",
            NyayaBlock::NewChain { .. } => "NEW",
            NyayaBlock::Patch { .. } => "PATCH",
            NyayaBlock::Cache { .. } => "CACHE",
            NyayaBlock::NoCache { .. } => "NOCACHE",
            NyayaBlock::ProposeFunc { .. } => "PROPOSE_FUNC",
        }
    }
}

/// Sanitize a string for safe YAML interpolation.
/// Strips characters that could break YAML structure or inject new keys.
fn sanitize_yaml_value(s: &str) -> String {
    s.chars()
        .filter(|c| {
            !matches!(
                c,
                '\n' | '\r'
                    | ':'
                    | '{'
                    | '}'
                    | '['
                    | ']'
                    | '#'
                    | '&'
                    | '*'
                    | '!'
                    | '|'
                    | '>'
                    | '\''
                    | '"'
                    | '%'
                    | '@'
                    | '`'
            )
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Validate a chain/step/param name: only alphanumeric, underscore, hyphen, dot allowed.
fn validate_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

/// Convert a MODE 2 (NewChain) NyayaBlock into a ChainDef YAML string
/// for storage in the chain store.
impl NyayaBlock {
    pub fn to_chain_yaml(&self) -> Option<String> {
        match self {
            NyayaBlock::NewChain {
                chain_name,
                params,
                steps,
                ..
            } => {
                // Validate chain name to prevent injection
                if !validate_identifier(chain_name) {
                    tracing::warn!(chain_name = %chain_name, "Invalid workflow name rejected");
                    return None;
                }

                let mut yaml = format!("id: {chain_name}\nname: {chain_name}\ndescription: Auto-compiled workflow\nparams:\n");
                for p in params {
                    if !validate_identifier(&p.name) {
                        tracing::warn!(param = %p.name, "Invalid param name skipped");
                        continue;
                    }
                    let safe_desc = sanitize_yaml_value(&p.param_type);
                    yaml.push_str(&format!(
                        "  - name: {}\n    param_type: text\n    description: \"{}\"\n    required: true\n",
                        p.name, safe_desc,
                    ));
                    if let Some(ref def) = p.default_value {
                        let safe_def = sanitize_yaml_value(def);
                        yaml.push_str(&format!("    default: \"{}\"\n", safe_def));
                    }
                }
                yaml.push_str("steps:\n");
                for (i, s) in steps.iter().enumerate() {
                    // Validate ability name
                    if !validate_identifier(&s.ability) {
                        tracing::warn!(ability = %s.ability, "Invalid ability name skipped");
                        continue;
                    }
                    let default_id = format!("step{}", i + 1);
                    let step_id = s.output_var.as_deref().unwrap_or(&default_id);
                    if !validate_identifier(step_id) {
                        tracing::warn!(step_id = %step_id, "Invalid step_id skipped");
                        continue;
                    }
                    let safe_params = sanitize_yaml_value(&s.params);
                    yaml.push_str(&format!(
                        "  - id: {}\n    ability: {}\n    args:\n      input: \"{}\"\n",
                        step_id, s.ability, safe_params,
                    ));
                    if let Some(ref out) = s.output_var {
                        if validate_identifier(out) {
                            yaml.push_str(&format!("    output_key: {}\n", out));
                        }
                    }
                }
                Some(yaml)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mode1_template_ref() {
        let response = "Setting up NVDA sentiment monitor...\n<nyaya>C:sentiment_trade|@cathiewood|x.com|NVDA|long|short|5m</nyaya>";
        let parsed = parse_response(response);

        assert_eq!(parsed.user_text, "Setting up NVDA sentiment monitor...");
        let nyaya = parsed.nyaya.unwrap();
        match nyaya {
            NyayaBlock::TemplateRef {
                template_name,
                params,
            } => {
                assert_eq!(template_name, "sentiment_trade");
                assert_eq!(params.len(), 6);
                assert_eq!(params[0], "@cathiewood");
                assert_eq!(params[2], "NVDA");
            }
            _ => panic!("Expected TemplateRef"),
        }
    }

    #[test]
    fn test_parse_mode2_new_chain() {
        let response = r#"I'll set up a daily briefing for you...
<nyaya>
NEW:morning_briefing
P:time:cron:0 8 * * *|channel:str:telegram
S:email.unread_count>count
S:calendar.today>events
S:nlp.summarize:$count emails, $events events>summary
S:notify.user:$summary|$channel
L:daily_briefing
R:morning briefing|daily summary|morning update
</nyaya>"#;

        let parsed = parse_response(response);
        assert_eq!(parsed.user_text, "I'll set up a daily briefing for you...");

        let nyaya = parsed.nyaya.unwrap();
        match nyaya {
            NyayaBlock::NewChain {
                chain_name,
                params,
                steps,
                intent_label,
                rephrasings,
                ..
            } => {
                assert_eq!(chain_name, "morning_briefing");
                assert_eq!(params.len(), 2);
                assert_eq!(params[0].name, "time");
                assert_eq!(params[0].param_type, "cron");
                assert_eq!(params[0].default_value.as_deref(), Some("0 8 * * *"));
                assert_eq!(steps.len(), 4);
                assert_eq!(steps[0].ability, "email.unread_count");
                assert_eq!(steps[0].output_var.as_deref(), Some("count"));
                assert_eq!(intent_label.as_deref(), Some("daily_briefing"));
                assert_eq!(rephrasings.len(), 3);
            }
            _ => panic!("Expected NewChain"),
        }
    }

    #[test]
    fn test_parse_mode3_patch() {
        let response = r#"Patching the sentiment monitor...
<nyaya>
PATCH:sentiment_trade|@elonmusk|x.com|TSLA|long|short|5m
ADD_PARAM:min_frequency:int:3
ADD_STEP:after:filter|freq_check|flow.branch|$filter.length<$min_frequency
R:track {handle} for {ticker}|monitor {handle} {ticker}
</nyaya>"#;

        let parsed = parse_response(response);
        let nyaya = parsed.nyaya.unwrap();
        match nyaya {
            NyayaBlock::Patch {
                base_template,
                base_params,
                add_params,
                add_steps,
                rephrasings,
                ..
            } => {
                assert_eq!(base_template, "sentiment_trade");
                assert_eq!(base_params.len(), 6);
                assert_eq!(add_params.len(), 1);
                assert_eq!(add_params[0].name, "min_frequency");
                assert_eq!(add_steps.len(), 1);
                assert_eq!(rephrasings.len(), 2);
            }
            _ => panic!("Expected Patch"),
        }
    }

    #[test]
    fn test_parse_mode4_cache() {
        let response = r#"Currently 28°C and sunny in Delhi...
<nyaya>
CACHE:1h
L:weather_query
R:weather in {city}|how's the weather {city}|{city} forecast|temperature in {city}
</nyaya>"#;

        let parsed = parse_response(response);
        assert_eq!(parsed.user_text, "Currently 28°C and sunny in Delhi...");

        let nyaya = parsed.nyaya.unwrap();
        match nyaya {
            NyayaBlock::Cache {
                ttl,
                intent_label,
                rephrasings,
            } => {
                assert_eq!(ttl, "1h");
                assert_eq!(intent_label.as_deref(), Some("weather_query"));
                assert_eq!(rephrasings.len(), 4);
            }
            _ => panic!("Expected Cache"),
        }
    }

    #[test]
    fn test_parse_mode5_nocache() {
        let response = r#"Paris.
<nyaya>
NOCACHE
L:factual_geography
R:capital of {country}|what is the capital of {country}
</nyaya>"#;

        let parsed = parse_response(response);
        assert_eq!(parsed.user_text, "Paris.");

        let nyaya = parsed.nyaya.unwrap();
        match nyaya {
            NyayaBlock::NoCache {
                intent_label,
                rephrasings,
            } => {
                assert_eq!(intent_label.as_deref(), Some("factual_geography"));
                assert_eq!(rephrasings.len(), 2);
            }
            _ => panic!("Expected NoCache"),
        }
    }

    #[test]
    fn test_no_nyaya_block() {
        let response = "Just a normal response with no metadata.";
        let parsed = parse_response(response);
        assert_eq!(parsed.user_text, "Just a normal response with no metadata.");
        assert!(parsed.nyaya.is_none());
    }

    #[test]
    fn test_mode_name() {
        let block = NyayaBlock::TemplateRef {
            template_name: "test".into(),
            params: vec![],
        };
        assert_eq!(block.mode_name(), "C");

        let block = NyayaBlock::Cache {
            ttl: "1h".into(),
            intent_label: None,
            rephrasings: vec![],
        };
        assert_eq!(block.mode_name(), "CACHE");
    }

    #[test]
    fn test_parse_mode6_propose_func() {
        let response = r#"I'll propose a calendar function for you.
<nyaya>
PROPOSE_FUNC:calendar.list_events
D:List upcoming calendar events for a date range
CAT:calendar
SEC:external_read
P:start_date:string:true::Start date ISO 8601
P:end_date:string:false::End date
P:max_results:integer:false:10:Max events
RET:object:List of calendar events
RF:events:array:Event objects
RF:count:integer:Number returned
EX:{"start_date":"2026-02-22"}->{"events":[],"count":0}
</nyaya>"#;

        let parsed = parse_response(response);
        assert_eq!(
            parsed.user_text,
            "I'll propose a calendar function for you."
        );

        let nyaya = parsed.nyaya.unwrap();
        match nyaya {
            NyayaBlock::ProposeFunc {
                func_name,
                description,
                category,
                security_tier,
                params,
                returns,
                return_fields,
                examples,
            } => {
                assert_eq!(func_name, "calendar.list_events");
                assert_eq!(
                    description,
                    "List upcoming calendar events for a date range"
                );
                assert_eq!(category, "calendar");
                assert_eq!(security_tier, "external_read");
                assert_eq!(params.len(), 3);
                assert_eq!(params[0].name, "start_date");
                assert!(params[0].required);
                assert_eq!(params[1].name, "end_date");
                assert!(!params[1].required);
                assert_eq!(params[2].default.as_deref(), Some("10"));
                let ret = returns.unwrap();
                assert_eq!(ret.schema_type, "object");
                assert_eq!(return_fields.len(), 2);
                assert_eq!(return_fields[0].name, "events");
                assert_eq!(examples.len(), 1);
                assert!(examples[0].input.contains("start_date"));
            }
            _ => panic!("Expected ProposeFunc"),
        }
    }

    #[test]
    fn test_parse_mode6_minimal() {
        let response = r#"Here's a simple function.
<nyaya>
PROPOSE_FUNC:misc.hello
D:Say hello
CAT:misc
SEC:read_only
</nyaya>"#;

        let parsed = parse_response(response);
        let nyaya = parsed.nyaya.unwrap();
        match nyaya {
            NyayaBlock::ProposeFunc {
                func_name,
                params,
                returns,
                ..
            } => {
                assert_eq!(func_name, "misc.hello");
                assert!(params.is_empty());
                assert!(returns.is_none());
            }
            _ => panic!("Expected ProposeFunc"),
        }
    }

    #[test]
    fn test_mode6_mode_name() {
        let block = NyayaBlock::ProposeFunc {
            func_name: "test.func".into(),
            description: "test".into(),
            category: "test".into(),
            security_tier: "read_only".into(),
            params: vec![],
            returns: None,
            return_fields: vec![],
            examples: vec![],
        };
        assert_eq!(block.mode_name(), "PROPOSE_FUNC");
        assert!(block.intent_label().is_none());
        assert!(block.rephrasings().is_empty());
    }

    #[test]
    fn test_new_chain_to_yaml() {
        let block = NyayaBlock::NewChain {
            chain_name: "test_chain".into(),
            params: vec![ParamSpec {
                name: "city".into(),
                param_type: "str".into(),
                default_value: None,
            }],
            steps: vec![StepSpec {
                ability: "data.fetch_url".into(),
                params: "https://api.weather.com/$city".into(),
                output_var: Some("weather".into()),
                confirm: false,
            }],
            trigger: None,
            circuit_breakers: vec![],
            intent_label: Some("weather".into()),
            rephrasings: vec![],
        };

        let yaml = block.to_chain_yaml().unwrap();
        assert!(yaml.contains("id: test_chain"));
        assert!(yaml.contains("ability: data.fetch_url"));
        assert!(yaml.contains("output_key: weather"));
    }
}
