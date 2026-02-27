//! Query fingerprint cache — Tier 1 instant lookup.
//!
//! SHA-256 hash of normalized query text -> classified IntentKey.
//! Populated by Tier 2 (SetFit ONNX) results. O(1) HashMap lookup.

use std::collections::HashMap;

use rusqlite::Connection;

use crate::core::error::{NyayaError, Result};
use crate::w5h2::types::IntentKey;

/// Maximum number of fingerprint entries to prevent unbounded memory growth / OOM DoS.
const MAX_FINGERPRINT_ENTRIES: usize = 50_000;

/// Maximum age of a fingerprint cache entry before it is considered stale (7 days).
const MAX_AGE_SECS: i64 = 86400 * 7;

/// `MAX_AGE_SECS` expressed in milliseconds, matching the `created_at` storage format.
const MAX_AGE_MILLIS: i64 = MAX_AGE_SECS * 1000;

/// A cached fingerprint entry
#[derive(Debug, Clone)]
struct FingerprintEntry {
    intent_key: IntentKey,
    confidence: f32,
    hit_count: u64,
    /// Unix-epoch timestamp in milliseconds when this entry was created.
    created_at: i64,
}

/// In-memory fingerprint cache backed by SQLite
pub struct FingerprintCache {
    entries: HashMap<String, FingerprintEntry>,
    db_path: String,
}

impl FingerprintCache {
    /// Open or create the fingerprint cache
    pub fn open(db: &Connection) -> Result<Self> {
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS fingerprints (
                hash TEXT PRIMARY KEY,
                intent_key TEXT NOT NULL,
                confidence REAL NOT NULL,
                hit_count INTEGER DEFAULT 0,
                query_sample TEXT,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_fp_intent ON fingerprints(intent_key);",
        )
        .map_err(|e| NyayaError::Cache(format!("Failed to create fingerprint table: {}", e)))?;

        let db_path = db.path().unwrap_or(":memory:").to_string();

        // Load all entries into memory
        let mut entries = HashMap::new();
        let now_ms = now_millis();
        let cutoff = now_ms - MAX_AGE_MILLIS;

        let mut stmt = db
            .prepare(
                "SELECT hash, intent_key, confidence, hit_count, created_at FROM fingerprints
             WHERE created_at > ?1",
            )
            .map_err(|e| {
                NyayaError::Cache(format!("Failed to prepare fingerprint query: {}", e))
            })?;

        let rows = stmt
            .query_map(rusqlite::params![cutoff], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f32>(2)?,
                    row.get::<_, i64>(3)? as u64,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(|e| NyayaError::Cache(format!("Failed to query fingerprints: {}", e)))?;

        for row in rows {
            let (hash, intent_key, confidence, hit_count, created_at) =
                row.map_err(|e| NyayaError::Cache(format!("Row read error: {}", e)))?;
            entries.insert(
                hash,
                FingerprintEntry {
                    intent_key: IntentKey(intent_key),
                    confidence,
                    hit_count,
                    created_at,
                },
            );
        }

        tracing::info!(entries = entries.len(), "Fingerprint cache loaded");

        Ok(Self { entries, db_path })
    }

    /// Look up a query in the fingerprint cache (Tier 1).
    ///
    /// Returns `None` if the entry does not exist or has exceeded `MAX_AGE_MILLIS`.
    pub fn lookup(&mut self, query: &str) -> Option<(IntentKey, f32)> {
        let hash = normalize_and_hash(query);
        let now_ms = now_millis();
        let cutoff = now_ms - MAX_AGE_MILLIS;

        if let Some(entry) = self.entries.get_mut(&hash) {
            // TTL check — skip stale entries
            if entry.created_at <= cutoff {
                return None;
            }

            entry.hit_count += 1;
            // Update hit count in DB (best-effort)
            if let Ok(db) = Connection::open(&self.db_path) {
                let _ = db.execute(
                    "UPDATE fingerprints SET hit_count = hit_count + 1 WHERE hash = ?1",
                    rusqlite::params![hash],
                );
            }
            Some((entry.intent_key.clone(), entry.confidence))
        } else {
            None
        }
    }

    /// Store a classification result in the fingerprint cache
    pub fn store(&mut self, query: &str, intent_key: &IntentKey, confidence: f32) -> Result<()> {
        let hash = normalize_and_hash(query);
        let now = now_millis();

        // Capacity limit: prevent unbounded memory growth
        if self.entries.len() >= MAX_FINGERPRINT_ENTRIES {
            self.invalidate_stale();
            // If still full after stale eviction, evict the oldest entry
            if self.entries.len() >= MAX_FINGERPRINT_ENTRIES {
                if let Some(oldest_key) = self
                    .entries
                    .iter()
                    .min_by_key(|(_, e)| e.created_at)
                    .map(|(k, _)| k.clone())
                {
                    self.entries.remove(&oldest_key);
                }
            }
        }

        let db = Connection::open(&self.db_path)
            .map_err(|e| NyayaError::Cache(format!("Failed to open DB: {}", e)))?;
        db.execute(
            "INSERT OR REPLACE INTO fingerprints (hash, intent_key, confidence, hit_count, query_sample, created_at)
             VALUES (?1, ?2, ?3, 0, ?4, ?5)",
            rusqlite::params![hash, intent_key.as_str(), confidence, query, now],
        ).map_err(|e| NyayaError::Cache(format!("Failed to store fingerprint: {}", e)))?;

        self.entries.insert(
            hash,
            FingerprintEntry {
                intent_key: intent_key.clone(),
                confidence,
                hit_count: 0,
                created_at: now,
            },
        );

        Ok(())
    }

    /// Remove all entries whose `created_at` is older than `MAX_AGE_MILLIS`.
    ///
    /// Returns the number of in-memory entries that were evicted.  Also deletes
    /// the corresponding rows from the backing SQLite database (best-effort).
    pub fn invalidate_stale(&mut self) -> usize {
        let now_ms = now_millis();
        let cutoff = now_ms - MAX_AGE_MILLIS;

        // Remove from in-memory cache
        let before = self.entries.len();
        self.entries.retain(|_, entry| entry.created_at > cutoff);
        let removed_mem = before - self.entries.len();

        // Remove from DB (best-effort)
        if let Ok(db) = Connection::open(&self.db_path) {
            let _ = db.execute(
                "DELETE FROM fingerprints WHERE created_at <= ?1",
                rusqlite::params![cutoff],
            );
        }

        if removed_mem > 0 {
            tracing::info!(
                removed = removed_mem,
                "Stale fingerprint entries invalidated"
            );
        }

        removed_mem
    }

    /// Clear all entries from the in-memory cache and the backing database.
    pub fn clear(&mut self) -> Result<()> {
        self.entries.clear();

        let db = Connection::open(&self.db_path)
            .map_err(|e| NyayaError::Cache(format!("Failed to open DB: {}", e)))?;
        db.execute("DELETE FROM fingerprints", [])
            .map_err(|e| NyayaError::Cache(format!("Failed to clear fingerprints: {}", e)))?;

        tracing::info!("Fingerprint cache cleared");
        Ok(())
    }

    /// Get cache statistics
    pub fn stats(&self) -> FingerprintStats {
        let total_hits: u64 = self.entries.values().map(|e| e.hit_count).sum();
        FingerprintStats {
            total_entries: self.entries.len() as u64,
            total_hits,
        }
    }

    /// Number of cached fingerprints
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug)]
pub struct FingerprintStats {
    pub total_entries: u64,
    pub total_hits: u64,
}

/// Normalize query text and compute SHA-256 hash
fn normalize_and_hash(query: &str) -> String {
    use sha2::{Digest, Sha256};

    // Normalize: lowercase, trim, collapse whitespace
    let normalized: String = query
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
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

    fn test_db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_fp.db");
        let db = Connection::open(&db_path).unwrap();
        (dir, db)
    }

    #[test]
    fn test_normalize_and_hash() {
        // Same content, different formatting -> same hash
        let h1 = normalize_and_hash("Check  My  Email");
        let h2 = normalize_and_hash("check my email");
        let h3 = normalize_and_hash("  check   my   email  ");
        assert_eq!(h1, h2);
        assert_eq!(h2, h3);

        // Different content -> different hash
        let h4 = normalize_and_hash("check the weather");
        assert_ne!(h1, h4);
    }

    #[test]
    fn test_fingerprint_store_lookup() {
        let (_dir, db) = test_db();
        let mut cache = FingerprintCache::open(&db).unwrap();

        let key = IntentKey("check_email".to_string());
        cache.store("check my email", &key, 0.95).unwrap();

        let result = cache.lookup("check my email");
        assert!(result.is_some());
        let (found_key, conf) = result.unwrap();
        assert_eq!(found_key.as_str(), "check_email");
        assert!((conf - 0.95).abs() < 0.01);

        // Normalized form should also match
        let result2 = cache.lookup("  Check  My  Email  ");
        assert!(result2.is_some());

        // Different query should miss
        let result3 = cache.lookup("check the weather");
        assert!(result3.is_none());
    }

    #[test]
    fn test_fingerprint_stats() {
        let (_dir, db) = test_db();
        let mut cache = FingerprintCache::open(&db).unwrap();

        assert_eq!(cache.len(), 0);

        let key = IntentKey("check_email".to_string());
        cache.store("check my email", &key, 0.95).unwrap();
        assert_eq!(cache.len(), 1);

        // Hit it twice
        cache.lookup("check my email");
        cache.lookup("check my email");

        let stats = cache.stats();
        assert_eq!(stats.total_entries, 1);
        assert_eq!(stats.total_hits, 2);
    }
}
