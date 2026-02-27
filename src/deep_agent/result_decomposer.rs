//! Heuristic goal decomposition for complex multi-step queries.
//!
//! Splits queries containing sequential steps ("then", "after that", numbered lists)
//! into ordered subtasks. No LLM call — pure string-based heuristic.
//! Phase 3 can add LLM-powered decomposition for ambiguous cases.

/// A subtask extracted from a complex query.
#[derive(Debug, Clone)]
pub struct Subtask {
    /// Human-readable description of this subtask.
    pub description: String,
    /// Whether this subtask depends on previous subtasks' output.
    pub depends_on_previous: bool,
    /// Estimated complexity: "simple", "medium", "complex".
    pub complexity: String,
}

/// Result of decomposing a complex query.
#[derive(Debug)]
pub struct DecompositionResult {
    /// The original query.
    pub original_query: String,
    /// Whether decomposition was applied (false if query is atomic).
    pub was_decomposed: bool,
    /// Ordered list of subtasks. Empty if not decomposed.
    pub subtasks: Vec<Subtask>,
    /// Summary for the user: what the agent plans to do.
    pub plan_summary: String,
}

/// Heuristic decomposer that splits multi-step queries into subtasks.
///
/// Recognized patterns (in priority order):
/// 1. Numbered lists: "1. Do X\n2. Do Y"
/// 2. Bullet lists: "- Task A\n- Task B"
/// 3. " then " separator
/// 4. " after that " separator
/// 5. " and also " separator
/// 6. ". next, " separator
pub fn decompose_query(query: &str) -> DecompositionResult {
    let trimmed = query.trim();
    let normalized = trimmed.to_lowercase();

    // Try numbered list first: "1. ... 2. ... 3. ..."
    let numbered_parts = extract_numbered_list(trimmed);
    if numbered_parts.len() >= 2 {
        return build_result(trimmed, numbered_parts);
    }

    // Try bullet list: "- ... \n- ..."
    let bullet_parts = extract_bullet_list(trimmed);
    if bullet_parts.len() >= 2 {
        return build_result(trimmed, bullet_parts);
    }

    // Try inline separators
    let separators = [" then ", " after that ", " and also ", ". next, "];

    for sep in &separators {
        if normalized.contains(sep) {
            // Case-insensitive split preserving original casing
            let lower = trimmed.to_lowercase();
            let mut result_parts = Vec::new();
            let mut last_end = 0;
            for (idx, _) in lower.match_indices(sep) {
                let part = trimmed[last_end..idx].trim().to_string();
                if !part.is_empty() {
                    result_parts.push(part);
                }
                last_end = idx + sep.len();
            }
            let final_part = trimmed[last_end..].trim().to_string();
            if !final_part.is_empty() {
                result_parts.push(final_part);
            }

            if result_parts.len() >= 2 {
                return build_result(trimmed, result_parts);
            }
        }
    }

    // No decomposition
    DecompositionResult {
        original_query: trimmed.to_string(),
        was_decomposed: false,
        subtasks: Vec::new(),
        plan_summary: "Single-step task, no decomposition needed.".to_string(),
    }
}

/// Extract parts from a numbered list (1. ... 2. ... or 1) ... 2) ...)
fn extract_numbered_list(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Match "N. text" or "N) text" where N is a digit
        if let Some(first_char) = trimmed.chars().next() {
            if first_char.is_ascii_digit() {
                if let Some(pos) = trimmed.find(". ") {
                    parts.push(trimmed[pos + 2..].trim().to_string());
                } else if let Some(pos) = trimmed.find(") ") {
                    parts.push(trimmed[pos + 2..].trim().to_string());
                }
            }
        }
    }
    parts.into_iter().filter(|s| !s.is_empty()).collect()
}

/// Extract parts from a bullet list (- ... or * ...)
fn extract_bullet_list(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- ") {
            if !rest.is_empty() {
                parts.push(rest.trim().to_string());
            }
        } else if let Some(rest) = trimmed.strip_prefix("* ") {
            if !rest.is_empty() {
                parts.push(rest.trim().to_string());
            }
        }
    }
    parts
}

fn build_result(original: &str, parts: Vec<String>) -> DecompositionResult {
    let subtasks: Vec<Subtask> = parts
        .iter()
        .enumerate()
        .map(|(i, desc)| {
            let complexity = if desc.len() > 200 {
                "complex"
            } else if desc.len() > 50 {
                "medium"
            } else {
                "simple"
            };
            Subtask {
                description: desc.clone(),
                depends_on_previous: i > 0,
                complexity: complexity.to_string(),
            }
        })
        .collect();

    let plan_summary = format!(
        "Decomposed into {} subtasks:\n{}",
        subtasks.len(),
        subtasks
            .iter()
            .enumerate()
            .map(|(i, s)| format!("  {}. {}", i + 1, s.description))
            .collect::<Vec<_>>()
            .join("\n")
    );

    DecompositionResult {
        original_query: original.to_string(),
        was_decomposed: true,
        subtasks,
        plan_summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_query_not_decomposed() {
        let result = decompose_query("What is the weather?");
        assert!(!result.was_decomposed);
        assert!(result.subtasks.is_empty());
    }

    #[test]
    fn test_then_splitter() {
        let result = decompose_query("Check email then summarize results");
        assert!(result.was_decomposed);
        assert_eq!(result.subtasks.len(), 2);
        assert_eq!(result.subtasks[0].description, "Check email");
        assert_eq!(result.subtasks[1].description, "summarize results");
        assert!(!result.subtasks[0].depends_on_previous);
        assert!(result.subtasks[1].depends_on_previous);
    }

    #[test]
    fn test_after_that_splitter() {
        let result = decompose_query("Download the file after that process it");
        assert!(result.was_decomposed);
        assert_eq!(result.subtasks.len(), 2);
    }

    #[test]
    fn test_numbered_list() {
        let result = decompose_query("1. Check email\n2. Summarize findings\n3. Send report");
        assert!(result.was_decomposed);
        assert_eq!(result.subtasks.len(), 3);
        assert_eq!(result.subtasks[0].description, "Check email");
        assert_eq!(result.subtasks[1].description, "Summarize findings");
        assert_eq!(result.subtasks[2].description, "Send report");
    }

    #[test]
    fn test_bullet_list() {
        let result = decompose_query("- Check logs\n- Find errors\n- Report issues");
        assert!(result.was_decomposed);
        assert_eq!(result.subtasks.len(), 3);
        assert_eq!(result.subtasks[0].description, "Check logs");
    }

    #[test]
    fn test_and_also_splitter() {
        let result = decompose_query("Send the report and also update the dashboard");
        assert!(result.was_decomposed);
        assert_eq!(result.subtasks.len(), 2);
    }

    #[test]
    fn test_subtask_dependencies() {
        let result = decompose_query("1. Step A\n2. Step B\n3. Step C");
        assert_eq!(result.subtasks.len(), 3);
        assert!(!result.subtasks[0].depends_on_previous);
        assert!(result.subtasks[1].depends_on_previous);
        assert!(result.subtasks[2].depends_on_previous);
    }

    #[test]
    fn test_complexity_estimation() {
        let short = "Do X";
        let medium = &"a".repeat(60); // > 50 chars
        let complex = &"b".repeat(250); // > 200 chars

        let result = decompose_query(&format!("- {}\n- {}\n- {}", short, medium, complex));
        assert!(result.was_decomposed);
        assert_eq!(result.subtasks[0].complexity, "simple");
        assert_eq!(result.subtasks[1].complexity, "medium");
        assert_eq!(result.subtasks[2].complexity, "complex");
    }
}
