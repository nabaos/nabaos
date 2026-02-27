//! Template library for meta-agent workflow matching.
//!
//! Stores pre-built workflow templates and matches them against user
//! requirements using keyword scoring.

use crate::chain::workflow::WorkflowDef;
use std::path::Path;

/// A workflow template with matching metadata.
#[derive(Debug, Clone)]
pub struct WorkflowTemplate {
    pub def: WorkflowDef,
    pub keywords: Vec<String>,
    pub category: String, // "ecommerce", "devops", "communication", "monitoring", "general"
}

pub struct TemplateLibrary {
    templates: Vec<WorkflowTemplate>,
}

impl TemplateLibrary {
    /// Create a new template library pre-loaded with builtin demo workflows.
    pub fn new() -> Self {
        let demos = crate::chain::demo_workflows::all_demo_workflows();
        let templates = demos
            .into_iter()
            .map(|def| {
                let keywords = extract_keywords(&def);
                let category = guess_category(&def);
                WorkflowTemplate {
                    def,
                    keywords,
                    category,
                }
            })
            .collect();
        TemplateLibrary { templates }
    }

    /// Load additional templates from YAML files in a directory (in addition to builtins).
    pub fn load_from_dir(&mut self, dir: &Path) -> std::io::Result<usize> {
        let mut count = 0;
        if !dir.is_dir() {
            return Ok(0);
        }
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "yaml" || ext == "yml" {
                let content = std::fs::read_to_string(&path)?;
                if let Ok(def) = serde_yaml::from_str::<WorkflowDef>(&content) {
                    let keywords = extract_keywords(&def);
                    let category = guess_category(&def);
                    self.templates.push(WorkflowTemplate {
                        def,
                        keywords,
                        category,
                    });
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    /// Find the best matching template for a user requirement string.
    ///
    /// Returns `None` if no template scores above the 0.3 threshold.
    /// Matching is case-insensitive.
    pub fn find_match(&self, requirement: &str) -> Option<&WorkflowTemplate> {
        let req_words: Vec<String> = requirement
            .split_whitespace()
            .map(|w| w.to_lowercase())
            .collect();

        let mut best: Option<(&WorkflowTemplate, f64)> = None;
        for tmpl in &self.templates {
            let score = keyword_score(&req_words, &tmpl.keywords);
            if score > 0.3 && (best.is_none() || score > best.unwrap().1) {
                best = Some((tmpl, score));
            }
        }
        best.map(|(t, _)| t)
    }

    /// List all templates in the library.
    pub fn list(&self) -> &[WorkflowTemplate] {
        &self.templates
    }
}

/// Compute keyword match score: ratio of requirement words that appear in the keyword set.
pub fn keyword_score(req_words: &[String], keywords: &[String]) -> f64 {
    if req_words.is_empty() {
        return 0.0;
    }
    let matches = req_words
        .iter()
        .filter(|w| keywords.iter().any(|k| k == *w))
        .count();
    matches as f64 / req_words.len() as f64
}

/// Extract keywords (words > 3 chars) from the workflow's name and description.
/// All keywords are lowercased.
pub fn extract_keywords(def: &WorkflowDef) -> Vec<String> {
    let combined = format!("{} {}", def.name, def.description);
    combined
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 3)
        .map(|w| w.to_lowercase())
        .collect()
}

/// Guess a category for the workflow based on keywords in its name and description.
/// Uses a scoring approach: the category with the most keyword hits wins.
pub fn guess_category(def: &WorkflowDef) -> String {
    let text = format!("{} {}", def.name, def.description).to_lowercase();

    let categories: &[(&str, &[&str])] = &[
        (
            "ecommerce",
            &[
                "shopify",
                "order",
                "dropship",
                "ecommerce",
                "fulfillment",
                "shipping",
                "cart",
                "product",
            ],
        ),
        (
            "devops",
            &[
                "ci/cd", "cicd", "pipeline", "deploy", "devops", "git", "lint", "build",
            ],
        ),
        (
            "monitoring",
            &[
                "price",
                "alert",
                "monitor",
                "trading",
                "threshold",
                "ticker",
                "instrument",
            ],
        ),
        (
            "communication",
            &[
                "email",
                "digest",
                "notification",
                "message",
                "onboarding",
                "welcome",
                "customer",
            ],
        ),
    ];

    let mut best_cat = "general";
    let mut best_score = 0usize;

    for (cat, keywords) in categories {
        let score = keywords.iter().filter(|kw| text.contains(**kw)).count();
        if score > best_score {
            best_score = score;
            best_cat = cat;
        }
    }

    best_cat.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_templates_loaded() {
        let lib = TemplateLibrary::new();
        assert_eq!(lib.list().len(), 5, "Should load 5 builtin demo workflows");
    }

    #[test]
    fn test_find_match_ecommerce() {
        let lib = TemplateLibrary::new();
        let result = lib.find_match("order fulfillment shipping");
        assert!(result.is_some(), "Should match an ecommerce workflow");
        let tmpl = result.unwrap();
        assert_eq!(
            tmpl.def.id, "shopify_dropship",
            "Should match the shopify_dropship workflow"
        );
    }

    #[test]
    fn test_find_match_no_match() {
        let lib = TemplateLibrary::new();
        let result = lib.find_match("quantum physics calculation");
        assert!(result.is_none(), "Should not match any workflow");
    }

    #[test]
    fn test_keyword_score() {
        let req = vec!["order".into(), "shipping".into(), "dropship".into()];
        let keywords = vec![
            "shopify".into(),
            "dropship".into(),
            "order".into(),
            "shipping".into(),
            "pipeline".into(),
        ];
        let score = keyword_score(&req, &keywords);
        assert!(
            (score - 1.0).abs() < f64::EPSILON,
            "All 3 req words match, score should be 1.0, got {}",
            score
        );

        let req2 = vec!["quantum".into(), "physics".into()];
        let score2 = keyword_score(&req2, &keywords);
        assert!(
            score2.abs() < f64::EPSILON,
            "No match should yield 0.0, got {}",
            score2
        );

        let empty: Vec<String> = vec![];
        let score3 = keyword_score(&empty, &keywords);
        assert!(
            score3.abs() < f64::EPSILON,
            "Empty req should yield 0.0, got {}",
            score3
        );
    }

    #[test]
    fn test_list_all() {
        let lib = TemplateLibrary::new();
        let all = lib.list();
        assert_eq!(all.len(), 5);
        let ids: Vec<&str> = all.iter().map(|t| t.def.id.as_str()).collect();
        assert!(ids.contains(&"shopify_dropship"));
        assert!(ids.contains(&"email_digest"));
        assert!(ids.contains(&"price_alert_workflow"));
        assert!(ids.contains(&"cicd_pipeline"));
        assert!(ids.contains(&"customer_onboarding"));
    }

    #[test]
    fn test_extract_keywords() {
        let def = crate::chain::demo_workflows::all_demo_workflows()
            .into_iter()
            .find(|d| d.id == "shopify_dropship")
            .unwrap();
        let kw = extract_keywords(&def);
        assert!(kw.contains(&"shopify".to_string()));
        assert!(kw.contains(&"dropship".to_string()));
        assert!(kw.contains(&"order".to_string()));
    }

    #[test]
    fn test_guess_category() {
        let demos = crate::chain::demo_workflows::all_demo_workflows();
        for def in &demos {
            let cat = guess_category(def);
            match def.id.as_str() {
                "shopify_dropship" => assert_eq!(cat, "ecommerce"),
                "cicd_pipeline" => assert_eq!(cat, "devops"),
                "email_digest" => assert_eq!(cat, "communication"),
                "price_alert_workflow" => assert_eq!(cat, "monitoring"),
                "customer_onboarding" => assert_eq!(cat, "communication"),
                _ => {}
            }
        }
    }
}
