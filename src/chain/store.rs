use crate::chain::dsl::ChainDef;
use crate::core::error::{NyayaError, Result};

/// SQLite-backed chain template store.
pub struct ChainStore {
    conn: rusqlite::Connection,
}

/// Stored chain metadata.
#[derive(Debug)]
pub struct ChainRecord {
    pub chain_id: String,
    pub name: String,
    pub description: String,
    pub yaml: String,
    pub trust_level: u32,
    pub hit_count: u64,
    pub success_count: u64,
    pub created_at: String,
}

impl ChainRecord {
    pub fn success_rate(&self) -> f64 {
        if self.hit_count == 0 {
            0.0
        } else {
            self.success_count as f64 / self.hit_count as f64
        }
    }
}

impl ChainStore {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let conn = rusqlite::Connection::open(path)
            .map_err(|e| NyayaError::Cache(format!("Workflow DB open failed: {}", e)))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS chains (
                chain_id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                yaml TEXT NOT NULL,
                trust_level INTEGER NOT NULL DEFAULT 0,
                hit_count INTEGER NOT NULL DEFAULT 0,
                success_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_chains_name ON chains(name);",
        )
        .map_err(|e| NyayaError::Cache(format!("Workflow table creation failed: {}", e)))?;
        Ok(Self { conn })
    }

    /// Store a chain definition.
    pub fn store(&self, chain: &ChainDef) -> Result<()> {
        let yaml = chain.to_yaml()?;
        self.conn
            .execute(
                "INSERT OR REPLACE INTO chains (chain_id, name, description, yaml) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![chain.id, chain.name, chain.description, yaml],
            )
            .map_err(|e| NyayaError::Cache(format!("Workflow store failed: {}", e)))?;
        Ok(())
    }

    /// Look up a chain by ID.
    pub fn lookup(&self, chain_id: &str) -> Result<Option<ChainRecord>> {
        use rusqlite::OptionalExtension;
        let record = self
            .conn
            .query_row(
                "SELECT chain_id, name, description, yaml, trust_level, hit_count, success_count, created_at
                 FROM chains WHERE chain_id = ?1",
                rusqlite::params![chain_id],
                |row| {
                    Ok(ChainRecord {
                        chain_id: row.get(0)?,
                        name: row.get(1)?,
                        description: row.get(2)?,
                        yaml: row.get(3)?,
                        trust_level: row.get(4)?,
                        hit_count: row.get(5)?,
                        success_count: row.get(6)?,
                        created_at: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(|e| NyayaError::Cache(format!("Workflow lookup failed: {}", e)))?;
        Ok(record)
    }

    /// Record a successful execution (increments hit_count and success_count).
    pub fn record_success(&self, chain_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE chains SET hit_count = hit_count + 1, success_count = success_count + 1 WHERE chain_id = ?1",
                rusqlite::params![chain_id],
            )
            .map_err(|e| NyayaError::Cache(format!("Workflow success record failed: {}", e)))?;
        Ok(())
    }

    /// Record a failed execution (increments hit_count only).
    pub fn record_failure(&self, chain_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE chains SET hit_count = hit_count + 1 WHERE chain_id = ?1",
                rusqlite::params![chain_id],
            )
            .map_err(|e| NyayaError::Cache(format!("Workflow failure record failed: {}", e)))?;
        Ok(())
    }

    /// Update trust level for a chain.
    pub fn set_trust_level(&self, chain_id: &str, level: u32) -> Result<()> {
        self.conn
            .execute(
                "UPDATE chains SET trust_level = ?1 WHERE chain_id = ?2",
                rusqlite::params![level, chain_id],
            )
            .map_err(|e| NyayaError::Cache(format!("Trust level update failed: {}", e)))?;
        Ok(())
    }

    /// List all chains, newest first.
    pub fn list(&self, limit: u32) -> Result<Vec<ChainRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT chain_id, name, description, yaml, trust_level, hit_count, success_count, created_at
                 FROM chains ORDER BY created_at DESC LIMIT ?1",
            )
            .map_err(|e| NyayaError::Cache(format!("Workflow list failed: {}", e)))?;

        let records = stmt
            .query_map(rusqlite::params![limit], |row| {
                Ok(ChainRecord {
                    chain_id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    yaml: row.get(3)?,
                    trust_level: row.get(4)?,
                    hit_count: row.get(5)?,
                    success_count: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })
            .map_err(|e| NyayaError::Cache(format!("Workflow list failed: {}", e)))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(records)
    }

    /// Find chains eligible for trust graduation (>50 hits, >95% success).
    pub fn graduation_candidates(&self) -> Result<Vec<ChainRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT chain_id, name, description, yaml, trust_level, hit_count, success_count, created_at
                 FROM chains
                 WHERE hit_count >= 50
                   AND trust_level = 0
                   AND CAST(success_count AS REAL) / CAST(hit_count AS REAL) > 0.95
                 ORDER BY hit_count DESC",
            )
            .map_err(|e| NyayaError::Cache(format!("Graduation query failed: {}", e)))?;

        let records = stmt
            .query_map([], |row| {
                Ok(ChainRecord {
                    chain_id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    yaml: row.get(3)?,
                    trust_level: row.get(4)?,
                    hit_count: row.get(5)?,
                    success_count: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })
            .map_err(|e| NyayaError::Cache(format!("Graduation query failed: {}", e)))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::dsl::{ChainDef, ChainStep, ParamDef, ParamType};

    fn test_chain() -> ChainDef {
        ChainDef {
            id: "test_chain".into(),
            name: "Test".into(),
            description: "A test chain".into(),
            params: vec![ParamDef {
                name: "city".into(),
                param_type: ParamType::Text,
                description: "City".into(),
                required: true,
                default: None,
            }],
            steps: vec![ChainStep {
                id: "step1".into(),
                ability: "data.fetch_url".into(),
                args: std::collections::HashMap::from([(
                    "url".into(),
                    "https://example.com".into(),
                )]),
                output_key: None,
                condition: None,
                on_failure: None,
            }],
        }
    }

    #[test]
    fn test_store_and_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let store = ChainStore::open(&dir.path().join("chains.db")).unwrap();
        let chain = test_chain();

        store.store(&chain).unwrap();
        let record = store.lookup("test_chain").unwrap().unwrap();
        assert_eq!(record.name, "Test");
        assert_eq!(record.trust_level, 0);
        assert_eq!(record.hit_count, 0);
    }

    #[test]
    fn test_success_tracking() {
        let dir = tempfile::tempdir().unwrap();
        let store = ChainStore::open(&dir.path().join("chains.db")).unwrap();
        store.store(&test_chain()).unwrap();

        store.record_success("test_chain").unwrap();
        store.record_success("test_chain").unwrap();
        store.record_failure("test_chain").unwrap();

        let record = store.lookup("test_chain").unwrap().unwrap();
        assert_eq!(record.hit_count, 3);
        assert_eq!(record.success_count, 2);
        assert!((record.success_rate() - 0.6667).abs() < 0.01);
    }
}
