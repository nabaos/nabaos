use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Instant;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for a wide research operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchConfig {
    /// Maximum number of sources to fetch (default 10).
    #[serde(default = "default_max_sources")]
    pub max_sources: usize,

    /// Per-source fetch timeout in seconds (default 30).
    #[serde(default = "default_timeout_per_source")]
    pub timeout_per_source_secs: u64,

    /// Total wall-clock timeout for the entire research operation (default 300).
    #[serde(default = "default_max_total")]
    pub max_total_secs: u64,

    /// Whether to deduplicate results by content hash (default true).
    #[serde(default = "default_dedup")]
    pub dedup: bool,
}

fn default_max_sources() -> usize {
    10
}
fn default_timeout_per_source() -> u64 {
    30
}
fn default_max_total() -> u64 {
    300
}
fn default_dedup() -> bool {
    true
}

impl Default for ResearchConfig {
    fn default() -> Self {
        Self {
            max_sources: default_max_sources(),
            timeout_per_source_secs: default_timeout_per_source(),
            max_total_secs: default_max_total(),
            dedup: default_dedup(),
        }
    }
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A source to fetch during research.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchSource {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub snippet: Option<String>,
}

/// Result for a single fetched source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceResult {
    pub url: String,
    pub title: Option<String>,
    /// Fetched content (truncated to ~8 KB for sanity).
    pub content: String,
    /// Time taken to fetch this source in milliseconds.
    pub fetch_ms: u64,
    /// SHA-256 hex digest of the full content (before truncation).
    pub content_hash: String,
}

/// Aggregate result of a wide research operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchResult {
    pub query: String,
    pub sources_fetched: usize,
    pub sources_after_dedup: usize,
    pub results: Vec<SourceResult>,
    /// Optional synthesis / summary produced by an LLM (populated externally).
    pub synthesis: Option<String>,
    pub total_ms: u64,
}

// ---------------------------------------------------------------------------
// Core logic
// ---------------------------------------------------------------------------

/// Maximum content length kept per source (8 KB).
const MAX_CONTENT_LEN: usize = 8192;

/// Fetch a single source. Returns `Err(description)` on failure.
pub fn fetch_source(url: &str, timeout_secs: u64) -> Result<SourceResult, String> {
    let start = Instant::now();

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| format!("reqwest client error: {e}"))?;

    let resp = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0 (compatible; nabaos/0.2; +https://github.com/nabaos/nabaos)")
        .send()
        .map_err(|e| format!("fetch error for {url}: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status} for {url}"));
    }

    let body = resp
        .text()
        .map_err(|e| format!("body read error for {url}: {e}"))?;

    let fetch_ms = start.elapsed().as_millis() as u64;

    // Hash the full body before truncation.
    let hash = sha256_hex(&body);

    // Parse HTML and extract readable content, removing boilerplate.
    let text = extract_page_text(&body);

    let content = if text.len() > MAX_CONTENT_LEN {
        text[..MAX_CONTENT_LEN].to_string()
    } else {
        text
    };

    // Try to extract <title> from original body.
    let title = extract_html_title(&body);

    Ok(SourceResult {
        url: url.to_string(),
        title,
        content,
        fetch_ms,
        content_hash: hash,
    })
}

/// Remove duplicate results by `content_hash`. Keeps first occurrence.
pub fn dedup_results(results: &mut Vec<SourceResult>) {
    let mut seen = std::collections::HashSet::new();
    results.retain(|r| seen.insert(r.content_hash.clone()));
}

/// Execute a wide research operation: fetch all URLs (thread pool), dedup,
/// compile into a `ResearchResult`.
pub fn execute_research(query: &str, urls: &[String], config: &ResearchConfig) -> ResearchResult {
    let overall_start = Instant::now();

    // Clamp to max_sources.
    let urls: Vec<&String> = urls.iter().take(config.max_sources).collect();

    // Use std::thread::scope for structured parallelism (no extra deps).
    let timeout_per = config.timeout_per_source_secs;
    let max_total = std::time::Duration::from_secs(config.max_total_secs);

    let mut fetched: Vec<SourceResult> = Vec::new();

    std::thread::scope(|s| {
        let handles: Vec<_> = urls
            .iter()
            .map(|url| {
                let u = url.to_string();
                s.spawn(move || fetch_source(&u, timeout_per))
            })
            .collect();

        for handle in handles {
            // Respect total timeout: if we've exceeded it, skip remaining.
            if overall_start.elapsed() > max_total {
                break;
            }
            if let Ok(Ok(result)) = handle.join() {
                fetched.push(result);
            }
        }
    });

    let sources_fetched = fetched.len();

    if config.dedup {
        dedup_results(&mut fetched);
    }

    let sources_after_dedup = fetched.len();
    let total_ms = overall_start.elapsed().as_millis() as u64;

    ResearchResult {
        query: query.to_string(),
        sources_fetched,
        sources_after_dedup,
        results: fetched,
        synthesis: None,
        total_ms,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sha256_hex(data: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Extract readable text from an HTML page, removing boilerplate elements.
fn extract_page_text(html: &str) -> String {
    use scraper::{Html, Selector};

    let doc = Html::parse_document(html);

    // Build a combined selector that matches all boilerplate elements
    let boilerplate_sel = Selector::parse(
        "script, style, nav, header, footer, aside, form, noscript, iframe, \
         [role=\"navigation\"], [role=\"banner\"], [role=\"contentinfo\"], \
         .cookie-banner, .ad, .advertisement, .sidebar",
    );

    // Collect all boilerplate element text so we can exclude it
    let boilerplate_text: std::collections::HashSet<String> =
        if let Ok(ref sel) = boilerplate_sel {
            doc.select(sel)
                .map(|el| el.text().collect::<String>())
                .filter(|t| !t.trim().is_empty())
                .collect()
        } else {
            std::collections::HashSet::new()
        };

    // Prefer main/article content if available, then body, then full doc
    let body_sel = Selector::parse("body").ok();
    let main_sel = Selector::parse("main, article, [role=\"main\"]").ok();

    let text = if let Some(ref sel) = main_sel {
        if let Some(main_el) = doc.select(sel).next() {
            main_el.text().collect::<Vec<_>>().join(" ")
        } else if let Some(ref bsel) = body_sel {
            if let Some(body) = doc.select(bsel).next() {
                body.text().collect::<Vec<_>>().join(" ")
            } else {
                doc.root_element().text().collect::<Vec<_>>().join(" ")
            }
        } else {
            doc.root_element().text().collect::<Vec<_>>().join(" ")
        }
    } else {
        doc.root_element().text().collect::<Vec<_>>().join(" ")
    };

    // Remove boilerplate text fragments from the result
    let mut result = text;
    for bp in &boilerplate_text {
        result = result.replace(bp.as_str(), "");
    }

    // Collapse whitespace
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extract the contents of the first `<title>` tag.
fn extract_html_title(html: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let start = lower.find("<title")?.checked_add(6)?;
    let after_tag = lower[start..].find('>')?.checked_add(1)?;
    let content_start = start + after_tag;
    let end = lower[content_start..].find("</title")?;
    let title = html[content_start..content_start + end].trim().to_string();
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let cfg = ResearchConfig::default();
        assert_eq!(cfg.max_sources, 10);
        assert_eq!(cfg.timeout_per_source_secs, 30);
        assert_eq!(cfg.max_total_secs, 300);
        assert!(cfg.dedup);
    }

    #[test]
    fn test_config_serde_defaults() {
        let json = "{}";
        let cfg: ResearchConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.max_sources, 10);
        assert_eq!(cfg.timeout_per_source_secs, 30);
        assert!(cfg.dedup);
    }

    #[test]
    fn test_dedup_removes_duplicates() {
        let mut results = vec![
            SourceResult {
                url: "https://a.com".into(),
                title: None,
                content: "hello".into(),
                fetch_ms: 10,
                content_hash: "abc123".into(),
            },
            SourceResult {
                url: "https://b.com".into(),
                title: None,
                content: "hello".into(),
                fetch_ms: 20,
                content_hash: "abc123".into(), // same hash
            },
            SourceResult {
                url: "https://c.com".into(),
                title: None,
                content: "world".into(),
                fetch_ms: 15,
                content_hash: "def456".into(),
            },
        ];
        dedup_results(&mut results);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].url, "https://a.com");
        assert_eq!(results[1].url, "https://c.com");
    }

    #[test]
    fn test_dedup_empty() {
        let mut results: Vec<SourceResult> = vec![];
        dedup_results(&mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_dedup_all_unique() {
        let mut results = vec![
            SourceResult {
                url: "https://a.com".into(),
                title: None,
                content: "a".into(),
                fetch_ms: 1,
                content_hash: "h1".into(),
            },
            SourceResult {
                url: "https://b.com".into(),
                title: None,
                content: "b".into(),
                fetch_ms: 2,
                content_hash: "h2".into(),
            },
        ];
        dedup_results(&mut results);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_sha256_hex() {
        let hash = sha256_hex("hello world");
        assert_eq!(hash.len(), 64);
        // Known SHA-256 for "hello world"
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_extract_page_text() {
        let html = "<html><body><p>Hello <b>world</b>!</p></body></html>";
        let text = extract_page_text(html);
        assert_eq!(text, "Hello world !");
    }

    #[test]
    fn test_extract_page_text_strips_boilerplate() {
        let html = r#"<html><body>
            <nav>Home About Contact</nav>
            <main><p>Actual content here.</p></main>
            <footer>Copyright 2026</footer>
        </body></html>"#;
        let text = extract_page_text(html);
        assert_eq!(text, "Actual content here.");
    }

    #[test]
    fn test_extract_page_text_strips_scripts() {
        let html = "<html><body><script>var x=1;</script><p>Clean text</p></body></html>";
        let text = extract_page_text(html);
        assert_eq!(text, "Clean text");
    }

    #[test]
    fn test_extract_html_title() {
        let html = "<html><head><title>My Page</title></head><body></body></html>";
        assert_eq!(extract_html_title(html), Some("My Page".to_string()));
    }

    #[test]
    fn test_extract_html_title_none() {
        let html = "<html><head></head><body>No title here</body></html>";
        assert_eq!(extract_html_title(html), None);
    }

    #[test]
    fn test_execute_research_empty_urls() {
        let config = ResearchConfig::default();
        let result = execute_research("test query", &[], &config);
        assert_eq!(result.query, "test query");
        assert_eq!(result.sources_fetched, 0);
        assert_eq!(result.sources_after_dedup, 0);
        assert!(result.results.is_empty());
        assert!(result.synthesis.is_none());
    }

    #[test]
    fn test_execute_research_clamps_sources() {
        let config = ResearchConfig {
            max_sources: 2,
            timeout_per_source_secs: 1,
            max_total_secs: 5,
            dedup: true,
        };
        // Pass more URLs than max_sources — should only attempt 2.
        let urls: Vec<String> = (0..5)
            .map(|i| format!("http://invalid-test-host-{i}.example.invalid/"))
            .collect();
        let result = execute_research("clamp test", &urls, &config);
        // All will fail (invalid hosts), so fetched == 0, but we verify it doesn't panic.
        assert_eq!(result.query, "clamp test");
        assert!(result.sources_fetched <= 2);
    }
}
