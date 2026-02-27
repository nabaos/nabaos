//! Training example queue — accumulates (query, intent_label, rephrasings)
//! from LLM <nyaya> block R: lines. When enough examples accumulate,
//! triggers a batch retrain of the SetFit classifier service.

use crate::core::error::{NyayaError, Result};

/// A training example for SetFit fine-tuning.
#[derive(Debug, Clone)]
pub struct TrainingExample {
    pub query: String,
    pub intent_label: String,
    pub source: TrainingSource,
    pub created_at: i64,
}

/// Where the training example came from.
#[derive(Debug, Clone, Copy)]
pub enum TrainingSource {
    /// LLM generated via R: line in <nyaya> block
    LlmRephrasing,
    /// User confirmed the intent label was correct
    UserConfirmed,
    /// Original query that triggered the LLM call
    OriginalQuery,
}

impl std::fmt::Display for TrainingSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrainingSource::LlmRephrasing => write!(f, "llm_rephrasing"),
            TrainingSource::UserConfirmed => write!(f, "user_confirmed"),
            TrainingSource::OriginalQuery => write!(f, "original_query"),
        }
    }
}

/// SQLite-backed training example queue.
pub struct TrainingQueue {
    conn: rusqlite::Connection,
}

impl TrainingQueue {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let conn = rusqlite::Connection::open(path)
            .map_err(|e| NyayaError::Cache(format!("Training queue DB open failed: {}", e)))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS training_queue (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                query TEXT NOT NULL,
                intent_label TEXT NOT NULL,
                source TEXT NOT NULL,
                exported INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_tq_label ON training_queue(intent_label);
            CREATE INDEX IF NOT EXISTS idx_tq_exported ON training_queue(exported);",
        )
        .map_err(|e| NyayaError::Cache(format!("Training queue table creation failed: {}", e)))?;

        Ok(Self { conn })
    }

    /// Enqueue a training example.
    pub fn enqueue(&self, query: &str, intent_label: &str, source: TrainingSource) -> Result<()> {
        let now = now_millis();
        self.conn
            .execute(
                "INSERT INTO training_queue (query, intent_label, source, created_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![query, intent_label, source.to_string(), now],
            )
            .map_err(|e| NyayaError::Cache(format!("Training enqueue failed: {}", e)))?;
        Ok(())
    }

    /// Enqueue multiple rephrasings from a single LLM response.
    pub fn enqueue_rephrasings(
        &self,
        original_query: &str,
        intent_label: &str,
        rephrasings: &[String],
    ) -> Result<usize> {
        // Always store the original query
        self.enqueue(original_query, intent_label, TrainingSource::OriginalQuery)?;

        // Store each rephrasing
        for r in rephrasings {
            self.enqueue(r, intent_label, TrainingSource::LlmRephrasing)?;
        }

        Ok(1 + rephrasings.len())
    }

    /// Get the number of unexported examples.
    pub fn pending_count(&self) -> Result<u64> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM training_queue WHERE exported = 0",
                [],
                |r| r.get(0),
            )
            .map_err(|e| NyayaError::Cache(format!("Count query failed: {}", e)))?;
        Ok(count as u64)
    }

    /// Export all unexported examples as JSON for the classifier service /retrain endpoint.
    /// Marks them as exported.
    pub fn export_batch(&self) -> Result<Vec<TrainingExample>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, query, intent_label, source, created_at
                 FROM training_queue WHERE exported = 0 ORDER BY id",
            )
            .map_err(|e| NyayaError::Cache(format!("Export query failed: {}", e)))?;

        let examples: Vec<(i64, TrainingExample)> = stmt
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let source_str: String = row.get(3)?;
                Ok((
                    id,
                    TrainingExample {
                        query: row.get(1)?,
                        intent_label: row.get(2)?,
                        source: match source_str.as_str() {
                            "user_confirmed" => TrainingSource::UserConfirmed,
                            "original_query" => TrainingSource::OriginalQuery,
                            _ => TrainingSource::LlmRephrasing,
                        },
                        created_at: row.get(4)?,
                    },
                ))
            })
            .map_err(|e| NyayaError::Cache(format!("Export query failed: {}", e)))?
            .filter_map(|r| r.ok())
            .collect();

        // Mark as exported
        if !examples.is_empty() {
            let max_id = examples.last().map(|(id, _)| *id).unwrap_or(0);
            self.conn
                .execute(
                    "UPDATE training_queue SET exported = 1 WHERE id <= ?1 AND exported = 0",
                    rusqlite::params![max_id],
                )
                .map_err(|e| NyayaError::Cache(format!("Export update failed: {}", e)))?;
        }

        Ok(examples.into_iter().map(|(_, ex)| ex).collect())
    }

    /// Get statistics about the training queue.
    pub fn stats(&self) -> Result<TrainingQueueStats> {
        let total: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM training_queue", [], |r| r.get(0))
            .map_err(|e| NyayaError::Cache(format!("Stats failed: {}", e)))?;

        let pending: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM training_queue WHERE exported = 0",
                [],
                |r| r.get(0),
            )
            .map_err(|e| NyayaError::Cache(format!("Stats failed: {}", e)))?;

        let labels: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(DISTINCT intent_label) FROM training_queue",
                [],
                |r| r.get(0),
            )
            .map_err(|e| NyayaError::Cache(format!("Stats failed: {}", e)))?;

        Ok(TrainingQueueStats {
            total_examples: total as u64,
            pending_examples: pending as u64,
            distinct_labels: labels as u64,
        })
    }
}

#[derive(Debug)]
pub struct TrainingQueueStats {
    pub total_examples: u64,
    pub pending_examples: u64,
    pub distinct_labels: u64,
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
    fn test_enqueue_and_export() {
        let dir = tempfile::tempdir().unwrap();
        let queue = TrainingQueue::open(&dir.path().join("tq.db")).unwrap();

        queue
            .enqueue(
                "check my email",
                "check_email",
                TrainingSource::OriginalQuery,
            )
            .unwrap();
        queue
            .enqueue(
                "read my inbox",
                "check_email",
                TrainingSource::LlmRephrasing,
            )
            .unwrap();

        assert_eq!(queue.pending_count().unwrap(), 2);

        let batch = queue.export_batch().unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].query, "check my email");
        assert_eq!(batch[1].intent_label, "check_email");

        // After export, pending should be 0
        assert_eq!(queue.pending_count().unwrap(), 0);
    }

    #[test]
    fn test_enqueue_rephrasings() {
        let dir = tempfile::tempdir().unwrap();
        let queue = TrainingQueue::open(&dir.path().join("tq.db")).unwrap();

        let count = queue
            .enqueue_rephrasings(
                "what's the weather in NYC",
                "weather_query",
                &[
                    "weather in {city}".into(),
                    "forecast for {city}".into(),
                    "temperature in {city}".into(),
                ],
            )
            .unwrap();

        assert_eq!(count, 4); // original + 3 rephrasings
        assert_eq!(queue.pending_count().unwrap(), 4);

        let stats = queue.stats().unwrap();
        assert_eq!(stats.total_examples, 4);
        assert_eq!(stats.distinct_labels, 1);
    }

    #[test]
    fn test_double_export_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let queue = TrainingQueue::open(&dir.path().join("tq.db")).unwrap();

        queue
            .enqueue("test", "label", TrainingSource::OriginalQuery)
            .unwrap();

        let batch1 = queue.export_batch().unwrap();
        assert_eq!(batch1.len(), 1);

        let batch2 = queue.export_batch().unwrap();
        assert_eq!(batch2.len(), 0); // Already exported
    }
}
