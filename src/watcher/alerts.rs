//! Alert persistence — SQLite storage and channel delivery formatting.

use crate::core::error::{NyayaError, Result};
use crate::watcher::events::Severity;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: String,
    pub timestamp: u64,
    pub component: String,
    pub severity: Severity,
    pub event_summary: String,
    pub llm_verdict_json: Option<String>,
    pub action_taken: String,
    pub resolved_at: Option<u64>,
}

pub struct AlertStore {
    conn: Connection,
}

impl AlertStore {
    pub fn open(db_path: &std::path::Path) -> Result<Self> {
        let conn = Connection::open(db_path)
            .map_err(|e| NyayaError::Cache(format!("open watcher db: {e}")))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS watcher_alerts (
                id TEXT PRIMARY KEY,
                timestamp INTEGER NOT NULL,
                component TEXT NOT NULL,
                severity TEXT NOT NULL,
                event_summary TEXT NOT NULL,
                llm_verdict_json TEXT,
                action_taken TEXT NOT NULL,
                resolved_at INTEGER,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_watcher_alerts_component
                ON watcher_alerts(component, timestamp);
            CREATE INDEX IF NOT EXISTS idx_watcher_alerts_unresolved
                ON watcher_alerts(resolved_at) WHERE resolved_at IS NULL;
            CREATE TABLE IF NOT EXISTS paused_components (
                component TEXT PRIMARY KEY,
                reason TEXT NOT NULL,
                paused_at INTEGER NOT NULL
            );",
        )
        .map_err(|e| NyayaError::Cache(format!("create watcher tables: {e}")))?;
        Ok(Self { conn })
    }

    pub fn save_alert(&self, alert: &Alert) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO watcher_alerts
                 (id, timestamp, component, severity, event_summary,
                  llm_verdict_json, action_taken, resolved_at, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    alert.id,
                    alert.timestamp,
                    alert.component,
                    format!("{}", alert.severity),
                    alert.event_summary,
                    alert.llm_verdict_json,
                    alert.action_taken,
                    alert.resolved_at,
                    now,
                ],
            )
            .map_err(|e| NyayaError::Cache(format!("save alert: {e}")))?;
        Ok(())
    }

    pub fn list_recent(&self, since_secs: u64) -> Result<Vec<Alert>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let cutoff = now.saturating_sub(since_secs);
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, timestamp, component, severity, event_summary,
                        llm_verdict_json, action_taken, resolved_at
                 FROM watcher_alerts WHERE timestamp >= ?1
                 ORDER BY timestamp DESC",
            )
            .map_err(|e| NyayaError::Cache(format!("prepare list_recent: {e}")))?;
        let rows = stmt
            .query_map(params![cutoff], |row| {
                Ok(Alert {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    component: row.get(2)?,
                    severity: match row.get::<_, String>(3)?.as_str() {
                        "FATAL" => Severity::Fatal,
                        "CRITICAL" => Severity::Critical,
                        "SUSPICIOUS" => Severity::Suspicious,
                        "WARNING" => Severity::Warning,
                        _ => Severity::Info,
                    },
                    event_summary: row.get(4)?,
                    llm_verdict_json: row.get(5)?,
                    action_taken: row.get(6)?,
                    resolved_at: row.get(7)?,
                })
            })
            .map_err(|e| NyayaError::Cache(format!("query list_recent: {e}")))?;
        let mut alerts = Vec::new();
        for row in rows {
            alerts.push(row.map_err(|e| NyayaError::Cache(format!("read alert row: {e}")))?);
        }
        Ok(alerts)
    }

    pub fn resolve_alert(&self, alert_id: &str) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.conn
            .execute(
                "UPDATE watcher_alerts SET resolved_at = ?1 WHERE id = ?2",
                params![now, alert_id],
            )
            .map_err(|e| NyayaError::Cache(format!("resolve alert: {e}")))?;
        Ok(())
    }

    pub fn prune_old(&self, retention_days: u64) -> Result<usize> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let cutoff = now.saturating_sub(retention_days * 86400);
        let deleted = self
            .conn
            .execute(
                "DELETE FROM watcher_alerts WHERE timestamp < ?1",
                params![cutoff],
            )
            .map_err(|e| NyayaError::Cache(format!("prune alerts: {e}")))?;
        Ok(deleted)
    }

    /// Persist a component pause to the database.
    pub fn save_pause(&self, component: &str, reason: &str, timestamp: u64) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO paused_components (component, reason, paused_at)
                 VALUES (?1, ?2, ?3)",
                params![component, reason, timestamp],
            )
            .map_err(|e| NyayaError::Cache(format!("save pause: {e}")))?;
        Ok(())
    }

    /// Remove a component pause from the database.
    pub fn remove_pause(&self, component: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM paused_components WHERE component = ?1",
                params![component],
            )
            .map_err(|e| NyayaError::Cache(format!("remove pause: {e}")))?;
        Ok(())
    }

    /// List all persisted paused components: (component, reason, paused_at).
    pub fn list_paused(&self) -> Result<Vec<(String, String, u64)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT component, reason, paused_at FROM paused_components")
            .map_err(|e| NyayaError::Cache(format!("prepare list_paused: {e}")))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u64>(2)?,
                ))
            })
            .map_err(|e| NyayaError::Cache(format!("query list_paused: {e}")))?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| NyayaError::Cache(format!("read pause row: {e}")))?);
        }
        Ok(result)
    }

    /// Format an alert for channel delivery (Telegram/Slack/Discord).
    pub fn format_alert(alert: &Alert) -> String {
        let icon = match alert.severity {
            Severity::Fatal => "[FATAL]",
            Severity::Critical => "[CRITICAL]",
            Severity::Suspicious => "[SUSPICIOUS]",
            Severity::Warning => "[WARNING]",
            Severity::Info => "[INFO]",
        };
        let action_line = match alert.action_taken.as_str() {
            "pause" => format!(
                "Action: PAUSED. Run `watcher resume {}` to continue.",
                alert.component
            ),
            "alert" => "Action: Alert sent (no pause).".to_string(),
            _ => format!("Action: {}", alert.action_taken),
        };
        format!(
            "{} {}: {}\n{}",
            icon, alert.component, alert.event_summary, action_line
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn temp_store() -> (AlertStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = AlertStore::open(&dir.path().join("watcher.db")).unwrap();
        (store, dir)
    }

    #[test]
    fn test_save_and_list_alert() {
        let (store, _dir) = temp_store();
        let alert = Alert {
            id: "a1".into(),
            timestamp: now_secs(),
            component: "pea".into(),
            severity: Severity::Suspicious,
            event_summary: "Budget burn rate high".into(),
            llm_verdict_json: None,
            action_taken: "alert".into(),
            resolved_at: None,
        };
        store.save_alert(&alert).unwrap();
        let alerts = store.list_recent(86400 * 365).unwrap(); // large window
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].component, "pea");
    }

    #[test]
    fn test_resolve_alert() {
        let (store, _dir) = temp_store();
        let alert = Alert {
            id: "a2".into(),
            timestamp: now_secs(),
            component: "security".into(),
            severity: Severity::Critical,
            event_summary: "Credential leak detected".into(),
            llm_verdict_json: None,
            action_taken: "pause".into(),
            resolved_at: None,
        };
        store.save_alert(&alert).unwrap();
        store.resolve_alert("a2").unwrap();
        let alerts = store.list_recent(86400 * 365).unwrap();
        assert!(alerts[0].resolved_at.is_some());
    }

    #[test]
    fn test_prune_old_alerts() {
        let (store, _dir) = temp_store();
        let alert = Alert {
            id: "old".into(),
            timestamp: 1, // very old
            component: "test".into(),
            severity: Severity::Info,
            event_summary: "ancient".into(),
            llm_verdict_json: None,
            action_taken: "alert".into(),
            resolved_at: None,
        };
        store.save_alert(&alert).unwrap();
        let pruned = store.prune_old(1).unwrap(); // 1 day retention
        assert_eq!(pruned, 1);
    }

    #[test]
    fn test_format_alert_pause() {
        let alert = Alert {
            id: "x".into(),
            timestamp: 0,
            component: "pea".into(),
            severity: Severity::Suspicious,
            event_summary: "Budget anomaly".into(),
            llm_verdict_json: None,
            action_taken: "pause".into(),
            resolved_at: None,
        };
        let formatted = AlertStore::format_alert(&alert);
        assert!(formatted.contains("[SUSPICIOUS]"));
        assert!(formatted.contains("PAUSED"));
        assert!(formatted.contains("watcher resume pea"));
    }
}
