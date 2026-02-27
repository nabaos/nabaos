//! Session Persistence — stores cookies and localStorage per domain in SQLite.
//!
//! Allows browser sessions to survive across agent restarts. Each domain
//! gets its own row with serialized cookies and localStorage.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::core::error::{NyayaError, Result};

/// A stored browser session for a domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSession {
    pub domain: String,
    pub cookies_json: String,
    pub local_storage_json: String,
    pub updated_at: i64,
}

/// SQLite-backed session store.
pub struct SessionStore {
    db: Connection,
}

impl SessionStore {
    /// Open (or create) the session store at the given path.
    pub fn new(db_path: &str) -> Result<Self> {
        let db = Connection::open(db_path)
            .map_err(|e| NyayaError::Config(format!("SessionStore DB open: {}", e)))?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS browser_sessions (
                domain TEXT PRIMARY KEY,
                cookies_json TEXT NOT NULL,
                local_storage_json TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            )",
        )
        .map_err(|e| NyayaError::Config(format!("SessionStore init: {}", e)))?;
        Ok(Self { db })
    }

    /// Save or update a session for a domain.
    pub fn save(&self, session: &StoredSession) -> Result<()> {
        self.db
            .execute(
                "INSERT OR REPLACE INTO browser_sessions (domain, cookies_json, local_storage_json, updated_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    session.domain,
                    session.cookies_json,
                    session.local_storage_json,
                    session.updated_at
                ],
            )
            .map_err(|e| NyayaError::Config(format!("SessionStore save: {}", e)))?;
        Ok(())
    }

    /// Load a session for a domain.
    pub fn load(&self, domain: &str) -> Result<Option<StoredSession>> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT domain, cookies_json, local_storage_json, updated_at FROM browser_sessions WHERE domain = ?1",
            )
            .map_err(|e| NyayaError::Config(format!("SessionStore load prepare: {}", e)))?;

        let mut rows = stmt
            .query_map(rusqlite::params![domain], |row| {
                Ok(StoredSession {
                    domain: row.get(0)?,
                    cookies_json: row.get(1)?,
                    local_storage_json: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            })
            .map_err(|e| NyayaError::Config(format!("SessionStore load query: {}", e)))?;

        match rows.next() {
            Some(Ok(session)) => Ok(Some(session)),
            Some(Err(e)) => Err(NyayaError::Config(format!("SessionStore load row: {}", e))),
            None => Ok(None),
        }
    }

    /// Delete a session for a domain.
    pub fn delete(&self, domain: &str) -> Result<()> {
        self.db
            .execute(
                "DELETE FROM browser_sessions WHERE domain = ?1",
                rusqlite::params![domain],
            )
            .map_err(|e| NyayaError::Config(format!("SessionStore delete: {}", e)))?;
        Ok(())
    }

    /// List all domains with saved sessions.
    pub fn list_domains(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .db
            .prepare("SELECT domain FROM browser_sessions ORDER BY updated_at DESC")
            .map_err(|e| NyayaError::Config(format!("SessionStore list prepare: {}", e)))?;

        let domains = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| NyayaError::Config(format!("SessionStore list query: {}", e)))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(domains)
    }

    /// Remove sessions older than max_age_secs. Returns count of deleted rows.
    pub fn cleanup_older_than(&self, max_age_secs: i64) -> Result<usize> {
        let cutoff = chrono::Utc::now().timestamp() - max_age_secs;
        let count = self
            .db
            .execute(
                "DELETE FROM browser_sessions WHERE updated_at < ?1",
                rusqlite::params![cutoff],
            )
            .map_err(|e| NyayaError::Config(format!("SessionStore cleanup: {}", e)))?;
        Ok(count)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store() -> SessionStore {
        SessionStore::new(":memory:").unwrap()
    }

    #[test]
    fn test_session_store_save_load() {
        let store = make_store();
        let session = StoredSession {
            domain: "example.com".into(),
            cookies_json: r#"[{"name":"sid","value":"abc123"}]"#.into(),
            local_storage_json: "{}".into(),
            updated_at: 1700000000,
        };
        store.save(&session).unwrap();

        let loaded = store.load("example.com").unwrap().unwrap();
        assert_eq!(loaded.domain, "example.com");
        assert_eq!(loaded.cookies_json, session.cookies_json);
        assert_eq!(loaded.updated_at, 1700000000);
    }

    #[test]
    fn test_session_store_not_found() {
        let store = make_store();
        let result = store.load("nonexistent.com").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_session_store_delete() {
        let store = make_store();
        let session = StoredSession {
            domain: "example.com".into(),
            cookies_json: "[]".into(),
            local_storage_json: "{}".into(),
            updated_at: 1700000000,
        };
        store.save(&session).unwrap();
        store.delete("example.com").unwrap();
        assert!(store.load("example.com").unwrap().is_none());
    }

    #[test]
    fn test_session_store_list_domains() {
        let store = make_store();
        for (domain, ts) in [("a.com", 100), ("b.com", 200), ("c.com", 50)] {
            store
                .save(&StoredSession {
                    domain: domain.into(),
                    cookies_json: "[]".into(),
                    local_storage_json: "{}".into(),
                    updated_at: ts,
                })
                .unwrap();
        }
        let domains = store.list_domains().unwrap();
        assert_eq!(domains.len(), 3);
        // Ordered by updated_at DESC
        assert_eq!(domains[0], "b.com");
        assert_eq!(domains[1], "a.com");
        assert_eq!(domains[2], "c.com");
    }

    #[test]
    fn test_session_store_cleanup() {
        let store = make_store();
        let now = chrono::Utc::now().timestamp();
        // One old, one recent
        store
            .save(&StoredSession {
                domain: "old.com".into(),
                cookies_json: "[]".into(),
                local_storage_json: "{}".into(),
                updated_at: now - 100000,
            })
            .unwrap();
        store
            .save(&StoredSession {
                domain: "new.com".into(),
                cookies_json: "[]".into(),
                local_storage_json: "{}".into(),
                updated_at: now,
            })
            .unwrap();

        let deleted = store.cleanup_older_than(50000).unwrap();
        assert_eq!(deleted, 1);
        assert!(store.load("old.com").unwrap().is_none());
        assert!(store.load("new.com").unwrap().is_some());
    }
}
