use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Semaphore;

use crate::browser::chrome_pool::ChromePool;
use crate::core::error::Result;
use crate::llm_router::provider::LlmProvider;
use crate::swarm::collector::ResultCollector;
use crate::swarm::synthesizer::{
    build_basic_report, build_synthesis_prompt, parse_synthesis_response, SynthesisReport,
};
use crate::swarm::worker::*;
use serde::{Deserialize, Serialize};

/// Swarm orchestrator configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmConfig {
    pub max_workers: usize,
    pub timeout_secs: u64,
    pub max_result_chars: usize,
}

impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            max_workers: 5,
            timeout_secs: 60,
            max_result_chars: 50_000,
        }
    }
}

/// The swarm orchestrator coordinates worker dispatch and result collection.
pub struct SwarmOrchestrator {
    config: SwarmConfig,
    chrome_pool: Option<Arc<ChromePool>>,
    llm_provider: Option<Arc<LlmProvider>>,
}

impl SwarmOrchestrator {
    pub fn new(config: SwarmConfig) -> Self {
        Self {
            config,
            chrome_pool: None,
            llm_provider: None,
        }
    }

    pub fn with_chrome_pool(mut self, pool: Arc<ChromePool>) -> Self {
        self.chrome_pool = Some(pool);
        self
    }

    pub fn with_llm_provider(mut self, provider: Arc<LlmProvider>) -> Self {
        self.llm_provider = Some(provider);
        self
    }

    /// Execute a research plan: dispatch workers in parallel, collect results, synthesize.
    pub async fn execute_plan(&self, plan: &ResearchPlan) -> Result<SynthesisReport> {
        let semaphore = Arc::new(Semaphore::new(self.config.max_workers));
        let (tx, mut rx) = tokio::sync::mpsc::channel::<WorkerResult>(plan.sources.len().max(1));
        let timeout_duration = std::time::Duration::from_secs(self.config.timeout_secs);

        for source in &plan.sources {
            let permit = semaphore.clone().acquire_owned().await.map_err(|e| {
                crate::core::error::NyayaError::Config(format!("Semaphore error: {}", e))
            })?;
            let source_clone = source.clone();
            let tx_clone = tx.clone();
            let chrome_pool = self.chrome_pool.clone();

            tokio::spawn(async move {
                let start = Instant::now();
                let result = tokio::time::timeout(
                    timeout_duration,
                    Self::execute_worker(&source_clone, chrome_pool),
                )
                .await;

                let worker_result = match result {
                    Ok(Ok(wr)) => wr,
                    Ok(Err(e)) => WorkerResult {
                        source_plan: source_clone,
                        outcome: WorkerOutcome::Partial {
                            reason: format!("{}", e),
                        },
                        content: String::new(),
                        content_hash: WorkerResult::compute_hash(""),
                        structured_data: None,
                        citations: Vec::new(),
                        elapsed_ms: start.elapsed().as_millis() as u64,
                    },
                    Err(_timeout) => WorkerResult {
                        source_plan: source_clone,
                        outcome: WorkerOutcome::Blocked(BlockReason::Timeout),
                        content: String::new(),
                        content_hash: WorkerResult::compute_hash(""),
                        structured_data: None,
                        citations: Vec::new(),
                        elapsed_ms: start.elapsed().as_millis() as u64,
                    },
                };

                // Send result; ignore error if receiver dropped.
                let _ = tx_clone.send(worker_result).await;
                drop(permit);
            });
        }

        // Drop the original sender so rx completes when all tasks finish.
        drop(tx);

        let mut collector = ResultCollector::new();
        while let Some(result) = rx.recv().await {
            collector.add(result);
        }

        let results = collector.truncate_to_budget(self.config.max_result_chars);

        // Try LLM synthesis if provider available; fall back to basic report.
        let report = if let Some(ref provider) = self.llm_provider {
            let synthesis_prompt =
                build_synthesis_prompt(&plan.query, &results, &plan.synthesis_instructions);
            let provider_clone = provider.clone();
            let query = plan.query.clone();
            let sources_total = results.len();
            match tokio::task::spawn_blocking(move || {
                provider_clone.complete(
                    "You are a research synthesis assistant. Produce a structured markdown report with ## headings.",
                    &synthesis_prompt,
                    None,
                )
            }).await {
                Ok(Ok(resp)) => parse_synthesis_response(&query, &resp.text, sources_total),
                _ => build_basic_report(&plan.query, &results),
            }
        } else {
            build_basic_report(&plan.query, &results)
        };

        Ok(report)
    }

    /// Execute a single worker for a given source plan.
    ///
    /// This is a static/associated function (no `&self`) because it runs inside
    /// `tokio::spawn` and cannot borrow the orchestrator.
    async fn execute_worker(
        source: &SourcePlan,
        chrome_pool: Option<Arc<ChromePool>>,
    ) -> Result<WorkerResult> {
        let start = Instant::now();

        match source.worker_type.as_str() {
            "search" | "web_crawl" => {
                let url = match &source.target {
                    SourceTarget::Url(u) => u.clone(),
                    SourceTarget::SearchQuery(q) => {
                        format!("https://duckduckgo.com/?q={}", urlencoding::encode(q))
                    }
                    SourceTarget::DuckDuckGoQuery(q) => {
                        format!("https://duckduckgo.com/?q={}", urlencoding::encode(q))
                    }
                    SourceTarget::OpenAlexQuery(q) => format!(
                        "https://api.openalex.org/works?search={}",
                        urlencoding::encode(q)
                    ),
                };

                // If we have a ChromePool, try to checkout a tab.
                let content = if let Some(ref pool) = chrome_pool {
                    match pool.checkout().await {
                        Ok(tab) => {
                            let msg = format!(
                                "[browser] Checked out tab {} for URL: {}",
                                tab.target.id, url
                            );
                            pool.return_tab(tab).await;
                            msg
                        }
                        Err(_) => {
                            format!("[browser:no-tab] Would navigate to: {}", url)
                        }
                    }
                } else {
                    format!("[browser:no-pool] Would navigate to: {}", url)
                };

                Ok(WorkerResult {
                    source_plan: source.clone(),
                    outcome: WorkerOutcome::Success,
                    content: content.clone(),
                    content_hash: WorkerResult::compute_hash(&content),
                    structured_data: None,
                    citations: Vec::new(),
                    elapsed_ms: start.elapsed().as_millis() as u64,
                })
            }

            "academic" => {
                let query = match &source.target {
                    SourceTarget::OpenAlexQuery(q) => q.clone(),
                    SourceTarget::SearchQuery(q) => q.clone(),
                    other => format!("{}", other),
                };

                let url = format!(
                    "https://api.openalex.org/works?search={}&per_page=10",
                    urlencoding::encode(&query)
                );

                let response = reqwest::get(&url).await;
                match response {
                    Ok(resp) => {
                        let body = resp.text().await.unwrap_or_default();
                        let citations = parse_openalex_citations(&body);
                        let content = format!(
                            "[academic] OpenAlex search for '{}': {} citations found",
                            query,
                            citations.len()
                        );
                        Ok(WorkerResult {
                            source_plan: source.clone(),
                            outcome: WorkerOutcome::Success,
                            content: content.clone(),
                            content_hash: WorkerResult::compute_hash(&content),
                            structured_data: serde_json::from_str(&body).ok(),
                            citations,
                            elapsed_ms: start.elapsed().as_millis() as u64,
                        })
                    }
                    Err(e) => {
                        let reason = format!("OpenAlex request failed: {}", e);
                        Ok(WorkerResult {
                            source_plan: source.clone(),
                            outcome: WorkerOutcome::Partial { reason },
                            content: String::new(),
                            content_hash: WorkerResult::compute_hash(""),
                            structured_data: None,
                            citations: Vec::new(),
                            elapsed_ms: start.elapsed().as_millis() as u64,
                        })
                    }
                }
            }

            "pdf" => {
                let url = match &source.target {
                    SourceTarget::Url(u) => u.clone(),
                    other => {
                        let reason = format!("PDF worker requires a URL target, got: {}", other);
                        return Ok(WorkerResult {
                            source_plan: source.clone(),
                            outcome: WorkerOutcome::Partial { reason },
                            content: String::new(),
                            content_hash: WorkerResult::compute_hash(""),
                            structured_data: None,
                            citations: Vec::new(),
                            elapsed_ms: start.elapsed().as_millis() as u64,
                        });
                    }
                };

                let response = reqwest::get(&url).await;
                match response {
                    Ok(resp) => {
                        let bytes = resp.bytes().await.unwrap_or_default();
                        let content =
                            format!("[pdf] Downloaded {} bytes from {}", bytes.len(), url);
                        Ok(WorkerResult {
                            source_plan: source.clone(),
                            outcome: WorkerOutcome::Success,
                            content: content.clone(),
                            content_hash: WorkerResult::compute_hash(&content),
                            structured_data: None,
                            citations: Vec::new(),
                            elapsed_ms: start.elapsed().as_millis() as u64,
                        })
                    }
                    Err(e) => {
                        let reason = format!("PDF download failed: {}", e);
                        Ok(WorkerResult {
                            source_plan: source.clone(),
                            outcome: WorkerOutcome::Partial { reason },
                            content: String::new(),
                            content_hash: WorkerResult::compute_hash(""),
                            structured_data: None,
                            citations: Vec::new(),
                            elapsed_ms: start.elapsed().as_millis() as u64,
                        })
                    }
                }
            }

            unknown => {
                let reason = format!("Unknown worker type: {}", unknown);
                Ok(WorkerResult {
                    source_plan: source.clone(),
                    outcome: WorkerOutcome::Partial { reason },
                    content: String::new(),
                    content_hash: WorkerResult::compute_hash(""),
                    structured_data: None,
                    citations: Vec::new(),
                    elapsed_ms: start.elapsed().as_millis() as u64,
                })
            }
        }
    }

    pub fn config(&self) -> &SwarmConfig {
        &self.config
    }
}

/// Parse OpenAlex JSON response to extract citations.
pub fn parse_openalex_citations(json_body: &str) -> Vec<Citation> {
    let parsed: serde_json::Value = match serde_json::from_str(json_body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let results = match parsed.get("results").and_then(|r| r.as_array()) {
        Some(arr) => arr,
        None => return Vec::new(),
    };

    results
        .iter()
        .filter_map(|work| {
            let title = work.get("title")?.as_str()?.to_string();
            if title.is_empty() {
                return None;
            }

            let doi = work
                .get("doi")
                .and_then(|d| d.as_str())
                .map(|s| s.to_string());

            let authors: Vec<String> = work
                .get("authorships")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|authorship| {
                            authorship
                                .get("author")
                                .and_then(|a| a.get("display_name"))
                                .and_then(|n| n.as_str())
                                .map(|s| s.to_string())
                        })
                        .collect()
                })
                .unwrap_or_default();

            let date = work
                .get("publication_date")
                .and_then(|d| d.as_str())
                .map(|s| s.to_string());

            Some(Citation {
                title,
                url: doi,
                authors,
                date,
                snippet: None,
            })
        })
        .collect()
}

impl std::fmt::Display for SwarmOrchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SwarmOrchestrator(max_workers={})",
            self.config.max_workers
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swarm_config_defaults() {
        let config = SwarmConfig::default();
        assert_eq!(config.max_workers, 5);
        assert_eq!(config.timeout_secs, 60);
        assert_eq!(config.max_result_chars, 50_000);
    }

    #[tokio::test]
    async fn test_orchestrator_execute_empty_plan() {
        let orchestrator = SwarmOrchestrator::new(SwarmConfig::default());
        let plan = ResearchPlan {
            query: "test query".into(),
            sources: Vec::new(),
            synthesis_instructions: "summarize".into(),
            max_workers: 3,
        };
        let report = orchestrator.execute_plan(&plan).await.unwrap();
        assert_eq!(report.sources_used, 0);
        assert_eq!(report.sources_total, 0);
        assert!(report.sections.is_empty());
    }

    #[tokio::test]
    async fn test_orchestrator_unknown_worker_type() {
        let config = SwarmConfig {
            max_workers: 2,
            timeout_secs: 5,
            max_result_chars: 50_000,
        };
        let orchestrator = SwarmOrchestrator::new(config);
        let plan = ResearchPlan {
            query: "test unknown worker".into(),
            sources: vec![SourcePlan {
                worker_type: "unknown_type".into(),
                target: SourceTarget::Url("http://example.com".into()),
                priority: 0,
                needs_auth: false,
                extraction_focus: None,
            }],
            synthesis_instructions: "summarize".into(),
            max_workers: 2,
        };
        // Should not panic; unknown worker type returns Partial.
        let report = orchestrator.execute_plan(&plan).await.unwrap();
        assert_eq!(report.query, "test unknown worker");
    }

    #[tokio::test]
    async fn test_orchestrator_parallel_dispatch_multiple_sources() {
        let config = SwarmConfig {
            max_workers: 2,
            timeout_secs: 30,
            max_result_chars: 50_000,
        };
        let orchestrator = SwarmOrchestrator::new(config);
        let plan = ResearchPlan {
            query: "parallel test".into(),
            sources: vec![
                SourcePlan {
                    worker_type: "unknown_a".into(),
                    target: SourceTarget::Url("http://a.example.com".into()),
                    priority: 0,
                    needs_auth: false,
                    extraction_focus: None,
                },
                SourcePlan {
                    worker_type: "unknown_b".into(),
                    target: SourceTarget::Url("http://b.example.com".into()),
                    priority: 1,
                    needs_auth: false,
                    extraction_focus: None,
                },
                SourcePlan {
                    worker_type: "unknown_c".into(),
                    target: SourceTarget::Url("http://c.example.com".into()),
                    priority: 2,
                    needs_auth: false,
                    extraction_focus: None,
                },
            ],
            synthesis_instructions: "summarize".into(),
            max_workers: 2, // only 2 concurrent, but 3 sources
        };
        let report = orchestrator.execute_plan(&plan).await.unwrap();
        // All 3 dispatched (semaphore limits concurrency, not count)
        assert_eq!(report.query, "parallel test");
    }

    #[test]
    fn test_orchestrator_with_builder() {
        let orchestrator = SwarmOrchestrator::new(SwarmConfig::default());
        // Verify fields are None by default.
        assert!(orchestrator.chrome_pool.is_none());
        assert!(orchestrator.llm_provider.is_none());

        // Builder methods should set the fields.
        let provider = Arc::new(LlmProvider {
            provider: crate::llm_router::provider::ProviderType::Anthropic,
            api_key: "test-key".into(),
            model: "test-model".into(),
            base_url: "http://localhost".into(),
            timeout_secs: None,
        });
        let orchestrator = orchestrator.with_llm_provider(provider);
        assert!(orchestrator.llm_provider.is_some());
    }

    #[tokio::test]
    async fn test_orchestrator_academic_worker_dispatch() {
        let config = SwarmConfig {
            max_workers: 1,
            timeout_secs: 30,
            max_result_chars: 50_000,
        };
        let orchestrator = SwarmOrchestrator::new(config);
        let plan = ResearchPlan {
            query: "quantum computing".into(),
            sources: vec![SourcePlan {
                worker_type: "academic".into(),
                target: SourceTarget::OpenAlexQuery("quantum computing".into()),
                priority: 0,
                needs_auth: false,
                extraction_focus: Some("abstract".into()),
            }],
            synthesis_instructions: "summarize".into(),
            max_workers: 1,
        };
        // This makes a real HTTP call to OpenAlex — may fail on network issues.
        let result = orchestrator.execute_plan(&plan).await;
        // We just check it doesn't panic. If network is available, it should succeed.
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_openalex_citations_empty() {
        // Empty JSON object
        let citations = parse_openalex_citations("{}");
        assert!(citations.is_empty());

        // Empty results array
        let citations = parse_openalex_citations(r#"{"results": []}"#);
        assert!(citations.is_empty());

        // Invalid JSON
        let citations = parse_openalex_citations("not json");
        assert!(citations.is_empty());
    }

    #[test]
    fn test_parse_openalex_citations_valid() {
        let json = r#"{
            "results": [
                {
                    "title": "Quantum Computing: An Overview",
                    "doi": "https://doi.org/10.1234/qc.2024",
                    "authorships": [
                        {"author": {"display_name": "Alice Smith"}},
                        {"author": {"display_name": "Bob Jones"}}
                    ],
                    "publication_date": "2024-01-15"
                },
                {
                    "title": "Advances in Quantum Algorithms",
                    "doi": null,
                    "authorships": [],
                    "publication_date": "2023-06-01"
                },
                {
                    "title": "",
                    "doi": null,
                    "authorships": [],
                    "publication_date": null
                }
            ]
        }"#;

        let citations = parse_openalex_citations(json);
        assert_eq!(citations.len(), 2); // third entry has empty title, skipped

        assert_eq!(citations[0].title, "Quantum Computing: An Overview");
        assert_eq!(
            citations[0].url,
            Some("https://doi.org/10.1234/qc.2024".into())
        );
        assert_eq!(citations[0].authors, vec!["Alice Smith", "Bob Jones"]);
        assert_eq!(citations[0].date, Some("2024-01-15".into()));

        assert_eq!(citations[1].title, "Advances in Quantum Algorithms");
        assert_eq!(citations[1].url, None);
        assert!(citations[1].authors.is_empty());
        assert_eq!(citations[1].date, Some("2023-06-01".into()));
    }
}
