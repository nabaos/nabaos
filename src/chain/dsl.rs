use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::core::error::{NyayaError, Result};

/// A compiled chain definition — a parameterized sequence of ability calls.
/// This is what the LLM "compiles" from natural language.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainDef {
    /// Unique chain identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description of what this chain does
    pub description: String,
    /// Parameter schema: name → type description
    pub params: Vec<ParamDef>,
    /// Ordered list of steps to execute
    pub steps: Vec<ChainStep>,
}

/// Parameter definition for a chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamDef {
    pub name: String,
    pub param_type: ParamType,
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamType {
    Text,
    Number,
    Boolean,
    Url,
    Email,
    DateTime,
}

/// A single step in a chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainStep {
    /// Step identifier (for branching references)
    pub id: String,
    /// The ability to invoke
    pub ability: String,
    /// Arguments — values can contain {{param_name}} template references
    pub args: HashMap<String, String>,
    /// Optional: store the result under this key for later steps
    #[serde(default)]
    pub output_key: Option<String>,
    /// Optional: conditional — only execute if this condition is true
    #[serde(default)]
    pub condition: Option<StepCondition>,
    /// Optional: jump to this step ID on failure instead of aborting
    #[serde(default)]
    pub on_failure: Option<String>,
}

/// Condition for conditional step execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepCondition {
    /// Reference to a previous step's output key
    pub ref_key: String,
    /// Operator
    pub op: ConditionOp,
    /// Value to compare against
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOp {
    Equals,
    NotEquals,
    Contains,
    GreaterThan,
    LessThan,
    IsEmpty,
    IsNotEmpty,
}

impl ChainDef {
    /// Parse a chain definition from YAML.
    pub fn from_yaml(yaml: &str) -> Result<Self> {
        serde_yaml::from_str(yaml)
            .map_err(|e| NyayaError::Config(format!("Workflow YAML parse error: {}", e)))
    }

    /// Serialize to YAML.
    pub fn to_yaml(&self) -> Result<String> {
        serde_yaml::to_string(self)
            .map_err(|e| NyayaError::Config(format!("Workflow YAML serialize error: {}", e)))
    }

    /// Check the chain definition for errors.
    pub fn check(&self) -> Result<()> {
        if self.id.is_empty() {
            return Err(NyayaError::Config("Workflow ID cannot be empty".into()));
        }
        if self.steps.is_empty() {
            return Err(NyayaError::Config(
                "Workflow must have at least one step".into(),
            ));
        }

        let step_ids: Vec<&str> = self.steps.iter().map(|s| s.id.as_str()).collect();

        // Check step IDs are unique
        let mut seen = std::collections::HashSet::new();
        for sid in &step_ids {
            if !seen.insert(sid) {
                return Err(NyayaError::Config(format!("Duplicate step ID: {}", sid)));
            }
        }

        // Check on_failure references exist
        for step in &self.steps {
            if let Some(ref target) = step.on_failure {
                if !step_ids.contains(&target.as_str()) {
                    return Err(NyayaError::Config(format!(
                        "Step '{}' references unknown failure target '{}'",
                        step.id, target
                    )));
                }
            }
        }

        Ok(())
    }

    /// Resolve template parameters in a step's args.
    /// Parameters are sanitized to prevent injection of template markers
    /// or control characters that could alter chain execution.
    pub fn resolve_args(
        args: &HashMap<String, String>,
        params: &HashMap<String, String>,
        step_outputs: &HashMap<String, String>,
    ) -> HashMap<String, String> {
        args.iter()
            .map(|(k, v)| {
                let mut resolved = v.clone();
                for (pname, pval) in params {
                    let sanitized = sanitize_template_value(pval);
                    resolved = resolved.replace(&format!("{{{{{}}}}}", pname), &sanitized);
                }
                for (okey, oval) in step_outputs {
                    let sanitized = sanitize_template_value(oval);
                    resolved = resolved.replace(&format!("{{{{{}}}}}", okey), &sanitized);
                }
                (k.clone(), resolved)
            })
            .collect()
    }
}

/// Sanitize a template parameter value to prevent injection.
/// Strips template markers ({{ }}) and control characters that could
/// alter chain execution flow or inject additional template references.
fn sanitize_template_value(value: &str) -> String {
    value
        .replace("{{", "")
        .replace("}}", "")
        .replace("{", "")
        .replace("}", "")
        // Strip null bytes and other control chars (except newline/tab)
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

impl StepCondition {
    /// Test whether the condition holds against step outputs.
    pub fn test(&self, step_outputs: &HashMap<String, String>) -> bool {
        let value = step_outputs.get(&self.ref_key);
        match self.op {
            ConditionOp::IsEmpty => value.map_or(true, |v| v.is_empty()),
            ConditionOp::IsNotEmpty => value.is_some_and(|v| !v.is_empty()),
            ConditionOp::Equals => value == Some(&self.value),
            ConditionOp::NotEquals => value != Some(&self.value),
            ConditionOp::Contains => value.is_some_and(|v| v.contains(&self.value)),
            ConditionOp::GreaterThan => value
                .and_then(|v| v.parse::<f64>().ok())
                .zip(self.value.parse::<f64>().ok())
                .is_some_and(|(a, b)| a > b),
            ConditionOp::LessThan => value
                .and_then(|v| v.parse::<f64>().ok())
                .zip(self.value.parse::<f64>().ok())
                .is_some_and(|(a, b)| a < b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_chain_yaml() {
        let yaml = r#"
id: check_weather
name: Check Weather
description: Fetch weather for a city and notify user
params:
  - name: city
    param_type: text
    description: City name
    required: true
steps:
  - id: fetch
    ability: data.fetch_url
    args:
      url: "https://api.weather.com/v1/{{city}}"
    output_key: weather_data
  - id: notify
    ability: notify.user
    args:
      message: "Weather in {{city}}: {{weather_data}}"
"#;
        let chain = ChainDef::from_yaml(yaml).unwrap();
        assert_eq!(chain.id, "check_weather");
        assert_eq!(chain.steps.len(), 2);
        assert_eq!(chain.params.len(), 1);
        assert!(chain.check().is_ok());
    }

    #[test]
    fn test_resolve_args() {
        let args: HashMap<String, String> = HashMap::from([
            ("url".into(), "https://api.com/{{city}}".into()),
            ("header".into(), "Bearer {{token}}".into()),
        ]);
        let params: HashMap<String, String> = HashMap::from([
            ("city".into(), "NYC".into()),
            ("token".into(), "abc123".into()),
        ]);
        let resolved = ChainDef::resolve_args(&args, &params, &HashMap::new());
        assert_eq!(resolved["url"], "https://api.com/NYC");
        assert_eq!(resolved["header"], "Bearer abc123");
    }

    #[test]
    fn test_resolve_args_sanitizes_injection() {
        let args: HashMap<String, String> =
            HashMap::from([("url".into(), "https://api.com/{{city}}".into())]);
        // Attacker tries to inject a template reference via parameter value
        let params: HashMap<String, String> =
            HashMap::from([("city".into(), "NYC{{admin_token}}".into())]);
        let resolved = ChainDef::resolve_args(&args, &params, &HashMap::new());
        // Template markers should be stripped from parameter value
        assert_eq!(resolved["url"], "https://api.com/NYCadmin_token");
        assert!(!resolved["url"].contains("{{"));
    }

    #[test]
    fn test_sanitize_strips_control_chars() {
        let args: HashMap<String, String> =
            HashMap::from([("msg".into(), "Hello {{name}}".into())]);
        let params: HashMap<String, String> =
            HashMap::from([("name".into(), "Bob\x00\x01\x02".into())]);
        let resolved = ChainDef::resolve_args(&args, &params, &HashMap::new());
        assert_eq!(resolved["msg"], "Hello Bob");
    }

    #[test]
    fn test_condition_test() {
        let outputs = HashMap::from([("count".into(), "5".into()), ("status".into(), "ok".into())]);

        let cond = StepCondition {
            ref_key: "count".into(),
            op: ConditionOp::GreaterThan,
            value: "3".into(),
        };
        assert!(cond.test(&outputs));

        let cond2 = StepCondition {
            ref_key: "status".into(),
            op: ConditionOp::Equals,
            value: "ok".into(),
        };
        assert!(cond2.test(&outputs));
    }
}
