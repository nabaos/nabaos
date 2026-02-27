//! Async heartbeat tick loop for scheduled chain execution.
//!
//! Drives the pull-based `Scheduler` by periodically calling `due_jobs()`
//! and recording run results. Provides a handle to stop the loop gracefully.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{interval, Duration};

use super::scheduler::Scheduler;
use crate::core::error::Result;

/// Configuration for the heartbeat tick loop.
pub struct HeartbeatConfig {
    /// How often to check for due jobs (seconds). Default: 10.
    pub tick_interval_secs: u64,
    /// Path to scheduler SQLite database.
    pub scheduler_db_path: PathBuf,
    /// Optional callback for job completion.
    /// Receives (job_id, chain_id, output, changed).
    pub on_job_complete: Option<Arc<dyn Fn(&str, &str, &str, bool) + Send + Sync>>,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            tick_interval_secs: 10,
            scheduler_db_path: PathBuf::from("scheduler.db"),
            on_job_complete: None,
        }
    }
}

/// Handle to a running heartbeat -- use `stop()` to terminate.
pub struct HeartbeatHandle {
    cancel: tokio::sync::watch::Sender<bool>,
}

impl HeartbeatHandle {
    /// Signal the heartbeat loop to stop.
    pub fn stop(&self) {
        let _ = self.cancel.send(true);
    }
}

/// Start the heartbeat tick loop. Returns a handle to stop it.
///
/// The loop opens the scheduler DB, checks `due_jobs()` every `tick_interval_secs`,
/// and for each due job: records a run with placeholder output and invokes
/// `on_job_complete` if the output changed.
pub fn start_heartbeat(config: HeartbeatConfig) -> Result<HeartbeatHandle> {
    let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
    let scheduler = Scheduler::open(&config.scheduler_db_path)?;
    let on_complete = config.on_job_complete;
    let tick_secs = config.tick_interval_secs;

    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(tick_secs));
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if let Ok(due) = scheduler.due_jobs() {
                        for job in due {
                            // Placeholder execution: format job info as output
                            let output = format!(
                                "heartbeat: executed chain '{}' with params: {}",
                                job.chain_id, job.params_json
                            );
                            if let Ok(result) = scheduler.record_run(&job.id, &output) {
                                if let Some(ref cb) = on_complete {
                                    cb(&result.job_id, &result.chain_id, &result.output, result.changed);
                                }
                            }
                        }
                    }
                }
                _ = cancel_rx.changed() => {
                    break;
                }
            }
        }
    });

    Ok(HeartbeatHandle { cancel: cancel_tx })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use tempfile::TempDir;

    /// Minimum allowed scheduler interval (must match Scheduler::MIN_INTERVAL_SECS).
    const MIN_INTERVAL: u64 = 30;

    fn setup_scheduler_with_job(dir: &TempDir) -> PathBuf {
        let db_path = dir.path().join("scheduler.db");
        let scheduler = Scheduler::open(&db_path).unwrap();
        scheduler
            .schedule(
                "test_chain",
                super::super::scheduler::ScheduleSpec::Interval(MIN_INTERVAL),
                "{}",
            )
            .unwrap();
        db_path
    }

    #[test]
    fn test_heartbeat_config_default() {
        let config = HeartbeatConfig::default();
        assert_eq!(config.tick_interval_secs, 10);
        assert!(config.on_job_complete.is_none());
        assert_eq!(config.scheduler_db_path, PathBuf::from("scheduler.db"));
    }

    #[test]
    fn test_heartbeat_handle_stop() {
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let handle = HeartbeatHandle { cancel: cancel_tx };
        assert!(!*cancel_rx.borrow());
        handle.stop();
        assert!(*cancel_rx.borrow());
    }

    #[tokio::test]
    async fn test_heartbeat_executes_due_jobs() {
        let dir = TempDir::new().unwrap();
        // Schedule job with 1-second interval
        let db_path = setup_scheduler_with_job(&dir);

        let config = HeartbeatConfig {
            tick_interval_secs: 1,
            scheduler_db_path: db_path.clone(),
            on_job_complete: None,
        };
        let handle = start_heartbeat(config).unwrap();

        // Wait for at least one tick
        tokio::time::sleep(Duration::from_secs(3)).await;
        handle.stop();

        // Verify the job was executed
        let scheduler = Scheduler::open(&db_path).unwrap();
        let jobs = scheduler.list().unwrap();
        assert!(!jobs.is_empty());
        assert!(
            jobs[0].run_count > 0,
            "Job should have been executed at least once"
        );
    }

    #[tokio::test]
    async fn test_heartbeat_calls_on_complete() {
        let dir = TempDir::new().unwrap();
        let db_path = setup_scheduler_with_job(&dir);

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        let config = HeartbeatConfig {
            tick_interval_secs: 1,
            scheduler_db_path: db_path,
            on_job_complete: Some(Arc::new(move |_job_id, _chain_id, _output, _changed| {
                called_clone.store(true, Ordering::SeqCst);
            })),
        };
        let handle = start_heartbeat(config).unwrap();

        tokio::time::sleep(Duration::from_secs(3)).await;
        handle.stop();

        assert!(
            called.load(Ordering::SeqCst),
            "on_job_complete callback should have been called"
        );
    }

    #[tokio::test]
    async fn test_heartbeat_skips_disabled_jobs() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("scheduler.db");
        let scheduler = Scheduler::open(&db_path).unwrap();
        let job_id = scheduler
            .schedule(
                "disabled_chain",
                super::super::scheduler::ScheduleSpec::Interval(MIN_INTERVAL),
                "{}",
            )
            .unwrap();
        scheduler.disable(&job_id).unwrap();

        let call_count = Arc::new(AtomicU32::new(0));
        let count_clone = call_count.clone();

        let config = HeartbeatConfig {
            tick_interval_secs: 1,
            scheduler_db_path: db_path,
            on_job_complete: Some(Arc::new(move |_job_id, _chain_id, _output, _changed| {
                count_clone.fetch_add(1, Ordering::SeqCst);
            })),
        };
        let handle = start_heartbeat(config).unwrap();

        tokio::time::sleep(Duration::from_secs(3)).await;
        handle.stop();

        assert_eq!(
            call_count.load(Ordering::SeqCst),
            0,
            "Disabled job should not have been executed"
        );
    }

    #[tokio::test]
    async fn test_heartbeat_detects_output_change() {
        let dir = TempDir::new().unwrap();
        let db_path = setup_scheduler_with_job(&dir);

        let changed_count = Arc::new(AtomicU32::new(0));
        let changed_clone = changed_count.clone();

        let config = HeartbeatConfig {
            tick_interval_secs: 1,
            scheduler_db_path: db_path,
            on_job_complete: Some(Arc::new(move |_job_id, _chain_id, _output, changed| {
                if changed {
                    changed_clone.fetch_add(1, Ordering::SeqCst);
                }
            })),
        };
        let handle = start_heartbeat(config).unwrap();

        // First run will always be "changed" (no previous output)
        tokio::time::sleep(Duration::from_secs(3)).await;
        handle.stop();

        assert!(
            changed_count.load(Ordering::SeqCst) >= 1,
            "First run should detect output change"
        );
    }
}
