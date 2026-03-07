//! Interval-based and cron-based scheduler for chain polling.
//!
//! Chains can be scheduled to run at regular intervals or via cron expressions:
//!   - "every 10m" — run chain every 10 minutes
//!   - "every 1h" — run chain every hour
//!   - "cron:0 8 * * *" — run at 8am daily using cron expression
//!
//! The scheduler checks due jobs and executes chains, tracking run history
//! and detecting output changes.

use crate::core::error::{NyayaError, Result};
use std::str::FromStr;

/// Specifies how a job should be scheduled.
#[derive(Debug, Clone)]
pub enum ScheduleSpec {
    /// Run at a fixed interval (in seconds).
    Interval(u64),
    /// Run according to a cron expression.
    Cron(String),
}

/// A scheduled chain execution.
#[derive(Debug, Clone)]
pub struct ScheduledJob {
    pub id: String,
    pub chain_id: String,
    pub interval_secs: u64,
    pub params_json: String,
    pub enabled: bool,
    pub last_run_at: Option<i64>,
    pub last_output: Option<String>,
    pub run_count: u64,
    pub created_at: i64,
    pub cron_expr: Option<String>,
}

impl ScheduledJob {
    /// Check if this job is due to run.
    pub fn is_due(&self, now_ms: i64) -> bool {
        if !self.enabled {
            return false;
        }

        // If this job has a cron expression, use cron-based scheduling
        if let Some(ref expr) = self.cron_expr {
            if !expr.is_empty() {
                return Self::is_due_cron(expr, self.last_run_at, now_ms);
            }
        }

        // Fallback to interval-based scheduling
        match self.last_run_at {
            None => true, // Never run before
            Some(last) => {
                let elapsed_ms = now_ms - last;
                elapsed_ms >= (self.interval_secs as i64 * 1000)
            }
        }
    }

    /// Check if a cron-scheduled job is due based on its expression and last run time.
    fn is_due_cron(expr: &str, last_run_at: Option<i64>, now_ms: i64) -> bool {
        let schedule = match cron::Schedule::from_str(expr) {
            Ok(s) => s,
            Err(_) => return false, // Invalid cron — don't fire
        };

        let now_utc = chrono::DateTime::<chrono::Utc>::from_timestamp(
            now_ms / 1000,
            ((now_ms % 1000) * 1_000_000) as u32,
        );
        let now_utc = match now_utc {
            Some(dt) => dt,
            None => return false,
        };

        match last_run_at {
            None => true, // Never run, always due
            Some(last_ms) => {
                let last_utc = match chrono::DateTime::<chrono::Utc>::from_timestamp(
                    last_ms / 1000,
                    ((last_ms % 1000) * 1_000_000) as u32,
                ) {
                    Some(dt) => dt,
                    None => return false,
                };

                // Find the next occurrence after last_run
                if let Some(next) = schedule.after(&last_utc).next() {
                    next <= now_utc
                } else {
                    false
                }
            }
        }
    }
}

/// Result of a scheduled run, including change detection.
#[derive(Debug)]
pub struct ScheduleRunResult {
    pub job_id: String,
    pub chain_id: String,
    pub output: String,
    pub changed: bool,
    pub previous_output: Option<String>,
    pub run_number: u64,
}

/// SQLite-backed scheduler.
pub struct Scheduler {
    conn: rusqlite::Connection,
}

impl Scheduler {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let conn = rusqlite::Connection::open(path)
            .map_err(|e| NyayaError::Cache(format!("Scheduler DB open failed: {}", e)))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS scheduled_jobs (
                id TEXT PRIMARY KEY,
                chain_id TEXT NOT NULL,
                interval_secs INTEGER NOT NULL,
                params_json TEXT NOT NULL DEFAULT '{}',
                enabled INTEGER NOT NULL DEFAULT 1,
                last_run_at INTEGER,
                last_output TEXT,
                run_count INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS schedule_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                job_id TEXT NOT NULL,
                chain_id TEXT NOT NULL,
                output TEXT,
                changed INTEGER NOT NULL DEFAULT 0,
                run_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_sh_job ON schedule_history(job_id);",
        )
        .map_err(|e| NyayaError::Cache(format!("Scheduler table creation failed: {}", e)))?;

        // Migration: add cron_expr column if it doesn't exist yet.
        // ALTER TABLE ADD COLUMN will fail if column already exists; we catch that.
        let _ = conn.execute_batch("ALTER TABLE scheduled_jobs ADD COLUMN cron_expr TEXT;");

        Ok(Self { conn })
    }

    /// Minimum allowed interval in seconds (30 seconds).
    const MIN_INTERVAL_SECS: u64 = 30;
    /// Maximum number of active (enabled) scheduled jobs.
    const MAX_ACTIVE_JOBS: usize = 100;

    /// Schedule a chain to run at regular intervals or via cron expression.
    /// Enforces minimum interval of 30 seconds and maximum of 100 active jobs
    /// to prevent scheduler abuse / DoS.
    pub fn schedule(
        &self,
        chain_id: &str,
        spec: ScheduleSpec,
        params_json: &str,
    ) -> Result<String> {
        let (interval_secs, cron_expr) = match spec {
            ScheduleSpec::Interval(secs) => {
                // Enforce minimum interval to prevent tight-loop DoS
                if secs < Self::MIN_INTERVAL_SECS {
                    return Err(NyayaError::Config(format!(
                        "Interval {}s is below minimum of {}s",
                        secs,
                        Self::MIN_INTERVAL_SECS
                    )));
                }
                (secs, None)
            }
            ScheduleSpec::Cron(ref expr) => {
                validate_cron(expr)?;
                (0u64, Some(expr.clone()))
            }
        };

        // Enforce maximum active job count
        let active_count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM scheduled_jobs WHERE enabled = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| NyayaError::Cache(format!("Active job count query failed: {}", e)))?;

        if active_count as usize >= Self::MAX_ACTIVE_JOBS {
            return Err(NyayaError::Config(format!(
                "Maximum active job limit ({}) reached. Disable existing jobs first.",
                Self::MAX_ACTIVE_JOBS
            )));
        }

        let id = format!("sched_{}_{}", chain_id, now_millis());
        let now = now_millis();

        self.conn
            .execute(
                "INSERT INTO scheduled_jobs (id, chain_id, interval_secs, params_json, created_at, cron_expr)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![id, chain_id, interval_secs as i64, params_json, now, cron_expr],
            )
            .map_err(|e| NyayaError::Cache(format!("Schedule insert failed: {}", e)))?;

        Ok(id)
    }

    /// Get all jobs that are due to run now.
    pub fn due_jobs(&self) -> Result<Vec<ScheduledJob>> {
        let now = now_millis();
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, chain_id, interval_secs, params_json, enabled,
                        last_run_at, last_output, run_count, created_at, cron_expr
                 FROM scheduled_jobs WHERE enabled = 1",
            )
            .map_err(|e| NyayaError::Cache(format!("Due jobs query failed: {}", e)))?;

        let jobs: Vec<ScheduledJob> = stmt
            .query_map([], |row| {
                Ok(ScheduledJob {
                    id: row.get(0)?,
                    chain_id: row.get(1)?,
                    interval_secs: row.get::<_, i64>(2)? as u64,
                    params_json: row.get(3)?,
                    enabled: row.get::<_, i64>(4)? != 0,
                    last_run_at: row.get(5)?,
                    last_output: row.get(6)?,
                    run_count: row.get::<_, i64>(7)? as u64,
                    created_at: row.get(8)?,
                    cron_expr: row.get(9)?,
                })
            })
            .map_err(|e| NyayaError::Cache(format!("Due jobs query failed: {}", e)))?
            .filter_map(|r| r.ok())
            .filter(|j| j.is_due(now))
            .collect();

        Ok(jobs)
    }

    /// Record a completed run, with change detection.
    pub fn record_run(&self, job_id: &str, output: &str) -> Result<ScheduleRunResult> {
        let now = now_millis();

        // Get previous output for change detection
        let previous: Option<String> = self
            .conn
            .query_row(
                "SELECT last_output FROM scheduled_jobs WHERE id = ?1",
                rusqlite::params![job_id],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        let changed = previous.as_deref() != Some(output);

        // Update the job
        self.conn
            .execute(
                "UPDATE scheduled_jobs SET last_run_at = ?1, last_output = ?2,
                 run_count = run_count + 1 WHERE id = ?3",
                rusqlite::params![now, output, job_id],
            )
            .map_err(|e| NyayaError::Cache(format!("Record run failed: {}", e)))?;

        // Get updated run count and chain_id
        let (chain_id, run_count): (String, i64) = self
            .conn
            .query_row(
                "SELECT chain_id, run_count FROM scheduled_jobs WHERE id = ?1",
                rusqlite::params![job_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| NyayaError::Cache(format!("Run count query failed: {}", e)))?;

        // Insert history entry
        self.conn
            .execute(
                "INSERT INTO schedule_history (job_id, chain_id, output, changed, run_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![job_id, chain_id, output, changed as i32, now],
            )
            .map_err(|e| NyayaError::Cache(format!("History insert failed: {}", e)))?;

        Ok(ScheduleRunResult {
            job_id: job_id.to_string(),
            chain_id,
            output: output.to_string(),
            changed,
            previous_output: previous,
            run_number: run_count as u64,
        })
    }

    /// Disable a scheduled job.
    pub fn disable(&self, job_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE scheduled_jobs SET enabled = 0 WHERE id = ?1",
                rusqlite::params![job_id],
            )
            .map_err(|e| NyayaError::Cache(format!("Disable failed: {}", e)))?;
        Ok(())
    }

    /// Enable a previously disabled scheduled job.
    pub fn enable(&self, job_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE scheduled_jobs SET enabled = 1 WHERE id = ?1",
                rusqlite::params![job_id],
            )
            .map_err(|e| NyayaError::Cache(format!("Enable failed: {}", e)))?;
        Ok(())
    }

    /// List all scheduled jobs.
    pub fn list(&self) -> Result<Vec<ScheduledJob>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, chain_id, interval_secs, params_json, enabled,
                        last_run_at, last_output, run_count, created_at, cron_expr
                 FROM scheduled_jobs ORDER BY created_at DESC",
            )
            .map_err(|e| NyayaError::Cache(format!("List query failed: {}", e)))?;

        let jobs = stmt
            .query_map([], |row| {
                Ok(ScheduledJob {
                    id: row.get(0)?,
                    chain_id: row.get(1)?,
                    interval_secs: row.get::<_, i64>(2)? as u64,
                    params_json: row.get(3)?,
                    enabled: row.get::<_, i64>(4)? != 0,
                    last_run_at: row.get(5)?,
                    last_output: row.get(6)?,
                    run_count: row.get::<_, i64>(7)? as u64,
                    created_at: row.get(8)?,
                    cron_expr: row.get(9)?,
                })
            })
            .map_err(|e| NyayaError::Cache(format!("List query failed: {}", e)))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(jobs)
    }

    /// Get run history for a job.
    pub fn history(&self, job_id: &str, limit: u32) -> Result<Vec<HistoryEntry>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT job_id, chain_id, output, changed, run_at
                 FROM schedule_history WHERE job_id = ?1
                 ORDER BY run_at DESC LIMIT ?2",
            )
            .map_err(|e| NyayaError::Cache(format!("History query failed: {}", e)))?;

        let entries = stmt
            .query_map(rusqlite::params![job_id, limit], |row| {
                Ok(HistoryEntry {
                    job_id: row.get(0)?,
                    chain_id: row.get(1)?,
                    output: row.get(2)?,
                    changed: row.get::<_, i64>(3)? != 0,
                    run_at: row.get(4)?,
                })
            })
            .map_err(|e| NyayaError::Cache(format!("History query failed: {}", e)))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(entries)
    }
}

#[derive(Debug)]
pub struct HistoryEntry {
    pub job_id: String,
    pub chain_id: String,
    pub output: Option<String>,
    pub changed: bool,
    pub run_at: i64,
}

/// Validate a cron expression.
///
/// Ensures the expression is parseable by the `cron` crate and that the minimum
/// gap between consecutive firings is at least 30 seconds (to prevent DoS via
/// overly frequent cron schedules).
pub fn validate_cron(expr: &str) -> Result<()> {
    let schedule = cron::Schedule::from_str(expr)
        .map_err(|e| NyayaError::Config(format!("Invalid cron expression '{}': {}", expr, e)))?;

    // Check minimum gap: compute the next two occurrences from now
    // and verify they are at least 30 seconds apart.
    let now = chrono::Utc::now();
    let mut upcoming = schedule.after(&now);
    let first = upcoming.next().ok_or_else(|| {
        NyayaError::Config(format!(
            "Cron expression '{}' has no upcoming occurrences",
            expr
        ))
    })?;
    let second = upcoming.next().ok_or_else(|| {
        NyayaError::Config(format!(
            "Cron expression '{}' has fewer than two upcoming occurrences",
            expr
        ))
    })?;

    let gap = second.signed_duration_since(first);
    if gap.num_seconds() < 30 {
        return Err(NyayaError::Config(format!(
            "Cron expression '{}' fires too frequently (gap: {}s, minimum: 30s)",
            expr,
            gap.num_seconds()
        )));
    }

    Ok(())
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Parse an interval string like "10m", "1h", "30s" into seconds.
pub fn parse_interval(s: &str) -> Result<u64> {
    let s = s.trim();
    if s.is_empty() {
        return Err(NyayaError::Config("Empty interval".into()));
    }

    // Handle "every Xm" or "every Xh" format
    let s = s.strip_prefix("every ").unwrap_or(s);

    let (num_str, unit) = if let Some(rest) = s.strip_suffix("ms") {
        (rest, "ms")
    } else {
        s.split_at(s.len() - 1)
    };

    let num: u64 = num_str
        .parse()
        .map_err(|_| NyayaError::Config(format!("Invalid interval number: {}", num_str)))?;

    match unit {
        "s" => Ok(num),
        "m" => Ok(num * 60),
        "h" => Ok(num * 3600),
        "d" => Ok(num * 86400),
        "ms" => Ok(num / 1000), // Convert ms to seconds
        _ => Err(NyayaError::Config(format!(
            "Unknown interval unit: {}",
            unit
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schedule_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let sched = Scheduler::open(&dir.path().join("sched.db")).unwrap();

        let id = sched
            .schedule(
                "weather_check",
                ScheduleSpec::Interval(600),
                r#"{"city":"NYC"}"#,
            )
            .unwrap();
        assert!(id.starts_with("sched_weather_check_"));

        let jobs = sched.list().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].chain_id, "weather_check");
        assert_eq!(jobs[0].interval_secs, 600);
        assert!(jobs[0].cron_expr.is_none());
    }

    #[test]
    fn test_due_jobs_first_run() {
        let dir = tempfile::tempdir().unwrap();
        let sched = Scheduler::open(&dir.path().join("sched.db")).unwrap();

        sched
            .schedule("chain1", ScheduleSpec::Interval(60), "{}")
            .unwrap();

        let due = sched.due_jobs().unwrap();
        assert_eq!(due.len(), 1); // First run is always due
    }

    #[test]
    fn test_schedule_rejects_zero_interval() {
        let dir = tempfile::tempdir().unwrap();
        let sched = Scheduler::open(&dir.path().join("sched.db")).unwrap();

        let result = sched.schedule("chain1", ScheduleSpec::Interval(0), "{}");
        assert!(result.is_err(), "Should reject zero interval");
    }

    #[test]
    fn test_schedule_rejects_below_minimum() {
        let dir = tempfile::tempdir().unwrap();
        let sched = Scheduler::open(&dir.path().join("sched.db")).unwrap();

        let result = sched.schedule("chain1", ScheduleSpec::Interval(10), "{}");
        assert!(result.is_err(), "Should reject interval below 30s minimum");

        // 30 seconds should be accepted
        let result = sched.schedule("chain1", ScheduleSpec::Interval(30), "{}");
        assert!(result.is_ok(), "30s interval should be accepted");
    }

    #[test]
    fn test_record_run_and_change_detection() {
        let dir = tempfile::tempdir().unwrap();
        let sched = Scheduler::open(&dir.path().join("sched.db")).unwrap();

        let id = sched
            .schedule("chain1", ScheduleSpec::Interval(60), "{}")
            .unwrap();

        // First run — always "changed" (no previous output)
        let r1 = sched.record_run(&id, "sunny 28C").unwrap();
        assert!(r1.changed);
        assert_eq!(r1.run_number, 1);
        assert!(r1.previous_output.is_none());

        // Second run — same output, not changed
        let r2 = sched.record_run(&id, "sunny 28C").unwrap();
        assert!(!r2.changed);
        assert_eq!(r2.run_number, 2);
        assert_eq!(r2.previous_output.as_deref(), Some("sunny 28C"));

        // Third run — different output, changed
        let r3 = sched.record_run(&id, "rainy 22C").unwrap();
        assert!(r3.changed);
        assert_eq!(r3.run_number, 3);
    }

    #[test]
    fn test_disable_job() {
        let dir = tempfile::tempdir().unwrap();
        let sched = Scheduler::open(&dir.path().join("sched.db")).unwrap();

        let id = sched
            .schedule("chain1", ScheduleSpec::Interval(60), "{}")
            .unwrap();
        assert_eq!(sched.due_jobs().unwrap().len(), 1);

        sched.disable(&id).unwrap();
        assert_eq!(sched.due_jobs().unwrap().len(), 0);
    }

    #[test]
    fn test_history() {
        let dir = tempfile::tempdir().unwrap();
        let sched = Scheduler::open(&dir.path().join("sched.db")).unwrap();

        let id = sched
            .schedule("chain1", ScheduleSpec::Interval(60), "{}")
            .unwrap();
        sched.record_run(&id, "output1").unwrap();
        sched.record_run(&id, "output2").unwrap();
        sched.record_run(&id, "output2").unwrap();

        let hist = sched.history(&id, 10).unwrap();
        assert_eq!(hist.len(), 3);
        // Most recent first
        assert!(!hist[0].changed); // output2 == output2
        assert!(hist[1].changed); // output2 != output1
    }

    #[test]
    fn test_parse_interval() {
        assert_eq!(parse_interval("10m").unwrap(), 600);
        assert_eq!(parse_interval("1h").unwrap(), 3600);
        assert_eq!(parse_interval("30s").unwrap(), 30);
        assert_eq!(parse_interval("every 5m").unwrap(), 300);
        assert_eq!(parse_interval("1d").unwrap(), 86400);
    }

    #[test]
    fn test_is_due() {
        let job = ScheduledJob {
            id: "test".into(),
            chain_id: "chain1".into(),
            interval_secs: 60,
            params_json: "{}".into(),
            enabled: true,
            last_run_at: Some(now_millis() - 120_000), // 2 minutes ago
            last_output: None,
            run_count: 1,
            created_at: 0,
            cron_expr: None,
        };
        assert!(job.is_due(now_millis())); // 60s interval, last run 120s ago

        let recent_job = ScheduledJob {
            last_run_at: Some(now_millis() - 10_000), // 10 seconds ago
            ..job.clone()
        };
        assert!(!recent_job.is_due(now_millis())); // Too soon
    }

    #[test]
    fn test_max_active_jobs_limit() {
        let dir = tempfile::tempdir().unwrap();
        let sched = Scheduler::open(&dir.path().join("sched.db")).unwrap();

        // Schedule up to the limit
        for i in 0..Scheduler::MAX_ACTIVE_JOBS {
            sched
                .schedule(&format!("chain_{}", i), ScheduleSpec::Interval(60), "{}")
                .unwrap();
        }

        // Next one should fail
        let result = sched.schedule("one_too_many", ScheduleSpec::Interval(60), "{}");
        assert!(result.is_err(), "Should reject when at max active jobs");

        // Disabling one should allow scheduling again
        let jobs = sched.list().unwrap();
        sched.disable(&jobs[0].id).unwrap();
        let result = sched.schedule("now_fits", ScheduleSpec::Interval(60), "{}");
        assert!(result.is_ok(), "Should allow after disabling one");
    }

    // --- Cron scheduling tests ---

    #[test]
    fn test_cron_schedule_valid() {
        let dir = tempfile::tempdir().unwrap();
        let sched = Scheduler::open(&dir.path().join("sched.db")).unwrap();

        // Every hour at minute 0 — "0 0 * * * * *" (sec min hour dom month dow year)
        // The cron crate uses 7-field format: sec min hour dom month dow year
        let id = sched
            .schedule("cron_chain", ScheduleSpec::Cron("0 0 * * * *".into()), "{}")
            .unwrap();
        assert!(id.starts_with("sched_cron_chain_"));

        let jobs = sched.list().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].interval_secs, 0);
        assert_eq!(jobs[0].cron_expr.as_deref(), Some("0 0 * * * *"));
    }

    #[test]
    fn test_cron_schedule_invalid_expression() {
        let dir = tempfile::tempdir().unwrap();
        let sched = Scheduler::open(&dir.path().join("sched.db")).unwrap();

        let result = sched.schedule(
            "bad_cron",
            ScheduleSpec::Cron("not a cron expression".into()),
            "{}",
        );
        assert!(result.is_err(), "Should reject invalid cron expression");
    }

    #[test]
    fn test_cron_schedule_too_frequent() {
        let dir = tempfile::tempdir().unwrap();
        let sched = Scheduler::open(&dir.path().join("sched.db")).unwrap();

        // Every second — fires way more than once per 30s
        let result = sched.schedule("fast_cron", ScheduleSpec::Cron("* * * * * *".into()), "{}");
        assert!(
            result.is_err(),
            "Should reject cron that fires every second"
        );
    }

    #[test]
    fn test_cron_is_due() {
        // A cron job with no last_run should be due
        let job = ScheduledJob {
            id: "cron_test".into(),
            chain_id: "chain1".into(),
            interval_secs: 0,
            params_json: "{}".into(),
            enabled: true,
            last_run_at: None,
            last_output: None,
            run_count: 0,
            created_at: 0,
            cron_expr: Some("0 * * * * *".into()), // every minute at second 0
        };
        assert!(
            job.is_due(now_millis()),
            "Cron job with no last_run should be due"
        );

        // A cron job that just ran should NOT be due (last run = now)
        let just_ran = ScheduledJob {
            last_run_at: Some(now_millis()),
            ..job.clone()
        };
        // Next occurrence is ~60s away, so it should not be due
        assert!(
            !just_ran.is_due(now_millis()),
            "Cron job that just ran should not be due yet"
        );
    }

    #[test]
    fn test_cron_is_due_past_occurrence() {
        // A cron job whose next occurrence after last_run is in the past should be due
        let two_min_ago = now_millis() - 120_000;
        let job = ScheduledJob {
            id: "cron_past".into(),
            chain_id: "chain1".into(),
            interval_secs: 0,
            params_json: "{}".into(),
            enabled: true,
            last_run_at: Some(two_min_ago),
            last_output: None,
            run_count: 1,
            created_at: 0,
            cron_expr: Some("0 * * * * *".into()), // every minute
        };
        // Last run was 2 minutes ago, next occurrence after that is at most 1 minute later,
        // which is still in the past => due
        assert!(
            job.is_due(now_millis()),
            "Cron job with past next occurrence should be due"
        );
    }

    #[test]
    fn test_validate_cron_valid() {
        // Every minute — gap is 60s, above minimum
        assert!(validate_cron("0 * * * * *").is_ok());
        // Every hour
        assert!(validate_cron("0 0 * * * *").is_ok());
        // Daily at 8am
        assert!(validate_cron("0 0 8 * * *").is_ok());
    }

    #[test]
    fn test_validate_cron_invalid() {
        assert!(validate_cron("not valid").is_err());
        assert!(validate_cron("").is_err());
    }

    #[test]
    fn test_validate_cron_too_frequent() {
        // Every second
        assert!(validate_cron("* * * * * *").is_err());
        // Every 10 seconds
        assert!(validate_cron("0/10 * * * * *").is_err());
    }

    #[test]
    fn test_cron_due_jobs_from_db() {
        let dir = tempfile::tempdir().unwrap();
        let sched = Scheduler::open(&dir.path().join("sched.db")).unwrap();

        // Schedule a cron job (every minute)
        let id = sched
            .schedule("cron_due", ScheduleSpec::Cron("0 * * * * *".into()), "{}")
            .unwrap();

        // Should be due immediately (never run before)
        let due = sched.due_jobs().unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, id);
        assert_eq!(due[0].cron_expr.as_deref(), Some("0 * * * * *"));
    }
}
