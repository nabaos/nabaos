use crate::swarm::worker::{Citation, WorkerResult};

/// Synthesized research report from swarm results.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SynthesisReport {
    pub query: String,
    pub summary: String,
    pub sections: Vec<ReportSection>,
    pub citations: Vec<Citation>,
    pub sources_used: usize,
    pub sources_total: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReportSection {
    pub heading: String,
    pub content: String,
    pub source_indices: Vec<usize>,
}

/// Build a synthesis prompt for the LLM from collected worker results.
pub fn build_synthesis_prompt(
    query: &str,
    results: &[&WorkerResult],
    instructions: &str,
) -> String {
    let mut prompt = format!(
        "You are synthesizing research results into a comprehensive report.\n\n\
         Research query: {}\n\n\
         Instructions: {}\n\n\
         Sources:\n",
        query, instructions
    );

    for (i, result) in results.iter().enumerate() {
        let source_desc = format!("{}", result.source_plan.target);
        let truncated = if result.content.len() > 2000 {
            format!("{}...[truncated]", &result.content[..2000])
        } else {
            result.content.clone()
        };
        prompt.push_str(&format!(
            "\n--- Source {} ({}, {}) ---\n{}\n",
            i + 1,
            result.source_plan.worker_type,
            source_desc,
            truncated
        ));
    }

    prompt.push_str(
        "\n\nProvide a structured research report with:\n\
         1. An executive summary (2-3 sentences)\n\
         2. Key findings organized by theme\n\
         3. Citations referencing source numbers [1], [2], etc.\n\
         4. Any gaps or areas needing further research\n",
    );

    prompt
}

/// Build a basic report without LLM (fallback).
pub fn build_basic_report(query: &str, results: &[&WorkerResult]) -> SynthesisReport {
    let mut all_citations = Vec::new();
    let mut sections = Vec::new();

    for (i, result) in results.iter().enumerate() {
        for cit in &result.citations {
            all_citations.push(cit.clone());
        }
        let preview = if result.content.len() > 500 {
            format!("{}...", &result.content[..500])
        } else {
            result.content.clone()
        };
        sections.push(ReportSection {
            heading: format!(
                "Source {}: {} ({})",
                i + 1,
                result.source_plan.worker_type,
                result.source_plan.target
            ),
            content: preview,
            source_indices: vec![i],
        });
    }

    let summary = format!(
        "Research results for '{}': {} sources collected, {} with content.",
        query,
        results.len(),
        results.iter().filter(|r| !r.content.is_empty()).count(),
    );

    SynthesisReport {
        query: query.to_string(),
        summary,
        sections,
        citations: all_citations,
        sources_used: results.len(),
        sources_total: results.len(),
    }
}

/// Parse an LLM synthesis response into a SynthesisReport.
///
/// Expects markdown with ## headings. Extracts [N] citation references.
pub fn parse_synthesis_response(
    query: &str,
    llm_text: &str,
    sources_total: usize,
) -> SynthesisReport {
    let mut sections = Vec::new();
    let mut current_heading = String::new();
    let mut current_content = String::new();
    let mut current_sources = Vec::new();

    for line in llm_text.lines() {
        if line.starts_with("## ") {
            // Save previous section
            if !current_heading.is_empty() {
                sections.push(ReportSection {
                    heading: current_heading.clone(),
                    content: current_content.trim().to_string(),
                    source_indices: current_sources.clone(),
                });
            }
            current_heading = line.trim_start_matches("## ").to_string();
            current_content.clear();
            current_sources.clear();
        } else {
            current_content.push_str(line);
            current_content.push('\n');
            // Extract [N] citation references
            let mut i = 0;
            let bytes = line.as_bytes();
            while i < bytes.len() {
                if bytes[i] == b'[' {
                    let start = i + 1;
                    i += 1;
                    while i < bytes.len() && bytes[i] != b']' {
                        i += 1;
                    }
                    if i < bytes.len() {
                        if let Ok(n) = line[start..i].parse::<usize>() {
                            if n > 0 && !current_sources.contains(&(n - 1)) {
                                current_sources.push(n - 1); // 0-indexed
                            }
                        }
                    }
                }
                i += 1;
            }
        }
    }

    // Save last section
    if !current_heading.is_empty() {
        sections.push(ReportSection {
            heading: current_heading,
            content: current_content.trim().to_string(),
            source_indices: current_sources,
        });
    }

    // If no sections parsed, put everything in one section
    if sections.is_empty() {
        sections.push(ReportSection {
            heading: "Summary".to_string(),
            content: llm_text.trim().to_string(),
            source_indices: Vec::new(),
        });
    }

    // Extract summary from first section or first paragraph
    let summary = sections
        .first()
        .map(|s| {
            let first_para = s.content.split("\n\n").next().unwrap_or(&s.content);
            if first_para.len() > 300 {
                let boundary = first_para.floor_char_boundary(300);
                format!("{}...", &first_para[..boundary])
            } else {
                first_para.to_string()
            }
        })
        .unwrap_or_default();

    SynthesisReport {
        query: query.to_string(),
        summary,
        sections,
        citations: Vec::new(),
        sources_used: sources_total,
        sources_total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swarm::worker::{SourcePlan, SourceTarget, WorkerOutcome};

    #[test]
    fn test_parse_synthesis_response_with_sections() {
        let llm_text = "## Executive Summary\nThis research covers quantum computing [1].\n\n## Key Findings\nQuantum supremacy was demonstrated [2] in 2024.\nMultiple approaches exist [1] [3].\n\n## Gaps\nMore research needed on error correction.\n";
        let report = parse_synthesis_response("quantum computing", llm_text, 3);
        assert_eq!(report.query, "quantum computing");
        assert_eq!(report.sections.len(), 3);
        assert_eq!(report.sections[0].heading, "Executive Summary");
        assert!(report.sections[0].source_indices.contains(&0)); // [1] -> index 0
        assert_eq!(report.sections[1].heading, "Key Findings");
        assert!(report.sections[1].source_indices.contains(&1)); // [2]
        assert!(report.sections[1].source_indices.contains(&0)); // [1]
        assert!(report.sections[1].source_indices.contains(&2)); // [3]
    }

    #[test]
    fn test_parse_synthesis_response_no_headings() {
        let llm_text = "Just a plain text response with no markdown headings.";
        let report = parse_synthesis_response("test", llm_text, 1);
        assert_eq!(report.sections.len(), 1);
        assert_eq!(report.sections[0].heading, "Summary");
        assert!(report.sections[0].content.contains("plain text response"));
    }

    #[test]
    fn test_build_synthesis_prompt_includes_sources() {
        let results = [WorkerResult {
            source_plan: SourcePlan {
                worker_type: "academic".into(),
                target: SourceTarget::SearchQuery("test".into()),
                priority: 0,
                needs_auth: false,
                extraction_focus: None,
            },
            outcome: WorkerOutcome::Success,
            content: "Some content here".into(),
            content_hash: WorkerResult::compute_hash("Some content here"),
            structured_data: None,
            citations: Vec::new(),
            elapsed_ms: 100,
        }];
        let refs: Vec<&WorkerResult> = results.iter().collect();
        let prompt = build_synthesis_prompt("my query", &refs, "summarize");
        assert!(prompt.contains("my query"));
        assert!(prompt.contains("Some content here"));
        assert!(prompt.contains("Source 1"));
    }
}
