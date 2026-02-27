//! OpenAlex research API client for academic paper search and citation.

use crate::core::error::{NyayaError, Result};
use serde::{Deserialize, Serialize};

const OPENALEX_BASE: &str = "https://api.openalex.org";
const POLITE_EMAIL: &str = "nabaos@users.noreply.github.com";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkResult {
    pub id: String,
    pub title: String,
    pub doi: Option<String>,
    pub publication_year: Option<i32>,
    pub cited_by_count: i32,
    pub authors: Vec<String>,
    pub abstract_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorResult {
    pub id: String,
    pub name: String,
    pub works_count: i32,
    pub cited_by_count: i32,
    pub affiliation: Option<String>,
}

pub struct OpenAlexClient {
    client: reqwest::blocking::Client,
}

impl OpenAlexClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::blocking::Client::builder()
                .user_agent(format!("NyayaAgent/1.0 (mailto:{})", POLITE_EMAIL))
                .build()
                .expect("HTTP client"),
        }
    }

    /// Search works by query string. Returns up to `per_page` results (max 50).
    pub fn search_works(&self, query: &str, per_page: usize) -> Result<Vec<WorkResult>> {
        let url = format!(
            "{}/works?search={}&per_page={}&mailto={}",
            OPENALEX_BASE,
            urlencoding::encode(query),
            per_page.min(50),
            POLITE_EMAIL,
        );
        let resp: serde_json::Value = self
            .client
            .get(&url)
            .send()
            .map_err(|e| NyayaError::Config(format!("OpenAlex request error: {}", e)))?
            .json()
            .map_err(|e| NyayaError::Config(format!("OpenAlex parse error: {}", e)))?;

        Self::parse_works_response(&resp)
    }

    /// Fetch a single work by OpenAlex ID (e.g., "W2741809807") or DOI.
    pub fn fetch_work(&self, id: &str) -> Result<WorkResult> {
        let url = format!("{}/works/{}?mailto={}", OPENALEX_BASE, id, POLITE_EMAIL);
        let resp: serde_json::Value = self
            .client
            .get(&url)
            .send()
            .map_err(|e| NyayaError::Config(format!("OpenAlex request error: {}", e)))?
            .json()
            .map_err(|e| NyayaError::Config(format!("OpenAlex parse error: {}", e)))?;

        Self::parse_single_work(&resp)
    }

    /// Search authors by name.
    pub fn search_authors(&self, name: &str) -> Result<Vec<AuthorResult>> {
        let url = format!(
            "{}/authors?search={}&per_page=5&mailto={}",
            OPENALEX_BASE,
            urlencoding::encode(name),
            POLITE_EMAIL,
        );
        let resp: serde_json::Value = self
            .client
            .get(&url)
            .send()
            .map_err(|e| NyayaError::Config(format!("OpenAlex request error: {}", e)))?
            .json()
            .map_err(|e| NyayaError::Config(format!("OpenAlex parse error: {}", e)))?;

        Self::parse_authors_response(&resp)
    }

    /// Format a WorkResult as an academic citation string.
    pub fn format_citation(work: &WorkResult) -> String {
        let authors = if work.authors.is_empty() {
            "Unknown".to_string()
        } else if work.authors.len() > 3 {
            format!("{} et al.", work.authors[0])
        } else {
            work.authors.join(", ")
        };
        let year = work
            .publication_year
            .map(|y| y.to_string())
            .unwrap_or_else(|| "n.d.".to_string());
        let doi = work
            .doi
            .as_ref()
            .map(|d| format!(" doi:{}", d))
            .unwrap_or_default();
        format!("{} ({}). {}.{}", authors, year, work.title, doi)
    }

    fn parse_works_response(resp: &serde_json::Value) -> Result<Vec<WorkResult>> {
        let results = resp
            .get("results")
            .and_then(|r| r.as_array())
            .ok_or_else(|| NyayaError::Config("No results array in response".into()))?;
        results.iter().map(Self::parse_single_work).collect()
    }

    fn parse_single_work(work: &serde_json::Value) -> Result<WorkResult> {
        let id = work
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let title = work
            .get("display_name")
            .or_else(|| work.get("title"))
            .and_then(|v| v.as_str())
            .unwrap_or("Untitled")
            .to_string();
        let doi = work.get("doi").and_then(|v| v.as_str()).map(String::from);
        let publication_year = work
            .get("publication_year")
            .and_then(|v| v.as_i64())
            .map(|y| y as i32);
        let cited_by_count = work
            .get("cited_by_count")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;

        let authors = work
            .get("authorships")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|a| {
                        a.get("author")
                            .and_then(|au| au.get("display_name"))
                            .and_then(|n| n.as_str())
                    })
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();

        let abstract_text = work
            .get("abstract_inverted_index")
            .and_then(Self::reconstruct_abstract);

        Ok(WorkResult {
            id,
            title,
            doi,
            publication_year,
            cited_by_count,
            authors,
            abstract_text,
        })
    }

    fn parse_authors_response(resp: &serde_json::Value) -> Result<Vec<AuthorResult>> {
        let results = resp
            .get("results")
            .and_then(|r| r.as_array())
            .ok_or_else(|| NyayaError::Config("No results array in response".into()))?;

        results
            .iter()
            .map(|author| {
                Ok(AuthorResult {
                    id: author
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    name: author
                        .get("display_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown")
                        .to_string(),
                    works_count: author
                        .get("works_count")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0) as i32,
                    cited_by_count: author
                        .get("cited_by_count")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0) as i32,
                    affiliation: author
                        .get("last_known_institutions")
                        .and_then(|arr| arr.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|inst| inst.get("display_name"))
                        .and_then(|v| v.as_str())
                        .map(String::from),
                })
            })
            .collect()
    }

    /// Reconstruct abstract text from OpenAlex inverted index format.
    /// The inverted index maps words to their positions: {"word": [0, 5], "hello": [1]}
    pub fn reconstruct_abstract(inverted_index: &serde_json::Value) -> Option<String> {
        let obj = inverted_index.as_object()?;
        let mut words: Vec<(usize, &str)> = Vec::new();
        for (word, positions) in obj {
            for pos in positions.as_array()? {
                if let Some(idx) = pos.as_u64() {
                    words.push((idx as usize, word));
                }
            }
        }
        if words.is_empty() {
            return None;
        }
        words.sort_by_key(|(idx, _)| *idx);
        Some(words.iter().map(|(_, w)| *w).collect::<Vec<_>>().join(" "))
    }
}

impl Default for OpenAlexClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reconstruct_abstract() {
        let idx = serde_json::json!({
            "Hello": [0],
            "world": [1],
            "this": [2],
            "is": [3],
            "a": [4],
            "test": [5]
        });
        let result = OpenAlexClient::reconstruct_abstract(&idx).unwrap();
        assert_eq!(result, "Hello world this is a test");
    }

    #[test]
    fn test_reconstruct_abstract_empty() {
        let idx = serde_json::json!({});
        assert!(OpenAlexClient::reconstruct_abstract(&idx).is_none());
    }

    #[test]
    fn test_parse_works_response() {
        let resp = serde_json::json!({
            "results": [{
                "id": "W123",
                "display_name": "Test Paper",
                "doi": "10.1234/test",
                "publication_year": 2024,
                "cited_by_count": 42,
                "authorships": [
                    {"author": {"display_name": "Alice Smith"}},
                    {"author": {"display_name": "Bob Jones"}}
                ],
                "abstract_inverted_index": {"Test": [0], "abstract": [1]}
            }]
        });
        let works = OpenAlexClient::parse_works_response(&resp).unwrap();
        assert_eq!(works.len(), 1);
        assert_eq!(works[0].title, "Test Paper");
        assert_eq!(works[0].doi.as_deref(), Some("10.1234/test"));
        assert_eq!(works[0].cited_by_count, 42);
        assert_eq!(works[0].authors, vec!["Alice Smith", "Bob Jones"]);
        assert_eq!(works[0].abstract_text.as_deref(), Some("Test abstract"));
    }

    #[test]
    fn test_parse_authors_response() {
        let resp = serde_json::json!({
            "results": [{
                "id": "A456",
                "display_name": "Dr. Alice",
                "works_count": 100,
                "cited_by_count": 5000,
                "last_known_institutions": [
                    {"display_name": "MIT"}
                ]
            }]
        });
        let authors = OpenAlexClient::parse_authors_response(&resp).unwrap();
        assert_eq!(authors.len(), 1);
        assert_eq!(authors[0].name, "Dr. Alice");
        assert_eq!(authors[0].affiliation.as_deref(), Some("MIT"));
    }

    #[test]
    fn test_format_citation() {
        let work = WorkResult {
            id: "W123".into(),
            title: "A Great Paper".into(),
            doi: Some("10.1234/great".into()),
            publication_year: Some(2024),
            cited_by_count: 10,
            authors: vec!["Alice".into(), "Bob".into()],
            abstract_text: None,
        };
        let cite = OpenAlexClient::format_citation(&work);
        assert!(cite.contains("Alice, Bob"));
        assert!(cite.contains("(2024)"));
        assert!(cite.contains("A Great Paper"));
        assert!(cite.contains("doi:10.1234/great"));
    }
}
