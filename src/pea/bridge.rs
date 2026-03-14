// PEA Execution Bridge — routes tasks to abilities via AbilityRegistry.
//
// The bridge translates PeaTask descriptions into ability calls, threading
// context from prior completed tasks so each step builds on the last.
// Web search grounding uses direct HTTP fetch (Brave Search primary, DDG fallback)
// to provide real-world context for all LLM tasks.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::pea::executor::{classify_task, TaskResult, TaskRoute};
use crate::runtime::host_functions::AbilityRegistry;
use crate::runtime::manifest::AgentManifest;

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
        cached_web_context: Option<&str>,
    ) -> TaskResult {
        let route = classify_task(task_description);

        match route {
            TaskRoute::Llm => self.execute_llm(task_description, objective_description, prior_results, cached_web_context),
            TaskRoute::Media => self.execute_media(task_description, objective_description, prior_results),
            TaskRoute::FileSystem => self.execute_filesystem(task_description, objective_description, prior_results, cached_web_context),
            TaskRoute::Swarm => self.execute_swarm(task_description, objective_description, prior_results, cached_web_context),
            _ => self.execute_llm(task_description, objective_description, prior_results, cached_web_context),
        }
    }

    // -- Route handlers -------------------------------------------------------

    fn execute_llm(
        &self,
        task_description: &str,
        objective_description: &str,
        prior_results: &[(String, String)],
        cached_web_context: Option<&str>,
    ) -> TaskResult {
        // Use cached research context if available, otherwise fetch fresh
        let web_context = match cached_web_context {
            Some(ctx) if !ctx.is_empty() => ctx.to_string(),
            _ => self.fetch_web_context(task_description, objective_description),
        };

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

        let context_len = system.len() + prompt.len();
        eprintln!("[pea] calling llm.chat for task: {} (context: {} chars)", task_description, context_len);

        match self.registry.execute_ability(
            self.manifest,
            "llm.chat",
            &input.to_string(),
        ) {
            Ok(result) => {
                let output = String::from_utf8_lossy(&result.output).to_string();
                let cost = parse_cost(&result.facts);
                eprintln!("[pea] llm.chat completed, output: {} chars, cost: ${:.4}", output.len(), cost);
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

    /// Fetch web context via the ResearchEngine (multi-query, LLM-scored, parallel fetch).
    /// Falls back to single-query search if research engine produces no results.
    fn fetch_web_context(&self, task_description: &str, objective_description: &str) -> String {
        use crate::pea::research::{ResearchConfig, ResearchEngine};

        let engine = ResearchEngine::new(self.registry, self.manifest, ResearchConfig::default());
        let corpus = engine.execute(objective_description, task_description);

        if !corpus.sources.is_empty() {
            return corpus.to_context_string();
        }

        // Fallback: single query if research engine found nothing
        eprintln!("[pea] research engine returned no sources, trying single-query fallback");
        let search_query = self.generate_search_query(task_description, objective_description);
        let search_results = search_brave(&search_query)
            .or_else(|e| {
                eprintln!("[pea] Brave Search failed: {}, trying DDG", e);
                search_ddg(&search_query)
            });

        let results = match search_results {
            Ok(r) if !r.is_empty() => r,
            _ => return String::new(),
        };

        let urls: Vec<&str> = results
            .iter()
            .filter_map(|r| r.url.as_deref())
            .take(3)
            .collect();
        let fetched = fetch_urls_parallel(&urls);

        let mut context = format!("# Web Search: {}\n\n", search_query);
        for (i, result) in results.iter().enumerate().take(8) {
            context.push_str(&format!("## {}. {}\n", i + 1, result.title));
            if let Some(ref url) = result.url {
                context.push_str(&format!("URL: {}\n", url));
            }
            context.push_str(&format!("{}\n\n", result.snippet));
        }

        if !fetched.is_empty() {
            context.push_str("---\n# Fetched Page Content\n\n");
            for (url, text) in &fetched {
                let truncated = if text.len() > 4000 { &text[..4000] } else { text.as_str() };
                context.push_str(&format!("## Source: {}\n{}\n\n", url, truncated));
            }
        }

        context
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
        cached_web_context: Option<&str>,
    ) -> TaskResult {
        // Use LLM to generate the content, then write directly to output_dir.
        // We use std::fs::write instead of the sandboxed files.write ability
        // because PEA output writes are internal engine operations and the
        // sandbox's path traversal guard rejects absolute paths.
        let content_result = self.execute_llm(task_description, objective_description, prior_results, cached_web_context);
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
        cached_web_context: Option<&str>,
    ) -> TaskResult {
        // Swarm route now handled by execute_llm which does web search for all tasks
        self.execute_llm(task_description, objective_description, prior_results, cached_web_context)
    }

    // -- Search helpers -------------------------------------------------------

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
                    task_description.chars().take(100).collect()
                } else {
                    query
                }
            }
            Err(_) => task_description.chars().take(100).collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// Web search functions delegated to research module
// ---------------------------------------------------------------------------

use crate::pea::research::{search_brave, search_ddg, fetch_urls_parallel};

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
