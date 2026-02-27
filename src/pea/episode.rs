use std::collections::HashSet;
use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::core::error::{NyayaError, Result};

// ---------------------------------------------------------------------------
// Outcome
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Succeeded,
    Failed(String),
    PartialSuccess(String),
}

// ---------------------------------------------------------------------------
// Episode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub id: String,
    pub objective_summary: String,
    pub task_description: String,
    pub approach_taken: String,
    pub outcome: Outcome,
    pub cost_usd: f64,
    pub duration_secs: u64,
    pub created_at: u64,
}

// ---------------------------------------------------------------------------
// EpisodeStore
// ---------------------------------------------------------------------------

pub struct EpisodeStore {
    conn: Connection,
}

impl EpisodeStore {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| NyayaError::Cache(format!("failed to open episode store: {e}")))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS pea_episodes (
                id                TEXT PRIMARY KEY,
                objective_summary TEXT NOT NULL,
                task_description  TEXT NOT NULL,
                approach_taken    TEXT NOT NULL,
                outcome_json      TEXT NOT NULL,
                cost_usd          REAL NOT NULL DEFAULT 0.0,
                duration_secs     INTEGER NOT NULL DEFAULT 0,
                created_at        INTEGER NOT NULL
            );",
        )
        .map_err(|e| NyayaError::Cache(format!("failed to create pea_episodes table: {e}")))?;

        Ok(Self { conn })
    }

    pub fn save(&self, episode: &Episode) -> Result<()> {
        let outcome_json = serde_json::to_string(&episode.outcome)
            .map_err(|e| NyayaError::Cache(format!("serialize outcome: {e}")))?;

        self.conn
            .execute(
                "INSERT OR REPLACE INTO pea_episodes
                 (id, objective_summary, task_description, approach_taken,
                  outcome_json, cost_usd, duration_secs, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    episode.id,
                    episode.objective_summary,
                    episode.task_description,
                    episode.approach_taken,
                    outcome_json,
                    episode.cost_usd,
                    episode.duration_secs,
                    episode.created_at,
                ],
            )
            .map_err(|e| NyayaError::Cache(format!("save episode: {e}")))?;

        Ok(())
    }

    pub fn list_all(&self) -> Result<Vec<Episode>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, objective_summary, task_description, approach_taken,
                        outcome_json, cost_usd, duration_secs, created_at
                 FROM pea_episodes ORDER BY created_at DESC",
            )
            .map_err(|e| NyayaError::Cache(format!("prepare list_all: {e}")))?;

        let rows = stmt
            .query_map([], |row| {
                let outcome_json: String = row.get(4)?;
                let outcome: Outcome = serde_json::from_str(&outcome_json)
                    .unwrap_or(Outcome::Failed("deserialization error".to_string()));
                Ok(Episode {
                    id: row.get(0)?,
                    objective_summary: row.get(1)?,
                    task_description: row.get(2)?,
                    approach_taken: row.get(3)?,
                    outcome,
                    cost_usd: row.get(5)?,
                    duration_secs: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })
            .map_err(|e| NyayaError::Cache(format!("query list_all: {e}")))?;

        let mut result = Vec::new();
        for r in rows {
            result.push(r.map_err(|e| NyayaError::Cache(format!("read episode row: {e}")))?);
        }
        Ok(result)
    }

    /// Keyword-overlap search for Upamana (analogical reasoning).
    ///
    /// Returns `(task_description, outcome_string, score)` tuples where
    /// `score = matching_words / total_query_words`, filtered to score > 0.3
    /// and sorted descending by score.
    pub fn search_similar(&self, query: &str) -> Result<Vec<(String, String, f64)>> {
        let query_words: HashSet<String> =
            query.split_whitespace().map(|w| w.to_lowercase()).collect();

        if query_words.is_empty() {
            return Ok(Vec::new());
        }

        let total = query_words.len() as f64;

        let mut stmt = self
            .conn
            .prepare("SELECT task_description, outcome_json FROM pea_episodes")
            .map_err(|e| NyayaError::Cache(format!("prepare search_similar: {e}")))?;

        let rows = stmt
            .query_map([], |row| {
                let task_desc: String = row.get(0)?;
                let outcome_json: String = row.get(1)?;
                Ok((task_desc, outcome_json))
            })
            .map_err(|e| NyayaError::Cache(format!("query search_similar: {e}")))?;

        let mut results = Vec::new();
        for r in rows {
            let (task_desc, outcome_json) =
                r.map_err(|e| NyayaError::Cache(format!("read search row: {e}")))?;

            let task_words: HashSet<String> = task_desc
                .split_whitespace()
                .map(|w| w.to_lowercase())
                .collect();

            let matching = query_words.intersection(&task_words).count() as f64;
            let score = matching / total;

            if score > 0.3 {
                let outcome_str = outcome_json;
                results.push((task_desc, outcome_str, score));
            }
        }

        results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        Ok(results)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_episode(id: &str, task: &str, outcome: Outcome) -> Episode {
        Episode {
            id: id.to_string(),
            objective_summary: "test objective".to_string(),
            task_description: task.to_string(),
            approach_taken: "test approach".to_string(),
            outcome,
            cost_usd: 0.5,
            duration_secs: 30,
            created_at: 1000,
        }
    }

    #[test]
    fn test_episode_store_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("episodes.db");
        let store = EpisodeStore::open(&db_path).unwrap();

        store
            .save(&make_episode("e1", "task one", Outcome::Succeeded))
            .unwrap();
        store
            .save(&make_episode(
                "e2",
                "task two",
                Outcome::Failed("oops".into()),
            ))
            .unwrap();

        let all = store.list_all().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_episode_search_similar() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("episodes.db");
        let store = EpisodeStore::open(&db_path).unwrap();

        store
            .save(&make_episode(
                "e1",
                "research Indian recipes",
                Outcome::Succeeded,
            ))
            .unwrap();
        store
            .save(&make_episode(
                "e2",
                "deploy server to production",
                Outcome::Succeeded,
            ))
            .unwrap();

        let results = store.search_similar("recipe research").unwrap();
        assert!(!results.is_empty());
        assert!(results[0].2 > 0.3);
    }

    #[test]
    fn test_episode_outcome_serialization() {
        let variants = vec![
            Outcome::Succeeded,
            Outcome::Failed("something broke".to_string()),
            Outcome::PartialSuccess("half done".to_string()),
        ];

        for variant in &variants {
            let json = serde_json::to_string(variant).unwrap();
            let deserialized: Outcome = serde_json::from_str(&json).unwrap();
            assert_eq!(&deserialized, variant);
        }
    }
}
