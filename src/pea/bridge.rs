// PEA Execution Bridge — routes tasks to abilities via AbilityRegistry.
//
// The bridge translates PeaTask descriptions into ability calls, threading
// context from prior completed tasks so each step builds on the last.
// Swarm-routed tasks use SwarmOrchestrator for real web search via DuckDuckGo,
// with sync fallback through browser.fetch + research.wide abilities.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::pea::executor::{classify_task, TaskResult, TaskRoute};
use crate::runtime::host_functions::AbilityRegistry;
use crate::runtime::manifest::AgentManifest;
use crate::swarm::orchestrator::{SwarmConfig, SwarmOrchestrator};
use crate::swarm::worker::{ResearchPlan, SourcePlan, SourceTarget};

/// Extract cost from ability result facts (if provider reports it).
fn parse_cost(facts: &HashMap<String, String>) -> f64 {
    facts
        .get("cost_usd")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0)
}

// ---------------------------------------------------------------------------
// PeaBridge
// ---------------------------------------------------------------------------

pub struct PeaBridge<'a> {
    registry: &'a AbilityRegistry,
    manifest: &'a AgentManifest,
    output_dir: PathBuf,
}

impl<'a> PeaBridge<'a> {
    pub fn new(
        registry: &'a AbilityRegistry,
        manifest: &'a AgentManifest,
        output_dir: &Path,
    ) -> Self {
        Self {
            registry,
            manifest,
            output_dir: output_dir.to_path_buf(),
        }
    }

    /// Execute a single task, using prior results for context threading.
    ///
    /// `prior_results` is a list of `(task_description, result_text)` from
    /// earlier completed tasks within the same objective.
    pub fn execute_task(
        &self,
        task_description: &str,
        objective_description: &str,
        prior_results: &[(String, String)],
    ) -> TaskResult {
        let route = classify_task(task_description);

        match route {
            TaskRoute::Llm => self.execute_llm(task_description, objective_description, prior_results),
            TaskRoute::Media => self.execute_media(task_description, objective_description, prior_results),
            TaskRoute::FileSystem => self.execute_filesystem(task_description, objective_description, prior_results),
            TaskRoute::Swarm => self.execute_swarm(task_description, objective_description, prior_results),
            _ => self.execute_llm(task_description, objective_description, prior_results),
        }
    }

    // -- Route handlers -------------------------------------------------------

    fn execute_llm(
        &self,
        task_description: &str,
        objective_description: &str,
        prior_results: &[(String, String)],
    ) -> TaskResult {
        // Step 1: Fetch web context for grounding (reduces hallucination, adds real data)
        let web_context = self.fetch_web_context(task_description, objective_description);

        let system = if web_context.is_empty() {
            build_system_prompt(objective_description, task_description)
        } else {
            format!(
                "{}\n\n\
                 WEB SEARCH GROUNDING (use these real sources to inform your response — \
                 cite URLs where relevant, do NOT fabricate references):\n{}",
                build_system_prompt(objective_description, task_description),
                web_context
            )
        };

        let prompt = build_user_prompt(task_description, prior_results);

        let input = serde_json::json!({
            "system": system,
            "prompt": prompt,
            "max_tokens": 16384,
        });

        match self.registry.execute_ability(
            self.manifest,
            "llm.chat",
            &input.to_string(),
        ) {
            Ok(result) => {
                let output = String::from_utf8_lossy(&result.output).to_string();
                let cost = parse_cost(&result.facts);
                TaskResult {
                    success: true,
                    output,
                    artifacts: vec![],
                    cost_usd: cost,
                }
            }
            Err(e) => TaskResult {
                success: false,
                output: format!("LLM execution failed: {}", e),
                artifacts: vec![],
                cost_usd: 0.0,
            },
        }
    }

    /// Fetch web context for grounding any task. Tries SwarmOrchestrator first,
    /// then sync fallback. Returns empty string on failure (non-blocking).
    fn fetch_web_context(&self, task_description: &str, objective_description: &str) -> String {
        let search_query = self.generate_search_query(task_description, objective_description);
        tracing::info!(query = %search_query, "PEA: fetching web context for task grounding");

        // Try SwarmOrchestrator first
        match self.try_swarm_orchestrator(&search_query) {
            Ok(report) => {
                let mut content = format!(
                    "# Web Search Results for: {}\n\n{}\n\nSources used: {}/{}",
                    report.query, report.summary, report.sources_used, report.sources_total
                );
                for cite in &report.citations {
                    if let Some(ref url) = cite.url {
                        content.push_str(&format!("\n- [{}]({})", cite.title, url));
                    } else {
                        content.push_str(&format!("\n- {}", cite.title));
                    }
                }
                tracing::info!(sources = report.sources_used, "PEA: SwarmOrchestrator returned results");
                content
            }
            Err(swarm_err) => {
                tracing::warn!(error = %swarm_err, "PEA: SwarmOrchestrator failed, trying sync fallback");
                let fallback = self.fallback_sync_search(&search_query);
                if fallback.contains("[Web search unavailable") {
                    tracing::warn!("PEA: sync fallback also failed — proceeding without web context");
                    String::new()
                } else {
                    tracing::info!("PEA: sync fallback returned web context");
                    fallback
                }
            }
        }
    }

    fn execute_media(
        &self,
        task_description: &str,
        objective_description: &str,
        _prior_results: &[(String, String)],
    ) -> TaskResult {
        // First, ask LLM to generate an image description prompt
        let desc_input = serde_json::json!({
            "system": "You are an image prompt engineer. Given a task description, produce a concise, vivid image generation prompt (1-2 sentences). Output ONLY the prompt text.",
            "prompt": format!("Task: {}\nObjective: {}", task_description, objective_description),
        });

        let image_prompt = match self.registry.execute_ability(
            self.manifest,
            "llm.chat",
            &desc_input.to_string(),
        ) {
            Ok(r) => String::from_utf8_lossy(&r.output).trim().to_string(),
            Err(_) => task_description.to_string(),
        };

        // Attempt media.generate_image if available
        let gen_input = serde_json::json!({
            "prompt": image_prompt,
            "output_dir": self.output_dir.to_string_lossy(),
        });

        match self.registry.execute_ability(
            self.manifest,
            "media.generate_image",
            &gen_input.to_string(),
        ) {
            Ok(result) => {
                let output = String::from_utf8_lossy(&result.output).to_string();
                let cost = parse_cost(&result.facts);
                TaskResult {
                    success: true,
                    output,
                    artifacts: vec![self.output_dir.to_string_lossy().into_owned()],
                    cost_usd: cost,
                }
            }
            Err(_) => {
                // Fallback: generate TikZ code via LLM
                let tikz_input = serde_json::json!({
                    "system": "You are a LaTeX/TikZ expert. Generate a TikZ picture that illustrates the concept described. Output ONLY the \\begin{tikzpicture}...\\end{tikzpicture} block.",
                    "prompt": format!("Create a TikZ illustration for: {}", image_prompt),
                });

                match self.registry.execute_ability(
                    self.manifest,
                    "llm.chat",
                    &tikz_input.to_string(),
                ) {
                    Ok(r) => {
                        let output = String::from_utf8_lossy(&r.output).to_string();
                        let cost = parse_cost(&r.facts);
                        TaskResult {
                            success: true,
                            output: format!("[TikZ illustration]\n{}", output),
                            artifacts: vec![],
                            cost_usd: cost,
                        }
                    }
                    Err(e) => TaskResult {
                        success: false,
                        output: format!("Media generation failed, TikZ fallback failed: {}", e),
                        artifacts: vec![],
                        cost_usd: 0.0,
                    },
                }
            }
        }
    }

    fn execute_filesystem(
        &self,
        task_description: &str,
        objective_description: &str,
        prior_results: &[(String, String)],
    ) -> TaskResult {
        // Use LLM to generate the content, then write directly to output_dir.
        // We use std::fs::write instead of the sandboxed files.write ability
        // because PEA output writes are internal engine operations and the
        // sandbox's path traversal guard rejects absolute paths.
        let content_result = self.execute_llm(task_description, objective_description, prior_results);
        if !content_result.success {
            return content_result;
        }

        let filename = sanitize_filename(task_description);
        let file_path = self.output_dir.join(&filename);

        // Ensure output directory exists
        if let Err(e) = std::fs::create_dir_all(&self.output_dir) {
            return TaskResult {
                success: true,
                output: format!("{}\n\n[Note: failed to create output dir: {}]", content_result.output, e),
                artifacts: vec![],
                cost_usd: content_result.cost_usd,
            };
        }

        match std::fs::write(&file_path, &content_result.output) {
            Ok(_) => TaskResult {
                success: true,
                output: content_result.output,
                artifacts: vec![file_path.to_string_lossy().into_owned()],
                cost_usd: content_result.cost_usd,
            },
            Err(e) => {
                // Writing failed but we still have the content
                TaskResult {
                    success: true,
                    output: format!("{}\n\n[Note: file write failed: {}]", content_result.output, e),
                    artifacts: vec![],
                    cost_usd: content_result.cost_usd,
                }
            }
        }
    }

    fn execute_swarm(
        &self,
        task_description: &str,
        objective_description: &str,
        prior_results: &[(String, String)],
    ) -> TaskResult {
        // Swarm route now handled by execute_llm which does web search for all tasks
        self.execute_llm(task_description, objective_description, prior_results)
    }

    // -- Swarm helpers --------------------------------------------------------

    /// Ask LLM to convert a task description into a concise web search query.
    fn generate_search_query(&self, task_description: &str, objective_description: &str) -> String {
        let input = serde_json::json!({
            "system": "You convert task descriptions into concise web search queries. \
                       Output ONLY the search query text — no explanation, no quotes, no prefix.",
            "prompt": format!(
                "Objective: {}\nTask: {}\n\nGenerate a focused web search query (5-10 words):",
                objective_description, task_description
            ),
        });

        match self.registry.execute_ability(self.manifest, "llm.chat", &input.to_string()) {
            Ok(r) => {
                let query = String::from_utf8_lossy(&r.output).trim().to_string();
                if query.is_empty() || query.len() > 200 {
                    // Fallback: use first 100 chars of task description
                    task_description.chars().take(100).collect()
                } else {
                    query
                }
            }
            Err(_) => task_description.chars().take(100).collect(),
        }
    }

    /// Try SwarmOrchestrator with DuckDuckGo search.
    /// Uses Handle::try_current / Runtime::new pattern for async-from-sync.
    fn try_swarm_orchestrator(
        &self,
        search_query: &str,
    ) -> crate::core::error::Result<crate::swarm::synthesizer::SynthesisReport> {
        let orchestrator = SwarmOrchestrator::new(SwarmConfig::default());

        // Build LLM provider for synthesis if available
        let orchestrator = if let Some(provider) = self.registry.llm_provider() {
            orchestrator.with_llm_provider(Arc::new(provider.clone()))
        } else {
            orchestrator
        };

        let plan = ResearchPlan {
            query: search_query.to_string(),
            sources: vec![SourcePlan {
                worker_type: "search".into(),
                target: SourceTarget::DuckDuckGoQuery(search_query.to_string()),
                priority: 0,
                needs_auth: false,
                extraction_focus: Some("relevant results".into()),
            }],
            synthesis_instructions: format!("Synthesize research findings for: {}", search_query),
            max_workers: 5,
        };

        tokio::runtime::Handle::try_current()
            .map(|handle| {
                tokio::task::block_in_place(|| handle.block_on(orchestrator.execute_plan(&plan)))
            })
            .unwrap_or_else(|_| {
                let rt = tokio::runtime::Runtime::new().map_err(|e| {
                    crate::core::error::NyayaError::Config(format!(
                        "Failed to create runtime: {}",
                        e
                    ))
                })?;
                rt.block_on(orchestrator.execute_plan(&plan))
            })
    }

    /// Sync fallback: use browser.fetch for DDG search + research.wide for URLs.
    fn fallback_sync_search(&self, search_query: &str) -> String {
        // Try browser.fetch with DuckDuckGo query
        let fetch_input = serde_json::json!({
            "url": format!("https://duckduckgo.com/?q={}", urlencoding::encode(search_query)),
        });

        let ddg_results = match self.registry.execute_ability(
            self.manifest,
            "browser.fetch",
            &fetch_input.to_string(),
        ) {
            Ok(r) => String::from_utf8_lossy(&r.output).to_string(),
            Err(e) => {
                tracing::warn!("browser.fetch DDG fallback failed: {e}");
                return format!("[Web search unavailable — search query was: {}]", search_query);
            }
        };

        // Extract URLs from DDG results and fetch them via research.wide
        let urls: Vec<String> = ddg_results
            .lines()
            .filter_map(|line| {
                if let Some(start) = line.find("http") {
                    let url_part = &line[start..];
                    let end = url_part.find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                        .unwrap_or(url_part.len());
                    let url = &url_part[..end];
                    if !url.contains("duckduckgo.com") {
                        Some(url.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .take(5)
            .collect();

        if urls.is_empty() {
            return format!(
                "# DuckDuckGo Search Results\nQuery: {}\n\n{}\n\n[No external URLs extracted]",
                search_query, &ddg_results[..ddg_results.len().min(4000)]
            );
        }

        // Fetch URLs via research.wide
        let wide_input = serde_json::json!({
            "urls": urls,
            "query": search_query,
        });

        match self.registry.execute_ability(
            self.manifest,
            "research.wide",
            &wide_input.to_string(),
        ) {
            Ok(r) => {
                let content = String::from_utf8_lossy(&r.output).to_string();
                format!(
                    "# Web Research Results\nQuery: {}\nSources: {} URLs fetched\n\n{}",
                    search_query,
                    urls.len(),
                    &content[..content.len().min(12000)]
                )
            }
            Err(e) => {
                tracing::warn!("research.wide fallback failed: {e}");
                format!(
                    "# DuckDuckGo Search Results\nQuery: {}\n\n{}",
                    search_query, &ddg_results[..ddg_results.len().min(8000)]
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Prompt construction
// ---------------------------------------------------------------------------

fn build_system_prompt(objective: &str, task: &str) -> String {
    format!(
        "You are an expert autonomous agent executing a multi-step objective.\n\n\
         Overall objective: {}\n\n\
         You are now executing the following specific task: {}\n\n\
         Produce detailed, structured, publication-quality output for this task. \
         Be thorough and substantive. If writing content, use clear headings and \
         well-organized paragraphs. If analyzing, provide comprehensive analysis \
         with evidence and reasoning.",
        objective, task
    )
}

fn build_user_prompt(task_description: &str, prior_results: &[(String, String)]) -> String {
    let context = build_context_summary(prior_results);
    if context.is_empty() {
        task_description.to_string()
    } else {
        format!(
            "Context from previously completed tasks (DO NOT repeat content already covered — \
             build on it, reference it, but produce only NEW content for this task):\n\n\
             {}\n\n---\n\nNow execute: {}",
            context, task_description
        )
    }
}

/// Maximum characters per prior task result in context. 12000 chars ≈ 3000
/// tokens per prior result — increased from 6000 to reduce cross-task repetition.
const CONTEXT_TRUNCATE_LIMIT: usize = 12000;

fn build_context_summary(prior_results: &[(String, String)]) -> String {
    if prior_results.is_empty() {
        return String::new();
    }

    prior_results
        .iter()
        .map(|(desc, result)| {
            let truncated = if result.len() > CONTEXT_TRUNCATE_LIMIT {
                format!("{}... [truncated]", &result[..CONTEXT_TRUNCATE_LIMIT])
            } else {
                result.clone()
            };
            format!("### {}\n{}", desc, truncated)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn sanitize_filename(description: &str) -> String {
    let base: String = description
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' })
        .collect();
    let trimmed = if base.len() > 60 { &base[..60] } else { &base };
    format!("{}.txt", trimmed.trim_end_matches('_'))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_system_prompt_contains_objective_and_task() {
        let prompt = build_system_prompt("Write a cookbook", "Create outline");
        assert!(prompt.contains("Write a cookbook"));
        assert!(prompt.contains("Create outline"));
    }

    #[test]
    fn test_build_user_prompt_no_prior() {
        let prompt = build_user_prompt("Create outline", &[]);
        assert_eq!(prompt, "Create outline");
    }

    #[test]
    fn test_build_user_prompt_with_prior() {
        let prior = vec![
            ("Research recipes".to_string(), "Found 20 recipes.".to_string()),
        ];
        let prompt = build_user_prompt("Write introduction", &prior);
        assert!(prompt.contains("Research recipes"));
        assert!(prompt.contains("Found 20 recipes"));
        assert!(prompt.contains("Write introduction"));
    }

    #[test]
    fn test_build_context_summary_truncation() {
        let long_text = "x".repeat(15000);
        let prior = vec![("Task A".to_string(), long_text)];
        let summary = build_context_summary(&prior);
        assert!(summary.contains("[truncated]"));
        assert!(summary.len() < 15000);
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("Write introduction chapter"), "write_introduction_chapter.txt");
        assert_eq!(sanitize_filename("Search web for sources"), "search_web_for_sources.txt");
    }

    #[test]
    fn test_sanitize_filename_long() {
        let long_desc = "a".repeat(100);
        let filename = sanitize_filename(&long_desc);
        assert!(filename.len() <= 64 + 4); // 60 chars + ".txt"
    }

    #[test]
    fn test_context_truncate_limit_increased() {
        // Verify the limit is now 12000, not 6000
        assert_eq!(CONTEXT_TRUNCATE_LIMIT, 12000);
    }
}
