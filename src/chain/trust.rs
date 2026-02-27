//! Progressive trust — chains graduate from supervised to autonomous
//! based on their success rate over time.
//!
//! Trust Level 0: LLM verifies every chain step (APPROVE/REJECT)
//! Trust Level 1: Skip verification for steps with >95% success over 50+ runs
//! Trust Level 2: Fully autonomous — no verification needed
//!
//! Financial operations are pinned to Level 0 forever (constitution-enforced).

use crate::core::error::{NyayaError, Result};

/// Trust level thresholds.
pub const GRADUATION_MIN_RUNS: u64 = 50;
pub const GRADUATION_MIN_SUCCESS_RATE: f64 = 0.95;

/// Financial abilities that are pinned to Level 0 forever.
pub const PINNED_ABILITIES: &[&str] = &[
    "trading.execute",
    "trading.sell",
    "trading.buy",
    "payment.send",
    "payment.transfer",
    "email.send", // Emails are irreversible
];

/// Trust assessment for a chain execution.
#[derive(Debug, Clone)]
pub struct TrustAssessment {
    /// Current trust level
    pub level: TrustLevel,
    /// Whether LLM verification is required for this execution
    pub requires_verification: bool,
    /// Reason for the assessment
    pub reason: String,
    /// Steps that require verification (if partial)
    pub steps_requiring_verification: Vec<String>,
}

/// Trust levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustLevel {
    /// Level 0: Every step verified by LLM
    Supervised = 0,
    /// Level 1: Only unproven steps verified
    Graduated = 1,
    /// Level 2: Fully autonomous, no verification
    Autonomous = 2,
}

impl std::fmt::Display for TrustLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrustLevel::Supervised => write!(f, "Level 0 (Supervised)"),
            TrustLevel::Graduated => write!(f, "Level 1 (Graduated)"),
            TrustLevel::Autonomous => write!(f, "Level 2 (Autonomous)"),
        }
    }
}

/// Per-step execution stats for graduation tracking.
#[derive(Debug, Clone)]
pub struct StepStats {
    pub chain_id: String,
    pub step_id: String,
    pub total_runs: u64,
    pub success_count: u64,
    pub failure_count: u64,
}

impl StepStats {
    pub fn success_rate(&self) -> f64 {
        if self.total_runs == 0 {
            0.0
        } else {
            self.success_count as f64 / self.total_runs as f64
        }
    }

    pub fn eligible_for_graduation(&self) -> bool {
        self.total_runs >= GRADUATION_MIN_RUNS && self.success_rate() >= GRADUATION_MIN_SUCCESS_RATE
    }
}

/// Trust manager — tracks per-step stats and determines trust levels.
pub struct TrustManager {
    conn: rusqlite::Connection,
}

impl TrustManager {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let conn = rusqlite::Connection::open(path)
            .map_err(|e| NyayaError::Cache(format!("Trust DB open failed: {}", e)))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS step_stats (
                chain_id TEXT NOT NULL,
                step_id TEXT NOT NULL,
                total_runs INTEGER NOT NULL DEFAULT 0,
                success_count INTEGER NOT NULL DEFAULT 0,
                failure_count INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (chain_id, step_id)
            );",
        )
        .map_err(|e| NyayaError::Cache(format!("Trust table creation failed: {}", e)))?;

        Ok(Self { conn })
    }

    /// Record a step execution outcome.
    pub fn record_step(&self, chain_id: &str, step_id: &str, success: bool) -> Result<()> {
        let success_inc = if success { 1 } else { 0 };
        let failure_inc = if success { 0 } else { 1 };

        self.conn
            .execute(
                "INSERT INTO step_stats (chain_id, step_id, total_runs, success_count, failure_count)
                 VALUES (?1, ?2, 1, ?3, ?4)
                 ON CONFLICT(chain_id, step_id) DO UPDATE SET
                   total_runs = total_runs + 1,
                   success_count = success_count + ?3,
                   failure_count = failure_count + ?4",
                rusqlite::params![chain_id, step_id, success_inc, failure_inc],
            )
            .map_err(|e| NyayaError::Cache(format!("Step record failed: {}", e)))?;

        Ok(())
    }

    /// Get stats for a specific step.
    pub fn get_step_stats(&self, chain_id: &str, step_id: &str) -> Result<Option<StepStats>> {
        use rusqlite::OptionalExtension;
        self.conn
            .query_row(
                "SELECT chain_id, step_id, total_runs, success_count, failure_count
                 FROM step_stats WHERE chain_id = ?1 AND step_id = ?2",
                rusqlite::params![chain_id, step_id],
                |row| {
                    Ok(StepStats {
                        chain_id: row.get(0)?,
                        step_id: row.get(1)?,
                        total_runs: row.get::<_, i64>(2)? as u64,
                        success_count: row.get::<_, i64>(3)? as u64,
                        failure_count: row.get::<_, i64>(4)? as u64,
                    })
                },
            )
            .optional()
            .map_err(|e| NyayaError::Cache(format!("Step stats query failed: {}", e)))
    }

    /// Assess trust level for a chain execution.
    /// Returns which steps (if any) need LLM verification.
    pub fn assess(
        &self,
        chain_id: &str,
        step_ids: &[String],
        abilities: &[String],
    ) -> Result<TrustAssessment> {
        // Check if any ability is pinned to Level 0
        let has_pinned = abilities
            .iter()
            .any(|a| PINNED_ABILITIES.iter().any(|p| a.contains(p)));

        if has_pinned {
            return Ok(TrustAssessment {
                level: TrustLevel::Supervised,
                requires_verification: true,
                reason: "Workflow uses financial/irreversible abilities — pinned to Level 0".into(),
                steps_requiring_verification: step_ids.to_vec(),
            });
        }

        // Check each step's graduation status
        let mut unproven_steps = Vec::new();
        let mut all_graduated = true;

        for step_id in step_ids {
            match self.get_step_stats(chain_id, step_id)? {
                Some(stats) if stats.eligible_for_graduation() => {
                    // This step has graduated
                }
                _ => {
                    all_graduated = false;
                    unproven_steps.push(step_id.clone());
                }
            }
        }

        if all_graduated {
            Ok(TrustAssessment {
                level: TrustLevel::Autonomous,
                requires_verification: false,
                reason: format!(
                    "All {} steps have >{}% success over {}+ runs",
                    step_ids.len(),
                    (GRADUATION_MIN_SUCCESS_RATE * 100.0) as u32,
                    GRADUATION_MIN_RUNS
                ),
                steps_requiring_verification: vec![],
            })
        } else {
            Ok(TrustAssessment {
                level: if unproven_steps.len() < step_ids.len() {
                    TrustLevel::Graduated // Some steps graduated
                } else {
                    TrustLevel::Supervised // No steps graduated
                },
                requires_verification: true,
                reason: format!(
                    "{}/{} steps need verification",
                    unproven_steps.len(),
                    step_ids.len()
                ),
                steps_requiring_verification: unproven_steps,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_query_step_stats() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = TrustManager::open(&dir.path().join("trust.db")).unwrap();

        for _ in 0..10 {
            mgr.record_step("chain1", "step1", true).unwrap();
        }
        mgr.record_step("chain1", "step1", false).unwrap();

        let stats = mgr.get_step_stats("chain1", "step1").unwrap().unwrap();
        assert_eq!(stats.total_runs, 11);
        assert_eq!(stats.success_count, 10);
        assert_eq!(stats.failure_count, 1);
        assert!((stats.success_rate() - 0.909).abs() < 0.01);
        assert!(!stats.eligible_for_graduation()); // < 50 runs
    }

    #[test]
    fn test_graduation_eligibility() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = TrustManager::open(&dir.path().join("trust.db")).unwrap();

        // 50 successes, 0 failures → eligible
        for _ in 0..50 {
            mgr.record_step("chain1", "step1", true).unwrap();
        }

        let stats = mgr.get_step_stats("chain1", "step1").unwrap().unwrap();
        assert!(stats.eligible_for_graduation());
    }

    #[test]
    fn test_graduation_fails_with_low_success_rate() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = TrustManager::open(&dir.path().join("trust.db")).unwrap();

        // 45 successes, 5 failures = 90% → not eligible (need 95%)
        for _ in 0..45 {
            mgr.record_step("chain1", "step1", true).unwrap();
        }
        for _ in 0..5 {
            mgr.record_step("chain1", "step1", false).unwrap();
        }

        let stats = mgr.get_step_stats("chain1", "step1").unwrap().unwrap();
        assert!(!stats.eligible_for_graduation());
    }

    #[test]
    fn test_assess_new_chain_is_supervised() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = TrustManager::open(&dir.path().join("trust.db")).unwrap();

        let assessment = mgr
            .assess(
                "new_chain",
                &["step1".into(), "step2".into()],
                &["data.fetch_url".into(), "notify.user".into()],
            )
            .unwrap();

        assert_eq!(assessment.level, TrustLevel::Supervised);
        assert!(assessment.requires_verification);
        assert_eq!(assessment.steps_requiring_verification.len(), 2);
    }

    #[test]
    fn test_assess_graduated_chain() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = TrustManager::open(&dir.path().join("trust.db")).unwrap();

        // Graduate both steps
        for _ in 0..55 {
            mgr.record_step("chain1", "step1", true).unwrap();
            mgr.record_step("chain1", "step2", true).unwrap();
        }

        let assessment = mgr
            .assess(
                "chain1",
                &["step1".into(), "step2".into()],
                &["data.fetch_url".into(), "notify.user".into()],
            )
            .unwrap();

        assert_eq!(assessment.level, TrustLevel::Autonomous);
        assert!(!assessment.requires_verification);
    }

    #[test]
    fn test_assess_financial_pinned_to_level_0() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = TrustManager::open(&dir.path().join("trust.db")).unwrap();

        // Even with 100% success over 100 runs...
        for _ in 0..100 {
            mgr.record_step("trade_chain", "execute", true).unwrap();
        }

        // ...financial abilities stay at Level 0
        let assessment = mgr
            .assess(
                "trade_chain",
                &["execute".into()],
                &["trading.execute".into()],
            )
            .unwrap();

        assert_eq!(assessment.level, TrustLevel::Supervised);
        assert!(assessment.requires_verification);
        assert!(assessment.reason.contains("pinned"));
    }

    #[test]
    fn test_partial_graduation() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = TrustManager::open(&dir.path().join("trust.db")).unwrap();

        // Graduate step1 but not step2
        for _ in 0..55 {
            mgr.record_step("chain1", "step1", true).unwrap();
        }

        let assessment = mgr
            .assess(
                "chain1",
                &["step1".into(), "step2".into()],
                &["data.fetch_url".into(), "notify.user".into()],
            )
            .unwrap();

        assert_eq!(assessment.level, TrustLevel::Graduated);
        assert!(assessment.requires_verification);
        assert_eq!(assessment.steps_requiring_verification, vec!["step2"]);
    }
}
