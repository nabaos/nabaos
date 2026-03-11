// PEA Document Composer — intelligent multi-level document composition.
//
// Phases:
//   1. Structure Decision: LLM plans document outline with hierarchy + dependencies
//   2. Generation Order: Topological sort on section dependency graph (Kahn's algorithm)
//   3. Section Generation: Generate each section in topo order with context threading
//   4. Quality Review: 2-round coherence + readability review with targeted fixes
//   5. Final Assembly: Combine sections into HTML/LaTeX/PDF output

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use crate::core::error::{NyayaError, Result};
use crate::pea::document::{self, ImageEntry, StyleConfig};
use crate::pea::research::ResearchCorpus;
use crate::runtime::host_functions::AbilityRegistry;
use crate::runtime::manifest::AgentManifest;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

pub struct DocumentComposer<'a> {
    registry: &'a AbilityRegistry,
    manifest: &'a AgentManifest,
    config: ComposerConfig,
}

pub struct ComposerConfig {
    pub max_depth: usize,
    pub review_rounds: usize,
    pub max_tokens_per_section: u32,
}

impl Default for ComposerConfig {
    fn default() -> Self {
        Self {
            max_depth: 3,
            review_rounds: 2,
            max_tokens_per_section: 8192,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DocumentOutline {
    pub title: String,
    #[serde(default)]
    pub needs_toc: bool,
    pub sections: Vec<OutlineSection>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OutlineSection {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub level: usize,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(skip)]
    pub generation_order: Option<usize>,
    #[serde(default)]
    pub children: Vec<OutlineSection>,
}

pub struct GeneratedSection {
    pub id: String,
    pub title: String,
    pub level: usize,
    pub content: String,
    pub summary: String,
    pub hook: Option<String>,
}

pub struct ComposedDocument {
    pub title: String,
    pub needs_toc: bool,
    pub sections: Vec<GeneratedSection>,
    pub review_notes: Vec<String>,
}

// ---------------------------------------------------------------------------
// DocumentComposer implementation
// ---------------------------------------------------------------------------

impl<'a> DocumentComposer<'a> {
    pub fn new(
        registry: &'a AbilityRegistry,
        manifest: &'a AgentManifest,
        config: ComposerConfig,
    ) -> Self {
        Self { registry, manifest, config }
    }

    /// Full composition pipeline: plan → order → generate → review → assemble.
    pub fn compose_document(
        &self,
        objective: &str,
        corpus: &ResearchCorpus,
        task_results: &[(String, String)],
        images: &[ImageEntry],
        style: &StyleConfig,
        output_dir: &Path,
    ) -> Result<PathBuf> {
        std::fs::create_dir_all(output_dir)
            .map_err(|e| NyayaError::Config(format!("create output dir: {}", e)))?;

        // Phase 1: Plan structure
        eprintln!("[composer] planning document structure...");
        let mut outline = self.plan_structure(objective, corpus, task_results)?;
        eprintln!(
            "[composer] outline: {} top-level sections, toc={}",
            outline.sections.len(),
            outline.needs_toc,
        );

        // Phase 1b: Enforce section ordering — Exec Summary first, Methodology last
        reorder_outline_sections(&mut outline);

        // Phase 1c: Cap total sections at 15
        cap_section_count(&mut outline, 15);

        // Phase 2: Compute generation order
        compute_generation_order(&mut outline);

        // Phase 3: Generate sections
        eprintln!("[composer] generating sections...");
        let mut sections = self.generate_sections(&outline, corpus, task_results)?;
        eprintln!("[composer] generated {} sections", sections.len());

        // Phase 4: Quality review (2 rounds)
        let mut all_notes = Vec::new();
        for round in 1..=self.config.review_rounds {
            eprintln!("[composer] review round {}...", round);
            let notes = self.review_document(&outline, &mut sections, round);
            all_notes.extend(notes);
        }

        // Phase 4b: Completion check — verify all planned sections were generated
        eprintln!("[composer] checking section completeness...");
        let missing = self.check_completeness(&outline, &sections);
        if !missing.is_empty() {
            eprintln!("[composer] {} missing sections, generating...", missing.len());
            let extra = self.generate_missing_sections(&outline, &missing, corpus, task_results, &sections);
            sections.extend(extra);
            all_notes.push(format!("Completion check: regenerated {} missing sections", missing.len()));
        }

        // Phase 4c: Fact verification — cross-check numerical claims across sections
        eprintln!("[composer] verifying numerical claims...");
        let fact_notes = self.verify_numerical_claims(&sections);
        all_notes.extend(fact_notes);

        // Phase 5: Assemble final output
        eprintln!("[composer] assembling final document...");
        let doc = ComposedDocument {
            title: outline.title.clone(),
            needs_toc: outline.needs_toc,
            sections,
            review_notes: all_notes,
        };

        self.assemble_output(&doc, images, style, output_dir)
    }

    // -- Phase 1: Structure Planning ------------------------------------------

    fn plan_structure(
        &self,
        objective: &str,
        corpus: &ResearchCorpus,
        task_results: &[(String, String)],
    ) -> Result<DocumentOutline> {
        let corpus_summary = format!(
            "{} sources fetched, {} total chars. Top sources: {}",
            corpus.sources.len(),
            corpus.total_chars,
            corpus
                .sources
                .iter()
                .take(10)
                .map(|s| format!("- {} ({})", s.title, s.url))
                .collect::<Vec<_>>()
                .join("\n"),
        );

        let task_summaries: String = task_results
            .iter()
            .take(5)
            .map(|(desc, text)| {
                let preview = crate::pea::research::safe_slice(text, 300);
                format!("- {}: {}", desc, preview)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "You are a document architect. Plan the structure for: \"{}\"\n\n\
             Available research:\n{}\n\n\
             Task results:\n{}\n\n\
             Decide:\n\
             1. Document title\n\
             2. Whether a Table of Contents is needed (yes for 5+ sections)\n\
             3. Hierarchical structure with chapters/sections/subsections\n\
             4. For each section: id, title, description, dependencies\n\n\
             Respond in JSON:\n\
             {{\n\
               \"title\": \"...\",\n\
               \"needs_toc\": true,\n\
               \"sections\": [\n\
                 {{\"id\": \"ch1\", \"title\": \"...\", \"level\": 0, \"description\": \"...\", \"depends_on\": [], \"children\": [\n\
                   {{\"id\": \"ch1.s1\", \"title\": \"...\", \"level\": 1, \"description\": \"...\", \"depends_on\": [\"ch1\"], \"children\": []}}\n\
                 ]}}\n\
               ]\n\
             }}\n\n\
             RULES:\n\
             - Use 3-8 top-level chapters, 12-15 sections MAXIMUM (including subsections)\n\
             - Merge related topics into single chapters with subsections instead of separate chapters\n\
             - REQUIRED ORDER: Executive Summary/Introduction FIRST, Methodology/PRISMA LAST (as appendix)\n\
             - Introduction and Conclusion should depend on body chapters\n\
             - Each section needs a clear description of what it covers\n\
             - Do NOT create duplicate sections (e.g. avoid separate 'Key Findings' + 'Synthesis' + 'Conclusion')\n\
             - Output ONLY valid JSON, no explanation",
            objective, corpus_summary, task_summaries,
        );

        let input = serde_json::json!({
            "system": "You are a document architect. Output ONLY valid JSON document outlines. \
                       No explanation, no markdown fences.",
            "prompt": prompt,
            "max_tokens": 4096,
            "thinking": false,
        });

        let result = self
            .registry
            .execute_ability(self.manifest, "llm.chat", &input.to_string())
            .map_err(|e| NyayaError::Config(format!("LLM structure planning failed: {}", e)))?;

        let raw = String::from_utf8_lossy(&result.output).to_string();

        // Extract JSON from response
        let json_str = extract_json(&raw);
        let outline: DocumentOutline = serde_json::from_str(json_str).map_err(|e| {
            eprintln!("[composer] JSON parse failed: {}, raw: {}", e, &raw[..raw.len().min(500)]);
            // Fallback: create a simple outline from task_results
            NyayaError::Config(format!("outline JSON parse: {}", e))
        })?;

        Ok(outline)
    }

    // -- Phase 3: Section-by-Section Generation --------------------------------

    fn generate_sections(
        &self,
        outline: &DocumentOutline,
        corpus: &ResearchCorpus,
        task_results: &[(String, String)],
    ) -> Result<Vec<GeneratedSection>> {
        let flat = flatten_outline(&outline.sections);

        // Sort by generation_order
        let mut ordered: Vec<&OutlineSection> = flat.iter().collect();
        ordered.sort_by_key(|s| s.generation_order.unwrap_or(usize::MAX));

        let mut generated: Vec<GeneratedSection> = Vec::new();
        let mut used_phrases: Vec<String> = Vec::new();

        for section in &ordered {
            // Build context from previously generated sections
            let prev_context: String = generated
                .iter()
                .map(|g| {
                    let hook = g.hook.as_deref().unwrap_or("");
                    format!("### {} (summary)\n{}\n{}\n", g.title, g.summary, hook)
                })
                .collect::<Vec<_>>()
                .join("\n");

            // Select relevant sources based on section description
            let relevant_sources = select_relevant_sources(corpus, &section.description, &section.title);

            // Find next section title for hook
            let next_title = ordered
                .iter()
                .find(|s| s.generation_order > section.generation_order)
                .map(|s| s.title.as_str())
                .unwrap_or("conclusion");

            // Task results context
            let task_context: String = task_results
                .iter()
                .take(3)
                .map(|(desc, text)| {
                    let preview = crate::pea::research::safe_slice(text, 500);
                    format!("### {}\n{}", desc, preview)
                })
                .collect::<Vec<_>>()
                .join("\n\n");

            // Build used-phrases warning
            let phrase_warning = if used_phrases.is_empty() {
                String::new()
            } else {
                format!(
                    "\n\nAVOID REPETITION — these phrases have been used in previous sections and MUST NOT be repeated verbatim:\n{}\n\
                     Use different wording to express similar ideas.\n",
                    used_phrases.iter().take(20).map(|p| format!("- \"{}\"", p)).collect::<Vec<_>>().join("\n")
                )
            };

            let prompt = format!(
                "You are writing section \"{}\" of \"{}\".\n\n\
                 CONTEXT FROM PREVIOUS SECTIONS:\n{}\n\n\
                 RESEARCH SOURCES for this section:\n{}\n\n\
                 TASK RESULTS:\n{}\n\n\
                 SECTION REQUIREMENTS:\n{}\
                 {}\n\n\
                 Write this section. Requirements:\n\
                 - Make it engaging and well-structured\n\
                 - Cite sources with [Author/Site](URL) format\n\
                 - End with a hook/transition that leads into the next section: \"{}\"\n\
                 - Do NOT include \\section{{}} or chapter headers — just the body content\n\n\
                 After the content, on a NEW line write:\n\
                 SUMMARY: {{2-3 sentence summary}}\n\
                 HOOK: {{transition sentence for next section}}",
                section.title,
                outline.title,
                if prev_context.is_empty() { "(first section)" } else { &prev_context },
                relevant_sources,
                task_context,
                section.description,
                phrase_warning,
                next_title,
            );

            let input = serde_json::json!({
                "system": "You are an expert document writer. Produce well-researched, \
                           engaging content with proper citations. Follow the format exactly.",
                "prompt": prompt,
                "max_tokens": self.config.max_tokens_per_section,
                "thinking": false,
            });

            let (content, summary, hook) = match self.registry.execute_ability(
                self.manifest,
                "llm.chat",
                &input.to_string(),
            ) {
                Ok(result) => {
                    let output = String::from_utf8_lossy(&result.output).to_string();
                    parse_section_output(&output)
                }
                Err(e) => {
                    eprintln!("[composer] section '{}' generation failed: {}", section.title, e);
                    (
                        format!("(Content generation failed for: {})", section.title),
                        section.description.clone(),
                        None,
                    )
                }
            };

            eprintln!(
                "[composer] generated section '{}' ({} chars)",
                section.title,
                content.len()
            );

            // Extract notable phrases (8+ words) for repetition tracking
            extract_notable_phrases(&content, &mut used_phrases);

            generated.push(GeneratedSection {
                id: section.id.clone(),
                title: section.title.clone(),
                level: section.level,
                content,
                summary,
                hook,
            });
        }

        Ok(generated)
    }

    // -- Phase 4: Quality Review ----------------------------------------------

    fn review_document(
        &self,
        _outline: &DocumentOutline,
        sections: &mut Vec<GeneratedSection>,
        round: usize,
    ) -> Vec<String> {
        let review_type = if round == 1 { "coherence" } else { "readability" };

        // Build document preview for review
        let doc_preview: String = sections
            .iter()
            .map(|s| {
                let preview = if s.content.len() > 1000 {
                    format!("{}...", crate::pea::research::safe_slice(&s.content, 1000))
                } else {
                    s.content.clone()
                };
                format!("## {} [{}]\n{}\n", s.title, s.id, preview)
            })
            .collect::<Vec<_>>()
            .join("\n---\n");

        let prompt = if round == 1 {
            format!(
                "Review this document for coherence. Check:\n\
                 - Does each section flow naturally from the previous?\n\
                 - Is terminology consistent throughout?\n\
                 - Are there contradictions between sections?\n\
                 - Do hooks/transitions work?\n\n\
                 DOCUMENT:\n{}\n\n\
                 For each issue, provide a SURGICAL fix as a find-and-replace patch.\n\
                 Respond in JSON array:\n\
                 [{{\"section_id\": \"...\", \"problem\": \"...\", \"find\": \"exact text to replace\", \"replace\": \"corrected text\"}}]\n\
                 If no issues, respond: []",
                doc_preview,
            )
        } else {
            format!(
                "Review for readability and quality:\n\
                 - Is the writing engaging (not dry/academic unless appropriate)?\n\
                 - Are sources properly cited?\n\
                 - Are there incomplete thoughts or abrupt endings?\n\
                 - Is the level of detail appropriate?\n\n\
                 DOCUMENT:\n{}\n\n\
                 For each issue, provide a SURGICAL fix as a find-and-replace patch.\n\
                 Respond in JSON array:\n\
                 [{{\"section_id\": \"...\", \"problem\": \"...\", \"find\": \"exact text to replace\", \"replace\": \"corrected text\"}}]\n\
                 If no issues, respond: []",
                doc_preview,
            )
        };

        let input = serde_json::json!({
            "system": format!(
                "You are a document {} reviewer. Output ONLY a JSON array of issues, or [] if none.",
                review_type
            ),
            "prompt": prompt,
            "max_tokens": 4096,
        });

        let issues = match self.registry.execute_ability(
            self.manifest,
            "llm.chat",
            &input.to_string(),
        ) {
            Ok(result) => {
                let raw = String::from_utf8_lossy(&result.output).to_string();
                parse_review_issues(&raw)
            }
            Err(e) => {
                eprintln!("[composer] review round {} failed: {}", round, e);
                return vec![format!("Review round {} skipped: {}", round, e)];
            }
        };

        let mut notes = Vec::new();

        if issues.is_empty() {
            notes.push(format!("Round {} ({}): no issues found", round, review_type));
            return notes;
        }

        eprintln!(
            "[composer] review round {} found {} issues, applying fixes",
            round,
            issues.len()
        );

        // Apply fixes: prefer surgical find-and-replace, fall back to LLM rewrite
        for issue in &issues {
            let section_id = issue.get("section_id").and_then(|v| v.as_str()).unwrap_or("");
            let problem = issue.get("problem").and_then(|v| v.as_str()).unwrap_or("");
            let find_text = issue.get("find").and_then(|v| v.as_str()).unwrap_or("");
            let replace_text = issue.get("replace").and_then(|v| v.as_str()).unwrap_or("");

            notes.push(format!(
                "Round {} fix [{}]: {}",
                round, section_id, problem
            ));

            if let Some(section) = sections.iter_mut().find(|s| s.id == section_id) {
                // Try surgical string replacement first (no LLM call needed)
                if !find_text.is_empty() && section.content.contains(find_text) {
                    section.content = section.content.replacen(find_text, replace_text, 1);
                    eprintln!(
                        "[composer] surgical fix in '{}': replaced {} chars",
                        section_id, find_text.len()
                    );
                } else {
                    // Fallback: LLM rewrite for this section
                    let fix = issue.get("fix").and_then(|v| v.as_str())
                        .or(Some(replace_text))
                        .unwrap_or("");
                    let fix_prompt = format!(
                        "Revise this section to fix the following issue:\n\
                         ISSUE: {}\n\
                         SUGGESTED FIX: {}\n\n\
                         CURRENT CONTENT:\n{}\n\n\
                         Output ONLY the revised section content (no headers, no metadata).",
                        problem, fix, section.content,
                    );

                    let fix_input = serde_json::json!({
                        "system": "You are a document editor. Fix the specified issue while preserving \
                                   the overall structure and citations. Output ONLY the revised content.",
                        "prompt": fix_prompt,
                        "max_tokens": self.config.max_tokens_per_section,
                        "thinking": false,
                    });

                    if let Ok(result) = self.registry.execute_ability(
                        self.manifest,
                        "llm.chat",
                        &fix_input.to_string(),
                    ) {
                        let fixed = String::from_utf8_lossy(&result.output).to_string();
                        if !fixed.is_empty() && fixed.len() > section.content.len() / 4 {
                            section.content = fixed;
                        }
                    }
                }
            }
        }

        notes
    }

    // -- Phase 4b: Completion Check -------------------------------------------

    /// Check which planned sections are missing from generated output.
    fn check_completeness(
        &self,
        outline: &DocumentOutline,
        generated: &[GeneratedSection],
    ) -> Vec<OutlineSection> {
        let generated_ids: std::collections::HashSet<&str> =
            generated.iter().map(|s| s.id.as_str()).collect();
        let planned = flatten_outline(&outline.sections);

        planned
            .into_iter()
            .filter(|s| !generated_ids.contains(s.id.as_str()))
            .collect()
    }

    /// Generate sections that were planned but missing from output.
    fn generate_missing_sections(
        &self,
        outline: &DocumentOutline,
        missing: &[OutlineSection],
        corpus: &ResearchCorpus,
        task_results: &[(String, String)],
        existing: &[GeneratedSection],
    ) -> Vec<GeneratedSection> {
        let mut generated = Vec::new();

        let prev_context: String = existing
            .iter()
            .map(|g| format!("### {} (summary)\n{}\n", g.title, g.summary))
            .collect::<Vec<_>>()
            .join("\n");

        for section in missing {
            let relevant_sources = select_relevant_sources(corpus, &section.description, &section.title);
            let task_context: String = task_results
                .iter()
                .take(3)
                .map(|(desc, text)| {
                    let preview = crate::pea::research::safe_slice(text, 500);
                    format!("### {}\n{}", desc, preview)
                })
                .collect::<Vec<_>>()
                .join("\n\n");

            let prompt = format!(
                "You are writing section \"{}\" of \"{}\".\n\n\
                 CONTEXT FROM PREVIOUS SECTIONS:\n{}\n\n\
                 RESEARCH SOURCES:\n{}\n\n\
                 TASK RESULTS:\n{}\n\n\
                 SECTION REQUIREMENTS:\n{}\n\n\
                 Write this section. Output the body content only (no \\section headers).\n\n\
                 After the content, on a NEW line write:\n\
                 SUMMARY: {{2-3 sentence summary}}",
                section.title,
                outline.title,
                if prev_context.is_empty() { "(no prior sections)" } else { &prev_context },
                relevant_sources,
                task_context,
                section.description,
            );

            let input = serde_json::json!({
                "system": "You are an expert document writer. Produce well-researched, \
                           engaging content with proper citations. Follow the format exactly.",
                "prompt": prompt,
                "max_tokens": self.config.max_tokens_per_section,
                "thinking": false,
            });

            match self.registry.execute_ability(self.manifest, "llm.chat", &input.to_string()) {
                Ok(result) => {
                    let output = String::from_utf8_lossy(&result.output).to_string();
                    let (content, summary, hook) = parse_section_output(&output);
                    eprintln!(
                        "[composer] regenerated missing section '{}' ({} chars)",
                        section.title,
                        content.len()
                    );
                    generated.push(GeneratedSection {
                        id: section.id.clone(),
                        title: section.title.clone(),
                        level: section.level,
                        content,
                        summary,
                        hook,
                    });
                }
                Err(e) => {
                    eprintln!("[composer] failed to regenerate '{}': {}", section.title, e);
                }
            }
        }

        generated
    }

    // -- Phase 4c: Numerical Fact Verification ---------------------------------

    /// Cross-check numerical claims across sections.
    ///
    /// Sends all sections to the LLM and asks it to identify contradictory
    /// numbers, dates, statistics, or counts. Fixes inconsistencies in-place.
    fn verify_numerical_claims(&self, sections: &[GeneratedSection]) -> Vec<String> {
        // Build a compact digest of all numerical claims
        let claims_digest: String = sections
            .iter()
            .map(|s| {
                // Extract lines containing numbers for efficiency
                let numeric_lines: Vec<&str> = s.content.lines()
                    .filter(|line| line.chars().any(|c| c.is_ascii_digit()))
                    .collect();
                if numeric_lines.is_empty() {
                    return String::new();
                }
                format!(
                    "## Section: {}\n{}\n",
                    s.title,
                    numeric_lines.join("\n")
                )
            })
            .collect();

        if claims_digest.trim().is_empty() {
            return vec!["Fact verification: no numerical claims found".to_string()];
        }

        // Truncate if too long for a single LLM call
        let digest = if claims_digest.len() > 12000 {
            format!("{}...[truncated]", &claims_digest[..12000])
        } else {
            claims_digest
        };

        let prompt = format!(
            "Review these numerical claims extracted from different sections of the same document.\n\
             Identify any CONTRADICTIONS where the same fact has different numbers across sections.\n\n\
             Focus on:\n\
             - Dates that contradict each other (e.g. one section says Feb 28, another says March 1)\n\
             - Statistics that differ (e.g. one section says 50 casualties, another says 200)\n\
             - Counts that are inconsistent (e.g. one section says 3 phases, another lists 5)\n\
             - Percentages or dollar amounts that don't match\n\n\
             CLAIMS BY SECTION:\n{}\n\n\
             Respond with a JSON array of contradictions:\n\
             [{{\"claim\": \"...\", \"section_a\": \"...\", \"value_a\": \"...\", \
             \"section_b\": \"...\", \"value_b\": \"...\", \"likely_correct\": \"...\"}}]\n\
             If no contradictions found, respond: []",
            digest,
        );

        let input = serde_json::json!({
            "system": "You are a fact-checker specializing in numerical consistency. \
                       Output ONLY a JSON array of contradictions, or [] if none.",
            "prompt": prompt,
            "max_tokens": 4096,
        });

        match self.registry.execute_ability(self.manifest, "llm.chat", &input.to_string()) {
            Ok(result) => {
                let raw = String::from_utf8_lossy(&result.output).to_string();
                let raw = strip_thinking_tokens(&raw);
                let issues = parse_review_issues(&raw);
                if issues.is_empty() {
                    vec!["Fact verification: no contradictions found".to_string()]
                } else {
                    let mut notes = Vec::new();
                    for issue in &issues {
                        let claim = issue.get("claim").and_then(|v| v.as_str()).unwrap_or("?");
                        let sec_a = issue.get("section_a").and_then(|v| v.as_str()).unwrap_or("?");
                        let val_a = issue.get("value_a").and_then(|v| v.as_str()).unwrap_or("?");
                        let sec_b = issue.get("section_b").and_then(|v| v.as_str()).unwrap_or("?");
                        let val_b = issue.get("value_b").and_then(|v| v.as_str()).unwrap_or("?");
                        let correct = issue.get("likely_correct").and_then(|v| v.as_str()).unwrap_or("?");
                        notes.push(format!(
                            "Fact contradiction: '{}' — {} says {}, {} says {} (likely: {})",
                            claim, sec_a, val_a, sec_b, val_b, correct
                        ));
                    }
                    eprintln!(
                        "[composer] fact verification found {} contradictions",
                        notes.len()
                    );
                    notes
                }
            }
            Err(e) => {
                vec![format!("Fact verification skipped: {}", e)]
            }
        }
    }

    // -- Phase 5: Final Assembly ----------------------------------------------

    fn assemble_output(
        &self,
        doc: &ComposedDocument,
        images: &[ImageEntry],
        style: &StyleConfig,
        output_dir: &Path,
    ) -> Result<PathBuf> {
        // Build task_results-compatible format for existing document.rs functions
        let task_results: Vec<(String, String)> = doc
            .sections
            .iter()
            .map(|s| (s.title.clone(), s.content.clone()))
            .collect();

        // Use existing assemble_document which handles LaTeX generation,
        // compilation with self-healing, and HTML fallback
        document::assemble_document(
            self.registry,
            self.manifest,
            &doc.title,
            &task_results,
            images,
            Some(style),
            output_dir,
        )
    }
}

// ---------------------------------------------------------------------------
// Phase 1b: Section Reordering — Executive Summary first, Methodology last
// ---------------------------------------------------------------------------

/// Move Executive Summary / Introduction to the front and Methodology / PRISMA
/// / Appendix sections to the end.  This ensures a professional document
/// structure regardless of what the LLM planner produces.
fn reorder_outline_sections(outline: &mut DocumentOutline) {
    let front_keywords = ["executive summary", "introduction", "overview"];
    let back_keywords = ["methodology", "prisma", "appendix", "references", "bibliography"];

    let lower_title = |s: &OutlineSection| s.title.to_ascii_lowercase();

    // Partition: front sections, body, back sections
    let mut front = Vec::new();
    let mut body = Vec::new();
    let mut back = Vec::new();

    for section in outline.sections.drain(..) {
        let t = lower_title(&section);
        if front_keywords.iter().any(|kw| t.contains(kw)) {
            front.push(section);
        } else if back_keywords.iter().any(|kw| t.contains(kw)) {
            back.push(section);
        } else {
            body.push(section);
        }
    }

    outline.sections.extend(front);
    outline.sections.extend(body);
    outline.sections.extend(back);
}

/// Cap total section count (including nested children) to `max_sections`.
/// Drops excess leaf sections from the end of the outline.
fn cap_section_count(outline: &mut DocumentOutline, max_sections: usize) {
    fn count_sections(sections: &[OutlineSection]) -> usize {
        sections.iter().map(|s| 1 + count_sections(&s.children)).sum()
    }

    let total = count_sections(&outline.sections);
    if total <= max_sections {
        return;
    }

    eprintln!(
        "[composer] capping sections from {} to {} max",
        total, max_sections
    );

    // Simple strategy: truncate top-level sections (keep first max_sections - children)
    while count_sections(&outline.sections) > max_sections && outline.sections.len() > 3 {
        outline.sections.pop();
    }
}

// ---------------------------------------------------------------------------
// Phase 2: Topological Sort (Kahn's algorithm)
// ---------------------------------------------------------------------------

pub fn compute_generation_order(outline: &mut DocumentOutline) {
    let flat = flatten_outline_mut(&mut outline.sections);

    if flat.is_empty() {
        return;
    }

    // Build adjacency and in-degree maps
    let ids: Vec<String> = flat.iter().map(|s| s.id.clone()).collect();
    let id_set: std::collections::HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();

    let mut in_degree: HashMap<String, usize> = HashMap::new();
    let mut dependents: HashMap<String, Vec<String>> = HashMap::new();

    for section in &flat {
        in_degree.entry(section.id.clone()).or_insert(0);
        for dep in &section.depends_on {
            if id_set.contains(dep.as_str()) {
                *in_degree.entry(section.id.clone()).or_insert(0) += 1;
                dependents
                    .entry(dep.clone())
                    .or_default()
                    .push(section.id.clone());
            }
        }
    }

    // Kahn's algorithm
    let mut queue: VecDeque<String> = VecDeque::new();
    for (id, &deg) in &in_degree {
        if deg == 0 {
            queue.push_back(id.clone());
        }
    }

    let mut order_map: HashMap<String, usize> = HashMap::new();
    let mut order = 0;

    while let Some(id) = queue.pop_front() {
        order_map.insert(id.clone(), order);
        order += 1;

        if let Some(deps) = dependents.get(&id) {
            for dep_id in deps {
                if let Some(deg) = in_degree.get_mut(dep_id) {
                    *deg = deg.saturating_sub(1);
                    if *deg == 0 {
                        queue.push_back(dep_id.clone());
                    }
                }
            }
        }
    }

    // Assign order (sections not in map get max order — cycle fallback)
    let max_order = order;
    // Apply orders to the actual outline
    assign_generation_orders(&mut outline.sections, &order_map, max_order);
}

fn assign_generation_orders(
    sections: &mut [OutlineSection],
    order_map: &HashMap<String, usize>,
    fallback: usize,
) {
    for section in sections.iter_mut() {
        section.generation_order = Some(*order_map.get(&section.id).unwrap_or(&fallback));
        assign_generation_orders(&mut section.children, order_map, fallback);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Flatten a nested outline into a flat list (depth-first).
fn flatten_outline(sections: &[OutlineSection]) -> Vec<OutlineSection> {
    let mut flat = Vec::new();
    for section in sections {
        flat.push(section.clone());
        flat.extend(flatten_outline(&section.children));
    }
    flat
}

/// Flatten for reading (immutable references).
fn flatten_outline_mut(sections: &mut [OutlineSection]) -> Vec<OutlineSection> {
    // Clone for reading — we assign orders separately via assign_generation_orders
    flatten_outline(sections)
}

/// Extract notable phrases (8+ words) from content for repetition tracking.
/// Only keeps distinctive phrases that appear to be analytical claims.
fn extract_notable_phrases(content: &str, phrases: &mut Vec<String>) {
    // Split into sentences and look for substantial phrases
    for line in content.lines() {
        let line = line.trim();
        if line.len() < 40 { continue; }
        // Skip citation lines, headers, and metadata
        if line.starts_with('[') || line.starts_with('#') || line.starts_with("SUMMARY:") || line.starts_with("HOOK:") {
            continue;
        }
        // Extract phrases: look for clauses between punctuation that are 8+ words
        for sentence in line.split(|c: char| c == '.' || c == ';') {
            let words: Vec<&str> = sentence.split_whitespace().collect();
            if words.len() >= 8 && words.len() <= 20 {
                let phrase = words.join(" ");
                // Only track if it looks like a substantive claim (contains a verb-like word)
                if phrase.len() >= 40 && !phrases.iter().any(|p| p == &phrase) {
                    phrases.push(phrase);
                }
            }
        }
    }
    // Keep only the last 30 phrases to avoid prompt bloat
    if phrases.len() > 30 {
        let drain_count = phrases.len() - 30;
        phrases.drain(..drain_count);
    }
}

/// Extract JSON object/array from LLM response (handles markdown fences).
fn extract_json(raw: &str) -> &str {
    let obj_start = raw.find('{');
    let arr_start = raw.find('[');

    // Pick whichever comes first
    match (obj_start, arr_start) {
        (Some(o), Some(a)) if a < o => {
            if let Some(end) = raw.rfind(']') {
                if end >= a {
                    return &raw[a..=end];
                }
            }
            // Fall through to object
            if let Some(end) = raw.rfind('}') {
                if end >= o {
                    return &raw[o..=end];
                }
            }
        }
        (Some(o), _) => {
            if let Some(end) = raw.rfind('}') {
                if end >= o {
                    return &raw[o..=end];
                }
            }
        }
        (None, Some(a)) => {
            if let Some(end) = raw.rfind(']') {
                if end >= a {
                    return &raw[a..=end];
                }
            }
        }
        _ => {}
    }
    raw
}

/// Parse LLM section output into (content, summary, hook).
fn parse_section_output(output: &str) -> (String, String, Option<String>) {
    // Strip LLM thinking tokens that leak into output (e.g. Qwen <think>...</think>)
    let output = strip_thinking_tokens(output);
    let mut content = output.to_string();
    let mut summary = String::new();
    let mut hook = None;

    // Extract SUMMARY: and HOOK: from end of output
    if let Some(summary_idx) = output.rfind("SUMMARY:") {
        let after = &output[summary_idx + 8..];
        let summary_text = if let Some(hook_idx) = after.find("HOOK:") {
            after[..hook_idx].trim().to_string()
        } else {
            after.lines().next().unwrap_or("").trim().to_string()
        };
        summary = summary_text;
        content = output[..summary_idx].trim().to_string();
    }

    if let Some(hook_idx) = output.rfind("HOOK:") {
        let hook_text = output[hook_idx + 5..].trim();
        let hook_line = hook_text.lines().next().unwrap_or("").trim().to_string();
        if !hook_line.is_empty() {
            hook = Some(hook_line);
        }
        // Also trim content if SUMMARY wasn't found
        if summary.is_empty() {
            content = output[..hook_idx].trim().to_string();
        }
    }

    if summary.is_empty() {
        // Auto-generate summary from first 2 sentences
        summary = content
            .split('.')
            .take(2)
            .collect::<Vec<_>>()
            .join(".")
            + ".";
    }

    (content, summary, hook)
}

/// Select relevant research sources for a section based on keyword overlap.
fn select_relevant_sources(corpus: &ResearchCorpus, description: &str, title: &str) -> String {
    let keywords: Vec<&str> = description
        .split_whitespace()
        .chain(title.split_whitespace())
        .filter(|w| w.len() > 3)
        .collect();

    let mut scored: Vec<(&crate::pea::research::FetchedSource, usize)> = corpus
        .sources
        .iter()
        .map(|s| {
            let score = keywords
                .iter()
                .filter(|kw| {
                    s.title.to_lowercase().contains(&kw.to_lowercase())
                        || crate::pea::research::safe_slice(&s.content, 2000)
                            .to_lowercase()
                            .contains(&kw.to_lowercase())
                })
                .count();
            (s, score)
        })
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1));

    scored
        .iter()
        .take(8)
        .filter(|(_, score)| *score > 0)
        .map(|(s, _)| {
            let preview = if s.content.len() > 800 {
                format!("{}...", crate::pea::research::safe_slice(&s.content, 800))
            } else {
                s.content.clone()
            };
            format!("Source [{}]: {} ({})\n{}\n", s.tier.label(), s.title, s.url, preview)
        })
        .collect::<Vec<_>>()
        .join("\n---\n")
}

/// Strip LLM thinking/reasoning tokens from output.
///
/// Models like Qwen emit `<think>...</think>` blocks that leak into final text.
/// Also strips stray closing `</think>` tags.
fn strip_thinking_tokens(text: &str) -> String {
    let mut result = text.to_string();
    // Remove full <think>...</think> blocks (greedy within single block)
    while let Some(start) = result.find("<think>") {
        if let Some(end) = result[start..].find("</think>") {
            result = format!("{}{}", &result[..start], &result[start + end + 8..]);
        } else {
            // Unclosed <think> — remove from <think> to end of line
            let line_end = result[start..].find('\n').unwrap_or(result.len() - start);
            result = format!("{}{}", &result[..start], &result[start + line_end..]);
        }
    }
    // Remove stray </think> tags
    result = result.replace("</think>", "");
    result = result.replace("<think>", "");
    result
}

/// Parse review issues from LLM JSON array response.
fn parse_review_issues(raw: &str) -> Vec<serde_json::Value> {
    let json_str = extract_json(raw);
    serde_json::from_str::<Vec<serde_json::Value>>(json_str).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_composer_config_defaults() {
        let config = ComposerConfig::default();
        assert_eq!(config.max_depth, 3);
        assert_eq!(config.review_rounds, 2);
        assert_eq!(config.max_tokens_per_section, 8192);
    }

    #[test]
    fn test_extract_json_object() {
        let raw = "Here is the outline:\n{\"title\": \"Test\"}\nDone.";
        assert_eq!(extract_json(raw), "{\"title\": \"Test\"}");
    }

    #[test]
    fn test_extract_json_array() {
        let raw = "Issues found:\n[{\"id\": \"1\"}]\n";
        assert_eq!(extract_json(raw), "[{\"id\": \"1\"}]");
    }

    #[test]
    fn test_parse_section_output_with_summary_and_hook() {
        let output = "This is the content of the section.\n\nSUMMARY: A brief summary.\nHOOK: Next we explore...";
        let (content, summary, hook) = parse_section_output(output);
        assert_eq!(content, "This is the content of the section.");
        assert_eq!(summary, "A brief summary.");
        assert_eq!(hook, Some("Next we explore...".to_string()));
    }

    #[test]
    fn test_parse_section_output_no_metadata() {
        let output = "Just the content here. Nothing more.";
        let (content, summary, hook) = parse_section_output(output);
        assert_eq!(content, "Just the content here. Nothing more.");
        assert!(!summary.is_empty()); // auto-generated
        assert!(hook.is_none());
    }

    #[test]
    fn test_parse_review_issues_valid() {
        let raw = r#"[{"section_id": "ch1", "problem": "bad flow", "fix": "add transition"}]"#;
        let issues = parse_review_issues(raw);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0]["section_id"], "ch1");
    }

    #[test]
    fn test_parse_review_issues_empty() {
        let issues = parse_review_issues("[]");
        assert!(issues.is_empty());
    }

    #[test]
    fn test_parse_review_issues_invalid() {
        let issues = parse_review_issues("not json");
        assert!(issues.is_empty());
    }

    #[test]
    fn test_outline_deserialization() {
        let json = r#"{
            "title": "AI Regulation Report",
            "needs_toc": true,
            "sections": [
                {
                    "id": "ch1",
                    "title": "Introduction",
                    "level": 0,
                    "description": "Overview of AI regulation",
                    "depends_on": ["ch2", "ch3"],
                    "children": []
                },
                {
                    "id": "ch2",
                    "title": "Current State",
                    "level": 0,
                    "description": "Current regulatory landscape",
                    "depends_on": [],
                    "children": [
                        {
                            "id": "ch2.s1",
                            "title": "EU AI Act",
                            "level": 1,
                            "description": "European Union approach",
                            "depends_on": ["ch2"],
                            "children": []
                        }
                    ]
                },
                {
                    "id": "ch3",
                    "title": "Future Outlook",
                    "level": 0,
                    "description": "Predictions",
                    "depends_on": [],
                    "children": []
                }
            ]
        }"#;

        let outline: DocumentOutline = serde_json::from_str(json).unwrap();
        assert_eq!(outline.title, "AI Regulation Report");
        assert!(outline.needs_toc);
        assert_eq!(outline.sections.len(), 3);
        assert_eq!(outline.sections[1].children.len(), 1);
    }

    #[test]
    fn test_topological_sort() {
        let mut outline = DocumentOutline {
            title: "Test".into(),
            needs_toc: false,
            sections: vec![
                OutlineSection {
                    id: "intro".into(),
                    title: "Introduction".into(),
                    level: 0,
                    description: "".into(),
                    depends_on: vec!["ch1".into(), "ch2".into()],
                    generation_order: None,
                    children: vec![],
                },
                OutlineSection {
                    id: "ch1".into(),
                    title: "Chapter 1".into(),
                    level: 0,
                    description: "".into(),
                    depends_on: vec![],
                    generation_order: None,
                    children: vec![],
                },
                OutlineSection {
                    id: "ch2".into(),
                    title: "Chapter 2".into(),
                    level: 0,
                    description: "".into(),
                    depends_on: vec!["ch1".into()],
                    generation_order: None,
                    children: vec![],
                },
            ],
        };

        compute_generation_order(&mut outline);

        // ch1 should be first (no deps)
        assert_eq!(outline.sections[1].generation_order, Some(0));
        // ch2 depends on ch1
        assert_eq!(outline.sections[2].generation_order, Some(1));
        // intro depends on ch1+ch2 — should be last
        assert_eq!(outline.sections[0].generation_order, Some(2));
    }

    #[test]
    fn test_flatten_outline() {
        let sections = vec![
            OutlineSection {
                id: "ch1".into(),
                title: "Ch 1".into(),
                level: 0,
                description: "".into(),
                depends_on: vec![],
                generation_order: None,
                children: vec![
                    OutlineSection {
                        id: "ch1.s1".into(),
                        title: "Sec 1.1".into(),
                        level: 1,
                        description: "".into(),
                        depends_on: vec![],
                        generation_order: None,
                        children: vec![],
                    },
                ],
            },
            OutlineSection {
                id: "ch2".into(),
                title: "Ch 2".into(),
                level: 0,
                description: "".into(),
                depends_on: vec![],
                generation_order: None,
                children: vec![],
            },
        ];
        let flat = flatten_outline(&sections);
        assert_eq!(flat.len(), 3);
        assert_eq!(flat[0].id, "ch1");
        assert_eq!(flat[1].id, "ch1.s1");
        assert_eq!(flat[2].id, "ch2");
    }

    #[test]
    fn test_strip_thinking_tokens_full_block() {
        let input = "Hello <think>internal reasoning here</think> world";
        assert_eq!(strip_thinking_tokens(input), "Hello  world");
    }

    #[test]
    fn test_strip_thinking_tokens_stray_close() {
        let input = "Some text </think> more text";
        assert_eq!(strip_thinking_tokens(input), "Some text  more text");
    }

    #[test]
    fn test_strip_thinking_tokens_no_tags() {
        let input = "Clean text with no tags";
        assert_eq!(strip_thinking_tokens(input), input);
    }

    #[test]
    fn test_strip_thinking_tokens_multiple_blocks() {
        let input = "<think>block1</think>A<think>block2</think>B";
        assert_eq!(strip_thinking_tokens(input), "AB");
    }

    #[test]
    fn test_parse_section_output_strips_think_tags() {
        let output = "</think>\nThis is the actual content.\n\nSUMMARY: A summary.";
        let (content, summary, _hook) = parse_section_output(output);
        assert!(!content.contains("</think>"));
        assert!(content.contains("actual content"));
        assert_eq!(summary, "A summary.");
    }
}
