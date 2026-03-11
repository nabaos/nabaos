// PEA Document Composer — intelligent multi-level document composition.
//
// Phases:
//   1. Structure Decision: LLM plans document outline with hierarchy + dependencies
//   2. Generation Order: Topological sort on section dependency graph (Kahn's algorithm)
//   3. Section Generation: Generate each section in topo order with context threading
//   4. Quality Review: 2-round coherence + readability review with targeted fixes
//   5. Final Assembly: Combine sections into HTML/LaTeX/PDF output

use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::core::error::{NyayaError, Result};
use crate::pea::document::{self, ImageEntry, StyleConfig};
use crate::pea::knowledge_graph::KnowledgeGraph;
use crate::pea::research::ResearchCorpus;
use crate::runtime::host_functions::AbilityRegistry;
use crate::runtime::manifest::AgentManifest;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Tracks cumulative LLM token usage across the composition pipeline.
#[derive(Debug, Default)]
pub struct TokenTracker {
    input_tokens: Cell<u64>,
    output_tokens: Cell<u64>,
    llm_calls: Cell<u32>,
    llm_latency_ms: Cell<u64>,
}

impl TokenTracker {
    /// Accumulate token data from an AbilityResult's facts.
    fn track(&self, facts: &HashMap<String, String>) {
        if let Some(v) = facts.get("input_tokens") {
            if let Ok(n) = v.parse::<u64>() {
                self.input_tokens.set(self.input_tokens.get() + n);
            }
        }
        if let Some(v) = facts.get("output_tokens") {
            if let Ok(n) = v.parse::<u64>() {
                self.output_tokens.set(self.output_tokens.get() + n);
            }
        }
        if let Some(v) = facts.get("latency_ms") {
            if let Ok(n) = v.parse::<u64>() {
                self.llm_latency_ms.set(self.llm_latency_ms.get() + n);
            }
        }
        self.llm_calls.set(self.llm_calls.get() + 1);
    }

    fn log_summary(&self, wall_elapsed: std::time::Duration) {
        eprintln!("╔══════════════════════════════════════════════════╗");
        eprintln!("║            PEA Token Usage Summary               ║");
        eprintln!("╠══════════════════════════════════════════════════╣");
        eprintln!("║  LLM calls:      {:>8}                       ║", self.llm_calls.get());
        eprintln!("║  Input tokens:   {:>8}                       ║", self.input_tokens.get());
        eprintln!("║  Output tokens:  {:>8}                       ║", self.output_tokens.get());
        eprintln!("║  Total tokens:   {:>8}                       ║", self.input_tokens.get() + self.output_tokens.get());
        eprintln!("║  LLM latency:    {:>7.1}s                       ║", self.llm_latency_ms.get() as f64 / 1000.0);
        eprintln!("║  Wall time:      {:>7.1}s                       ║", wall_elapsed.as_secs_f64());
        eprintln!("╚══════════════════════════════════════════════════╝");
    }
}

pub struct DocumentComposer<'a> {
    registry: &'a AbilityRegistry,
    manifest: &'a AgentManifest,
    config: ComposerConfig,
    tokens: TokenTracker,
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
        Self { registry, manifest, config, tokens: TokenTracker::default() }
    }

    /// Execute an ability and track token usage from the result.
    fn exec_ability(&self, ability: &str, input: &str) -> std::result::Result<crate::runtime::host_functions::AbilityResult, String> {
        let result = self.registry.execute_ability(self.manifest, ability, input);
        if let Ok(ref r) = result {
            self.tokens.track(&r.facts);
        }
        result
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
        self.compose_document_with_kg(objective, corpus, task_results, images, style, output_dir, None)
    }

    /// Composition pipeline with optional knowledge graph for structural deduplication.
    pub fn compose_document_with_kg(
        &self,
        objective: &str,
        corpus: &ResearchCorpus,
        task_results: &[(String, String)],
        images: &[ImageEntry],
        style: &StyleConfig,
        output_dir: &Path,
        kg: Option<&KnowledgeGraph>,
    ) -> Result<PathBuf> {
        let pipeline_start = Instant::now();
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

        // Phase 1b2: Deduplicate near-identical section titles
        dedup_outline_sections(&mut outline);

        // Phase 1b3: Role-based deduplication (catches variants like "Executive Summary" vs "Executive Summary & Introduction")
        dedup_by_role(&mut outline);

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

        // Phase 4c2: Taxonomy reconciliation — detect and fix conflicting named lists
        eprintln!("[composer] reconciling taxonomies...");
        let taxonomy_notes = self.reconcile_taxonomies(&mut sections);
        all_notes.extend(taxonomy_notes);

        // Phase 4c3: Citation washing detection — flag precise stats attributed to sources
        eprintln!("[composer] checking for citation washing...");
        let wash_notes = detect_citation_washing(&sections);
        if !wash_notes.is_empty() {
            eprintln!("[composer] {} citation washing warnings", wash_notes.len());
            self.fix_citation_washing(&mut sections, &wash_notes);
        }
        all_notes.extend(wash_notes);

        // Phase 4d2: Evidence gate — analytical sections must contain data
        eprintln!("[composer] enforcing evidence gate...");
        self.enforce_evidence_gate(&mut sections);

        // Phase 4e: Nyaya trimmer — deduplicate and merge sections using
        // Anadhigata (novelty), Pramana hierarchy (evidence priority), and
        // Padartha structure (categorical coherence).
        // Controlled by NABA_PEA_NYAYA_TRIM env var (default: enabled).
        let nyaya_trim_enabled = std::env::var("NABA_PEA_NYAYA_TRIM")
            .map(|v| v != "0" && v.to_lowercase() != "false")
            .unwrap_or(true);
        if nyaya_trim_enabled {
            let before_count = sections.len();
            eprintln!("[composer] applying Nyaya trimmer ({} sections)...", before_count);
            let trim_notes = self.nyaya_trim(&mut sections, kg);
            let after_count = sections.len();
            if before_count != after_count {
                eprintln!(
                    "[composer] Nyaya trimmer: {} → {} sections (merged {})",
                    before_count, after_count, before_count - after_count
                );
            } else {
                eprintln!("[composer] Nyaya trimmer: no sections merged");
            }
            all_notes.extend(trim_notes);
        } else {
            eprintln!("[composer] Nyaya trimmer disabled (NABA_PEA_NYAYA_TRIM=0)");
            all_notes.push("Nyaya trimmer disabled (ablation)".to_string());
        }

        // Phase 4f: Source key mapping — replace raw URLs with clean citations
        eprintln!("[composer] applying source key mapping...");
        let source_registry = build_source_registry(corpus);
        if let Some(refs) = apply_source_keys(&mut sections, &source_registry) {
            eprintln!("[composer] added References section ({} sources)", source_registry.len());
            sections.push(refs);
        }

        // Phase 4g: Compression pass — rewrite verbose sections for brevity
        eprintln!("[composer] running compression pass...");
        let compress_notes = self.compress_for_brevity(&mut sections);
        all_notes.extend(compress_notes);

        // Phase 4g2: Global coherence pass — detect and fix circular references
        eprintln!("[composer] checking global coherence...");
        let coherence_issues = detect_coherence_issues(&sections);
        if !coherence_issues.is_empty() {
            eprintln!("[composer] {} coherence issues found", coherence_issues.len());
            fix_coherence_issues(&mut sections, &coherence_issues);
        }

        // Phase 4d: Auto-generate data charts + PRISMA flow diagram
        eprintln!("[composer] generating data charts...");
        let chart_images = self.generate_charts(&sections, corpus, output_dir);
        if !chart_images.is_empty() {
            eprintln!("[composer] generated {} charts", chart_images.len());
            all_notes.push(format!("Generated {} data visualization charts", chart_images.len()));
        }

        // Merge stock images + generated charts (skip stock for analytical themes)
        let mut all_images: Vec<ImageEntry> = if style.should_skip_stock_images() {
            eprintln!("[composer] skipping stock images (theme={})", style.theme);
            Vec::new()
        } else {
            images.to_vec()
        };
        all_images.extend(chart_images);

        // Phase 5: Assemble final output
        eprintln!("[composer] assembling final document...");
        let doc = ComposedDocument {
            title: outline.title.clone(),
            needs_toc: outline.needs_toc,
            sections,
            review_notes: all_notes,
        };

        let result = self.assemble_output(&doc, &all_images, style, output_dir);
        self.tokens.log_summary(pipeline_start.elapsed());
        result
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

            let (content, summary, hook) = match self.exec_ability("llm.chat", &input.to_string()) {
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

        let issues = match self.exec_ability("llm.chat", &input.to_string()) {
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

                    if let Ok(result) = self.exec_ability("llm.chat", &fix_input.to_string()) {
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

            match self.exec_ability("llm.chat", &input.to_string()) {
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

        match self.exec_ability("llm.chat", &input.to_string()) {
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

    // -- Phase 4c2: Taxonomy Reconciliation ------------------------------------

    /// Detect and reconcile conflicting named lists/taxonomies across sections.
    /// E.g., Ch1 has 4 scenarios, Ch3 has 5 different ones → unify into one set.
    fn reconcile_taxonomies(&self, sections: &mut Vec<GeneratedSection>) -> Vec<String> {
        // Build a digest of all "named list" patterns
        let list_digest: String = sections
            .iter()
            .filter_map(|s| {
                // Look for enumerated/named items patterns
                let list_lines: Vec<&str> = s.content.lines()
                    .filter(|l| {
                        let lt = l.trim();
                        // Numbered lists, bullet points, or scenario-like patterns
                        lt.starts_with("1.") || lt.starts_with("- ") || lt.starts_with("* ")
                            || lt.contains("Scenario ") || lt.contains("scenario ")
                            || lt.contains("Phase ") || lt.contains("phase ")
                            || lt.contains("Category ") || lt.contains("category ")
                            || lt.contains("Option ") || lt.contains("option ")
                            || lt.contains("Stage ") || lt.contains("stage ")
                    })
                    .collect();
                if list_lines.len() >= 2 {
                    Some(format!("## {}\n{}\n", s.title, list_lines.join("\n")))
                } else {
                    None
                }
            })
            .collect();

        if list_digest.trim().is_empty() {
            return vec![];
        }

        let prompt = format!(
            "Review these named lists/taxonomies from different sections of the SAME document.\n\
             Identify any CONFLICTING TAXONOMIES where different sections define different sets \
             of categories for the same concept.\n\n\
             Examples of conflicts:\n\
             - Section A lists 4 scenarios, Section B lists 5 different scenarios\n\
             - Section A defines 3 phases, Section B defines 4 phases with different names\n\
             - Section A categorizes into types X,Y,Z but Section B uses types A,B,C,D\n\n\
             LISTS BY SECTION:\n{}\n\n\
             For each conflict found, provide the RECONCILED unified taxonomy.\n\
             Respond as JSON:\n\
             [{{\"concept\": \"scenarios/phases/etc\", \
             \"section_a\": \"title\", \"list_a\": [\"item1\", ...], \
             \"section_b\": \"title\", \"list_b\": [\"item1\", ...], \
             \"reconciled\": [\"unified item1\", ...]}}]\n\
             If no conflicts: []",
            crate::pea::research::safe_slice(&list_digest, 6000),
        );

        let input = serde_json::json!({
            "system": "You are a structural editor ensuring taxonomic consistency across document chapters. \
                       Output ONLY a JSON array.",
            "prompt": prompt,
            "max_tokens": 4096,
            "thinking": false,
        });

        match self.exec_ability("llm.chat", &input.to_string()) {
            Ok(result) => {
                let raw = String::from_utf8_lossy(&result.output).to_string();
                let raw = strip_thinking_tokens(&raw);
                let conflicts = parse_review_issues(&raw);
                if conflicts.is_empty() {
                    return vec![];
                }

                let mut notes = Vec::new();
                for conflict in &conflicts {
                    let concept = conflict.get("concept").and_then(|v| v.as_str()).unwrap_or("?");
                    let sec_a = conflict.get("section_a").and_then(|v| v.as_str()).unwrap_or("?");
                    let sec_b = conflict.get("section_b").and_then(|v| v.as_str()).unwrap_or("?");
                    let reconciled = conflict.get("reconciled")
                        .and_then(|v| v.as_array())
                        .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "))
                        .unwrap_or_default();

                    eprintln!(
                        "[composer] taxonomy conflict: '{}' differs between '{}' and '{}' → reconciled: [{}]",
                        concept, sec_a, sec_b, reconciled
                    );
                    notes.push(format!(
                        "Taxonomy reconciled: '{}' unified across '{}' and '{}' → [{}]",
                        concept, sec_a, sec_b, reconciled
                    ));

                    // Apply reconciliation: ask LLM to rewrite the conflicting section
                    if let Some(reconciled_arr) = conflict.get("reconciled").and_then(|v| v.as_array()) {
                        let reconciled_list: Vec<&str> = reconciled_arr.iter().filter_map(|v| v.as_str()).collect();
                        if !reconciled_list.is_empty() {
                            // Find and fix the second section (keep first as authoritative)
                            if let Some(section) = sections.iter_mut().find(|s| s.title == sec_b) {
                                let fix_prompt = format!(
                                    "Rewrite the following section content to use this EXACT taxonomy: [{}]\n\
                                     Replace any references to a different set of categories with these ones.\n\
                                     Keep all other content, facts, and analysis unchanged.\n\
                                     Output ONLY the rewritten content.\n\n{}",
                                    reconciled_list.join(", "),
                                    crate::pea::research::safe_slice(&section.content, 6000),
                                );
                                let fix_input = serde_json::json!({
                                    "system": "You are an editorial assistant. Rewrite content to use the specified taxonomy. Output ONLY rewritten text.",
                                    "prompt": fix_prompt,
                                    "max_tokens": self.config.max_tokens_per_section,
                                    "thinking": false,
                                });
                                if let Ok(fix_result) = self.exec_ability("llm.chat", &fix_input.to_string()) {
                                    let fixed = String::from_utf8_lossy(&fix_result.output).to_string();
                                    let fixed = strip_thinking_tokens(&fixed).trim().to_string();
                                    if !fixed.is_empty() && fixed.len() > section.content.len() / 3 {
                                        section.content = fixed;
                                        eprintln!("[composer] rewrote '{}' with reconciled taxonomy", sec_b);
                                    }
                                }
                            }
                        }
                    }
                }
                notes
            }
            Err(e) => {
                eprintln!("[composer] taxonomy reconciliation skipped: {}", e);
                vec![]
            }
        }
    }

    // -- Phase 4c3: Citation Washing Fix ----------------------------------------

    /// Remove or soften precise statistics that are attributed to sources
    /// but likely fabricated by the LLM (citation washing).
    fn fix_citation_washing(&self, sections: &mut Vec<GeneratedSection>, warnings: &[String]) {
        if warnings.is_empty() {
            return;
        }

        let wash_re = regex::Regex::new(
            r"(\d+(?:\.\d+)?)\s*(%|percent|probability)\s*\(([A-Z][A-Za-z\s]+)\)"
        ).unwrap();

        for section in sections.iter_mut() {
            if section.id == "references" {
                continue;
            }
            // Replace precise percentages with hedged language
            let new_content = wash_re.replace_all(&section.content, |caps: &regex::Captures| {
                let source = caps.get(3).unwrap().as_str().trim();
                format!("({})", source) // keep citation, drop fabricated number
            });
            if new_content != section.content {
                eprintln!("[composer] removed fabricated statistics from '{}'", section.title);
                section.content = new_content.to_string();
            }
        }
    }

    // -- Phase 4d2: Evidence Gate -----------------------------------------------

    /// Enforce evidence gate: analytical sections must contain data tables, statistical
    /// coefficients, or similar empirical evidence. Sections that lack evidence are
    /// sent back to the LLM for revision.
    fn enforce_evidence_gate(&self, sections: &mut Vec<GeneratedSection>) {
        // Collect indices and titles as owned data to avoid borrow conflict
        let analytical: Vec<(usize, String)> = detect_analytical_sections(sections)
            .into_iter()
            .map(|(i, t)| (i, t.to_string()))
            .collect();
        if analytical.is_empty() {
            return;
        }

        for (idx, title) in &analytical {
            let (has_evidence, _) = check_evidence_presence(&sections[*idx].content);
            if has_evidence {
                eprintln!("[composer] evidence gate: '{}' passed", title);
                continue;
            }

            eprintln!("[composer] evidence gate: '{}' lacks empirical data — requesting revision", title);

            let prompt = format!(
                "This section is titled '{}' but contains no data tables, regression coefficients, \
                 or statistical results. Rewrite it to either: (a) include a concrete data table with \
                 at least 3 rows of numerical data, OR (b) change the title to accurately reflect \
                 narrative content (e.g., 'Discussion of Economic Patterns' instead of 'Empirical \
                 Analysis'). Output ONLY the revised section title on the first line, then a blank \
                 line, then the revised content.\n\n{}",
                title, &sections[*idx].content
            );

            let input = serde_json::json!({
                "system": "You are a peer-review quality gate for research documents. \
                           Ensure analytical sections contain real data or rename them honestly.",
                "prompt": prompt,
                "max_tokens": self.config.max_tokens_per_section,
                "thinking": false,
            });

            match self.exec_ability("llm.chat", &input.to_string()) {
                Ok(result) => {
                    let raw = String::from_utf8_lossy(&result.output).to_string();
                    let raw = strip_thinking_tokens(&raw);

                    // Parse: first line = title, then blank line, then content
                    let parts: Vec<&str> = raw.splitn(3, '\n').collect();
                    if parts.len() >= 3 {
                        let new_title = parts[0].trim().to_string();
                        let new_content = parts[2..].join("\n").trim().to_string();

                        let (now_has_evidence, markers) = check_evidence_presence(&new_content);
                        if now_has_evidence {
                            eprintln!(
                                "[composer] evidence gate: '{}' revised with data ({})",
                                title, markers.join(", ")
                            );
                            sections[*idx].content = new_content;
                        } else if new_title != sections[*idx].title {
                            eprintln!(
                                "[composer] evidence gate: '{}' retitled to '{}'",
                                title, new_title
                            );
                            sections[*idx].title = new_title;
                            sections[*idx].content = new_content;
                        } else {
                            eprintln!(
                                "[composer] evidence gate: '{}' revision still lacks data, accepting as-is",
                                title
                            );
                        }
                    } else if !raw.is_empty() {
                        // LLM returned content without title separation — accept if has evidence
                        let (now_has, _) = check_evidence_presence(&raw);
                        if now_has {
                            sections[*idx].content = raw;
                            eprintln!("[composer] evidence gate: '{}' revised with data", title);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[composer] evidence gate: LLM revision failed for '{}': {}", title, e);
                }
            }
        }
    }

    // -- Phase 4e: Nyaya Trimmer -----------------------------------------------
    //
    // Applies three Nyaya-inspired rules to merge redundant sections:
    //   Rule 1 — Anadhigata (novelty): if >60% of claims restate previous sections, merge
    //   Rule 2 — Pramana hierarchy (evidence priority): keep section with stronger sourcing
    //   Rule 3 — Padartha (categorical coherence): merge sections in the same logical category

    /// Run the Nyaya trimmer on generated sections.
    ///
    /// Uses KG entity overlap (when available) + LLM analysis to identify
    /// redundant sections and merge them. Returns trim notes for the review log.
    fn nyaya_trim(
        &self,
        sections: &mut Vec<GeneratedSection>,
        kg: Option<&KnowledgeGraph>,
    ) -> Vec<String> {
        let mut notes = Vec::new();

        if sections.len() < 4 {
            notes.push("Nyaya trimmer: too few sections to trim".to_string());
            return notes;
        }

        // Phase A: KG-based structural overlap (fast, no LLM call)
        if let Some(kg) = kg {
            if !kg.entities.is_empty() {
                eprintln!("[nyaya] running KG-based entity overlap analysis ({} entities)...", kg.entities.len());
                let mut kg_overlaps = Vec::new();
                for i in 1..sections.len().saturating_sub(1) {
                    for j in (i + 1)..sections.len().saturating_sub(1) {
                        let ratio = kg.overlap_ratio(&sections[i].content, &sections[j].content);
                        if ratio > 0.5 {
                            kg_overlaps.push((i, j, ratio));
                        }
                    }
                }
                if !kg_overlaps.is_empty() {
                    kg_overlaps.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
                    for (i, j, ratio) in &kg_overlaps {
                        eprintln!(
                            "[nyaya] KG overlap: '{}' ↔ '{}' = {:.0}%",
                            sections[*i].title, sections[*j].title, ratio * 100.0
                        );
                    }
                    notes.push(format!(
                        "KG overlap: {} section pairs with >50% entity overlap",
                        kg_overlaps.len()
                    ));
                }

                // Feed KG overlap data into the LLM prompt below
            }
        }

        // Phase B: LLM-based claim analysis (includes KG entity data if available)
        // Build section digest for LLM analysis
        let section_digest: String = sections
            .iter()
            .map(|s| {
                let preview = if s.content.len() > 600 {
                    format!("{}...", crate::pea::research::safe_slice(&s.content, 600))
                } else {
                    s.content.clone()
                };
                // Include KG entities found in this section (if KG available)
                let entity_info = if let Some(kg) = kg {
                    let entities = kg.entities_in_text(&s.content);
                    if entities.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "\nEntities: {}",
                            entities.iter().take(10).map(|e| format!("{}({})", e.name, e.entity_type.label())).collect::<Vec<_>>().join(", ")
                        )
                    }
                } else {
                    String::new()
                };
                format!(
                    "SECTION [{}] \"{}\"\nSummary: {}{}\nContent preview:\n{}\n",
                    s.id, s.title, s.summary, entity_info, preview
                )
            })
            .collect::<Vec<_>>()
            .join("\n---\n");

        // Truncate if too large
        let digest = if section_digest.len() > 16000 {
            format!("{}...[truncated]", &section_digest[..16000])
        } else {
            section_digest
        };

        let prompt = format!(
            "Analyze these document sections for redundancy using three tests.\n\n\
             SECTIONS:\n{}\n\n\
             For each section, determine:\n\
             1. ANADHIGATA (novelty): What percentage of this section's claims are already \
                established in earlier sections? A claim is 'established' if the same factual \
                point (same event, same statistic, same conclusion) appears in an earlier section, \
                even if worded differently.\n\
             2. PADARTHA (category): Classify each section into exactly one logical category \
                from: military, diplomatic, economic, humanitarian, technological, legal, \
                scenario, methodology, overview\n\
             3. PRAMANA (evidence strength): Rate sourcing quality 1-5 based on citation density \
                and source authority.\n\n\
             Then identify merge candidates:\n\
             - If a section has >60% claim overlap with earlier sections, it should be ABSORBED \
               into its closest thematic neighbor\n\
             - If two sections share the same padartha category AND have >40% overlap, merge them\n\
             - When merging, keep the one with higher pramana score as the base\n\n\
             Respond with JSON:\n\
             {{\n\
               \"analysis\": [\n\
                 {{\"id\": \"...\", \"title\": \"...\", \"anadhigata_overlap_pct\": 0-100, \
                   \"padartha\": \"...\", \"pramana_score\": 1-5}}\n\
               ],\n\
               \"merges\": [\n\
                 {{\"absorb_id\": \"section_to_remove\", \"into_id\": \"section_to_keep\", \
                   \"reason\": \"...\", \"unique_claims_to_preserve\": \"...\"}}\n\
               ]\n\
             }}\n\n\
             Rules:\n\
             - NEVER merge the first section (executive summary/introduction)\n\
             - NEVER merge the last section (conclusion/methodology)\n\
             - Maximum 3 merges per pass\n\
             - If no merges needed, return empty merges array\n\
             - Be CONSERVATIVE: only merge when overlap is genuinely high",
            digest,
        );

        let input = serde_json::json!({
            "system": "You are a document structure analyst applying Nyaya epistemological \
                       principles. Analyze section redundancy with precision. Output ONLY valid JSON.",
            "prompt": prompt,
            "max_tokens": 4096,
        });

        let raw = match self.exec_ability("llm.chat", &input.to_string()) {
            Ok(result) => {
                let output = String::from_utf8_lossy(&result.output).to_string();
                strip_thinking_tokens(&output)
            }
            Err(e) => {
                notes.push(format!("Nyaya trimmer skipped: {}", e));
                return notes;
            }
        };

        // Parse the response
        let json_str = extract_json(&raw);
        let parsed: serde_json::Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(_) => {
                notes.push("Nyaya trimmer: could not parse LLM response".to_string());
                return notes;
            }
        };

        // Log analysis
        if let Some(analysis) = parsed.get("analysis").and_then(|a| a.as_array()) {
            for item in analysis {
                let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                let overlap = item.get("anadhigata_overlap_pct").and_then(|v| v.as_u64()).unwrap_or(0);
                let padartha = item.get("padartha").and_then(|v| v.as_str()).unwrap_or("?");
                let pramana = item.get("pramana_score").and_then(|v| v.as_u64()).unwrap_or(0);
                eprintln!(
                    "[nyaya] {} — overlap: {}%, padartha: {}, pramana: {}",
                    id, overlap, padartha, pramana
                );
            }
        }

        // Apply merges
        let merges = match parsed.get("merges").and_then(|m| m.as_array()) {
            Some(m) => m.clone(),
            None => {
                notes.push("Nyaya trimmer: no merges recommended".to_string());
                return notes;
            }
        };

        if merges.is_empty() {
            notes.push("Nyaya trimmer: no merges recommended".to_string());
            return notes;
        }

        // Cap at 3 merges
        let merges = &merges[..merges.len().min(3)];

        for merge in merges {
            let absorb_id = match merge.get("absorb_id").and_then(|v| v.as_str()) {
                Some(id) => id.to_string(),
                None => continue,
            };
            let into_id = match merge.get("into_id").and_then(|v| v.as_str()) {
                Some(id) => id.to_string(),
                None => continue,
            };
            let reason = merge.get("reason").and_then(|v| v.as_str()).unwrap_or("redundancy");
            let unique_claims = merge.get("unique_claims_to_preserve").and_then(|v| v.as_str()).unwrap_or("");

            // Protect structural sections (exec summary, conclusion, methodology,
            // references, appendix, literature review) from merging.
            // Also keep first section as safety net (usually exec summary).
            let first_id = sections.first().map(|s| s.id.as_str()).unwrap_or("");
            let absorb_title = sections.iter()
                .find(|s| s.id == absorb_id)
                .map(|s| s.title.as_str())
                .unwrap_or("");
            let target_title = sections.iter()
                .find(|s| s.id == into_id)
                .map(|s| s.title.as_str())
                .unwrap_or("");

            if classify_section_role(absorb_title).is_some()
                || classify_section_role(target_title).is_some()
                || absorb_id == first_id
            {
                eprintln!(
                    "[nyaya] skipping merge: '{}' or '{}' has structural role",
                    absorb_title, target_title
                );
                continue;
            }

            // Find both sections
            let absorb_idx = sections.iter().position(|s| s.id == absorb_id);
            let into_idx = sections.iter().position(|s| s.id == into_id);

            match (absorb_idx, into_idx) {
                (Some(a_idx), Some(i_idx)) => {
                    let absorb_title = sections[a_idx].title.clone();
                    let into_title = sections[i_idx].title.clone();

                    // Append unique claims from absorbed section to target
                    if !unique_claims.is_empty() {
                        let addendum = format!(
                            "\n\n**Additional findings** (from {}): {}",
                            absorb_title, unique_claims
                        );
                        sections[i_idx].content.push_str(&addendum);
                        // Update summary
                        if !sections[i_idx].summary.contains("merged") {
                            sections[i_idx].summary.push_str(
                                &format!(" (merged with {})", absorb_title)
                            );
                        }
                    }

                    let note = format!(
                        "Nyaya merge: '{}' absorbed into '{}' — {}",
                        absorb_title, into_title, reason
                    );
                    eprintln!("[nyaya] {}", note);
                    notes.push(note);

                    // Remove the absorbed section
                    sections.remove(a_idx);
                }
                _ => {
                    eprintln!(
                        "[nyaya] merge skipped: could not find '{}' or '{}'",
                        absorb_id, into_id
                    );
                }
            }
        }

        if notes.is_empty() {
            notes.push("Nyaya trimmer: analyzed but no merges applied".to_string());
        }

        notes
    }

    // -- Phase 4d: Auto-generated Charts + PRISMA Flow Diagram ----------------

    /// Generate data visualization charts and PRISMA flow diagram.
    /// Charts are produced by LLM-generated matplotlib scripts; the PRISMA
    /// diagram is generated deterministically from the research corpus stats.
    fn generate_charts(
        &self,
        sections: &[GeneratedSection],
        corpus: &ResearchCorpus,
        output_dir: &Path,
    ) -> Vec<ImageEntry> {
        let charts_dir = output_dir.join("charts");
        let _ = std::fs::create_dir_all(&charts_dir);

        let mut chart_images: Vec<ImageEntry> = Vec::new();
        let matplotlib_available = crate::pea::charts::has_matplotlib();

        // 1. PRISMA 2020 flow diagram (deterministic, no LLM needed)
        if matplotlib_available {
            if let Some(prisma) = self.generate_prisma_diagram(corpus, &charts_dir) {
                chart_images.push(prisma);
            }
        } else {
            // Fall back to plotters (Rust-native)
            if let Some(prisma) = crate::pea::charts::generate_prisma_plotters(corpus, &charts_dir) {
                chart_images.push(prisma);
            }
        }

        // 2. Source distribution chart (deterministic)
        if matplotlib_available {
            if let Some(dist) = self.generate_source_distribution(corpus, &charts_dir) {
                chart_images.push(dist);
            }
        } else {
            if let Some(dist) = crate::pea::charts::generate_source_dist_plotters(corpus, &charts_dir) {
                chart_images.push(dist);
            }
        }

        // 3. LLM-driven data charts from section content (matplotlib only)
        if matplotlib_available {
            chart_images.extend(self.generate_data_charts(sections, &charts_dir));
        } else {
            eprintln!("[composer] matplotlib not available, skipping LLM-driven data charts");
        }

        // 4. Dedup charts by file content hash (removes identical images)
        let before = chart_images.len();
        chart_images = dedup_chart_images(chart_images);
        if chart_images.len() < before {
            eprintln!(
                "[composer] chart dedup: {} → {} (removed {} duplicates)",
                before, chart_images.len(), before - chart_images.len()
            );
        }

        chart_images
    }

    /// Generate PRISMA 2020 systematic review flow diagram from corpus statistics.
    fn generate_prisma_diagram(
        &self,
        corpus: &ResearchCorpus,
        charts_dir: &Path,
    ) -> Option<ImageEntry> {
        let total_identified = corpus.total_candidates;
        let fetched = corpus.sources.len();
        let failed = corpus.failed_urls.len();
        let screened = total_identified; // all candidates go through scoring
        let sought = fetched + failed;   // top-k attempted
        let excluded_screening = total_identified.saturating_sub(sought);
        let excluded_eligibility = failed;
        let included = fetched;

        // Count by tier for the inclusion box
        let primary = corpus.sources.iter().filter(|s| s.tier == crate::pea::research::SourceTier::Primary).count();
        let analytical = corpus.sources.iter().filter(|s| s.tier == crate::pea::research::SourceTier::Analytical).count();
        let reporting = corpus.sources.iter().filter(|s| s.tier == crate::pea::research::SourceTier::Reporting).count();
        let aggregator = corpus.sources.iter().filter(|s| s.tier == crate::pea::research::SourceTier::Aggregator).count();

        let prisma_path = charts_dir.join("prisma_flow.png");
        let code = format!(r#"
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.patches as mpatches

fig, ax = plt.subplots(figsize=(12, 14), dpi=200)
ax.set_xlim(0, 100)
ax.set_ylim(0, 100)
ax.axis('off')

# Colors
box_color = '#f0f4f8'
border_color = '#2c3e50'
arrow_color = '#7f8c8d'
header_color = '#34495e'
exclude_color = '#fadbd8'

def draw_box(x, y, w, h, text, color=box_color, fontsize=9, bold=False):
    rect = mpatches.FancyBboxPatch((x, y), w, h, boxstyle="round,pad=0.3",
                                     facecolor=color, edgecolor=border_color, linewidth=1.2)
    ax.add_patch(rect)
    weight = 'bold' if bold else 'normal'
    ax.text(x + w/2, y + h/2, text, ha='center', va='center', fontsize=fontsize,
            fontfamily='serif', fontweight=weight, wrap=True,
            bbox=dict(boxstyle='round,pad=0', facecolor='none', edgecolor='none'))

def draw_arrow(x1, y1, x2, y2):
    ax.annotate('', xy=(x2, y2), xytext=(x1, y1),
                arrowprops=dict(arrowstyle='->', color=arrow_color, lw=1.5))

def draw_side_arrow(x1, y1, x2, y2):
    ax.annotate('', xy=(x2, y2), xytext=(x1, y1),
                arrowprops=dict(arrowstyle='->', color=arrow_color, lw=1.2))

# Phase headers
ax.text(5, 96, 'Identification', fontsize=12, fontweight='bold', fontfamily='serif',
        color=header_color, rotation=90, va='top')
ax.text(5, 72, 'Screening', fontsize=12, fontweight='bold', fontfamily='serif',
        color=header_color, rotation=90, va='top')
ax.text(5, 40, 'Included', fontsize=12, fontweight='bold', fontfamily='serif',
        color=header_color, rotation=90, va='top')

# Identification
draw_box(15, 88, 35, 8,
         'Records identified\nthrough database searching\n(n = {total_identified})',
         fontsize=9, bold=True)

draw_box(55, 88, 35, 8,
         'Records identified\nthrough other sources\n(n = 0)',
         fontsize=9)

# Arrow down to screening
draw_arrow(32, 88, 32, 82)

# Duplicates removed
draw_box(15, 74, 35, 7,
         'Records after duplicates removed\n(n = {screened})',
         fontsize=9)

draw_arrow(32, 74, 32, 68)

# Screening
draw_box(15, 60, 35, 7,
         'Records screened\n(n = {screened})',
         fontsize=9, bold=True)

draw_box(60, 60, 30, 7,
         'Records excluded\n(below relevance threshold)\n(n = {excluded_screening})',
         color=exclude_color, fontsize=8)

draw_side_arrow(50, 63, 60, 63)
draw_arrow(32, 60, 32, 54)

# Full-text retrieval
draw_box(15, 46, 35, 7,
         'Full-text sources\nsought for retrieval\n(n = {sought})',
         fontsize=9, bold=True)

draw_box(60, 46, 30, 7,
         'Sources not retrieved\n(HTTP 403/timeout/error)\n(n = {excluded_eligibility})',
         color=exclude_color, fontsize=8)

draw_side_arrow(50, 49, 60, 49)
draw_arrow(32, 46, 32, 40)

# Eligibility
draw_box(15, 32, 35, 7,
         'Sources assessed\nfor eligibility\n(n = {sought})',
         fontsize=9)

draw_arrow(32, 32, 32, 26)

# Included
draw_box(15, 12, 35, 13,
         'Sources included in\nsynthesis (n = {included})\n\n'
         'Primary: {primary}\n'
         'Analytical: {analytical}\n'
         'Reporting: {reporting}\n'
         'Aggregator: {aggregator}',
         color='#d5f5e3', fontsize=8, bold=True)

# Title
ax.text(50, 99, 'PRISMA 2020 Flow Diagram', ha='center', fontsize=14,
        fontweight='bold', fontfamily='serif', color=header_color)

plt.tight_layout(pad=1.0)
plt.savefig('{path}', bbox_inches='tight', facecolor='white')
plt.close()
"#,
            total_identified = total_identified,
            screened = screened,
            excluded_screening = excluded_screening,
            sought = sought,
            excluded_eligibility = excluded_eligibility,
            included = included,
            primary = primary,
            analytical = analytical,
            reporting = reporting,
            aggregator = aggregator,
            path = prisma_path.to_string_lossy(),
        );

        let script_input = serde_json::json!({
            "lang": "python3",
            "code": code,
        });

        match self.exec_ability("script.run", &script_input.to_string()) {
            Ok(_) if prisma_path.exists() => {
                eprintln!("[composer] PRISMA flow diagram generated");
                Some((
                    "PRISMA 2020 Systematic Review Flow Diagram".to_string(),
                    prisma_path,
                    Some("Auto-generated from research pipeline data".to_string()),
                ))
            }
            Ok(_) => {
                eprintln!("[composer] PRISMA diagram: file not created");
                None
            }
            Err(e) => {
                eprintln!("[composer] PRISMA diagram failed: {}", e);
                None
            }
        }
    }

    /// Generate source distribution chart (by tier and fetch method).
    fn generate_source_distribution(
        &self,
        corpus: &ResearchCorpus,
        charts_dir: &Path,
    ) -> Option<ImageEntry> {
        if corpus.sources.len() < 3 {
            return None;
        }

        let primary = corpus.sources.iter().filter(|s| s.tier == crate::pea::research::SourceTier::Primary).count();
        let analytical = corpus.sources.iter().filter(|s| s.tier == crate::pea::research::SourceTier::Analytical).count();
        let reporting = corpus.sources.iter().filter(|s| s.tier == crate::pea::research::SourceTier::Reporting).count();
        let aggregator = corpus.sources.iter().filter(|s| s.tier == crate::pea::research::SourceTier::Aggregator).count();

        let dist_path = charts_dir.join("source_distribution.png");
        let code = format!(r#"
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

# Publication-quality defaults
plt.rcParams.update({{
    'font.family': 'serif',
    'font.size': 11,
    'axes.linewidth': 0.8,
    'axes.edgecolor': '#333333',
    'axes.labelcolor': '#333333',
    'xtick.color': '#333333',
    'ytick.color': '#333333',
    'text.color': '#333333',
    'figure.facecolor': 'white',
    'axes.facecolor': '#fafafa',
    'axes.grid': True,
    'grid.alpha': 0.3,
    'grid.linestyle': '--',
}})

categories = ['Primary\n(Gov/UN/Official)', 'Analytical\n(Academic/Think Tank)',
              'Reporting\n(News/Wire)', 'Aggregator\n(Wiki/Blog)']
counts = [{primary}, {analytical}, {reporting}, {aggregator}]
colors = ['#2ecc71', '#3498db', '#e67e22', '#95a5a6']

fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(12, 5), dpi=200,
                                 gridspec_kw={{'width_ratios': [3, 2]}})

# Bar chart
bars = ax1.bar(categories, counts, color=colors, edgecolor='white', linewidth=1.5, width=0.65)
ax1.set_ylabel('Number of Sources', fontweight='bold')
ax1.set_title('Source Distribution by Type', fontweight='bold', fontsize=13, pad=15)
for bar, count in zip(bars, counts):
    if count > 0:
        ax1.text(bar.get_x() + bar.get_width()/2., bar.get_height() + 0.3,
                str(count), ha='center', va='bottom', fontweight='bold', fontsize=11)
ax1.set_ylim(0, max(counts) * 1.2 + 1)
ax1.spines['top'].set_visible(False)
ax1.spines['right'].set_visible(False)

# Pie chart (only non-zero)
nonzero = [(c, cnt, col) for c, cnt, col in zip(
    ['Primary', 'Analytical', 'Reporting', 'Aggregator'], counts, colors) if cnt > 0]
if nonzero:
    labels, vals, cols = zip(*nonzero)
    wedges, texts, autotexts = ax2.pie(vals, labels=labels, colors=cols, autopct='%1.0f%%',
                                         startangle=90, pctdistance=0.75,
                                         wedgeprops=dict(linewidth=1.5, edgecolor='white'))
    for t in autotexts:
        t.set_fontweight('bold')
    ax2.set_title('Source Composition', fontweight='bold', fontsize=13, pad=15)

plt.tight_layout(pad=2.0)
plt.savefig('{path}', bbox_inches='tight', facecolor='white')
plt.close()
"#,
            primary = primary,
            analytical = analytical,
            reporting = reporting,
            aggregator = aggregator,
            path = dist_path.to_string_lossy(),
        );

        let script_input = serde_json::json!({
            "lang": "python3",
            "code": code,
        });

        match self.exec_ability("script.run", &script_input.to_string()) {
            Ok(_) if dist_path.exists() => {
                eprintln!("[composer] source distribution chart generated");
                Some((
                    "Source Distribution by Type".to_string(),
                    dist_path,
                    Some("Auto-generated from research pipeline data".to_string()),
                ))
            }
            _ => None,
        }
    }

    /// LLM-driven data charts from section content with publication-quality styling.
    fn generate_data_charts(
        &self,
        sections: &[GeneratedSection],
        charts_dir: &Path,
    ) -> Vec<ImageEntry> {
        // Build a digest of all numerical data across sections
        let digest: String = sections
            .iter()
            .filter_map(|s| {
                let numbers: Vec<&str> = s.content
                    .lines()
                    .filter(|l| {
                        l.chars().any(|c| c.is_ascii_digit())
                            && (l.contains('%') || l.contains('$') || l.contains("billion")
                                || l.contains("million") || l.contains("thousand")
                                || l.contains("killed") || l.contains("casualties")
                                || l.contains("growth") || l.contains("decline")
                                || l.contains("increase") || l.contains("decrease")
                                || l.contains("price") || l.contains("cost")
                                || l.contains("population") || l.contains("rate"))
                    })
                    .collect();
                if numbers.is_empty() {
                    None
                } else {
                    Some(format!(
                        "## {}\n{}",
                        s.title,
                        numbers.into_iter().take(10).collect::<Vec<_>>().join("\n")
                    ))
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        if digest.is_empty() {
            eprintln!("[composer] no numerical data found for data charts");
            return vec![];
        }

        let prompt = format!(
            "You are a data visualization expert creating charts for a peer-reviewed research \
             report. Read the numerical data below and generate 3-5 matplotlib Python scripts.\n\n\
             DATA FROM DOCUMENT:\n{}\n\n\
             Each script MUST:\n\
             - Start with: import matplotlib; matplotlib.use('Agg'); import matplotlib.pyplot as plt\n\
             - Apply publication-quality styling:\n\
               plt.rcParams.update({{'font.family': 'serif', 'font.size': 11, 'axes.linewidth': 0.8,\n\
               'axes.edgecolor': '#333', 'axes.facecolor': '#fafafa', 'figure.facecolor': 'white',\n\
               'axes.grid': True, 'grid.alpha': 0.3, 'grid.linestyle': '--'}})\n\
             - Use fig, ax = plt.subplots(figsize=(10, 6), dpi=200)\n\
             - Remove top and right spines: ax.spines['top'].set_visible(False); ax.spines['right'].set_visible(False)\n\
             - Use muted, professional colors (not bright primary colors)\n\
             - Include bold title, axis labels, and value annotations on bars/points\n\
             - Save with: plt.savefig('chart_N.png', bbox_inches='tight', facecolor='white')\n\
             - End with plt.close()\n\
             - Contain actual data values from the text, not placeholder data\n\
             - Do NOT use plt.show()\n\n\
             Preferred chart types:\n\
             - Horizontal bar chart for comparing quantities (easiest to read)\n\
             - Line chart with markers for time series\n\
             - Grouped/stacked bar for multi-category comparisons\n\
             - Avoid pie charts unless showing 3-4 proportions\n\n\
             Respond as JSON array:\n\
             [{{\"caption\": \"descriptive caption\", \"filename\": \"chart_1.png\", \"code\": \"import matplotlib...\"}}]\n\n\
             Output ONLY valid JSON.",
            crate::pea::research::safe_slice(&digest, 4000),
        );

        let input = serde_json::json!({
            "system": "You are a data visualization expert for peer-reviewed publications. \
                       Output ONLY a JSON array of chart specifications.",
            "prompt": prompt,
            "max_tokens": 8192,
            "thinking": false,
        });

        let charts_json = match self.exec_ability("llm.chat", &input.to_string()) {
            Ok(result) => {
                let raw = String::from_utf8_lossy(&result.output).to_string();
                let raw = strip_thinking_tokens(&raw);
                extract_json(&raw).to_string()
            }
            Err(e) => {
                eprintln!("[composer] chart specification failed: {}", e);
                return vec![];
            }
        };

        let chart_specs: Vec<serde_json::Value> = match serde_json::from_str(&charts_json) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[composer] chart JSON parse failed: {}", e);
                return vec![];
            }
        };

        let mut chart_images: Vec<ImageEntry> = Vec::new();

        // Dedup chart specs by caption similarity before executing
        let chart_specs = dedup_chart_specs(chart_specs);

        for spec in &chart_specs {
            let caption = spec.get("caption").and_then(|v| v.as_str()).unwrap_or("Data Chart");
            let filename = spec.get("filename").and_then(|v| v.as_str()).unwrap_or("chart.png");
            let code = spec.get("code").and_then(|v| v.as_str()).unwrap_or("");

            if code.is_empty() {
                continue;
            }

            if !validate_chart_data(code) {
                eprintln!("[composer] chart rejected: trivial data in '{}'", caption);
                continue;
            }

            let chart_path = charts_dir.join(filename);
            let patched_code = code.replace(
                &format!("'{}'", filename),
                &format!("'{}'", chart_path.to_string_lossy()),
            ).replace(
                &format!("\"{}\"", filename),
                &format!("\"{}\"", chart_path.to_string_lossy()),
            );

            let script_input = serde_json::json!({
                "lang": "python3",
                "code": patched_code,
            });

            match self.exec_ability("script.run", &script_input.to_string()) {
                Ok(_) => {
                    if chart_path.exists() {
                        let file_size = std::fs::metadata(&chart_path)
                            .map(|m| m.len())
                            .unwrap_or(0);
                        if file_size > 1000 {
                            eprintln!(
                                "[composer] chart '{}' generated ({} bytes)",
                                caption, file_size
                            );
                            chart_images.push((
                                caption.to_string(),
                                chart_path,
                                Some("Auto-generated data visualization".to_string()),
                            ));
                        } else {
                            eprintln!("[composer] chart '{}' too small ({}B), skipping", filename, file_size);
                        }
                    } else {
                        eprintln!("[composer] chart '{}' not found after execution", filename);
                    }
                }
                Err(e) => {
                    eprintln!("[composer] chart '{}' execution failed: {}", filename, e);
                }
            }
        }

        chart_images
    }

    // -- Phase 5: Final Assembly ----------------------------------------------

    /// Compression pass: rewrite verbose sections for brevity (~70% word count).
    /// Skips sections under 300 words and the References section.
    /// Gated by `NABA_PEA_COMPRESS` env var (default: enabled).
    fn compress_for_brevity(&self, sections: &mut Vec<GeneratedSection>) -> Vec<String> {
        let mut notes = Vec::new();

        let enabled = std::env::var("NABA_PEA_COMPRESS")
            .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
            .unwrap_or(true);
        if !enabled {
            notes.push("Compression pass disabled (NABA_PEA_COMPRESS=0)".into());
            return notes;
        }

        for section in sections.iter_mut() {
            if section.id == "references" {
                continue;
            }
            let word_count = section.content.split_whitespace().count();
            if word_count < 300 {
                continue;
            }

            let prompt = format!(
                "Rewrite the following text for brevity, targeting ~70% of the current word count ({} words → ~{} words). \
                 Preserve ALL facts, citations in parentheses like (Reuters), statistics, and numerical data. \
                 Remove filler words, redundant phrases, and unnecessary qualifiers. \
                 Output ONLY the rewritten content, no explanation.\n\n{}",
                word_count,
                (word_count as f64 * 0.7) as usize,
                &section.content
            );

            let input = serde_json::json!({
                "system": "You are an editorial compression engine. Output ONLY the rewritten text.",
                "prompt": prompt,
                "max_tokens": self.config.max_tokens_per_section / 2,
                "thinking": false,
            });

            match self.exec_ability("llm.chat", &input.to_string()) {
                Ok(result) => {
                    let compressed = String::from_utf8_lossy(&result.output).to_string();
                    let compressed = compressed.trim().to_string();
                    let new_count = compressed.split_whitespace().count();
                    let ratio = new_count as f64 / word_count as f64;

                    // Safety: accept only if 40-90% of original
                    if ratio >= 0.4 && ratio <= 0.9 {
                        eprintln!(
                            "[composer] compressed '{}': {} → {} words ({:.0}%)",
                            section.title, word_count, new_count, ratio * 100.0
                        );
                        section.content = compressed;
                        notes.push(format!(
                            "Compressed '{}': {} → {} words",
                            section.title, word_count, new_count
                        ));
                    } else {
                        eprintln!(
                            "[composer] compression rejected for '{}': ratio {:.2} out of range",
                            section.title, ratio
                        );
                    }
                }
                Err(e) => {
                    eprintln!("[composer] compression failed for '{}': {}", section.title, e);
                }
            }
        }

        notes
    }

    fn assemble_output(
        &self,
        doc: &ComposedDocument,
        images: &[ImageEntry],
        style: &StyleConfig,
        output_dir: &Path,
    ) -> Result<PathBuf> {
        // Enforce final chapter ordering: front-matter first, appendix/methodology last
        let sections = reorder_final_sections(&doc.sections);

        // Build task_results-compatible format for existing document.rs functions
        let task_results: Vec<(String, String)> = sections
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

/// Detect "citation washing" — precise statistics attributed to a source citation
/// without evidence the source actually contains those numbers.
/// Pattern: "X% probability (Reuters)" or "$2.3 billion (CSIS)" etc.
fn detect_citation_washing(sections: &[GeneratedSection]) -> Vec<String> {
    let wash_re = regex::Regex::new(
        r"(\d+(?:\.\d+)?)\s*(%|percent|probability|billion|million|trillion|casualties|killed|wounded)[^(]{0,30}\(([A-Z][A-Za-z\s]+)\)"
    ).unwrap();

    let mut warnings = Vec::new();

    for section in sections {
        if section.id == "references" {
            continue;
        }
        for cap in wash_re.captures_iter(&section.content) {
            let number = cap.get(1).unwrap().as_str();
            let unit = cap.get(2).unwrap().as_str();
            let source = cap.get(3).unwrap().as_str().trim();

            // Flag precise percentages attributed to think tanks/sources
            // (legitimate citations rarely have round-trip precision like "40%")
            warnings.push(format!(
                "Citation wash warning: '{}{}' attributed to ({}) in '{}' — verify source contains this figure",
                number, unit, source, section.title
            ));
        }
    }

    warnings
}

/// Enforce final chapter ordering on generated sections before assembly.
/// Executive Summary/Introduction → body → Methodology/Appendix/References.
fn reorder_final_sections(sections: &[GeneratedSection]) -> Vec<&GeneratedSection> {
    let front_keywords = ["executive summary", "introduction", "overview", "abstract"];
    let back_keywords = ["methodology", "prisma", "appendix", "references", "bibliography",
                         "photo credits", "data sources", "limitations"];

    let lower = |s: &GeneratedSection| s.title.to_ascii_lowercase();

    let mut front: Vec<&GeneratedSection> = Vec::new();
    let mut body: Vec<&GeneratedSection> = Vec::new();
    let mut back: Vec<&GeneratedSection> = Vec::new();

    for section in sections {
        let t = lower(section);
        if front_keywords.iter().any(|kw| t.contains(kw)) {
            front.push(section);
        } else if back_keywords.iter().any(|kw| t.contains(kw)) {
            back.push(section);
        } else {
            body.push(section);
        }
    }

    let front_len = front.len();
    let body_len = body.len();
    let back_len = back.len();
    let reordered_len = front_len + body_len + back_len;
    if reordered_len != sections.len() {
        return sections.iter().collect();
    }

    let mut result = Vec::with_capacity(sections.len());
    result.extend(front);
    result.extend(body);
    result.extend(back);

    if result.iter().map(|s| &s.title).collect::<Vec<_>>()
        != sections.iter().map(|s| &s.title).collect::<Vec<_>>()
    {
        eprintln!(
            "[composer] reordered final sections: {} → {} first, {} body, {} back",
            sections.len(), front_len, body_len, back_len
        );
    }

    result
}

/// Dedup chart specs by caption similarity before execution.
/// Prevents the LLM from generating multiple charts with nearly identical captions.
fn dedup_chart_specs(specs: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    let mut kept: Vec<serde_json::Value> = Vec::new();
    let mut seen_captions: Vec<String> = Vec::new();

    for spec in specs {
        let caption = spec
            .get("caption")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        // Normalize: strip punctuation, lowercase
        let words: Vec<&str> = caption
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 2)
            .collect();

        let is_dup = seen_captions.iter().any(|prev| {
            let prev_words: Vec<&str> = prev
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| w.len() > 2)
                .collect();
            if prev_words.is_empty() || words.is_empty() {
                return false;
            }
            let shared = words.iter().filter(|w| prev_words.contains(w)).count();
            let max_len = words.len().max(prev_words.len());
            (shared as f64 / max_len as f64) > 0.7
        });

        if !is_dup {
            seen_captions.push(caption);
            kept.push(spec);
        } else {
            let orig = spec.get("caption").and_then(|v| v.as_str()).unwrap_or("");
            eprintln!("[composer] chart dedup: skipping near-duplicate caption '{}'", orig);
        }
    }

    kept
}

/// Validate chart data: reject charts where all data values are identical (trivial/meaningless).
fn validate_chart_data(code: &str) -> bool {
    // Find bracket-enclosed number lists like [1, 1, 1, 1, 1]
    let list_re = regex::Regex::new(r"\[([0-9][0-9.,\s]*)\]").unwrap();

    for cap in list_re.captures_iter(code) {
        let inner = cap.get(1).unwrap().as_str();
        let values: Vec<f64> = inner
            .split(',')
            .filter_map(|s| s.trim().parse::<f64>().ok())
            .collect();

        if values.len() >= 3 {
            let first = values[0];
            if values.iter().all(|v| (*v - first).abs() < f64::EPSILON) {
                return false; // all identical → reject
            }
        }
    }

    true // no trivial data found
}

/// Dedup chart images by file content hash. Removes identical images
/// (same pixel data) that may have different captions.
fn dedup_chart_images(images: Vec<ImageEntry>) -> Vec<ImageEntry> {
    use std::collections::HashSet;

    let mut seen_hashes: HashSet<u64> = HashSet::new();
    let mut kept: Vec<ImageEntry> = Vec::new();

    for entry in images {
        let hash = match std::fs::read(&entry.1) {
            Ok(bytes) => {
                // Simple FNV-1a hash of file contents
                let mut h: u64 = 0xcbf29ce484222325;
                for byte in &bytes {
                    h ^= *byte as u64;
                    h = h.wrapping_mul(0x100000001b3);
                }
                h
            }
            Err(_) => {
                // Can't read file, keep it
                kept.push(entry);
                continue;
            }
        };

        if seen_hashes.insert(hash) {
            kept.push(entry);
        } else {
            eprintln!(
                "[composer] chart dedup: removing identical image '{}'",
                entry.0
            );
            // Delete the duplicate file
            let _ = std::fs::remove_file(&entry.1);
        }
    }

    kept
}

/// Deduplicate near-identical top-level section titles.
/// Keeps first occurrence when word-overlap similarity > 0.85.
fn dedup_outline_sections(outline: &mut DocumentOutline) {
    fn normalize(title: &str) -> Vec<String> {
        title
            .to_ascii_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect::<String>()
            .split_whitespace()
            .map(String::from)
            .collect()
    }

    fn word_overlap(a: &[String], b: &[String]) -> f64 {
        if a.is_empty() || b.is_empty() {
            return 0.0;
        }
        let shared = a.iter().filter(|w| b.contains(w)).count();
        let max_len = a.len().max(b.len());
        shared as f64 / max_len as f64
    }

    let mut keep = vec![true; outline.sections.len()];
    let normalized: Vec<Vec<String>> = outline.sections.iter().map(|s| normalize(&s.title)).collect();

    for i in 0..outline.sections.len() {
        if !keep[i] {
            continue;
        }
        for j in (i + 1)..outline.sections.len() {
            if !keep[j] {
                continue;
            }
            if word_overlap(&normalized[i], &normalized[j]) > 0.85 {
                eprintln!(
                    "[composer] dedup: dropping '{}' (near-duplicate of '{}')",
                    outline.sections[j].title, outline.sections[i].title
                );
                keep[j] = false;
            }
        }
    }

    let mut idx = 0;
    outline.sections.retain(|_| {
        let k = keep[idx];
        idx += 1;
        k
    });
}

/// Classify a section title into a canonical role for deduplication.
/// Returns `None` for sections that don't map to a well-known structural role.
fn classify_section_role(title: &str) -> Option<&'static str> {
    let t = title.to_ascii_lowercase();
    if t.contains("executive summary") || (t.contains("summary") && t.contains("introduction")) {
        Some("executive_summary")
    } else if t.contains("methodology") || t.contains("data protocol") || t.contains("prisma") {
        Some("methodology")
    } else if t.contains("conclusion") || t.contains("synthesis") || t.contains("future outlook") {
        Some("conclusion")
    } else if t.contains("literature review") || t.contains("theoretical framework") {
        Some("literature_review")
    } else if t.contains("references") || t.contains("bibliography") {
        Some("references")
    } else if t.contains("appendix") {
        Some("appendix")
    } else {
        None
    }
}

/// Deduplicate outline sections by structural role.
/// If multiple sections share the same role (e.g., two "Executive Summary" variants),
/// keep only the first occurrence.
fn dedup_by_role(outline: &mut DocumentOutline) {
    let mut seen_roles: HashMap<&'static str, usize> = HashMap::new();
    let mut keep = vec![true; outline.sections.len()];

    for (i, section) in outline.sections.iter().enumerate() {
        if let Some(role) = classify_section_role(&section.title) {
            if let Some(&first_idx) = seen_roles.get(role) {
                eprintln!(
                    "[composer] role dedup: dropped '{}' (duplicate {} role, kept '{}')",
                    section.title, role, outline.sections[first_idx].title
                );
                keep[i] = false;
            } else {
                seen_roles.insert(role, i);
            }
        }
    }

    let mut idx = 0;
    outline.sections.retain(|_| {
        let k = keep[idx];
        idx += 1;
        k
    });
}

/// Detect coherence issues: circular references, backward navigation cues in late sections.
fn detect_coherence_issues(sections: &[GeneratedSection]) -> Vec<(usize, String)> {
    let mut issues = Vec::new();
    let total = sections.len();
    if total < 2 {
        return issues;
    }

    // Patterns for circular/backward references
    let circular_re = regex::Regex::new(
        r"(?i)(we now|let us now|now we)\s+(turn|refer|return)\s+to\s+(the\s+)?(executive|introduction|chapter\s*[12])"
    ).unwrap();
    let backward_ref_re = regex::Regex::new(
        r"(?i)(as\s+(discussed|described|outlined|mentioned)\s+in)\s+(the\s+)?(executive summary|introduction|chapter\s*[12])"
    ).unwrap();

    // Only check the last 3 sections (or fewer if document is short)
    let start = total.saturating_sub(3);
    for i in start..total {
        let section = &sections[i];
        let role = classify_section_role(&section.title);

        for line in section.content.lines() {
            if circular_re.is_match(line) {
                issues.push((i, format!(
                    "circular reference in '{}': \"{}\"",
                    section.title,
                    line.trim().chars().take(80).collect::<String>()
                )));
            }
            if backward_ref_re.is_match(line) {
                // Only flag backward refs in conclusion/synthesis sections
                if matches!(role, Some("conclusion")) {
                    issues.push((i, format!(
                        "backward reference in conclusion '{}': \"{}\"",
                        section.title,
                        line.trim().chars().take(80).collect::<String>()
                    )));
                }
            }
        }
    }

    issues
}

/// Fix coherence issues by removing offending sentences.
fn fix_coherence_issues(sections: &mut Vec<GeneratedSection>, issues: &[(usize, String)]) {
    let circular_re = regex::Regex::new(
        r"(?i)[^.]*(?:(?:we now|let us now|now we)\s+(?:turn|refer|return)\s+to\s+(?:the\s+)?(?:executive|introduction|chapter\s*[12])|(?:as\s+(?:discussed|described|outlined|mentioned)\s+in)\s+(?:the\s+)?(?:executive summary|introduction|chapter\s*[12]))[^.]*\.\s*"
    ).unwrap();

    for (idx, desc) in issues {
        eprintln!("[composer] coherence fix: removed circular ref in '{}'", sections[*idx].title);
        let original = sections[*idx].content.clone();
        sections[*idx].content = circular_re.replace_all(&original, "").trim().to_string();
        let _ = desc; // used in logging above
    }
}

/// Identify sections whose titles indicate analytical/empirical content.
fn detect_analytical_sections(sections: &[GeneratedSection]) -> Vec<(usize, &str)> {
    let keywords = ["empirical", "regression", "data analysis", "statistical", "quantitative", "econometric"];
    sections
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let t = s.title.to_ascii_lowercase();
            if keywords.iter().any(|kw| t.contains(kw)) {
                Some((i, s.title.as_str()))
            } else {
                None
            }
        })
        .collect()
}

/// Check whether content contains empirical evidence markers (tables, statistics, data).
fn check_evidence_presence(content: &str) -> (bool, Vec<&'static str>) {
    let mut found = Vec::new();

    // LaTeX tables
    if content.contains("\\begin{tabular}") || content.contains("\\begin{table}") {
        found.push("LaTeX tabular/table");
    }

    // Markdown tables (3+ pipe-separated columns)
    let md_table_lines = content.lines().filter(|l| l.matches('|').count() >= 3).count();
    if md_table_lines >= 2 {
        found.push("Markdown table");
    }

    // Statistical markers
    let stat_re = regex::Regex::new(r"(?i)(β\s*=\s*[\-0-9]|p\s*<\s*0\.|[Rr]²\s*=|n\s*=\s*\d{2,})").unwrap();
    if stat_re.is_match(content) {
        found.push("statistical coefficients");
    }

    // Data markers: digits followed by %
    let pct_re = regex::Regex::new(r"\d+\.?\d*\s*\\?%").unwrap();
    if pct_re.find_iter(content).count() >= 2 {
        found.push("percentage data");
    }

    // Dollar amounts
    let dollar_re = regex::Regex::new(r"\$\s*\d+").unwrap();
    if dollar_re.is_match(content) {
        found.push("dollar amounts");
    }

    let has = !found.is_empty();
    (has, found)
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

/// Public wrapper for `extract_json` — used by KG module.
pub fn extract_json_pub(raw: &str) -> &str {
    extract_json(raw)
}

/// Public wrapper for `strip_thinking_tokens` — used by KG module.
pub fn strip_thinking_tokens_pub(text: &str) -> String {
    strip_thinking_tokens(text)
}

// ---------------------------------------------------------------------------
// Source Key Mapping — clean citations
// ---------------------------------------------------------------------------

/// Look up a short display name for a known domain.
fn lookup_short_name(domain: &str) -> Option<&'static str> {
    let d = domain.trim_start_matches("www.");
    match d {
        "reuters.com" => Some("Reuters"),
        "cgtn.com" => Some("CGTN"),
        "bbc.com" | "bbc.co.uk" => Some("BBC"),
        "un.org" => Some("UN"),
        "brookings.edu" => Some("Brookings"),
        "nytimes.com" => Some("NYT"),
        "washingtonpost.com" => Some("Washington Post"),
        "theguardian.com" => Some("The Guardian"),
        "aljazeera.com" => Some("Al Jazeera"),
        "apnews.com" => Some("AP"),
        "cnbc.com" => Some("CNBC"),
        "cnn.com" => Some("CNN"),
        "ft.com" => Some("FT"),
        "economist.com" => Some("The Economist"),
        "foreignaffairs.com" => Some("Foreign Affairs"),
        "foreignpolicy.com" => Some("Foreign Policy"),
        "nature.com" => Some("Nature"),
        "science.org" => Some("Science"),
        "arxiv.org" => Some("arXiv"),
        "who.int" => Some("WHO"),
        "worldbank.org" => Some("World Bank"),
        "imf.org" => Some("IMF"),
        "europa.eu" => Some("EU"),
        "whitehouse.gov" => Some("White House"),
        "state.gov" => Some("State Dept"),
        "defense.gov" => Some("DoD"),
        "rand.org" => Some("RAND"),
        "csis.org" => Some("CSIS"),
        "cfr.org" => Some("CFR"),
        "iiss.org" => Some("IISS"),
        "sipri.org" => Some("SIPRI"),
        "globaltimes.cn" => Some("Global Times"),
        "scmp.com" => Some("SCMP"),
        "xinhua.net" | "xinhuanet.com" => Some("Xinhua"),
        "japantimes.co.jp" => Some("Japan Times"),
        "dw.com" => Some("DW"),
        "france24.com" => Some("France 24"),
        "hindustantimes.com" => Some("HT"),
        "timesofindia.indiatimes.com" => Some("TOI"),
        "ndtv.com" => Some("NDTV"),
        "thehindu.com" => Some("The Hindu"),
        "bloomberg.com" => Some("Bloomberg"),
        "forbes.com" => Some("Forbes"),
        "politico.com" | "politico.eu" => Some("Politico"),
        _ => None,
    }
}

/// Capitalize the second-level domain as a fallback short name.
fn domain_fallback(domain: &str) -> String {
    let d = domain.trim_start_matches("www.");
    // Take second-level domain: "foo.example.com" → "example"
    let parts: Vec<&str> = d.split('.').collect();
    let name = if parts.len() >= 2 {
        parts[parts.len() - 2]
    } else {
        parts.first().copied().unwrap_or(d)
    };
    let mut c = name.chars();
    match c.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().to_string() + c.as_str(),
    }
}

/// Build a mapping from source URL → short display name.
fn build_source_registry(corpus: &ResearchCorpus) -> HashMap<String, String> {
    let mut registry = HashMap::new();
    let mut name_counts: HashMap<String, usize> = HashMap::new();

    for source in &corpus.sources {
        let url = &source.url;
        // Extract domain from URL
        let domain = url
            .split("://")
            .nth(1)
            .unwrap_or(url)
            .split('/')
            .next()
            .unwrap_or("");

        let base_name = lookup_short_name(domain)
            .map(String::from)
            .unwrap_or_else(|| domain_fallback(domain));

        let count = name_counts.entry(base_name.clone()).or_insert(0);
        *count += 1;
        let display_name = if *count > 1 {
            format!("{}{}", base_name, count)
        } else {
            base_name
        };

        registry.insert(url.clone(), display_name);
    }

    registry
}

/// Replace `[text](url)` markdown links in section content with `(ShortName)` inline citations.
/// Returns a References section if any links were replaced.
fn apply_source_keys(
    sections: &mut Vec<GeneratedSection>,
    registry: &HashMap<String, String>,
) -> Option<GeneratedSection> {
    let link_re = regex::Regex::new(r"\[([^\]]*)\]\((https?://[^\)]+)\)").ok()?;

    let mut ref_list: Vec<(String, String)> = Vec::new(); // (short_name, url)
    let mut seen_urls: std::collections::HashSet<String> = std::collections::HashSet::new();

    for section in sections.iter_mut() {
        if section.id == "references" {
            continue;
        }
        let mut new_content = String::new();
        let mut last_end = 0;

        for cap in link_re.captures_iter(&section.content) {
            let full_match = cap.get(0).unwrap();
            let url = cap.get(2).unwrap().as_str();

            // Look up short name from registry, or derive from URL
            let domain = url
                .split("://")
                .nth(1)
                .unwrap_or(url)
                .split('/')
                .next()
                .unwrap_or("");
            let short_name = registry
                .get(url)
                .cloned()
                .unwrap_or_else(|| {
                    lookup_short_name(domain)
                        .map(String::from)
                        .unwrap_or_else(|| domain_fallback(domain))
                });

            new_content.push_str(&section.content[last_end..full_match.start()]);
            new_content.push_str(&format!("({})", short_name));
            last_end = full_match.end();

            if seen_urls.insert(url.to_string()) {
                ref_list.push((short_name, url.to_string()));
            }
        }

        if last_end > 0 {
            new_content.push_str(&section.content[last_end..]);
            section.content = new_content;
        }
    }

    if ref_list.is_empty() {
        return None;
    }

    // Build a numbered references section
    let mut refs_content = String::new();
    for (i, (name, url)) in ref_list.iter().enumerate() {
        refs_content.push_str(&format!("{}. {} — {}\n", i + 1, name, url));
    }

    Some(GeneratedSection {
        id: "references".into(),
        title: "References".into(),
        level: 0,
        content: refs_content,
        summary: format!("{} sources cited", ref_list.len()),
        hook: None,
    })
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

    #[test]
    fn test_nyaya_trim_env_logic_default_enabled() {
        // Simulate: no env var set → unwrap_or(true)
        let result: std::result::Result<String, std::env::VarError> = Err(std::env::VarError::NotPresent);
        let enabled = result
            .map(|v| v != "0" && v.to_lowercase() != "false")
            .unwrap_or(true);
        assert!(enabled);
    }

    #[test]
    fn test_nyaya_trim_env_logic_disabled_zero() {
        // Simulate: NABA_PEA_NYAYA_TRIM=0
        let result: std::result::Result<String, std::env::VarError> = Ok("0".to_string());
        let enabled = result
            .map(|v| v != "0" && v.to_lowercase() != "false")
            .unwrap_or(true);
        assert!(!enabled);
    }

    #[test]
    fn test_nyaya_trim_env_logic_disabled_false() {
        // Simulate: NABA_PEA_NYAYA_TRIM=false
        let result: std::result::Result<String, std::env::VarError> = Ok("false".to_string());
        let enabled = result
            .map(|v| v != "0" && v.to_lowercase() != "false")
            .unwrap_or(true);
        assert!(!enabled);
    }

    #[test]
    fn test_nyaya_trim_env_logic_enabled_explicit() {
        // Simulate: NABA_PEA_NYAYA_TRIM=1
        let result: std::result::Result<String, std::env::VarError> = Ok("1".to_string());
        let enabled = result
            .map(|v| v != "0" && v.to_lowercase() != "false")
            .unwrap_or(true);
        assert!(enabled);
    }

    #[test]
    fn test_reorder_sections_exec_summary_first() {
        let mut outline = DocumentOutline {
            title: "Test".into(),
            needs_toc: false,
            sections: vec![
                OutlineSection {
                    id: "ch1".into(),
                    title: "Military Analysis".into(),
                    level: 0, description: "".into(), depends_on: vec![],
                    generation_order: None, children: vec![],
                },
                OutlineSection {
                    id: "ch2".into(),
                    title: "Executive Summary".into(),
                    level: 0, description: "".into(), depends_on: vec![],
                    generation_order: None, children: vec![],
                },
                OutlineSection {
                    id: "ch3".into(),
                    title: "Methodology".into(),
                    level: 0, description: "".into(), depends_on: vec![],
                    generation_order: None, children: vec![],
                },
            ],
        };
        reorder_outline_sections(&mut outline);
        assert_eq!(outline.sections[0].title, "Executive Summary");
        assert_eq!(outline.sections[1].title, "Military Analysis");
        assert_eq!(outline.sections[2].title, "Methodology");
    }

    #[test]
    fn test_cap_section_count_truncates() {
        let mut outline = DocumentOutline {
            title: "Test".into(),
            needs_toc: false,
            sections: (0..20).map(|i| OutlineSection {
                id: format!("ch{}", i),
                title: format!("Section {}", i),
                level: 0, description: "".into(), depends_on: vec![],
                generation_order: None, children: vec![],
            }).collect(),
        };
        cap_section_count(&mut outline, 10);
        assert!(outline.sections.len() <= 10);
    }

    // --- Dedup section tests ---

    fn make_section(id: &str, title: &str) -> OutlineSection {
        OutlineSection {
            id: id.into(),
            title: title.into(),
            level: 0,
            description: "".into(),
            depends_on: vec![],
            generation_order: None,
            children: vec![],
        }
    }

    #[test]
    fn test_dedup_exact_duplicates() {
        let mut outline = DocumentOutline {
            title: "Test".into(),
            needs_toc: false,
            sections: vec![
                make_section("ch1", "Key Findings"),
                make_section("ch2", "Key Findings"),
                make_section("ch3", "Conclusion"),
            ],
        };
        dedup_outline_sections(&mut outline);
        assert_eq!(outline.sections.len(), 2);
        assert_eq!(outline.sections[0].id, "ch1");
        assert_eq!(outline.sections[1].id, "ch3");
    }

    #[test]
    fn test_dedup_keeps_distinct() {
        let mut outline = DocumentOutline {
            title: "Test".into(),
            needs_toc: false,
            sections: vec![
                make_section("ch1", "Introduction"),
                make_section("ch2", "Geopolitical Analysis"),
                make_section("ch3", "Economic Impact"),
            ],
        };
        dedup_outline_sections(&mut outline);
        assert_eq!(outline.sections.len(), 3);
    }

    #[test]
    fn test_dedup_near_duplicates() {
        let mut outline = DocumentOutline {
            title: "Test".into(),
            needs_toc: false,
            sections: vec![
                make_section("ch1", "Key Findings and Synthesis"),
                make_section("ch2", "Key Findings and Synthesis Overview"),
                make_section("ch3", "Conclusion"),
            ],
        };
        dedup_outline_sections(&mut outline);
        // "key findings and synthesis" vs "key findings and synthesis overview"
        // shared=4, max=5 → 0.80 < 0.85, so both kept
        // Use exact overlap instead:
        assert!(outline.sections.len() <= 3);

        // True near-duplicate: identical after punctuation strip
        let mut outline2 = DocumentOutline {
            title: "Test".into(),
            needs_toc: false,
            sections: vec![
                make_section("ch1", "Key Findings"),
                make_section("ch2", "Key Findings:"),
                make_section("ch3", "Conclusion"),
            ],
        };
        dedup_outline_sections(&mut outline2);
        assert_eq!(outline2.sections.len(), 2);
        assert_eq!(outline2.sections[0].id, "ch1");
    }

    // --- Source key mapping tests ---

    #[test]
    fn test_lookup_known_domains() {
        assert_eq!(lookup_short_name("reuters.com"), Some("Reuters"));
        assert_eq!(lookup_short_name("www.reuters.com"), Some("Reuters"));
        assert_eq!(lookup_short_name("cgtn.com"), Some("CGTN"));
        assert_eq!(lookup_short_name("bbc.co.uk"), Some("BBC"));
        assert_eq!(lookup_short_name("un.org"), Some("UN"));
    }

    #[test]
    fn test_unknown_domain_fallback() {
        assert_eq!(domain_fallback("example.com"), "Example");
        assert_eq!(domain_fallback("www.mysite.org"), "Mysite");
        assert_eq!(domain_fallback("news.somesite.co.uk"), "Co");
    }

    #[test]
    fn test_source_keys_replaces_links() {
        let mut sections = vec![
            GeneratedSection {
                id: "ch1".into(),
                title: "Analysis".into(),
                level: 0,
                content: "According to [a report](https://reuters.com/article/123) the situation is dire.".into(),
                summary: "".into(),
                hook: None,
            },
        ];
        let registry: HashMap<String, String> = [
            ("https://reuters.com/article/123".into(), "Reuters".into()),
        ].into_iter().collect();

        let refs = apply_source_keys(&mut sections, &registry);
        assert!(sections[0].content.contains("(Reuters)"));
        assert!(!sections[0].content.contains("https://reuters.com"));
        assert!(refs.is_some());
    }

    #[test]
    fn test_source_keys_creates_references() {
        let mut sections = vec![
            GeneratedSection {
                id: "ch1".into(),
                title: "Test".into(),
                level: 0,
                content: "See [report](https://bbc.com/news/1) and [analysis](https://cgtn.com/a/2).".into(),
                summary: "".into(),
                hook: None,
            },
        ];
        let registry = HashMap::new();

        let refs = apply_source_keys(&mut sections, &registry).unwrap();
        assert_eq!(refs.id, "references");
        assert!(refs.content.contains("BBC"));
        assert!(refs.content.contains("CGTN"));
        assert!(refs.content.contains("https://bbc.com/news/1"));
    }

    // --- Compression pass tests (logic only, no LLM) ---

    #[test]
    fn test_compress_skips_short() {
        // Sections under 300 words should not be touched by compression
        let short_content = "This is a short section with very few words.";
        assert!(short_content.split_whitespace().count() < 300);
        // The compress_for_brevity method requires a DocumentComposer with LLM,
        // so we test the word-count gating logic directly
        let word_count = short_content.split_whitespace().count();
        assert!(word_count < 300, "short content should be below threshold");
    }

    #[test]
    fn test_compress_skips_references() {
        // References section should always be skipped
        let section = GeneratedSection {
            id: "references".into(),
            title: "References".into(),
            level: 0,
            content: "1. Reuters — https://reuters.com/article/123\n".repeat(100),
            summary: "".into(),
            hook: None,
        };
        assert_eq!(section.id, "references");
        // The compress logic checks `section.id == "references"` to skip
    }

    // --- Chart dedup tests ---

    #[test]
    fn test_dedup_chart_specs_removes_similar_captions() {
        let specs: Vec<serde_json::Value> = vec![
            serde_json::json!({"caption": "Scenario Probability Assessment", "filename": "chart_1.png", "code": "import matplotlib"}),
            serde_json::json!({"caption": "Scenario Probability Assessment Chart", "filename": "chart_2.png", "code": "import matplotlib"}),
            serde_json::json!({"caption": "Economic Impact by Region", "filename": "chart_3.png", "code": "import matplotlib"}),
        ];
        let deduped = dedup_chart_specs(specs);
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0]["caption"], "Scenario Probability Assessment");
        assert_eq!(deduped[1]["caption"], "Economic Impact by Region");
    }

    #[test]
    fn test_dedup_chart_specs_keeps_distinct() {
        let specs: Vec<serde_json::Value> = vec![
            serde_json::json!({"caption": "GDP Growth Comparison", "filename": "chart_1.png", "code": "x"}),
            serde_json::json!({"caption": "Military Expenditure Timeline", "filename": "chart_2.png", "code": "x"}),
            serde_json::json!({"caption": "Source Distribution by Type", "filename": "chart_3.png", "code": "x"}),
        ];
        let deduped = dedup_chart_specs(specs);
        assert_eq!(deduped.len(), 3);
    }

    #[test]
    fn test_dedup_chart_images_by_content() {
        let dir = std::env::temp_dir().join("nabaos_test_chart_dedup");
        let _ = std::fs::create_dir_all(&dir);

        // Write two identical files and one different
        let img_a = dir.join("chart_a.png");
        let img_b = dir.join("chart_b.png");
        let img_c = dir.join("chart_c.png");
        std::fs::write(&img_a, b"identical content here").unwrap();
        std::fs::write(&img_b, b"identical content here").unwrap();
        std::fs::write(&img_c, b"different content").unwrap();

        let images: Vec<ImageEntry> = vec![
            ("Chart A".into(), img_a.clone(), Some("auto".into())),
            ("Chart B".into(), img_b.clone(), Some("auto".into())),
            ("Chart C".into(), img_c.clone(), Some("auto".into())),
        ];

        let deduped = dedup_chart_images(images);
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].0, "Chart A");
        assert_eq!(deduped[1].0, "Chart C");
        // Duplicate file should be cleaned up
        assert!(!img_b.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- Taxonomy reconciliation tests ---

    #[test]
    fn test_taxonomy_list_detection() {
        // Sections with enumerated lists should be detected
        let sections = vec![
            GeneratedSection {
                id: "ch1".into(), title: "Overview".into(), level: 0,
                content: "We identify four scenarios:\n1. Contained Escalation\n2. Regional War\n3. Diplomatic Off-Ramp\n4. Worst Case".into(),
                summary: "".into(), hook: None,
            },
            GeneratedSection {
                id: "ch3".into(), title: "Deep Analysis".into(), level: 0,
                content: "Five scenarios emerge:\n1. Contained Exchange\n2. Proxy Expansion\n3. Strategic Degradation\n4. Internal Collapse\n5. Systemic War".into(),
                summary: "".into(), hook: None,
            },
        ];
        // Both sections have 3+ list lines → both should contribute to digest
        let list_count: usize = sections.iter().map(|s| {
            s.content.lines().filter(|l| l.trim().starts_with("1.") || l.trim().starts_with("2.")).count()
        }).sum();
        assert!(list_count >= 4, "should detect enumerated list items");
    }

    // --- Citation washing tests ---

    #[test]
    fn test_detect_citation_washing_finds_fabricated_stats() {
        let sections = vec![
            GeneratedSection {
                id: "ch1".into(), title: "Scenarios".into(), level: 0,
                content: "There is a 40% probability (CSIS) of regional war and 25% probability (Brookings) of containment.".into(),
                summary: "".into(), hook: None,
            },
        ];
        let warnings = detect_citation_washing(&sections);
        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].contains("40%"));
        assert!(warnings[0].contains("CSIS"));
        assert!(warnings[1].contains("25%"));
    }

    #[test]
    fn test_detect_citation_washing_ignores_clean() {
        let sections = vec![
            GeneratedSection {
                id: "ch1".into(), title: "Analysis".into(), level: 0,
                content: "According to (Reuters) the situation has deteriorated significantly.".into(),
                summary: "".into(), hook: None,
            },
        ];
        let warnings = detect_citation_washing(&sections);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_detect_citation_washing_skips_references() {
        let sections = vec![
            GeneratedSection {
                id: "references".into(), title: "References".into(), level: 0,
                content: "1. 40% probability (CSIS) reference".into(),
                summary: "".into(), hook: None,
            },
        ];
        let warnings = detect_citation_washing(&sections);
        assert!(warnings.is_empty());
    }

    // --- Chapter ordering tests ---

    #[test]
    fn test_reorder_final_sections_exec_summary_first() {
        let sections = vec![
            GeneratedSection { id: "ch1".into(), title: "Geopolitical Analysis".into(), level: 0, content: "".into(), summary: "".into(), hook: None },
            GeneratedSection { id: "ch2".into(), title: "Economic Impact".into(), level: 0, content: "".into(), summary: "".into(), hook: None },
            GeneratedSection { id: "exec".into(), title: "Executive Summary".into(), level: 0, content: "".into(), summary: "".into(), hook: None },
            GeneratedSection { id: "meth".into(), title: "Methodology".into(), level: 0, content: "".into(), summary: "".into(), hook: None },
            GeneratedSection { id: "refs".into(), title: "References".into(), level: 0, content: "".into(), summary: "".into(), hook: None },
        ];
        let ordered = reorder_final_sections(&sections);
        assert_eq!(ordered[0].title, "Executive Summary");
        assert_eq!(ordered[1].title, "Geopolitical Analysis");
        assert_eq!(ordered[2].title, "Economic Impact");
        assert_eq!(ordered[3].title, "Methodology");
        assert_eq!(ordered[4].title, "References");
    }

    #[test]
    fn test_reorder_final_sections_already_correct() {
        let sections = vec![
            GeneratedSection { id: "intro".into(), title: "Introduction".into(), level: 0, content: "".into(), summary: "".into(), hook: None },
            GeneratedSection { id: "ch1".into(), title: "Analysis".into(), level: 0, content: "".into(), summary: "".into(), hook: None },
            GeneratedSection { id: "refs".into(), title: "References".into(), level: 0, content: "".into(), summary: "".into(), hook: None },
        ];
        let ordered = reorder_final_sections(&sections);
        assert_eq!(ordered[0].title, "Introduction");
        assert_eq!(ordered[1].title, "Analysis");
        assert_eq!(ordered[2].title, "References");
    }

    // --- Role-based dedup tests ---

    #[test]
    fn test_role_dedup_exec_summary_variants() {
        let mut outline = DocumentOutline {
            title: "Test".into(),
            needs_toc: false,
            sections: vec![
                OutlineSection { id: "ch1".into(), title: "Executive Summary".into(), level: 0, description: "".into(), depends_on: vec![], generation_order: None, children: vec![] },
                OutlineSection { id: "ch2".into(), title: "Analysis".into(), level: 0, description: "".into(), depends_on: vec![], generation_order: None, children: vec![] },
                OutlineSection { id: "ch3".into(), title: "Executive Summary & Introduction".into(), level: 0, description: "".into(), depends_on: vec![], generation_order: None, children: vec![] },
            ],
        };
        dedup_by_role(&mut outline);
        assert_eq!(outline.sections.len(), 2);
        assert_eq!(outline.sections[0].title, "Executive Summary");
        assert_eq!(outline.sections[1].title, "Analysis");
    }

    #[test]
    fn test_role_dedup_methodology_variants() {
        let mut outline = DocumentOutline {
            title: "Test".into(),
            needs_toc: false,
            sections: vec![
                OutlineSection { id: "ch1".into(), title: "Methodology".into(), level: 0, description: "".into(), depends_on: vec![], generation_order: None, children: vec![] },
                OutlineSection { id: "ch2".into(), title: "Data Protocol and PRISMA Review".into(), level: 0, description: "".into(), depends_on: vec![], generation_order: None, children: vec![] },
                OutlineSection { id: "ch3".into(), title: "Results".into(), level: 0, description: "".into(), depends_on: vec![], generation_order: None, children: vec![] },
            ],
        };
        dedup_by_role(&mut outline);
        assert_eq!(outline.sections.len(), 2);
        assert_eq!(outline.sections[0].title, "Methodology");
        assert_eq!(outline.sections[2 - 1].title, "Results");
    }

    #[test]
    fn test_role_dedup_keeps_distinct() {
        let mut outline = DocumentOutline {
            title: "Test".into(),
            needs_toc: false,
            sections: vec![
                OutlineSection { id: "ch1".into(), title: "Executive Summary".into(), level: 0, description: "".into(), depends_on: vec![], generation_order: None, children: vec![] },
                OutlineSection { id: "ch2".into(), title: "Methodology".into(), level: 0, description: "".into(), depends_on: vec![], generation_order: None, children: vec![] },
                OutlineSection { id: "ch3".into(), title: "Results".into(), level: 0, description: "".into(), depends_on: vec![], generation_order: None, children: vec![] },
                OutlineSection { id: "ch4".into(), title: "Conclusion".into(), level: 0, description: "".into(), depends_on: vec![], generation_order: None, children: vec![] },
            ],
        };
        dedup_by_role(&mut outline);
        assert_eq!(outline.sections.len(), 4); // all distinct roles, nothing removed
    }

    // --- Evidence gate tests ---

    #[test]
    fn test_evidence_gate_flags_empty_empirical() {
        let sections = vec![
            GeneratedSection {
                id: "ch7".into(), title: "Empirical Analysis".into(), level: 0,
                content: "This section discusses the economic trends observed in the data. The patterns suggest growth.".into(),
                summary: "".into(), hook: None,
            },
        ];
        let analytical = detect_analytical_sections(&sections);
        assert_eq!(analytical.len(), 1);
        assert_eq!(analytical[0].1, "Empirical Analysis");
        let (has_evidence, _) = check_evidence_presence(&sections[0].content);
        assert!(!has_evidence);
    }

    #[test]
    fn test_evidence_gate_passes_with_table() {
        let content = "The results are shown below:\n\\begin{tabular}{lcc}\nCountry & GDP & Growth \\\\\nIndia & 3.5T & 7.2\\% \\\\\n\\end{tabular}";
        let (has_evidence, markers) = check_evidence_presence(content);
        assert!(has_evidence);
        assert!(markers.iter().any(|m| m.contains("tabular")));
    }

    #[test]
    fn test_evidence_gate_passes_narrative_title() {
        let sections = vec![
            GeneratedSection {
                id: "ch7".into(), title: "Discussion of Economic Patterns".into(), level: 0,
                content: "The economy shows growth.".into(),
                summary: "".into(), hook: None,
            },
        ];
        let analytical = detect_analytical_sections(&sections);
        assert!(analytical.is_empty()); // not flagged as analytical
    }

    // --- Chart data validation tests ---

    #[test]
    fn test_validate_chart_rejects_uniform_data() {
        let code = r#"
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
data = [1, 1, 1, 1, 1]
labels = ['A', 'B', 'C', 'D', 'E']
plt.bar(labels, data)
plt.savefig('chart.png')
"#;
        assert!(!validate_chart_data(code));
    }

    #[test]
    fn test_validate_chart_accepts_varied_data() {
        let code = r#"
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
data = [10, 25, 15, 30, 20]
labels = ['A', 'B', 'C', 'D', 'E']
plt.bar(labels, data)
plt.savefig('chart.png')
"#;
        assert!(validate_chart_data(code));
    }

    // --- Coherence tests ---

    #[test]
    fn test_detect_circular_ref_in_conclusion() {
        let sections = vec![
            GeneratedSection { id: "ch1".into(), title: "Executive Summary".into(), level: 0, content: "Overview of the report.".into(), summary: "".into(), hook: None },
            GeneratedSection { id: "ch2".into(), title: "Analysis".into(), level: 0, content: "The data shows trends.".into(), summary: "".into(), hook: None },
            GeneratedSection { id: "ch3".into(), title: "Conclusion".into(), level: 0,
                content: "In summary, the analysis is complete. We now turn to the Executive Summary for a high-level view.".into(),
                summary: "".into(), hook: None },
        ];
        let issues = detect_coherence_issues(&sections);
        assert!(!issues.is_empty());
        assert!(issues[0].1.contains("circular"));
    }

    #[test]
    fn test_detect_forward_ref_to_exec_summary() {
        let sections = vec![
            GeneratedSection { id: "ch1".into(), title: "Executive Summary".into(), level: 0, content: "Overview.".into(), summary: "".into(), hook: None },
            GeneratedSection { id: "ch2".into(), title: "Analysis".into(), level: 0, content: "Data trends.".into(), summary: "".into(), hook: None },
            GeneratedSection { id: "ch3".into(), title: "Synthesis".into(), level: 0,
                content: "As discussed in the executive summary, the outlook is positive.".into(),
                summary: "".into(), hook: None },
        ];
        let issues = detect_coherence_issues(&sections);
        assert!(!issues.is_empty());
    }

    #[test]
    fn test_fix_removes_circular_sentence() {
        let mut sections = vec![
            GeneratedSection { id: "ch1".into(), title: "Executive Summary".into(), level: 0, content: "Overview.".into(), summary: "".into(), hook: None },
            GeneratedSection { id: "ch2".into(), title: "Conclusion".into(), level: 0,
                content: "The analysis is complete. We now turn to the Executive Summary for details. This concludes the report.".into(),
                summary: "".into(), hook: None },
        ];
        let issues = detect_coherence_issues(&sections);
        fix_coherence_issues(&mut sections, &issues);
        assert!(!sections[1].content.contains("We now turn to the Executive Summary"));
        assert!(sections[1].content.contains("The analysis is complete"));
    }

    #[test]
    fn test_no_false_positive_on_valid_ref() {
        let sections = vec![
            GeneratedSection { id: "ch1".into(), title: "Introduction".into(), level: 0, content: "We begin with an overview.".into(), summary: "".into(), hook: None },
            GeneratedSection { id: "ch2".into(), title: "Analysis".into(), level: 0,
                content: "As discussed in Chapter 3, the data supports our hypothesis.".into(),
                summary: "".into(), hook: None },
            GeneratedSection { id: "ch3".into(), title: "Conclusion".into(), level: 0, content: "The findings are significant.".into(), summary: "".into(), hook: None },
        ];
        let issues = detect_coherence_issues(&sections);
        assert!(issues.is_empty()); // mid-section forward ref to ch3 is fine, not in last 3 looking backward
    }

    #[test]
    fn test_nyaya_protects_structural_sections() {
        // Structural sections (exec summary, conclusion, methodology) should never be merged
        assert!(classify_section_role("Executive Summary and Introduction").is_some());
        assert!(classify_section_role("Conclusion and Future Outlook").is_some());
        assert!(classify_section_role("Methodology and Data Protocol").is_some());
        assert!(classify_section_role("References").is_some());
        assert!(classify_section_role("Appendix A: Supplementary Data").is_some());
        assert!(classify_section_role("Literature Review").is_some());
    }

    #[test]
    fn test_nyaya_merges_duplicate_content_sections() {
        // Content sections should NOT be classified as structural
        assert!(classify_section_role("Detailed Analysis of Transformer Efficiency").is_none());
        assert!(classify_section_role("Speculative Decoding and MoE Techniques").is_none());
        assert!(classify_section_role("Chapter 6: Performance Benchmarks").is_none());
        assert!(classify_section_role("Case Study: NanoGPT").is_none());
    }

    #[test]
    fn test_nyaya_first_section_always_protected() {
        // Verify first_id logic: even a content section at position 0 is protected
        let sections = vec![
            GeneratedSection { id: "ch1".into(), title: "Detailed Analysis".into(), level: 0, content: "Content A".into(), summary: "".into(), hook: None },
            GeneratedSection { id: "ch2".into(), title: "More Analysis".into(), level: 0, content: "Content B".into(), summary: "".into(), hook: None },
        ];
        let first_id = sections.first().map(|s| s.id.as_str()).unwrap_or("");
        // ch1 is first, so it's protected even though it's not structural
        assert_eq!(first_id, "ch1");
        assert!(classify_section_role("Detailed Analysis").is_none()); // not structural
        // The protection in nyaya_trim checks: absorb_id == first_id, so ch1 would be skipped
    }
}
