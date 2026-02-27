//! Intent cache — maps IntentKey to cached execution plans.
//!
//! Key: IntentKey (e.g., "check_email")
//! Value: CachedWork (tool sequence, parameters, metadata)
//! O(1) hash lookup, SQLite-backed persistence.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::core::error::{NyayaError, Result};
use crate::w5h2::types::IntentKey;

/// A cached execution plan for an intent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentCacheEntry {
    pub intent_key: String,
    pub description: String,
    pub tool_sequence: Vec<CachedToolCall>,
    pub hit_count: u64,
    pub success_count: u64,
    pub failure_count: u64,
    pub enabled: bool,
    pub created_at: i64,
    pub last_used_at: i64,
    pub response_text: Option<String>,
}

/// A tool call in a cached execution plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedToolCall {
    pub tool: String,
    pub args: serde_json::Map<String, serde_json::Value>,
}

impl IntentCacheEntry {
    pub fn success_rate(&self) -> f64 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            1.0
        } else {
            self.success_count as f64 / total as f64
        }
    }
}

/// Hash-based intent-to-execution-plan cache
pub struct IntentCache {
    db: Connection,
}

impl IntentCache {
    pub fn open(db_path: &std::path::Path) -> Result<Self> {
        let db = Connection::open(db_path)
            .map_err(|e| NyayaError::Cache(format!("Failed to open intent cache: {}", e)))?;

        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS intent_cache (
                intent_key TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                tool_sequence TEXT NOT NULL,
                hit_count INTEGER DEFAULT 0,
                success_count INTEGER DEFAULT 0,
                failure_count INTEGER DEFAULT 0,
                enabled INTEGER DEFAULT 1,
                created_at INTEGER NOT NULL,
                last_used_at INTEGER NOT NULL
            );",
        )
        .map_err(|e| NyayaError::Cache(format!("Failed to create intent_cache table: {}", e)))?;

        // Migration: add response_text column if not exists
        let _ = db.execute_batch("ALTER TABLE intent_cache ADD COLUMN response_text TEXT;");

        Ok(Self { db })
    }

    /// Look up a cached execution plan for an intent
    pub fn lookup(&self, key: &IntentKey) -> Result<Option<IntentCacheEntry>> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT intent_key, description, tool_sequence, hit_count, success_count,
                    failure_count, enabled, created_at, last_used_at, response_text
             FROM intent_cache WHERE intent_key = ?1 AND enabled = 1",
            )
            .map_err(|e| NyayaError::Cache(format!("Prepare failed: {}", e)))?;

        let result = stmt.query_row(rusqlite::params![key.as_str()], |row| {
            let tool_seq_json: String = row.get(2)?;
            Ok(IntentCacheEntry {
                intent_key: row.get(0)?,
                description: row.get(1)?,
                tool_sequence: serde_json::from_str(&tool_seq_json).unwrap_or_default(),
                hit_count: row.get::<_, i64>(3)? as u64,
                success_count: row.get::<_, i64>(4)? as u64,
                failure_count: row.get::<_, i64>(5)? as u64,
                enabled: row.get::<_, i64>(6)? != 0,
                created_at: row.get(7)?,
                last_used_at: row.get(8)?,
                response_text: row.get(9).ok(),
            })
        });

        match result {
            Ok(entry) => {
                // Fix 15: TTL enforcement — reject entries older than 7 days
                let now = now_millis();
                let max_age_ms: i64 = 7 * 24 * 60 * 60 * 1000; // 7 days
                if now - entry.created_at > max_age_ms {
                    return Ok(None);
                }
                // Update hit count
                let _ = self.db.execute(
                    "UPDATE intent_cache SET hit_count = hit_count + 1, last_used_at = ?1 WHERE intent_key = ?2",
                    rusqlite::params![now_millis(), key.as_str()],
                );
                Ok(Some(entry))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(NyayaError::Cache(format!("Query failed: {}", e))),
        }
    }

    /// Store a new execution plan
    pub fn store(
        &self,
        key: &IntentKey,
        description: &str,
        tool_sequence: &[CachedToolCall],
        response_text: Option<&str>,
    ) -> Result<()> {
        let now = now_millis();
        let tool_seq_json = serde_json::to_string(tool_sequence)?;

        self.db
            .execute(
                "INSERT OR REPLACE INTO intent_cache
             (intent_key, description, tool_sequence, hit_count, success_count,
              failure_count, enabled, created_at, last_used_at, response_text)
             VALUES (?1, ?2, ?3, 0, 0, 0, 1, ?4, ?5, ?6)",
                rusqlite::params![
                    key.as_str(),
                    description,
                    tool_seq_json,
                    now,
                    now,
                    response_text
                ],
            )
            .map_err(|e| NyayaError::Cache(format!("Store failed: {}", e)))?;

        Ok(())
    }

    /// Record outcome of a cached execution
    pub fn record_outcome(&self, key: &IntentKey, success: bool) -> Result<()> {
        let col = if success {
            "success_count"
        } else {
            "failure_count"
        };
        self.db
            .execute(
                &format!(
                    "UPDATE intent_cache SET {} = {} + 1 WHERE intent_key = ?1",
                    col, col
                ),
                rusqlite::params![key.as_str()],
            )
            .map_err(|e| NyayaError::Cache(format!("Record outcome failed: {}", e)))?;
        Ok(())
    }

    /// Get cache statistics
    pub fn stats(&self) -> Result<IntentCacheStats> {
        let total: i64 = self
            .db
            .query_row("SELECT COUNT(*) FROM intent_cache", [], |r| r.get(0))
            .map_err(|e| NyayaError::Cache(format!("Stats query failed: {}", e)))?;

        let enabled: i64 = self
            .db
            .query_row(
                "SELECT COUNT(*) FROM intent_cache WHERE enabled = 1",
                [],
                |r| r.get(0),
            )
            .map_err(|e| NyayaError::Cache(format!("Stats query failed: {}", e)))?;

        let total_hits: i64 = self
            .db
            .query_row(
                "SELECT COALESCE(SUM(hit_count), 0) FROM intent_cache",
                [],
                |r| r.get(0),
            )
            .map_err(|e| NyayaError::Cache(format!("Stats query failed: {}", e)))?;

        Ok(IntentCacheStats {
            total_entries: total as u64,
            enabled_entries: enabled as u64,
            total_hits: total_hits as u64,
        })
    }
}

#[derive(Debug)]
pub struct IntentCacheStats {
    pub total_entries: u64,
    pub enabled_entries: u64,
    pub total_hits: u64,
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intent_cache_store_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let cache = IntentCache::open(&db_path).unwrap();

        let key = IntentKey("check_email".to_string());
        let tools = vec![CachedToolCall {
            tool: "email_fetch".to_string(),
            args: serde_json::Map::new(),
        }];

        cache
            .store(&key, "Check email inbox", &tools, None)
            .unwrap();

        let entry = cache.lookup(&key).unwrap();
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert_eq!(entry.intent_key, "check_email");
        assert_eq!(entry.tool_sequence.len(), 1);
    }

    #[test]
    fn test_intent_cache_miss() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let cache = IntentCache::open(&db_path).unwrap();

        let key = IntentKey("nonexistent".to_string());
        let result = cache.lookup(&key).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_intent_cache_stats() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let cache = IntentCache::open(&db_path).unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 0);

        let key = IntentKey("check_email".to_string());
        cache.store(&key, "Check email", &[], None).unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 1);
    }
}
