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

    // Use rfind to get the LAST '>' — avoids matching '>' inside quoted code values
    let (main, output_var) = if let Some(idx) = s.rfind('>') {
        let candidate = s[idx + 1..].trim();
        // Only treat as output_var if it looks like an identifier (no spaces, no special chars)
        if !candidate.is_empty() && validate_identifier(candidate) {
            (s[..idx].to_string(), Some(candidate.to_string()))
        } else {
            (s.to_string(), None)
        }
    } else {
        (s.to_string(), None)
    };

    // Split ability from params.
    // Format 1: ability.name:params (colon separator)
    // Format 2: ability.name params (space separator — LLMs often use this)
    let (ability, params) = if let Some(idx) = main.find(':') {
        let candidate = main[..idx].trim();
        // Validate: ability names are like "script.run", "llm.chat" — no spaces
        if !candidate.contains(' ') && candidate.contains('.') {
            (candidate.to_string(), main[idx + 1..].trim().to_string())
        } else if let Some(sp) = main.find(' ') {
            let first_word = main[..sp].trim();
            if first_word.contains('.') {
                (first_word.to_string(), main[sp + 1..].trim().to_string())
            } else {
                (main[..idx].trim().to_string(), main[idx + 1..].trim().to_string())
            }
        } else {
            (main[..idx].trim().to_string(), main[idx + 1..].trim().to_string())
        }
    } else if let Some(sp) = main.find(' ') {
        // No colon — try space separator (e.g. "script.run lang=python code=...")
        let first_word = main[..sp].trim();
        if first_word.contains('.') {
            (first_word.to_string(), main[sp + 1..].trim().to_string())
        } else {
            (main.trim().to_string(), String::new())
        }
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
    // Properly escape for YAML double-quoted strings.
    // Preserve {{template}} markers for variable substitution.
    let mut result = String::with_capacity(s.len() + 16);
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // Preserve {{ and }} template markers
        if c == '{' && i + 1 < chars.len() && chars[i + 1] == '{' {
            result.push_str("{{");
            i += 2;
            continue;
        }
        if c == '}' && i + 1 < chars.len() && chars[i + 1] == '}' {
            result.push_str("}}");
            i += 2;
            continue;
        }
        match c {
            '\\' => result.push_str("\\\\"),
            '"' => result.push_str("\\\""),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            _ => result.push(c),
        }
        i += 1;
    }
    result.trim().to_string()
}

/// Sanitize a single line for YAML block scalar (`|`) content.
/// Block scalars are literal text in YAML — no special character escaping
/// needed. We only trim trailing whitespace to keep the YAML clean.
/// `{`, `}`, backtick are preserved because they're essential in code
/// (Python dicts, f-strings, JSON literals, shell commands).
fn sanitize_yaml_block_line(s: &str) -> String {
    s.trim_end().to_string()
}

/// Convert $variable references to {{variable}} template syntax.
/// First converts known param names, then auto-detects any remaining
/// $word patterns (alphanumeric + underscore).
fn convert_dollar_refs(s: &str, param_names: &[&str]) -> String {
    let mut result = s.to_string();
    // Convert known params first
    for name in param_names {
        let dollar_ref = format!("${}", name);
        let template_ref = format!("{{{{{}}}}}", name);
        result = result.replace(&dollar_ref, &template_ref);
    }
    // Auto-detect remaining $word references not yet converted
    // Match $identifier patterns (alphanumeric + underscore, not already in {{}})
    let mut final_result = String::with_capacity(result.len());
    let chars: Vec<char> = result.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' && i + 1 < chars.len() && (chars[i + 1].is_ascii_alphabetic() || chars[i + 1] == '_') {
            // Collect the identifier
            let start = i + 1;
            let mut end = start;
            while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
                end += 1;
            }
            let var_name: String = chars[start..end].iter().collect();
            final_result.push_str(&format!("{{{{{}}}}}", var_name));
            i = end;
        } else {
            final_result.push(chars[i]);
            i += 1;
        }
    }
    final_result
}

/// Validate a chain/step/param name: only alphanumeric, underscore, hyphen, dot allowed.
fn validate_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

/// When a quoted value runs off the end of input (no closing `"`), scan
/// backwards for the last `"` that is followed by ` key=` — that was likely
/// the real closing delimiter, with a stray `\` before it interpreted as escape.
/// Returns the byte offset of the `"` to split at, or None.
fn find_runaway_quote_split(value: &str) -> Option<usize> {
    // Search backwards for `" ` followed by an identifier and `=`
    let bytes = value.as_bytes();
    let mut i = value.len().saturating_sub(1);
    while i > 0 {
        if bytes[i] == b'"' && i + 1 < bytes.len() && bytes[i + 1] == b' ' {
            // Check if what follows looks like `key=`
            let rest = &value[i + 2..];
            if let Some(eq) = rest.find('=') {
                let candidate = &rest[..eq];
                if !candidate.is_empty()
                    && candidate
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    return Some(i);
                }
            }
        }
        i -= 1;
    }
    None
}

fn parse_step_args(params: &str) -> Vec<(String, String)> {
    // Quick check: if params contains no '=' at all, it's not key=value format
    if !params.contains('=') {
        return Vec::new();
    }
    let mut args = Vec::new();
    let mut chars = params.chars().peekable();
    while chars.peek().is_some() {
        // Skip whitespace
        while chars.peek() == Some(&' ') {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }
        // Read key (up to '=') — key must be alphanumeric/underscore only
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c == '=' {
                chars.next(); // consume '='
                break;
            }
            if c == ' ' {
                // Hit space before '=' — not a valid key=value token, skip
                break;
            }
            key.push(c);
            chars.next();
        }
        if key.is_empty() {
            // Skip non-key chars
            chars.next();
            continue;
        }
        // Read value (possibly quoted)
        let value = if chars.peek() == Some(&'"') {
            chars.next(); // consume opening quote
            let mut v = String::new();
            let mut escaped = false;
            let mut found_closing_quote = false;
            for c in chars.by_ref() {
                if escaped {
                    // Interpret standard escape sequences so real newlines/tabs
                    // reach sanitize_yaml_value and the rest of the pipeline.
                    match c {
                        'n' => v.push('\n'),
                        't' => v.push('\t'),
                        'r' => v.push('\r'),
                        '\\' => v.push('\\'),
                        '"' => v.push('"'),
                        other => { v.push('\\'); v.push(other); }
                    }
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    found_closing_quote = true;
                    break;
                } else {
                    v.push(c);
                }
            }
            // Backtrack heuristic: if we exhausted input without finding a
            // closing quote, the LLM likely wrote \" before the real closing
            // delimiter (e.g. code="...print('hi')\" input=).  Scan backwards
            // for `" key=` pattern and split there.
            let mut tail_args = Vec::new();
            if !found_closing_quote {
                if let Some(split) = find_runaway_quote_split(&v) {
                    let tail = v[split + 1..].to_string();
                    v.truncate(split);
                    tail_args = parse_step_args(&tail);
                }
            }
            // Push main arg first, then any recovered tail args
            args.push((key.trim().to_string(), v));
            args.extend(tail_args);
            continue; // skip the push at end of loop
        } else {
            chars.by_ref().take_while(|&c| c != ' ').collect()
        };
        args.push((key.trim().to_string(), value));
    }
    args
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
                // Collect param names for $var → {{var}} conversion
                let param_names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
                // Also include step output vars as known references
                let step_output_names: Vec<String> = steps
                    .iter()
                    .filter_map(|s| s.output_var.clone())
                    .collect();
                let all_ref_names: Vec<&str> = param_names
                    .iter()
                    .copied()
                    .chain(step_output_names.iter().map(|s| s.as_str()))
                    .collect();

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
                    // Convert $param → {{param}} before parsing args
                    let converted_params = convert_dollar_refs(&s.params, &all_ref_names);
                    let parsed_args = parse_step_args(&converted_params);
                    yaml.push_str(&format!(
                        "  - id: {}\n    ability: {}\n    args:\n",
                        step_id, s.ability,
                    ));
                    if parsed_args.is_empty() {
                        // No key=value pairs — treat entire params as input
                        if converted_params.contains('\n') {
                            yaml.push_str("      input: |\n");
                            for line in converted_params.lines() {
                                let safe_line = sanitize_yaml_block_line(line);
                                yaml.push_str(&format!("        {}\n", safe_line));
                            }
                        } else {
                            let safe_params = sanitize_yaml_value(&converted_params);
                            yaml.push_str(&format!("      input: \"{}\"\n", safe_params));
                        }
                    } else {
                        for (key, val) in &parsed_args {
                            if val.contains('\n') {
                                yaml.push_str(&format!("      {}: |\n", key));
                                for line in val.lines() {
                                    let safe_line = sanitize_yaml_block_line(line);
                                    yaml.push_str(&format!("        {}\n", safe_line));
                                }
                            } else {
                                let safe_val = sanitize_yaml_value(val);
                                yaml.push_str(&format!("      {}: \"{}\"\n", key, safe_val));
                            }
                        }
                    }
                    if let Some(ref out) = s.output_var {
                        if validate_identifier(out) {
                            yaml.push_str(&format!("    output_key: {}\n", out));
                        }
                    }
                }

                // Convert $param_name references to {{param_name}} for template resolution.
                // Also convert $output_var references from previous steps.
                for p in params {
                    yaml = yaml.replace(
                        &format!("${}", p.name),
                        &format!("{{{{{}}}}}", p.name),
                    );
                }
                // Convert $step_output references (step ids used as output_key)
                for s in steps {
                    if let Some(ref out) = s.output_var {
                        yaml = yaml.replace(
                            &format!("${}", out),
                            &format!("{{{{{}}}}}", out),
                        );
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

    #[test]
    fn test_parse_step_args_interprets_escapes() {
        // \n should become a real newline, \" a real quote
        let args = parse_step_args(r#"code="def foo():\n    return 42" name="test\"file""#);
        assert_eq!(args.len(), 2);
        let (key, val) = &args[0];
        assert_eq!(key, "code");
        assert!(val.contains('\n'), "Expected real newline in value, got: {:?}", val);
        assert_eq!(val, "def foo():\n    return 42");
        let (key2, val2) = &args[1];
        assert_eq!(key2, "name");
        assert_eq!(val2, "test\"file");
    }

    #[test]
    fn test_parse_step_args_tab_and_backslash() {
        let args = parse_step_args(r#"data="col1\tcol2\\end""#);
        assert_eq!(args.len(), 1);
        assert_eq!(args[0].1, "col1\tcol2\\end");
    }

    #[test]
    fn test_parse_step_args_unknown_escape_preserved() {
        // Unknown escape sequences like \x should be preserved literally
        let args = parse_step_args(r#"val="hello\xworld""#);
        assert_eq!(args[0].1, "hello\\xworld");
    }

    #[test]
    fn test_yaml_block_scalar_for_multiline() {
        // Build a NewChain with multi-line code in step params
        let block = NyayaBlock::NewChain {
            chain_name: "code_writer".into(),
            params: vec![],
            steps: vec![StepSpec {
                ability: "file.write".into(),
                params: "path=\"/tmp/test.py\" code=\"def hello():\\n    print(\\\"hi\\\")\\n    return True\"".into(),
                output_var: Some("result".into()),
                confirm: false,
            }],
            trigger: None,
            circuit_breakers: vec![],
            intent_label: Some("write_code".into()),
            rephrasings: vec![],
        };

        let yaml = block.to_chain_yaml().unwrap();
        // The code arg should use block scalar (|) since it contains newlines
        assert!(yaml.contains("code: |"), "Expected block scalar for multiline code, got:\n{}", yaml);
        // Should contain the actual code lines indented
        assert!(yaml.contains("def hello():"), "Expected function def in yaml:\n{}", yaml);
        assert!(yaml.contains("print(\"hi\")"), "Expected print statement in yaml:\n{}", yaml);
    }

    #[test]
    fn test_sanitize_yaml_block_line_preserves_code() {
        assert_eq!(sanitize_yaml_block_line("hello {{name}} world"), "hello {{name}} world");
        // Lone braces preserved (essential for Python dicts, JSON, f-strings)
        assert_eq!(sanitize_yaml_block_line("data = {\"key\": \"value\"}"), "data = {\"key\": \"value\"}");
        assert_eq!(sanitize_yaml_block_line("f\"hello {name}\""), "f\"hello {name}\"");
        // Backticks preserved (shell commands, markdown)
        assert_eq!(sanitize_yaml_block_line("run `ls`"), "run `ls`");
        // Trailing whitespace trimmed
        assert_eq!(sanitize_yaml_block_line("hello   "), "hello");
    }

    #[test]
    fn test_single_line_stays_quoted() {
        // Single-line values should still use double-quoted YAML strings
        let block = NyayaBlock::NewChain {
            chain_name: "simple".into(),
            params: vec![],
            steps: vec![StepSpec {
                ability: "file.write".into(),
                params: "path=\"/tmp/test.txt\" content=\"hello world\"".into(),
                output_var: None,
                confirm: false,
            }],
            trigger: None,
            circuit_breakers: vec![],
            intent_label: None,
            rephrasings: vec![],
        };

        let yaml = block.to_chain_yaml().unwrap();
        assert!(yaml.contains("content: \"hello world\""), "Single-line should be quoted:\n{}", yaml);
        assert!(yaml.contains("path: \"/tmp/test.txt\""), "Path should be quoted:\n{}", yaml);
    }

    #[test]
    fn test_backtrack_runaway_quote() {
        // Simulates LLM writing: code="print('hello')\" input=
        // The \" before input= should be treated as closing quote via backtracking
        let args = parse_step_args(r#"code="print('hello')\" input="#);
        assert_eq!(args.len(), 2, "Should find 2 args, got: {:?}", args);
        assert_eq!(args[0].0, "code");
        // Value is truncated at the " that precedes " input="
        assert_eq!(args[0].1, "print('hello')");
        assert_eq!(args[1].0, "input");
    }

    #[test]
    fn test_backtrack_does_not_trigger_on_normal_quotes() {
        // Normal case: closing quote found properly
        let args = parse_step_args(r#"code="hello world" input="test""#);
        assert_eq!(args.len(), 2);
        assert_eq!(args[0].1, "hello world");
        assert_eq!(args[1].1, "test");
    }
}
