// PEA Research Engine — Manus-scale multi-query search, LLM curation, tiered fetch.
//
// Phases:
//   1. Query Fan-Out: LLM generates 15-20 diverse search queries from objective
//   2. Multi-Engine Search: Brave + DDG in parallel, deduplicate by URL → ~200 candidates
//   3. LLM Relevance Scoring: batch scoring, sort, take top_k_fetch
//   4. Tiered Parallel Fetch: HTTP → ChromePool fallback, with backfill on failure
//   5. Corpus Assembly: deduplicate content, build ResearchCorpus

use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use crate::runtime::host_functions::AbilityRegistry;
use crate::runtime::manifest::AgentManifest;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

pub struct ResearchEngine<'a> {
    registry: &'a AbilityRegistry,
    manifest: &'a AgentManifest,
    config: ResearchConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchBackend {
    ScrapeRotation,
    ChromePool,
    BraveApi,
    SearXng,
}

impl Default for SearchBackend {
    fn default() -> Self {
        Self::ScrapeRotation
    }
}

impl SearchBackend {
    pub fn from_env_str(s: &str) -> Self {
        match s.to_ascii_lowercase().trim() {
            "chrome_pool" | "chromepool" => Self::ChromePool,
            "brave_api" | "braveapi" => Self::BraveApi,
            "searxng" | "searx" => Self::SearXng,
            _ => Self::ScrapeRotation,
        }
    }

    pub fn as_env_str(&self) -> &'static str {
        match self {
            Self::ScrapeRotation => "scrape_rotation",
            Self::ChromePool => "chrome_pool",
            Self::BraveApi => "brave_api",
            Self::SearXng => "searxng",
        }
    }
}

pub struct ResearchConfig {
    pub max_search_queries: usize,
    pub max_candidates: usize,
    pub top_k_fetch: usize,
    pub backfill_pct: f32,
    pub fetch_timeout_secs: u64,
    pub max_content_per_source: usize,
    pub search_backend: SearchBackend,
    pub brave_api_key: Option<String>,
    pub searxng_url: Option<String>,
}

impl Default for ResearchConfig {
    fn default() -> Self {
        Self {
            max_search_queries: 20,
            max_candidates: 200,
            top_k_fetch: 50,
            backfill_pct: 0.10,
            fetch_timeout_secs: 15,
            max_content_per_source: 12000,
            search_backend: SearchBackend::from_env_str(
                &std::env::var("NABA_PEA_SEARCH_BACKEND").unwrap_or_default(),
            ),
            brave_api_key: std::env::var("NABA_BRAVE_API_KEY").ok(),
            searxng_url: std::env::var("NABA_SEARXNG_URL").ok(),
        }
    }
}

#[derive(Clone)]
pub struct SearchCandidate {
    pub url: String,
    pub title: String,
    pub snippet: String,
    pub source_engine: String,
    pub relevance_score: Option<f32>,
}

#[derive(Clone)]
pub struct FetchedSource {
    pub url: String,
    pub title: String,
    pub content: String,
    pub fetch_method: FetchMethod,
    pub char_count: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum FetchMethod {
    Http,
    ChromePool,
}

#[derive(Clone)]
pub struct ResearchCorpus {
    pub query: String,
    pub sources: Vec<FetchedSource>,
    pub failed_urls: Vec<(String, String)>,
    pub total_candidates: usize,
    pub total_chars: usize,
}

impl ResearchCorpus {
    /// Format corpus as context string for LLM grounding.
    pub fn to_context_string(&self) -> String {
        let mut ctx = format!(
            "# Research Corpus ({} sources, {} total chars)\n\n",
            self.sources.len(),
            self.total_chars,
        );

        for (i, source) in self.sources.iter().enumerate() {
            ctx.push_str(&format!(
                "## Source {} — {} ({})\nURL: {}\n{}\n\n",
                i + 1,
                source.title,
                match source.fetch_method {
                    FetchMethod::Http => "HTTP",
                    FetchMethod::ChromePool => "Chrome",
                },
                source.url,
                if source.content.len() > 6000 {
                    format!("{}... [truncated]", safe_slice(&source.content, 6000))
                } else {
                    source.content.clone()
                }
            ));
        }

        if !self.failed_urls.is_empty() {
            ctx.push_str(&format!(
                "\n---\n{} URLs failed to fetch.\n",
                self.failed_urls.len()
            ));
        }

        ctx
    }
}

// ---------------------------------------------------------------------------
// Internal search result type (moved from bridge.rs)
// ---------------------------------------------------------------------------

pub(crate) struct SearchResult {
    pub title: String,
    pub url: Option<String>,
    pub snippet: String,
}

// ---------------------------------------------------------------------------
// ResearchEngine implementation
// ---------------------------------------------------------------------------

impl<'a> ResearchEngine<'a> {
    pub fn new(
        registry: &'a AbilityRegistry,
        manifest: &'a AgentManifest,
        config: ResearchConfig,
    ) -> Self {
        Self { registry, manifest, config }
    }

    /// Execute the full research pipeline: query → search → score → fetch → corpus.
    pub fn execute(&self, objective: &str, task: &str) -> ResearchCorpus {
        eprintln!("[research] starting research for: {}", objective);

        // Phase 1: Generate diverse search queries
        let queries = self.generate_search_queries(objective, task);
        eprintln!("[research] generated {} search queries", queries.len());

        // Phase 2: Multi-engine search
        let mut candidates = self.search_all_engines(&queries);
        eprintln!("[research] found {} unique candidates", candidates.len());

        let total_candidates = candidates.len();

        // Phase 3: LLM relevance scoring
        self.score_candidates(&mut candidates, objective);
        eprintln!(
            "[research] scored candidates, top score: {:.2}",
            candidates.first().and_then(|c| c.relevance_score).unwrap_or(0.0)
        );

        // Phase 4: Tiered fetch with backfill
        let top_k = candidates.len().min(self.config.top_k_fetch);
        let (mut fetched, failed) = self.fetch_sources(&candidates[..top_k]);
        eprintln!(
            "[research] fetched {} sources, {} failed",
            fetched.len(),
            failed.len()
        );

        // Backfill if too many failures
        let failure_rate = if top_k > 0 {
            failed.len() as f32 / top_k as f32
        } else {
            0.0
        };
        if failure_rate > self.config.backfill_pct && candidates.len() > top_k {
            let backfill_count = failed.len().min(candidates.len() - top_k);
            if backfill_count > 0 {
                eprintln!("[research] backfilling {} sources", backfill_count);
                let backfill_slice = &candidates[top_k..top_k + backfill_count];
                let (extra, _) = self.fetch_sources(backfill_slice);
                fetched.extend(extra);
            }
        }

        // Deduplicate fetched content by SipHash
        let fetched = dedup_sources(fetched);

        let total_chars = fetched.iter().map(|s| s.char_count).sum();
        eprintln!(
            "[research] corpus: {} sources, {} chars",
            fetched.len(),
            total_chars
        );

        ResearchCorpus {
            query: objective.to_string(),
            sources: fetched,
            failed_urls: failed,
            total_candidates,
            total_chars,
        }
    }

    // -- Phase 1: Query Fan-Out -----------------------------------------------

    fn generate_search_queries(&self, objective: &str, task: &str) -> Vec<String> {
        let count = self.config.max_search_queries;

        let input = serde_json::json!({
            "system": "You generate diverse web search queries for deep research. \
                       Output one query per line, no numbering, no explanation.",
            "prompt": format!(
                "Generate {} diverse search queries for researching this objective:\n\
                 Objective: {}\n\
                 Current task context: {}\n\n\
                 Include:\n\
                 - Specific factual queries\n\
                 - Broad overview queries\n\
                 - Academic/research queries (add 'research paper' or 'study')\n\
                 - Recent news queries (add '2025' or '2026')\n\
                 - Different angles and subtopics\n\
                 - Expert opinion queries\n\n\
                 Output ONLY the queries, one per line:",
                count, objective, task
            ),
        });

        match self.registry.execute_ability(self.manifest, "llm.chat", &input.to_string()) {
            Ok(result) => {
                let output = String::from_utf8_lossy(&result.output).to_string();
                let queries: Vec<String> = output
                    .lines()
                    .map(|l| l.trim().trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')'))
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty() && l.len() > 3 && l.len() < 200)
                    .take(count)
                    .collect();

                if queries.is_empty() {
                    // Fallback: use objective + task as queries
                    vec![
                        objective.chars().take(100).collect(),
                        task.chars().take(100).collect(),
                    ]
                } else {
                    queries
                }
            }
            Err(e) => {
                eprintln!("[research] query generation failed: {}, using fallback", e);
                vec![
                    objective.chars().take(100).collect(),
                    task.chars().take(100).collect(),
                ]
            }
        }
    }

    // -- Phase 2: Multi-Engine Search -----------------------------------------

    fn search_all_engines(&self, queries: &[String]) -> Vec<SearchCandidate> {
        eprintln!("[research] using search backend: {}", self.config.search_backend.as_env_str());
        match self.config.search_backend {
            SearchBackend::ScrapeRotation => self.search_scrape_rotation(queries),
            SearchBackend::ChromePool => self.search_via_chrome_pool(queries),
            SearchBackend::BraveApi => self.search_via_brave_api(queries),
            SearchBackend::SearXng => self.search_via_searxng(queries),
        }
    }

    /// Original 4-engine HTML scraping rotation (Bing/DDG/Brave/Google).
    fn search_scrape_rotation(&self, queries: &[String]) -> Vec<SearchCandidate> {
        type SearchFn = fn(&str) -> std::result::Result<Vec<SearchResult>, String>;

        let engines: &[(SearchFn, &str)] = &[
            (search_bing, "bing"),
            (search_ddg, "ddg"),
            (search_brave, "brave"),
            (search_google, "google"),
        ];

        let mut candidates = Vec::new();
        let mut seen_urls = HashSet::new();
        let mut engine_failures: [u32; 4] = [0; 4];
        const MAX_CONSECUTIVE_FAILURES: u32 = 3;

        for (i, query) in queries.iter().enumerate() {
            let mut got_results = false;

            for offset in 0..engines.len() {
                let idx = (i + offset) % engines.len();
                if engine_failures[idx] >= MAX_CONSECUTIVE_FAILURES {
                    continue;
                }

                let (search_fn, engine_name) = engines[idx];
                match search_fn(query) {
                    Ok(results) if !results.is_empty() => {
                        engine_failures[idx] = 0;
                        Self::collect_results(&mut candidates, &mut seen_urls, results, engine_name);
                        got_results = true;
                        break;
                    }
                    _ => {
                        engine_failures[idx] += 1;
                        eprintln!("[research] {} failed for query {}, trying next engine", engine_name, i);
                        std::thread::sleep(std::time::Duration::from_millis(300));
                    }
                }
            }

            if !got_results {
                eprintln!("[research] all engines failed for query {}: {:?}", i, &query[..query.len().min(60)]);
            }

            if candidates.len() >= self.config.max_candidates {
                break;
            }

            std::thread::sleep(std::time::Duration::from_millis(1500));
        }

        candidates.truncate(self.config.max_candidates);
        candidates
    }

    /// Search using Brave Search JSON API, fallback to scrape rotation on missing key.
    fn search_via_brave_api(&self, queries: &[String]) -> Vec<SearchCandidate> {
        let api_key = match self.config.brave_api_key.as_deref() {
            Some(k) if !k.is_empty() => k.to_string(),
            _ => {
                eprintln!("[research] Brave API key not set, falling back to scrape rotation");
                return self.search_scrape_rotation(queries);
            }
        };

        let mut candidates = Vec::new();
        let mut seen_urls = HashSet::new();

        for (i, query) in queries.iter().enumerate() {
            match search_brave_api(query, &api_key) {
                Ok(results) => {
                    Self::collect_results(&mut candidates, &mut seen_urls, results, "brave_api");
                }
                Err(e) => {
                    eprintln!("[research] brave_api failed for query {}: {}", i, e);
                }
            }

            if candidates.len() >= self.config.max_candidates {
                break;
            }

            std::thread::sleep(std::time::Duration::from_millis(600));
        }

        candidates.truncate(self.config.max_candidates);
        candidates
    }

    /// Search using self-hosted SearXNG instance.
    fn search_via_searxng(&self, queries: &[String]) -> Vec<SearchCandidate> {
        let base_url = self.config.searxng_url.as_deref().unwrap_or("http://localhost:8888");

        let mut candidates = Vec::new();
        let mut seen_urls = HashSet::new();

        for (i, query) in queries.iter().enumerate() {
            match search_searxng(query, base_url) {
                Ok(results) => {
                    Self::collect_results(&mut candidates, &mut seen_urls, results, "searxng");
                }
                Err(e) => {
                    eprintln!("[research] searxng failed for query {}: {}", i, e);
                }
            }

            if candidates.len() >= self.config.max_candidates {
                break;
            }

            std::thread::sleep(std::time::Duration::from_millis(200));
        }

        candidates.truncate(self.config.max_candidates);
        candidates
    }

    /// Search using headless Chrome via ChromePool, fallback to scrape after 3 consecutive failures.
    fn search_via_chrome_pool(&self, queries: &[String]) -> Vec<SearchCandidate> {
        let mut candidates = Vec::new();
        let mut seen_urls = HashSet::new();
        let mut consecutive_failures: u32 = 0;

        for (i, query) in queries.iter().enumerate() {
            if consecutive_failures >= 3 {
                eprintln!("[research] ChromePool failed 3x, falling back to scrape rotation for remaining queries");
                let remaining: Vec<String> = queries[i..].to_vec();
                let mut fallback = self.search_scrape_rotation(&remaining);
                // Dedup against already-seen URLs
                fallback.retain(|c| seen_urls.insert(c.url.clone()));
                candidates.extend(fallback);
                break;
            }

            match search_chrome_pool(query) {
                Ok(results) => {
                    consecutive_failures = 0;
                    Self::collect_results(&mut candidates, &mut seen_urls, results, "chrome_pool");
                }
                Err(e) => {
                    consecutive_failures += 1;
                    eprintln!("[research] chrome_pool failed for query {}: {}", i, e);
                }
            }

            if candidates.len() >= self.config.max_candidates {
                break;
            }

            std::thread::sleep(std::time::Duration::from_millis(1000));
        }

        candidates.truncate(self.config.max_candidates);
        candidates
    }

    /// Shared helper: collect search results into candidates, deduplicating by URL.
    fn collect_results(
        candidates: &mut Vec<SearchCandidate>,
        seen_urls: &mut HashSet<String>,
        results: Vec<SearchResult>,
        engine_name: &str,
    ) {
        for result in results {
            if let Some(ref url) = result.url {
                if !seen_urls.insert(url.clone()) {
                    continue;
                }
                candidates.push(SearchCandidate {
                    url: url.clone(),
                    title: result.title,
                    snippet: result.snippet,
                    source_engine: engine_name.to_string(),
                    relevance_score: None,
                });
            }
        }
    }

    // -- Phase 3: LLM Relevance Scoring ---------------------------------------

    fn score_candidates(&self, candidates: &mut [SearchCandidate], objective: &str) {
        if candidates.is_empty() {
            return;
        }

        // Score in batches of 50 to reduce LLM calls (especially with thinking models)
        for batch in candidates.chunks_mut(50) {
            let items: String = batch
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let snippet_preview = if c.snippet.len() > 100 {
                        format!("{}...", safe_slice(&c.snippet, 100))
                    } else {
                        c.snippet.clone()
                    };
                    format!("{}. {} — {} — {}", i + 1, c.title, c.url, snippet_preview)
                })
                .collect::<Vec<_>>()
                .join("\n");

            let input = serde_json::json!({
                "system": "You rate search result relevance. Output ONLY numbers (0.0-1.0), one per line. \
                           Consider: authority of source, topical relevance, likely content depth, recency.",
                "prompt": format!(
                    "Rate each URL's relevance to: \"{}\"\n\n{}\n\n\
                     Respond with ONLY numbers, one per line (e.g. 0.85):",
                    objective, items
                ),
            });

            match self.registry.execute_ability(self.manifest, "llm.chat", &input.to_string()) {
                Ok(result) => {
                    let output = String::from_utf8_lossy(&result.output).to_string();
                    let scores: Vec<f32> = output
                        .lines()
                        .filter_map(|l| {
                            l.trim()
                                .trim_start_matches(|c: char| c.is_ascii_digit() && c != '0' || c == '.' || c == ')')
                                .trim()
                                .parse::<f32>()
                                .ok()
                        })
                        .collect();

                    for (i, candidate) in batch.iter_mut().enumerate() {
                        candidate.relevance_score = scores.get(i).copied().or(Some(0.5));
                    }
                }
                Err(e) => {
                    eprintln!("[research] scoring batch failed: {}, assigning 0.5", e);
                    for candidate in batch.iter_mut() {
                        candidate.relevance_score = Some(0.5);
                    }
                }
            }
        }

        // Sort by relevance score descending
        candidates.sort_by(|a, b| {
            b.relevance_score
                .unwrap_or(0.0)
                .partial_cmp(&a.relevance_score.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    // -- Phase 4: Tiered Parallel Fetch ---------------------------------------

    fn fetch_sources(
        &self,
        candidates: &[SearchCandidate],
    ) -> (Vec<FetchedSource>, Vec<(String, String)>) {
        // Filter out domains known to always 403/block bot requests
        let blocked_count = candidates.iter().filter(|c| is_blocked_domain(&c.url)).count();
        if blocked_count > 0 {
            eprintln!("[research] skipped {} candidates from blocked domains", blocked_count);
        }
        let filtered: Vec<&SearchCandidate> = candidates
            .iter()
            .filter(|c| !is_blocked_domain(&c.url))
            .collect();

        let fetched = std::sync::Mutex::new(Vec::new());
        let failed = std::sync::Mutex::new(Vec::new());
        let max_content = self.config.max_content_per_source;
        let timeout_secs = self.config.fetch_timeout_secs;

        // Parallel fetch, 8 concurrent threads
        for batch in filtered.chunks(8) {
            std::thread::scope(|s| {
                for candidate in batch {
                    let fetched = &fetched;
                    let failed = &failed;
                    s.spawn(move || {
                        // Tier 1: Direct HTTP
                        match fetch_url_http(&candidate.url, timeout_secs, max_content) {
                            Ok(content) if !content.is_empty() => {
                                let char_count = content.len();
                                fetched.lock().unwrap().push(FetchedSource {
                                    url: candidate.url.clone(),
                                    title: candidate.title.clone(),
                                    content,
                                    fetch_method: FetchMethod::Http,
                                    char_count,
                                });
                            }
                            Ok(_) => {
                                failed.lock().unwrap().push((
                                    candidate.url.clone(),
                                    "empty content".to_string(),
                                ));
                            }
                            Err(e) => {
                                // Tier 2: Try ChromePool if available (async)
                                // For now, log the failure — ChromePool integration
                                // requires async runtime which PEA doesn't have yet
                                eprintln!(
                                    "[research] HTTP fetch failed for {}: {}",
                                    candidate.url, e
                                );
                                failed.lock().unwrap().push((
                                    candidate.url.clone(),
                                    e,
                                ));
                            }
                        }
                    });
                }
            });
        }

        (
            fetched.into_inner().unwrap(),
            failed.into_inner().unwrap(),
        )
    }
}

// ---------------------------------------------------------------------------
// 403 Domain Blocklist — sites that always reject bot/scraper requests
// ---------------------------------------------------------------------------

/// Domains known to return 403 Forbidden for automated requests.
/// Skipping these saves fetch time and backfill cycles.
const BLOCKED_DOMAINS: &[&str] = &[
    "nytimes.com",
    "washingtonpost.com",
    "iiss.org",
    "chathamhouse.org",
    "britannica.com",
    "researchgate.net",
    "understandingwar.org",
    "securitycouncilreport.org",
    "crisisgroup.org",
    "fdd.org",
    "commonslibrary.parliament.uk",
];

fn is_blocked_domain(url: &str) -> bool {
    BLOCKED_DOMAINS.iter().any(|d| url.contains(d))
}

// ---------------------------------------------------------------------------
// Standalone search functions (reused by bridge.rs)
// ---------------------------------------------------------------------------

/// Build a reqwest::blocking::Client with reasonable defaults.
pub(crate) fn http_client() -> Result<reqwest::blocking::Client, String> {
    http_client_with_timeout(15)
}

fn http_client_with_timeout(secs: u64) -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(secs))
        .redirect(reqwest::redirect::Policy::limited(3))
        .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0")
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))
}

/// Search via Brave Search HTML (no API key required).
pub(crate) fn search_brave(query: &str) -> Result<Vec<SearchResult>, String> {
    use scraper::{Html, Selector};

    let client = http_client()?;
    let url = format!(
        "https://search.brave.com/search?q={}&source=web",
        urlencoding::encode(query)
    );

    let resp = client.get(&url).send().map_err(|e| format!("Brave fetch: {}", e))?;
    let html = resp.text().map_err(|e| format!("Brave body: {}", e))?;
    let doc = Html::parse_document(&html);

    let mut results = Vec::new();
    let a_sel = Selector::parse("a[href]").map_err(|e| format!("selector: {:?}", e))?;
    let mut seen_urls = HashSet::new();

    for a in doc.select(&a_sel) {
        let href = match a.value().attr("href") {
            Some(h) => h,
            None => continue,
        };

        if !href.starts_with("http") || href.contains("brave.com") || href.contains("favicon") {
            continue;
        }

        if !seen_urls.insert(href.to_string()) {
            continue;
        }

        let title_text: String = a.text().collect::<String>().trim().to_string();
        if title_text.is_empty() || title_text.len() < 5 {
            continue;
        }

        results.push(SearchResult {
            title: title_text,
            url: Some(href.to_string()),
            snippet: String::new(),
        });

        if results.len() >= 10 {
            break;
        }
    }

    // Enhance: try to extract snippets
    if let Ok(desc_sel) = Selector::parse(".snippet-description, .snippet-content") {
        for (i, el) in doc.select(&desc_sel).enumerate() {
            let text: String = el.text().collect::<String>().trim().to_string();
            if i < results.len() && !text.is_empty() {
                results[i].snippet = text;
            }
        }
    }

    if results.is_empty() {
        Err("Brave returned no results".into())
    } else {
        Ok(results)
    }
}

/// Parse DDG search results from raw HTML.
pub(crate) fn search_ddg_from_html(html: &str) -> Result<Vec<SearchResult>, String> {
    use scraper::{Html, Selector};

    if html.contains("anomaly-modal") || html.contains("bot detection") {
        return Err("DDG returned CAPTCHA".into());
    }

    let doc = Html::parse_document(html);
    let result_sel = Selector::parse(".result").map_err(|e| format!("{:?}", e))?;
    let title_sel = Selector::parse(".result__a").map_err(|e| format!("{:?}", e))?;
    let snippet_sel = Selector::parse(".result__snippet").map_err(|e| format!("{:?}", e))?;
    let url_sel = Selector::parse(".result__url").map_err(|e| format!("{:?}", e))?;

    let mut results = Vec::new();
    for el in doc.select(&result_sel) {
        let title = el
            .select(&title_sel)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .unwrap_or_default();
        let snippet = el
            .select(&snippet_sel)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .unwrap_or_default();
        let url = el
            .select(&url_sel)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .or_else(|| {
                el.select(&title_sel)
                    .next()
                    .and_then(|e| e.value().attr("href").map(|s| s.to_string()))
            });

        if !title.is_empty() {
            results.push(SearchResult { title, url, snippet });
        }
        if results.len() >= 10 {
            break;
        }
    }

    if results.is_empty() {
        Err("DDG returned no results".into())
    } else {
        Ok(results)
    }
}

/// Search via DuckDuckGo HTML.
pub(crate) fn search_ddg(query: &str) -> Result<Vec<SearchResult>, String> {
    let client = http_client()?;
    let url = format!(
        "https://html.duckduckgo.com/html/?q={}",
        urlencoding::encode(query)
    );

    let resp = client.get(&url).send().map_err(|e| format!("DDG fetch: {}", e))?;
    let html = resp.text().map_err(|e| format!("DDG body: {}", e))?;
    search_ddg_from_html(&html)
}

/// Run an async future from a sync context using the existing tokio runtime or a new one.
fn run_async_in_sync<F, T>(future: F) -> Result<T, String>
where
    F: std::future::Future<Output = Result<T, String>> + Send + 'static,
    T: Send + 'static,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(future)),
        Err(_) => {
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| format!("Failed to create runtime: {}", e))?;
            rt.block_on(future)
        }
    }
}

/// Search via headless Chrome (ChromePool) — avoids bot detection by using a real browser.
pub(crate) fn search_chrome_pool(query: &str) -> Result<Vec<SearchResult>, String> {
    use crate::browser::chrome_pool::{ChromePool, TabHandle};
    use crate::modules::browser::{BrowserConfig, CdpTransport};

    let search_url = format!(
        "https://html.duckduckgo.com/html/?q={}",
        urlencoding::encode(query)
    );

    run_async_in_sync(async move {
        // Discover targets from the local Chrome DevTools endpoint
        let targets = CdpTransport::discover_targets("127.0.0.1", 9222)
            .await
            .map_err(|e| format!("ChromePool: cannot discover targets at 127.0.0.1:9222 — is headless Chrome running? {}", e))?;

        if targets.is_empty() {
            return Err("ChromePool: no Chrome targets available".into());
        }

        // Create a pool and populate it
        let pool = ChromePool::new(targets.len(), BrowserConfig::default());
        pool.populate_from_targets(targets).await;

        // Checkout a tab
        let tab = pool.checkout().await
            .map_err(|e| format!("ChromePool checkout: {}", e))?;

        // Navigate to search URL
        tab.transport
            .send_command("Page.navigate", serde_json::json!({"url": search_url}))
            .await
            .map_err(|e| format!("ChromePool navigate: {}", e))?;

        // Wait for page to load
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Extract page HTML
        let result = tab.transport
            .send_command(
                "Runtime.evaluate",
                serde_json::json!({"expression": "document.documentElement.outerHTML"}),
            )
            .await
            .map_err(|e| format!("ChromePool evaluate: {}", e))?;

        let html = result
            .get("result")
            .and_then(|r| r.get("value"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        // Return tab to pool
        pool.return_tab(tab).await;

        // Reuse DDG HTML parser
        search_ddg_from_html(&html)
    })
}

/// Search via Bing HTML (most lenient for datacenter IPs, ~20-30 queries/hr).
pub(crate) fn search_bing(query: &str) -> Result<Vec<SearchResult>, String> {
    use scraper::{Html, Selector};

    let client = http_client()?;
    let url = format!(
        "https://www.bing.com/search?q={}&setlang=en",
        urlencoding::encode(query)
    );

    let resp = client.get(&url).send().map_err(|e| format!("Bing fetch: {}", e))?;
    let status = resp.status();
    let html = resp.text().map_err(|e| format!("Bing body: {}", e))?;

    if status == 429 || html.contains("captcha") || html.contains("unusual traffic") {
        return Err("Bing rate limited or CAPTCHA".into());
    }

    let doc = Html::parse_document(&html);
    let mut results = Vec::new();

    // Bing organic results: <li class="b_algo">
    if let Ok(algo_sel) = Selector::parse("li.b_algo") {
        let a_sel = Selector::parse("h2 a[href]").map_err(|e| format!("{:?}", e))?;
        let snippet_sel = Selector::parse(".b_caption p, .b_lineclamp2").map_err(|e| format!("{:?}", e))?;

        for el in doc.select(&algo_sel) {
            let (title, href) = match el.select(&a_sel).next() {
                Some(a) => {
                    let t: String = a.text().collect::<String>().trim().to_string();
                    let h = a.value().attr("href").unwrap_or("").to_string();
                    (t, h)
                }
                None => continue,
            };

            if title.is_empty() || !href.starts_with("http") {
                continue;
            }

            let snippet = el
                .select(&snippet_sel)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            results.push(SearchResult {
                title,
                url: Some(href),
                snippet,
            });

            if results.len() >= 10 {
                break;
            }
        }
    }

    if results.is_empty() {
        Err("Bing returned no results".into())
    } else {
        Ok(results)
    }
}

/// Search via Google HTML (strict rate limits from datacenter IPs, ~5-10 before CAPTCHA).
pub(crate) fn search_google(query: &str) -> Result<Vec<SearchResult>, String> {
    use scraper::{Html, Selector};

    let client = http_client()?;
    let url = format!(
        "https://www.google.com/search?q={}&hl=en&num=10",
        urlencoding::encode(query)
    );

    let resp = client.get(&url).send().map_err(|e| format!("Google fetch: {}", e))?;
    let status = resp.status();
    let html = resp.text().map_err(|e| format!("Google body: {}", e))?;

    if status == 429 || html.contains("unusual traffic") || html.contains("/sorry/") || html.contains("captcha") {
        return Err("Google rate limited or CAPTCHA".into());
    }

    let doc = Html::parse_document(&html);
    let mut results = Vec::new();
    let mut seen_urls = HashSet::new();

    // Google wraps results in <div class="g"> or similar; extract all outbound links with titles
    let a_sel = Selector::parse("a[href]").map_err(|e| format!("{:?}", e))?;

    for a in doc.select(&a_sel) {
        let href = match a.value().attr("href") {
            Some(h) => h,
            None => continue,
        };

        // Google uses /url?q=REAL_URL&... for result links
        let real_url = if href.starts_with("/url?") {
            href.split("q=")
                .nth(1)
                .and_then(|s| s.split('&').next())
                .map(|s| urlencoding::decode(s).unwrap_or_default().into_owned())
        } else if href.starts_with("http") && !href.contains("google.com") {
            Some(href.to_string())
        } else {
            None
        };

        let real_url = match real_url {
            Some(u) if u.starts_with("http") && !u.contains("google.com") => u,
            _ => continue,
        };

        if !seen_urls.insert(real_url.clone()) {
            continue;
        }

        let title_text: String = a.text().collect::<String>().trim().to_string();
        if title_text.is_empty() || title_text.len() < 5 {
            continue;
        }

        // Try to get snippet from sibling/parent elements
        let snippet = String::new(); // Google snippets are harder to reliably extract

        results.push(SearchResult {
            title: title_text,
            url: Some(real_url),
            snippet,
        });

        if results.len() >= 10 {
            break;
        }
    }

    if results.is_empty() {
        Err("Google returned no results".into())
    } else {
        Ok(results)
    }
}

/// Search via Brave Search JSON API (free tier: 2000 queries/month).
pub(crate) fn search_brave_api(query: &str, api_key: &str) -> Result<Vec<SearchResult>, String> {
    let client = http_client()?;
    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count=10",
        urlencoding::encode(query)
    );

    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .header("X-Subscription-Token", api_key)
        .send()
        .map_err(|e| format!("Brave API fetch: {}", e))?;

    let status = resp.status().as_u16();
    if status == 429 {
        return Err("Brave API quota exceeded (429)".into());
    }
    if status == 401 {
        return Err("Brave API unauthorized — check NABA_BRAVE_API_KEY".into());
    }
    if status != 200 {
        return Err(format!("Brave API returned status {}", status));
    }

    let body: serde_json::Value = resp
        .json()
        .map_err(|e| format!("Brave API JSON parse: {}", e))?;

    let mut results = Vec::new();
    if let Some(web_results) = body.get("web").and_then(|w| w.get("results")).and_then(|r| r.as_array()) {
        for item in web_results.iter().take(10) {
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let url = item.get("url").and_then(|v| v.as_str()).map(|s| s.to_string());
            let snippet = item.get("description").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            if !title.is_empty() {
                results.push(SearchResult { title, url, snippet });
            }
        }
    }

    if results.is_empty() {
        Err("Brave API returned no results".into())
    } else {
        Ok(results)
    }
}

/// Search via self-hosted SearXNG instance (JSON API).
pub(crate) fn search_searxng(query: &str, base_url: &str) -> Result<Vec<SearchResult>, String> {
    let client = http_client()?;
    let url = format!(
        "{}/search?q={}&format=json&language=en",
        base_url.trim_end_matches('/'),
        urlencoding::encode(query)
    );

    let resp = client
        .get(&url)
        .send()
        .map_err(|e| format!("SearXNG fetch: {}", e))?;

    let status = resp.status().as_u16();
    if status != 200 {
        return Err(format!("SearXNG returned status {}", status));
    }

    let body: serde_json::Value = resp
        .json()
        .map_err(|e| format!("SearXNG JSON parse: {}", e))?;

    let mut results = Vec::new();
    if let Some(items) = body.get("results").and_then(|r| r.as_array()) {
        for item in items.iter().take(10) {
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let url = item.get("url").and_then(|v| v.as_str()).map(|s| s.to_string());
            let snippet = item.get("content").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            if !title.is_empty() {
                results.push(SearchResult { title, url, snippet });
            }
        }
    }

    if results.is_empty() {
        Err("SearXNG returned no results".into())
    } else {
        Ok(results)
    }
}

/// Fetch multiple URLs in parallel using thread::scope, extract text.
pub(crate) fn fetch_urls_parallel(urls: &[&str]) -> Vec<(String, String)> {
    let results = std::sync::Mutex::new(Vec::new());
    let client = match http_client() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    std::thread::scope(|s| {
        for &url in urls {
            let client = &client;
            let results = &results;
            s.spawn(move || {
                if let Ok(resp) = client.get(url).send() {
                    if let Ok(body) = resp.text() {
                        let text = extract_readable_text(&body);
                        if !text.is_empty() {
                            if let Ok(mut r) = results.lock() {
                                r.push((url.to_string(), text));
                            }
                        }
                    }
                }
            });
        }
    });

    results.into_inner().unwrap_or_default()
}

/// Extract readable text from HTML, stripping scripts/styles/nav.
pub(crate) fn extract_readable_text(html: &str) -> String {
    use scraper::{Html, Selector};

    let doc = Html::parse_document(html);

    let body_sel = match Selector::parse("article, main, .content, .post, .article, body") {
        Ok(s) => s,
        Err(_) => return String::new(),
    };

    let mut text = String::new();
    for el in doc.select(&body_sel) {
        let el_text: String = el.text().collect::<Vec<_>>().join(" ");
        let cleaned: String = el_text.split_whitespace().collect::<Vec<_>>().join(" ");
        if cleaned.len() > text.len() {
            text = cleaned;
        }
    }

    safe_truncate(&mut text, 8000);
    text
}

/// Truncate a string at the nearest char boundary at or before `max_len`.
fn safe_truncate(s: &mut String, max_len: usize) {
    if s.len() <= max_len {
        return;
    }
    let mut end = max_len;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
}

/// Safely slice a string at the nearest char boundary at or before `max_len`.
pub(crate) fn safe_slice(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        return s;
    }
    let mut end = max_len;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Fetch a single URL via HTTP with configurable timeout and content cap.
fn fetch_url_http(url: &str, timeout_secs: u64, max_content: usize) -> Result<String, String> {
    let client = http_client_with_timeout(timeout_secs)?;

    let resp = client
        .get(url)
        .send()
        .map_err(|e| format!("fetch: {}", e))?;

    let status = resp.status();
    if status.as_u16() == 403 || status.as_u16() == 429 || status.as_u16() == 503 {
        return Err(format!("HTTP {}", status));
    }

    let body = resp.text().map_err(|e| format!("body: {}", e))?;
    let mut text = extract_readable_text(&body);

    safe_truncate(&mut text, max_content);

    Ok(text)
}

/// Deduplicate fetched sources by content hash (SipHash pattern from swarm/collector.rs).
fn dedup_sources(sources: Vec<FetchedSource>) -> Vec<FetchedSource> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for source in sources {
        let hash = compute_content_hash(&source.content);
        if seen.insert(hash) {
            result.push(source);
        }
    }

    result
}

fn compute_content_hash(content: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_research_config_defaults() {
        let config = ResearchConfig::default();
        assert_eq!(config.max_search_queries, 20);
        assert_eq!(config.top_k_fetch, 50);
        assert_eq!(config.max_candidates, 200);
    }

    #[test]
    fn test_search_backend_roundtrip() {
        let cases = &[
            ("scrape_rotation", SearchBackend::ScrapeRotation),
            ("chrome_pool", SearchBackend::ChromePool),
            ("brave_api", SearchBackend::BraveApi),
            ("searxng", SearchBackend::SearXng),
        ];
        for (s, expected) in cases {
            let backend = SearchBackend::from_env_str(s);
            assert_eq!(backend, *expected, "from_env_str({}) mismatch", s);
            assert_eq!(backend.as_env_str(), *s, "as_env_str roundtrip for {}", s);
        }
    }

    #[test]
    fn test_search_backend_case_insensitive() {
        assert_eq!(SearchBackend::from_env_str("BRAVE_API"), SearchBackend::BraveApi);
        assert_eq!(SearchBackend::from_env_str("BraveApi"), SearchBackend::BraveApi);
        assert_eq!(SearchBackend::from_env_str("Chrome_Pool"), SearchBackend::ChromePool);
        assert_eq!(SearchBackend::from_env_str("SEARXNG"), SearchBackend::SearXng);
        assert_eq!(SearchBackend::from_env_str("searx"), SearchBackend::SearXng);
        assert_eq!(SearchBackend::from_env_str("chromepool"), SearchBackend::ChromePool);
        assert_eq!(SearchBackend::from_env_str("unknown"), SearchBackend::ScrapeRotation);
        assert_eq!(SearchBackend::from_env_str(""), SearchBackend::ScrapeRotation);
    }

    #[test]
    fn test_research_config_default_backend() {
        let config = ResearchConfig::default();
        // Without env var set, should default to ScrapeRotation
        assert_eq!(config.search_backend, SearchBackend::ScrapeRotation);
        assert!(config.brave_api_key.is_none() || config.brave_api_key.as_deref() == Some(""));
    }

    #[test]
    fn test_searxng_url_format() {
        // Verify the default URL is well-formed for SearXNG
        let base = "http://localhost:8888";
        let query = "test query";
        let url = format!(
            "{}/search?q={}&format=json&language=en",
            base.trim_end_matches('/'),
            urlencoding::encode(query)
        );
        assert!(url.starts_with("http://localhost:8888/search?q="));
        assert!(url.contains("format=json"));
        assert!(url.contains("test%20query"));
    }

    #[test]
    fn test_dedup_sources() {
        let sources = vec![
            FetchedSource {
                url: "https://a.com".into(),
                title: "A".into(),
                content: "same content".into(),
                fetch_method: FetchMethod::Http,
                char_count: 12,
            },
            FetchedSource {
                url: "https://b.com".into(),
                title: "B".into(),
                content: "same content".into(),
                fetch_method: FetchMethod::Http,
                char_count: 12,
            },
            FetchedSource {
                url: "https://c.com".into(),
                title: "C".into(),
                content: "different".into(),
                fetch_method: FetchMethod::Http,
                char_count: 9,
            },
        ];
        let deduped = dedup_sources(sources);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn test_compute_content_hash_deterministic() {
        let h1 = compute_content_hash("hello world");
        let h2 = compute_content_hash("hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_compute_content_hash_different() {
        let h1 = compute_content_hash("hello");
        let h2 = compute_content_hash("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_corpus_to_context_string() {
        let corpus = ResearchCorpus {
            query: "test query".into(),
            sources: vec![
                FetchedSource {
                    url: "https://example.com".into(),
                    title: "Example".into(),
                    content: "Example content here".into(),
                    fetch_method: FetchMethod::Http,
                    char_count: 20,
                },
            ],
            failed_urls: vec![("https://fail.com".into(), "timeout".into())],
            total_candidates: 100,
            total_chars: 20,
        };
        let ctx = corpus.to_context_string();
        assert!(ctx.contains("1 sources"));
        assert!(ctx.contains("Example"));
        assert!(ctx.contains("https://example.com"));
        assert!(ctx.contains("1 URLs failed"));
    }

    #[test]
    fn test_extract_readable_text_basic() {
        let html = "<html><body><article>Hello world</article></body></html>";
        let text = extract_readable_text(html);
        assert!(text.contains("Hello world"));
    }

    #[test]
    fn test_extract_readable_text_capped() {
        let long_content = "word ".repeat(5000);
        let html = format!("<html><body>{}</body></html>", long_content);
        let text = extract_readable_text(&html);
        assert!(text.len() <= 8000);
    }

    #[test]
    fn test_blocked_domain_nytimes() {
        assert!(is_blocked_domain("https://www.nytimes.com/2026/03/07/world.html"));
    }

    #[test]
    fn test_blocked_domain_allowed() {
        assert!(!is_blocked_domain("https://www.reuters.com/world/article"));
        assert!(!is_blocked_domain("https://en.wikipedia.org/wiki/Test"));
    }
}
