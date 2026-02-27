use crate::runtime::host_functions::AbilitySpec;

/// Summary of an ability for inclusion in the capability digest.
#[derive(Debug, Clone)]
pub struct AbilitySummary {
    /// Dotted ability name (e.g. "email.send").
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Parameter names; optional params are suffixed with '?'.
    pub params: Vec<String>,
}

/// Summary of a workflow template for inclusion in the capability digest.
#[derive(Debug, Clone)]
pub struct WorkflowSummary {
    /// Unique workflow identifier.
    pub id: String,
    /// Human-readable workflow name.
    pub name: String,
    /// Brief description of what the workflow does.
    pub description: String,
    /// Number of nodes in the workflow graph.
    pub node_count: usize,
}

/// Compact index of all system capabilities (abilities, workflows, constraints).
///
/// Designed to be serialised into a short text digest that fits inside an LLM
/// prompt, giving the model enough context to generate valid workflows.
#[derive(Debug, Clone)]
pub struct CapabilityIndex {
    pub abilities: Vec<AbilitySummary>,
    pub workflows: Vec<WorkflowSummary>,
    pub constraints: Vec<String>,
}

impl CapabilityIndex {
    /// Build a capability index from raw ability specs, workflow summaries and
    /// a list of blocked keywords/phrases.
    pub fn build(
        abilities: &[AbilitySpec],
        workflows: Vec<WorkflowSummary>,
        blocked_keywords: &[String],
    ) -> Self {
        let abilities = abilities
            .iter()
            .map(|spec| AbilitySummary {
                name: spec.name.clone(),
                description: spec.description.clone(),
                params: extract_params_from_schema(&spec.input_schema),
            })
            .collect();

        let constraints = blocked_keywords
            .iter()
            .map(|kw| format!("Blocked: {}", kw))
            .collect();

        Self {
            abilities,
            workflows,
            constraints,
        }
    }

    /// Render a compact text digest suitable for injection into an LLM prompt.
    ///
    /// Format:
    /// ```text
    /// ABILITIES (N):
    /// - name: description (param1, param2?)
    /// ...
    ///
    /// WORKFLOW TEMPLATES (M):
    /// - id: name (K nodes)
    /// ...
    ///
    /// CONSTRAINTS:
    /// - Blocked: phrase
    /// ...
    /// ```
    pub fn to_digest(&self) -> String {
        let mut out = String::new();

        // -- Abilities --
        out.push_str(&format!("ABILITIES ({}):\n", self.abilities.len()));
        for a in &self.abilities {
            if a.params.is_empty() {
                out.push_str(&format!("- {}: {}\n", a.name, a.description));
            } else {
                out.push_str(&format!(
                    "- {}: {} ({})\n",
                    a.name,
                    a.description,
                    a.params.join(", ")
                ));
            }
        }

        // -- Workflow templates --
        out.push('\n');
        out.push_str(&format!("WORKFLOW TEMPLATES ({}):\n", self.workflows.len()));
        for w in &self.workflows {
            out.push_str(&format!(
                "- {}: {} ({} nodes)\n",
                w.id, w.name, w.node_count
            ));
        }

        // -- Constraints --
        out.push('\n');
        out.push_str("CONSTRAINTS:\n");
        if self.constraints.is_empty() {
            out.push_str("- (none)\n");
        } else {
            for c in &self.constraints {
                out.push_str(&format!("- {}\n", c));
            }
        }

        out
    }
}

/// Extract parameter names from a JSON Schema `input_schema`.
///
/// Expects a standard JSON Schema object with `properties` and optionally
/// `required`.  Parameters that are NOT listed in `required` are suffixed
/// with `?` to indicate they are optional.
///
/// Returns an empty vec when `schema` is `None` or has no `properties`.
pub fn extract_params_from_schema(schema: &Option<serde_json::Value>) -> Vec<String> {
    let schema = match schema {
        Some(s) => s,
        None => return Vec::new(),
    };

    let properties = match schema.get("properties").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => return Vec::new(),
    };

    let required: Vec<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let mut params: Vec<String> = properties
        .keys()
        .map(|key| {
            if required.contains(&key.as_str()) {
                key.clone()
            } else {
                format!("{}?", key)
            }
        })
        .collect();

    // Sort so required params come first, then optional, both alphabetically.
    params.sort_by(|a, b| {
        let a_opt = a.ends_with('?');
        let b_opt = b.ends_with('?');
        match (a_opt, b_opt) {
            (false, true) => std::cmp::Ordering::Less,
            (true, false) => std::cmp::Ordering::Greater,
            _ => a.cmp(b),
        }
    });

    params
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::plugin::AbilitySource;

    fn make_ability(name: &str, desc: &str, schema: Option<serde_json::Value>) -> AbilitySpec {
        AbilitySpec {
            name: name.to_string(),
            description: desc.to_string(),
            permission: name.to_string(),
            source: AbilitySource::BuiltIn,
            input_schema: schema,
        }
    }

    fn sample_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "to": { "type": "string" },
                "subject": { "type": "string" },
                "body": { "type": "string" }
            },
            "required": ["to", "subject"]
        })
    }

    #[test]
    fn test_capability_index_build() {
        let abilities = vec![
            make_ability("email.send", "Send email", Some(sample_schema())),
            make_ability("api.call", "HTTP API call", None),
            make_ability("storage.get", "Read from KV store", None),
        ];

        let workflows = vec![
            WorkflowSummary {
                id: "shopify_dropship".into(),
                name: "Shopify Dropship Order Fulfillment".into(),
                description: "End-to-end dropship flow".into(),
                node_count: 15,
            },
            WorkflowSummary {
                id: "support_ticket".into(),
                name: "Support Ticket Triage".into(),
                description: "Route incoming tickets".into(),
                node_count: 8,
            },
        ];

        let blocked = vec!["delete all".to_string(), "rm -rf".to_string()];

        let index = CapabilityIndex::build(&abilities, workflows, &blocked);

        assert_eq!(index.abilities.len(), 3);
        assert_eq!(index.workflows.len(), 2);
        assert_eq!(index.constraints.len(), 2);

        // Verify param extraction happened for the ability with a schema.
        let email = &index.abilities[0];
        assert_eq!(email.name, "email.send");
        assert!(!email.params.is_empty());

        // Ability without schema should have empty params.
        let api = &index.abilities[1];
        assert!(api.params.is_empty());
    }

    #[test]
    fn test_digest_format() {
        let abilities = vec![make_ability(
            "email.send",
            "Send email",
            Some(sample_schema()),
        )];

        let workflows = vec![WorkflowSummary {
            id: "shopify_dropship".into(),
            name: "Shopify Dropship Order Fulfillment".into(),
            description: "End-to-end dropship flow".into(),
            node_count: 15,
        }];

        let blocked = vec!["delete all".to_string(), "rm -rf".to_string()];
        let index = CapabilityIndex::build(&abilities, workflows, &blocked);
        let digest = index.to_digest();

        // Section headers present.
        assert!(digest.contains("ABILITIES (1):"));
        assert!(digest.contains("WORKFLOW TEMPLATES (1):"));
        assert!(digest.contains("CONSTRAINTS:"));

        // Ability line format.
        assert!(digest.contains("- email.send: Send email ("));
        assert!(digest.contains("subject"));
        assert!(digest.contains("body?"));

        // Workflow line format.
        assert!(
            digest.contains("- shopify_dropship: Shopify Dropship Order Fulfillment (15 nodes)")
        );

        // Constraints.
        assert!(digest.contains("- Blocked: delete all"));
        assert!(digest.contains("- Blocked: rm -rf"));
    }

    #[test]
    fn test_digest_empty() {
        let index = CapabilityIndex::build(&[], Vec::new(), &[]);
        let digest = index.to_digest();

        assert!(digest.contains("ABILITIES (0):"));
        assert!(digest.contains("WORKFLOW TEMPLATES (0):"));
        assert!(digest.contains("CONSTRAINTS:"));
        assert!(digest.contains("- (none)"));
    }

    #[test]
    fn test_extract_params() {
        let schema = sample_schema();
        let params = extract_params_from_schema(&Some(schema));

        // "to" and "subject" are required, "body" is optional.
        assert_eq!(params.len(), 3);
        assert!(params.contains(&"to".to_string()));
        assert!(params.contains(&"subject".to_string()));
        assert!(params.contains(&"body?".to_string()));

        // Required params come before optional.
        let first_optional = params.iter().position(|p| p.ends_with('?')).unwrap();
        for p in &params[..first_optional] {
            assert!(!p.ends_with('?'));
        }
    }

    #[test]
    fn test_extract_params_no_schema() {
        let params = extract_params_from_schema(&None);
        assert!(params.is_empty());
    }
}
