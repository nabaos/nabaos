use crate::core::error::{NyayaError, Result};
use rusqlite::Connection;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct SpendingConfig {
    #[serde(default = "default_per_task")]
    pub max_per_task_usd: f64,
    #[serde(default = "default_daily")]
    pub max_daily_usd: f64,
    #[serde(default = "default_monthly")]
    pub max_monthly_usd: f64,
    #[serde(default = "default_approval")]
    pub approval_threshold_usd: f64,
}

fn default_per_task() -> f64 {
    5.0
}
fn default_daily() -> f64 {
    20.0
}
fn default_monthly() -> f64 {
    200.0
}
fn default_approval() -> f64 {
    2.0
}

impl Default for SpendingConfig {
    fn default() -> Self {
        Self {
            max_per_task_usd: 5.0,
            max_daily_usd: 20.0,
            max_monthly_usd: 200.0,
            approval_threshold_usd: 2.0,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum SpendingDecision {
    Approved,
    NeedsApproval { estimated_cost: f64, reason: String },
    Denied { reason: String },
}

pub struct SpendingGuard {
    db: Connection,
    config: SpendingConfig,
}

impl SpendingGuard {
    pub fn new(db_path: &str, config: SpendingConfig) -> Result<Self> {
        let db = Connection::open(db_path)
            .map_err(|e| NyayaError::Config(format!("SpendingGuard DB: {}", e)))?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS deep_agent_spending (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                backend TEXT NOT NULL,
                task TEXT NOT NULL,
                cost_usd REAL NOT NULL,
                created_at INTEGER NOT NULL
            )",
        )
        .map_err(|e| NyayaError::Config(format!("SpendingGuard init: {}", e)))?;
        Ok(Self { db, config })
    }

    pub fn check(&self, estimated_cost: f64) -> Result<SpendingDecision> {
        // Per-task limit
        if estimated_cost > self.config.max_per_task_usd {
            return Ok(SpendingDecision::Denied {
                reason: format!(
                    "Estimated ${:.2} exceeds per-task limit ${:.2}",
                    estimated_cost, self.config.max_per_task_usd
                ),
            });
        }

        // Daily limit
        let daily = self.daily_total()?;
        if daily + estimated_cost > self.config.max_daily_usd {
            return Ok(SpendingDecision::Denied {
                reason: format!(
                    "Daily total ${:.2} + ${:.2} exceeds limit ${:.2}",
                    daily, estimated_cost, self.config.max_daily_usd
                ),
            });
        }

        // Monthly limit
        let monthly = self.monthly_total()?;
        if monthly + estimated_cost > self.config.max_monthly_usd {
            return Ok(SpendingDecision::Denied {
                reason: format!(
                    "Monthly total ${:.2} + ${:.2} exceeds limit ${:.2}",
                    monthly, estimated_cost, self.config.max_monthly_usd
                ),
            });
        }

        // Approval threshold
        if estimated_cost > self.config.approval_threshold_usd {
            return Ok(SpendingDecision::NeedsApproval {
                estimated_cost,
                reason: format!(
                    "Estimated ${:.2} exceeds approval threshold ${:.2}",
                    estimated_cost, self.config.approval_threshold_usd
                ),
            });
        }

        Ok(SpendingDecision::Approved)
    }

    pub fn record_spend(&self, backend: &str, task: &str, cost: f64) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        self.db.execute(
            "INSERT INTO deep_agent_spending (backend, task, cost_usd, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![backend, task, cost, now],
        ).map_err(|e| NyayaError::Config(format!("Record spend: {}", e)))?;

        Ok(())
    }

    pub fn daily_total(&self) -> Result<f64> {
        let day_ago = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
            - 86400;

        let total: f64 = self.db.query_row(
            "SELECT COALESCE(SUM(cost_usd), 0.0) FROM deep_agent_spending WHERE created_at > ?1",
            rusqlite::params![day_ago],
            |row| row.get(0),
        ).map_err(|e| NyayaError::Config(format!("Daily total: {}", e)))?;

        Ok(total)
    }

    pub fn monthly_total(&self) -> Result<f64> {
        let month_ago = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
            - (30 * 86400);

        let total: f64 = self.db.query_row(
            "SELECT COALESCE(SUM(cost_usd), 0.0) FROM deep_agent_spending WHERE created_at > ?1",
            rusqlite::params![month_ago],
            |row| row.get(0),
        ).map_err(|e| NyayaError::Config(format!("Monthly total: {}", e)))?;

        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_guard() -> SpendingGuard {
        SpendingGuard::new(":memory:", SpendingConfig::default()).unwrap()
    }

    #[test]
    fn test_approved_under_threshold() {
        let guard = test_guard();
        assert_eq!(guard.check(1.0).unwrap(), SpendingDecision::Approved);
    }

    #[test]
    fn test_needs_approval_above_threshold() {
        let guard = test_guard();
        match guard.check(3.0).unwrap() {
            SpendingDecision::NeedsApproval { estimated_cost, .. } => {
                assert!((estimated_cost - 3.0).abs() < f64::EPSILON);
            }
            other => panic!("Expected NeedsApproval, got {:?}", other),
        }
    }

    #[test]
    fn test_denied_per_task_limit() {
        let guard = test_guard();
        match guard.check(6.0).unwrap() {
            SpendingDecision::Denied { reason } => {
                assert!(reason.contains("per-task limit"));
            }
            other => panic!("Expected Denied, got {:?}", other),
        }
    }

    #[test]
    fn test_denied_daily_limit() {
        let guard = test_guard();
        // Record spending up to near the daily limit
        guard.record_spend("test", "task1", 19.0).unwrap();
        match guard.check(2.0).unwrap() {
            SpendingDecision::Denied { reason } => {
                assert!(reason.contains("Daily total"));
            }
            other => panic!("Expected Denied, got {:?}", other),
        }
    }

    #[test]
    fn test_denied_monthly_limit() {
        let guard = SpendingGuard::new(
            ":memory:",
            SpendingConfig {
                max_per_task_usd: 50.0,
                max_daily_usd: 500.0,
                max_monthly_usd: 100.0,
                approval_threshold_usd: 2.0,
            },
        )
        .unwrap();
        guard.record_spend("test", "task1", 99.0).unwrap();
        match guard.check(2.0).unwrap() {
            SpendingDecision::Denied { reason } => {
                assert!(reason.contains("Monthly total"));
            }
            other => panic!("Expected Denied, got {:?}", other),
        }
    }

    #[test]
    fn test_record_and_check_accumulation() {
        let guard = test_guard();
        guard.record_spend("manus", "research", 1.5).unwrap();
        guard.record_spend("claude", "analysis", 2.5).unwrap();
        let daily = guard.daily_total().unwrap();
        assert!((daily - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_default_config_values() {
        let config = SpendingConfig::default();
        assert!((config.max_per_task_usd - 5.0).abs() < f64::EPSILON);
        assert!((config.max_daily_usd - 20.0).abs() < f64::EPSILON);
        assert!((config.max_monthly_usd - 200.0).abs() < f64::EPSILON);
        assert!((config.approval_threshold_usd - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_zero_cost_approved() {
        let guard = test_guard();
        assert_eq!(guard.check(0.0).unwrap(), SpendingDecision::Approved);
    }
}
