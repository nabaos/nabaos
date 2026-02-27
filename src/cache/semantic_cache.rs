// src/cache/semantic_cache.rs
// The semantic work cache — stores parameterized solutions to recurring tasks.
// When a task matches a cached entry, it executes directly without any LLM call.

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

/// Similarity threshold for a cache hit (cosine similarity)
pub const DEFAULT_SIMILARITY_THRESHOLD: f32 = 0.92;
/// Minimum success rate before a cache entry is disabled
const MIN_SUCCESS_RATE: f64 = 0.6;
/// Number of successful uses before threshold relaxes
const RELAX_AFTER_HITS: u64 = 5;

/// Parameter types that cached functions accept
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamType {
    Text,
    Number,
    Boolean,
    FilePath,
    Url,
    EmailAddress,
    DateTime,
    List(Box<ParamType>),
}

/// A single parameter in a cached function signature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub param_type: ParamType,
    pub description: String,
    pub required: bool,
}

/// A single tool call in a deterministic plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool: String,
    /// Args can contain {{param_name}} placeholders
    pub args: serde_json::Map<String, serde_json::Value>,
}

/// How the cached solution is implemented
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum CacheImplementation {
    /// A sequence of tool calls with parameter substitution
    ToolSequence { steps: Vec<ToolCall> },
    /// Compiled Wasm module (stored as bytes, executed in sandbox)
    Wasm { module_path: String },
    /// Rust source code (for future JIT compilation)
    RustSource { code: String },
}

impl Default for CacheImplementation {
    fn default() -> Self {
        Self::ToolSequence { steps: Vec::new() }
    }
}

/// A cached work entry — a parameterized, reusable solution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedWork {
    pub id: String,
    pub description: String,
    pub original_task: String,
    pub rationale: String,
    pub improvement_notes: Option<String>,
    pub parameters: Vec<Parameter>,
    pub implementation: CacheImplementation,
    pub hit_count: u64,
    pub success_count: u64,
    pub failure_count: u64,
    pub created_at: u64,
    pub last_used_at: u64,
    pub similarity_threshold: f32,
    pub enabled: bool,
}

impl CachedWork {
    pub fn success_rate(&self) -> f64 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            return 1.0; // assume good until proven otherwise
        }
        self.success_count as f64 / total as f64
    }

    /// Adaptive threshold: relaxes after repeated success, tightens on failures
    pub fn effective_threshold(&self) -> f32 {
        if self.hit_count >= RELAX_AFTER_HITS && self.success_rate() > 0.9 {
            // Relax threshold for well-proven entries
            (self.similarity_threshold - 0.03).max(0.88)
        } else if self.success_rate() < 0.8 {
            // Tighten threshold for unreliable entries
            (self.similarity_threshold + 0.03).min(0.98)
        } else {
            self.similarity_threshold
        }
    }
}

/// Result of a cache lookup
pub enum CacheLookup {
    /// Found a matching entry with extracted parameters
    Hit {
        entry: CachedWork,
        similarity: f32,
        extracted_params: serde_json::Map<String, serde_json::Value>,
    },
    /// No match found — need LLM
    Miss,
}

/// The semantic work cache
pub struct SemanticCache {
    db: Connection,
    // embedding_index: usearch::Index, // TODO: initialize with usearch
}

impl SemanticCache {
    /// Open or create the cache database
    pub fn open(data_dir: &Path) -> Result<Self> {
        let db_path = data_dir.join("cache.db");
        let db = Connection::open(&db_path).context("Failed to open cache database")?;

        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS cached_work (
                id TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                original_task TEXT NOT NULL,
                rationale TEXT NOT NULL,
                improvement_notes TEXT,
                parameters TEXT NOT NULL,     -- JSON
                implementation TEXT NOT NULL,  -- JSON
                hit_count INTEGER DEFAULT 0,
                success_count INTEGER DEFAULT 0,
                failure_count INTEGER DEFAULT 0,
                created_at INTEGER NOT NULL,
                last_used_at INTEGER NOT NULL,
                similarity_threshold REAL DEFAULT 0.95,
                enabled INTEGER DEFAULT 1,
                embedding BLOB                -- f32 vector
            );

            CREATE TABLE IF NOT EXISTS cache_log (
                id TEXT PRIMARY KEY,
                cache_entry_id TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                action TEXT NOT NULL,          -- hit | miss | success | failure | invalidated
                details TEXT,
                FOREIGN KEY (cache_entry_id) REFERENCES cached_work(id)
            );

            CREATE INDEX IF NOT EXISTS idx_cache_enabled
                ON cached_work(enabled) WHERE enabled = 1;
            CREATE INDEX IF NOT EXISTS idx_cache_log_entry
                ON cache_log(cache_entry_id, timestamp);",
        )?;

        Ok(Self { db })
    }

    /// Search for a matching cached solution
    pub fn lookup(&self, task_embedding: &[f32], task_text: &str) -> Result<CacheLookup> {
        // TODO: Use usearch index for fast ANN search
        // For MVP: brute-force scan over enabled entries
        // This is fine for <1000 entries, optimize later

        let mut stmt = self.db.prepare(
            "SELECT id, description, original_task, rationale, improvement_notes,
                    parameters, implementation, hit_count, success_count, failure_count,
                    created_at, last_used_at, similarity_threshold, embedding
             FROM cached_work
             WHERE enabled = 1",
        )?;

        let mut best_match: Option<(CachedWork, f32)> = None;

        let entries = stmt.query_map([], |row| {
            let embedding_blob: Vec<u8> = row.get(13)?;
            let embedding = bytes_to_f32(&embedding_blob);

            let entry = CachedWork {
                id: row.get(0)?,
                description: row.get(1)?,
                original_task: row.get(2)?,
                rationale: row.get(3)?,
                improvement_notes: row.get(4)?,
                parameters: serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or_default(),
                implementation: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or_default(),
                hit_count: row.get::<_, i64>(7)? as u64,
                success_count: row.get::<_, i64>(8)? as u64,
                failure_count: row.get::<_, i64>(9)? as u64,
                created_at: row.get::<_, i64>(10)? as u64,
                last_used_at: row.get::<_, i64>(11)? as u64,
                similarity_threshold: row.get(12)?,
                enabled: true,
            };

            Ok((entry, embedding))
        })?;

        for result in entries {
            let (entry, stored_embedding) = result?;
            let similarity = cosine_similarity(task_embedding, &stored_embedding);
            let threshold = entry.effective_threshold();

            if similarity >= threshold {
                match &best_match {
                    Some((_, best_sim)) if similarity <= *best_sim => {}
                    _ => best_match = Some((entry, similarity)),
                }
            }
        }

        match best_match {
            Some((entry, similarity)) => {
                // Update hit count
                self.db.execute(
                    "UPDATE cached_work SET hit_count = hit_count + 1, last_used_at = ?1 WHERE id = ?2",
                    rusqlite::params![now_millis(), entry.id],
                )?;

                // Log the hit
                self.log_event(
                    &entry.id,
                    "hit",
                    Some(&format!("similarity={similarity:.4}")),
                )?;

                // Extract parameters from task_text using entry.parameters schema
                let extracted_params =
                    extract_parameters(task_text, &entry.parameters, Some(&entry.original_task));

                Ok(CacheLookup::Hit {
                    entry,
                    similarity,
                    extracted_params,
                })
            }
            None => Ok(CacheLookup::Miss),
        }
    }

    /// Store a new cached work entry from LLM metacognition output
    pub fn store(
        &self,
        description: &str,
        original_task: &str,
        rationale: &str,
        improvement_notes: Option<&str>,
        parameters: &[Parameter],
        implementation: &CacheImplementation,
        embedding: &[f32],
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = now_millis();

        self.db.execute(
            "INSERT INTO cached_work
             (id, description, original_task, rationale, improvement_notes,
              parameters, implementation, created_at, last_used_at,
              similarity_threshold, enabled, embedding)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11)",
            rusqlite::params![
                id,
                description,
                original_task,
                rationale,
                improvement_notes,
                serde_json::to_string(parameters)?,
                serde_json::to_string(implementation)?,
                now,
                now,
                0.95_f32, // start conservative
                f32_to_bytes(embedding),
            ],
        )?;

        self.log_event(&id, "created", None)?;

        tracing::info!(
            cache_id = %id,
            description = %description,
            params = parameters.len(),
            "New cache entry stored"
        );

        Ok(id)
    }

    /// Record a success or failure for a cache entry
    pub fn record_outcome(&self, entry_id: &str, success: bool) -> Result<()> {
        let column = if success {
            "success_count"
        } else {
            "failure_count"
        };
        self.db.execute(
            &format!("UPDATE cached_work SET {column} = {column} + 1 WHERE id = ?1"),
            rusqlite::params![entry_id],
        )?;

        let action = if success { "success" } else { "failure" };
        self.log_event(entry_id, action, None)?;

        // Check if entry should be disabled
        if !success {
            let (success_count, failure_count): (i64, i64) = self.db.query_row(
                "SELECT success_count, failure_count FROM cached_work WHERE id = ?1",
                rusqlite::params![entry_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;

            let total = success_count + failure_count;
            let rate = if total > 0 {
                success_count as f64 / total as f64
            } else {
                1.0
            };

            if total >= 3 && rate < MIN_SUCCESS_RATE {
                self.db.execute(
                    "UPDATE cached_work SET enabled = 0 WHERE id = ?1",
                    rusqlite::params![entry_id],
                )?;
                self.log_event(
                    entry_id,
                    "disabled",
                    Some(&format!("success_rate={rate:.2}")),
                )?;
                tracing::warn!(cache_id = %entry_id, rate = %rate, "Cache entry disabled due to low success rate");
            }
        }

        Ok(())
    }

    /// Invalidate a specific cache entry
    pub fn invalidate(&self, entry_id: &str) -> Result<()> {
        self.db.execute(
            "UPDATE cached_work SET enabled = 0 WHERE id = ?1",
            rusqlite::params![entry_id],
        )?;
        self.log_event(entry_id, "invalidated", None)
    }

    /// Get cache statistics
    pub fn stats(&self) -> Result<CacheStats> {
        let total: i64 = self
            .db
            .query_row("SELECT COUNT(*) FROM cached_work", [], |r| r.get(0))?;
        let enabled: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM cached_work WHERE enabled = 1",
            [],
            |r| r.get(0),
        )?;
        let total_hits: i64 = self.db.query_row(
            "SELECT COALESCE(SUM(hit_count), 0) FROM cached_work",
            [],
            |r| r.get(0),
        )?;
        let total_successes: i64 = self.db.query_row(
            "SELECT COALESCE(SUM(success_count), 0) FROM cached_work",
            [],
            |r| r.get(0),
        )?;

        Ok(CacheStats {
            total_entries: total as u64,
            enabled_entries: enabled as u64,
            total_hits: total_hits as u64,
            total_successes: total_successes as u64,
        })
    }

    fn log_event(&self, entry_id: &str, action: &str, details: Option<&str>) -> Result<()> {
        self.db.execute(
            "INSERT INTO cache_log (id, cache_entry_id, timestamp, action, details)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                Uuid::new_v4().to_string(),
                entry_id,
                now_millis(),
                action,
                details,
            ],
        )?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct CacheStats {
    pub total_entries: u64,
    pub enabled_entries: u64,
    pub total_hits: u64,
    pub total_successes: u64,
}

// === Utility functions ===

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0_f32;
    let mut norm_a = 0.0_f32;
    let mut norm_b = 0.0_f32;

    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

fn f32_to_bytes(floats: &[f32]) -> Vec<u8> {
    floats.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Return the regex pattern string for a given ParamType.
fn regex_for_type(param_type: &ParamType) -> Option<&'static str> {
    match param_type {
        ParamType::EmailAddress => Some(r"[\w.+-]+@[\w-]+\.[\w.-]+"),
        ParamType::Url => Some(r"https?://[^\s]+"),
        ParamType::FilePath => Some(r"[/~][\w/.@\-]+"),
        ParamType::Number => Some(r"\b\d+(?:\.\d+)?\b"),
        ParamType::DateTime => {
            Some(r"\d{4}-\d{2}-\d{2}(?:T\d{2}:\d{2})?|\b(?:today|yesterday|tomorrow)\b")
        }
        ParamType::Boolean => Some(r"\b(?:true|false|yes|no)\b"),
        ParamType::Text => None,    // handled separately
        ParamType::List(_) => None, // not yet supported
    }
}

/// Check whether a byte offset range overlaps with any already-used span.
fn overlaps_used(used: &[(usize, usize)], start: usize, end: usize) -> bool {
    used.iter().any(|&(s, e)| start < e && end > s)
}

/// Extract parameter values from task text based on the parameter schema.
///
/// Pass 1: type-specific regex extraction (email, url, path, number, datetime, boolean).
/// For Text params: try quoted strings first, then fall back to original-task diffing.
/// Pass 2: for unmatched Text params, diff task_text against original_task to find
/// words present in task_text but absent from the original template.
fn extract_parameters(
    task_text: &str,
    params: &[Parameter],
    original_task: Option<&str>,
) -> serde_json::Map<String, serde_json::Value> {
    use regex::Regex;
    use std::collections::HashSet;

    let mut result = serde_json::Map::new();
    let mut used_spans: Vec<(usize, usize)> = Vec::new();
    let mut unmatched_text_params: Vec<&Parameter> = Vec::new();

    // Pass 1: regex-based extraction for typed params, quoted-string for Text
    for param in params {
        if let Some(pattern_str) = regex_for_type(&param.param_type) {
            if let Ok(re) = Regex::new(pattern_str) {
                for m in re.find_iter(task_text) {
                    if !overlaps_used(&used_spans, m.start(), m.end()) {
                        used_spans.push((m.start(), m.end()));
                        result.insert(
                            param.name.clone(),
                            serde_json::Value::String(m.as_str().to_string()),
                        );
                        break; // one match per parameter
                    }
                }
            }
        } else if matches!(param.param_type, ParamType::Text) {
            // Try quoted strings first: "..." or '...'
            let quote_re = Regex::new(r#"(?:"([^"]+)"|'([^']+)')"#).unwrap();
            let mut found = false;
            for caps in quote_re.captures_iter(task_text) {
                let full = caps.get(0).unwrap();
                if !overlaps_used(&used_spans, full.start(), full.end()) {
                    let value = caps.get(1).or_else(|| caps.get(2)).unwrap();
                    used_spans.push((full.start(), full.end()));
                    result.insert(
                        param.name.clone(),
                        serde_json::Value::String(value.as_str().to_string()),
                    );
                    found = true;
                    break;
                }
            }
            if !found {
                unmatched_text_params.push(param);
            }
        }
    }

    // Pass 2: original-task diffing for unmatched Text params
    if !unmatched_text_params.is_empty() {
        if let Some(original) = original_task {
            // Tokenize both strings into words
            let original_words: HashSet<&str> = original
                .split_whitespace()
                .filter(|w| !w.starts_with('{'))
                .collect();
            let new_words: Vec<&str> = task_text
                .split_whitespace()
                .filter(|w| !original_words.contains(w))
                .collect();

            if !new_words.is_empty() {
                // Assign diff words to unmatched text params (one param gets all diff words joined)
                for param in &unmatched_text_params {
                    if !new_words.is_empty() {
                        result.insert(
                            param.name.clone(),
                            serde_json::Value::String(new_words.join(" ")),
                        );
                    }
                }
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        assert!((cosine_similarity(&a, &b) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_f32_roundtrip() {
        let original = vec![1.0_f32, 2.5, -3.14, 0.0];
        let bytes = f32_to_bytes(&original);
        let recovered = bytes_to_f32(&bytes);
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_effective_threshold_new_entry() {
        let entry = CachedWork {
            id: "test".into(),
            description: "test".into(),
            original_task: "test".into(),
            rationale: "test".into(),
            improvement_notes: None,
            parameters: vec![],
            implementation: CacheImplementation::ToolSequence { steps: vec![] },
            hit_count: 0,
            success_count: 0,
            failure_count: 0,
            created_at: 0,
            last_used_at: 0,
            similarity_threshold: 0.95,
            enabled: true,
        };
        // New entry: no adjustments
        assert!((entry.effective_threshold() - 0.95).abs() < 1e-6);
    }

    #[test]
    fn test_effective_threshold_proven_entry() {
        let entry = CachedWork {
            id: "test".into(),
            description: "test".into(),
            original_task: "test".into(),
            rationale: "test".into(),
            improvement_notes: None,
            parameters: vec![],
            implementation: CacheImplementation::ToolSequence { steps: vec![] },
            hit_count: 10,
            success_count: 10,
            failure_count: 0,
            created_at: 0,
            last_used_at: 0,
            similarity_threshold: 0.95,
            enabled: true,
        };
        // Proven entry: threshold should relax
        assert!(entry.effective_threshold() < 0.95);
    }

    #[test]
    fn test_extract_email_param() {
        let params = vec![Parameter {
            name: "recipient".into(),
            param_type: ParamType::EmailAddress,
            description: "Email recipient".into(),
            required: true,
        }];
        let result =
            extract_parameters("send email to alice@test.com about meeting", &params, None);
        assert_eq!(
            result.get("recipient").and_then(|v| v.as_str()),
            Some("alice@test.com")
        );
    }

    #[test]
    fn test_extract_url_param() {
        let params = vec![Parameter {
            name: "url".into(),
            param_type: ParamType::Url,
            description: "Target URL".into(),
            required: true,
        }];
        let result = extract_parameters(
            "fetch data from https://api.example.com/data",
            &params,
            None,
        );
        assert_eq!(
            result.get("url").and_then(|v| v.as_str()),
            Some("https://api.example.com/data")
        );
    }

    #[test]
    fn test_extract_filepath_param() {
        let params = vec![Parameter {
            name: "path".into(),
            param_type: ParamType::FilePath,
            description: "File path".into(),
            required: true,
        }];
        let result = extract_parameters("read /tmp/report.csv and summarize", &params, None);
        assert_eq!(
            result.get("path").and_then(|v| v.as_str()),
            Some("/tmp/report.csv")
        );
    }

    #[test]
    fn test_extract_number_param() {
        let params = vec![Parameter {
            name: "count".into(),
            param_type: ParamType::Number,
            description: "Number of results".into(),
            required: true,
        }];
        let result = extract_parameters("show top 5 results", &params, None);
        assert_eq!(result.get("count").and_then(|v| v.as_str()), Some("5"));
    }

    #[test]
    fn test_extract_boolean_param() {
        let params = vec![Parameter {
            name: "verbose".into(),
            param_type: ParamType::Boolean,
            description: "Verbose mode".into(),
            required: true,
        }];
        let result = extract_parameters("set verbose to true", &params, None);
        assert_eq!(result.get("verbose").and_then(|v| v.as_str()), Some("true"));
    }

    #[test]
    fn test_extract_text_via_diff() {
        let params = vec![Parameter {
            name: "sender".into(),
            param_type: ParamType::Text,
            description: "Email sender".into(),
            required: true,
        }];
        let result = extract_parameters(
            "check email from Alice",
            &params,
            Some("check email from {sender}"),
        );
        assert_eq!(result.get("sender").and_then(|v| v.as_str()), Some("Alice"));
    }

    #[test]
    fn test_extract_multiple_params() {
        let params = vec![
            Parameter {
                name: "recipient".into(),
                param_type: ParamType::EmailAddress,
                description: "".into(),
                required: true,
            },
            Parameter {
                name: "count".into(),
                param_type: ParamType::Number,
                description: "".into(),
                required: true,
            },
        ];
        let result = extract_parameters("send 3 emails to bob@test.com", &params, None);
        assert_eq!(
            result.get("recipient").and_then(|v| v.as_str()),
            Some("bob@test.com")
        );
        assert_eq!(result.get("count").and_then(|v| v.as_str()), Some("3"));
    }

    #[test]
    fn test_extract_no_match_returns_empty() {
        let params = vec![Parameter {
            name: "email".into(),
            param_type: ParamType::EmailAddress,
            description: "".into(),
            required: true,
        }];
        let result = extract_parameters("do something generic", &params, None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_extract_quoted_text() {
        let params = vec![Parameter {
            name: "query".into(),
            param_type: ParamType::Text,
            description: "Search query".into(),
            required: true,
        }];
        let result = extract_parameters("search for \"machine learning\"", &params, None);
        assert_eq!(
            result.get("query").and_then(|v| v.as_str()),
            Some("machine learning")
        );
    }

    #[test]
    fn test_extract_datetime_param() {
        let params = vec![Parameter {
            name: "date".into(),
            param_type: ParamType::DateTime,
            description: "Date".into(),
            required: true,
        }];
        let result = extract_parameters("events since 2024-01-15", &params, None);
        assert_eq!(
            result.get("date").and_then(|v| v.as_str()),
            Some("2024-01-15")
        );
    }
}
