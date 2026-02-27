use crate::core::error::{NyayaError, Result};
use crate::pea::bdi::{BdiAction, BdiEngine};
use crate::pea::budget::{BudgetController, BudgetMode};
use crate::pea::episode::EpisodeStore;
use crate::pea::htn::HtnDecomposer;
use crate::pea::objective::*;
use crate::pea::pramana::PramanaValidator;
use std::path::Path;

// ---------------------------------------------------------------------------
// TickActivity
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct TickActivity {
    pub objective_id: String,
    pub actions_taken: Vec<String>,
}

// ---------------------------------------------------------------------------
// PeaEngine
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub struct PeaEngine {
    store: ObjectiveStore,
    episode_store: EpisodeStore,
    bdi: BdiEngine,
    htn: HtnDecomposer,
    pramana: PramanaValidator,
}

fn uuid_simple() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:x}_{:x}", nanos, seq)
}

impl PeaEngine {
    /// Open the PEA engine, creating databases under `data_dir`.
    pub fn open(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)
            .map_err(|e| NyayaError::Cache(format!("failed to create PEA data dir: {e}")))?;
        let store = ObjectiveStore::open(&data_dir.join("pea.db"))?;
        let episode_store = EpisodeStore::open(&data_dir.join("pea_episodes.db"))?;
        Ok(Self {
            store,
            episode_store,
            bdi: BdiEngine::default(),
            htn: HtnDecomposer::default(),
            pramana: PramanaValidator::default(),
        })
    }

    /// Create a new objective with the given desires.
    ///
    /// `desires` is a list of `(description, completion_criteria, priority)`.
    /// Returns the objective ID.
    pub fn create_objective(
        &self,
        description: &str,
        budget_usd: f64,
        desires: Vec<(String, String, i32)>,
    ) -> Result<String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let obj_id = format!("obj_{}", uuid_simple());

        let obj = Objective {
            id: obj_id.clone(),
            description: description.to_string(),
            status: ObjectiveStatus::Active,
            created_at: now,
            updated_at: now,
            beliefs: BeliefStore::default(),
            budget_usd,
            spent_usd: 0.0,
            budget_strategy: BudgetStrategy::default(),
            progress_score: 0.0,
            milestones: vec![],
            heartbeat_interval_secs: 300,
            last_tick_at: 0,
        };
        self.store.save_objective(&obj)?;

        for (i, (desc, criteria, priority)) in desires.iter().enumerate() {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let desire_id = format!("des_{:x}", nanos.wrapping_add(i as u128));

            let desire = Desire {
                id: desire_id,
                objective_id: obj_id.clone(),
                description: desc.clone(),
                priority: *priority,
                status: DesireStatus::Active,
                completion_criteria: criteria.clone(),
                intention_id: None,
                created_at: now,
            };
            self.store.save_desire(&desire)?;
        }

        Ok(obj_id)
    }

    /// Tick all active objectives whose heartbeat interval has elapsed.
    pub fn tick(&self) -> Result<Vec<TickActivity>> {
        let objectives = self.store.list_active_objectives()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut activities = Vec::new();

        for obj in &objectives {
            // Check heartbeat interval
            if now.saturating_sub(obj.last_tick_at) < obj.heartbeat_interval_secs {
                continue;
            }

            let mut actions: Vec<String> = Vec::new();

            // 1. Check budget
            let budget_ctrl =
                BudgetController::new(obj.budget_usd, obj.spent_usd, obj.budget_strategy.clone());
            if budget_ctrl.current_mode() == BudgetMode::Exhausted {
                self.store
                    .update_objective_status(&obj.id, &ObjectiveStatus::Paused)?;
                actions.push("Budget exhausted — pausing objective".to_string());
                activities.push(TickActivity {
                    objective_id: obj.id.clone(),
                    actions_taken: actions,
                });
                continue;
            }

            // 2. BDI deliberation
            let desires = self.store.list_desires(&obj.id)?;
            let tasks = self.store.list_tasks(&obj.id)?;
            let action = self.bdi.deliberate(obj, &desires, &tasks);

            match action {
                BdiAction::Persist => {
                    // Promote ready tasks
                    let mut all_tasks = self.store.list_tasks(&obj.id)?;
                    HtnDecomposer::promote_ready_tasks(&mut all_tasks);
                    for t in &all_tasks {
                        self.store.save_task(t)?;
                    }
                    // Find next ready task and mark Running
                    let ready = self.store.list_ready_tasks(&obj.id)?;
                    if let Some(next) = ready.first() {
                        self.store
                            .update_task_status(&next.id, &TaskStatus::Running)?;
                        actions.push(format!("Persisting — running task '{}'", next.id));
                    } else {
                        actions.push("Persisting — no ready tasks".to_string());
                    }
                }
                BdiAction::Commit { desire_id } => {
                    actions.push(format!("Committing to desire '{}'", desire_id));

                    // Find the desire to get its description
                    let desire_desc = desires
                        .iter()
                        .find(|d| d.id == desire_id)
                        .map(|d| d.description.clone())
                        .unwrap_or_else(|| desire_id.clone());

                    // Create root task
                    let task_id = format!("task_{}", uuid_simple());
                    let task_now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();

                    let root_task = PeaTask {
                        id: task_id.clone(),
                        objective_id: obj.id.clone(),
                        desire_id: desire_id.clone(),
                        parent_task_id: None,
                        description: desire_desc,
                        task_type: TaskType::Compound,
                        status: TaskStatus::Ready,
                        ordering: 0,
                        depends_on: vec![],
                        capability_required: None,
                        result_json: None,
                        pramana_record_json: None,
                        retry_count: 0,
                        max_retries: 3,
                        created_at: task_now,
                        completed_at: None,
                    };

                    // Try HTN decomposition
                    if let Some(subtasks) = self.htn.decompose(&root_task, &obj.id, &desire_id) {
                        self.store.save_task(&root_task)?;
                        for st in &subtasks {
                            self.store.save_task(st)?;
                        }
                        actions.push(format!("HTN decomposed into {} subtasks", subtasks.len()));
                    } else {
                        // No decomposition — save as primitive
                        let mut prim = root_task;
                        prim.task_type = TaskType::Primitive;
                        self.store.save_task(&prim)?;
                    }

                    // Update desire with intention_id
                    let mut updated_desire = desires
                        .iter()
                        .find(|d| d.id == desire_id)
                        .cloned()
                        .ok_or_else(|| {
                            NyayaError::Cache(format!("desire {} not found", desire_id))
                        })?;
                    updated_desire.intention_id = Some(task_id);
                    self.store.save_desire(&updated_desire)?;
                }
                BdiAction::Advance {
                    completed_desire_id,
                    next_desire_id,
                } => {
                    self.store
                        .update_desire_status(&completed_desire_id, &DesireStatus::Achieved)?;
                    actions.push(format!(
                        "Advanced — desire '{}' achieved",
                        completed_desire_id
                    ));
                    if let Some(next) = next_desire_id {
                        actions.push(format!("Next desire: '{}'", next));
                    }
                }
                BdiAction::Reconsider { reason } => {
                    actions.push(format!("Reconsidering: {}", reason));
                }
                BdiAction::HegelianReview { stuck_ticks } => {
                    actions.push(format!(
                        "Hegelian review triggered — stuck for {} ticks",
                        stuck_ticks
                    ));
                }
                BdiAction::ObjectiveComplete => {
                    self.store
                        .update_objective_status(&obj.id, &ObjectiveStatus::Completed)?;
                    actions.push("Objective completed".to_string());
                }
                BdiAction::ObjectiveFailed { reason } => {
                    self.store
                        .update_objective_status(&obj.id, &ObjectiveStatus::Failed)?;
                    actions.push(format!("Objective failed: {}", reason));
                }
            }

            // 3. Update last_tick_at
            let mut updated_obj = obj.clone();
            updated_obj.last_tick_at = now;
            updated_obj.updated_at = now;
            self.store.save_objective(&updated_obj)?;

            activities.push(TickActivity {
                objective_id: obj.id.clone(),
                actions_taken: actions,
            });
        }

        Ok(activities)
    }

    /// Get the status of a single objective.
    pub fn get_status(&self, id: &str) -> Result<Option<Objective>> {
        self.store.load_objective(id)
    }

    /// List all active objectives.
    pub fn list_objectives(&self) -> Result<Vec<Objective>> {
        self.store.list_active_objectives()
    }

    /// Get the task tree for an objective.
    pub fn get_tasks(&self, id: &str) -> Result<Vec<PeaTask>> {
        self.store.list_tasks(id)
    }

    /// Pause an objective.
    pub fn pause(&self, id: &str) -> Result<()> {
        self.store
            .update_objective_status(id, &ObjectiveStatus::Paused)
    }

    /// Resume a paused objective.
    pub fn resume(&self, id: &str) -> Result<()> {
        self.store
            .update_objective_status(id, &ObjectiveStatus::Active)
    }

    /// Cancel an objective.
    pub fn cancel(&self, id: &str) -> Result<()> {
        self.store
            .update_objective_status(id, &ObjectiveStatus::Failed)
    }
}

#[cfg(feature = "watcher")]
pub fn emit_budget_event(
    tx: &tokio::sync::broadcast::Sender<crate::watcher::events::WatchEvent>,
    objective_id: &str,
    burn_rate: f64,
    projected_overshoot: f64,
) {
    use crate::watcher::events::*;
    let event = WatchEvent {
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        kind: WatchEventKind::BudgetAnomaly {
            objective_id: objective_id.to_string(),
            burn_rate,
            projected_overshoot,
        },
        severity: Severity::Warning,
    };
    let _ = tx.send(event);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_create_objective() {
        let dir = tempfile::tempdir().unwrap();
        let engine = PeaEngine::open(dir.path()).unwrap();

        let desires = vec![
            ("Build website".to_string(), "site is live".to_string(), 0),
            ("Write docs".to_string(), "docs published".to_string(), 1),
        ];
        let obj_id = engine
            .create_objective("Launch product", 50.0, desires)
            .unwrap();

        assert!(obj_id.starts_with("obj_"));

        let obj = engine.get_status(&obj_id).unwrap().unwrap();
        assert_eq!(obj.description, "Launch product");
        assert!((obj.budget_usd - 50.0).abs() < f64::EPSILON);
        assert_eq!(obj.status, ObjectiveStatus::Active);

        // Verify desires were saved
        let desires = engine.store.list_desires(&obj_id).unwrap();
        assert_eq!(desires.len(), 2);
        assert_eq!(desires[0].description, "Build website");
        assert_eq!(desires[1].description, "Write docs");
    }

    #[test]
    fn test_engine_tick_commits_first_desire() {
        let dir = tempfile::tempdir().unwrap();
        let engine = PeaEngine::open(dir.path()).unwrap();

        let desires = vec![
            ("Research topic".to_string(), "research done".to_string(), 0),
            ("Write report".to_string(), "report written".to_string(), 1),
        ];
        let obj_id = engine
            .create_objective("Complete research", 20.0, desires)
            .unwrap();

        let activities = engine.tick().unwrap();
        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].objective_id, obj_id);

        // Should have committed to the first desire
        let has_committing = activities[0]
            .actions_taken
            .iter()
            .any(|a| a.contains("Committing"));
        assert!(
            has_committing,
            "Expected 'Committing' action, got: {:?}",
            activities[0].actions_taken
        );
    }

    #[test]
    fn test_engine_tick_skips_before_interval() {
        let dir = tempfile::tempdir().unwrap();
        let engine = PeaEngine::open(dir.path()).unwrap();

        let obj_id = engine
            .create_objective(
                "Test objective",
                10.0,
                vec![("Do something".to_string(), "done".to_string(), 0)],
            )
            .unwrap();

        // Set heartbeat to 600s and last_tick_at to now
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut obj = engine.get_status(&obj_id).unwrap().unwrap();
        obj.heartbeat_interval_secs = 600;
        obj.last_tick_at = now;
        engine.store.save_objective(&obj).unwrap();

        // Tick should skip since heartbeat hasn't elapsed
        let activities = engine.tick().unwrap();
        assert!(
            activities.is_empty(),
            "Expected no activities before interval elapsed, got: {:?}",
            activities
        );
    }

    #[test]
    fn test_engine_pause_resume() {
        let dir = tempfile::tempdir().unwrap();
        let engine = PeaEngine::open(dir.path()).unwrap();

        let obj_id = engine
            .create_objective(
                "Pausable objective",
                10.0,
                vec![("Task A".to_string(), "done".to_string(), 0)],
            )
            .unwrap();

        // Pause
        engine.pause(&obj_id).unwrap();
        let obj = engine.get_status(&obj_id).unwrap().unwrap();
        assert_eq!(obj.status, ObjectiveStatus::Paused);

        // Resume
        engine.resume(&obj_id).unwrap();
        let obj = engine.get_status(&obj_id).unwrap().unwrap();
        assert_eq!(obj.status, ObjectiveStatus::Active);
    }

    #[test]
    fn test_engine_budget_exhaustion_pauses() {
        let dir = tempfile::tempdir().unwrap();
        let engine = PeaEngine::open(dir.path()).unwrap();

        let obj_id = engine
            .create_objective(
                "Budget test",
                0.0,
                vec![("Something".to_string(), "done".to_string(), 0)],
            )
            .unwrap();

        // Set spent > budget to trigger exhaustion
        let mut obj = engine.get_status(&obj_id).unwrap().unwrap();
        obj.budget_usd = 0.0;
        obj.spent_usd = 1.0;
        engine.store.save_objective(&obj).unwrap();

        // Tick should pause due to budget exhaustion
        let activities = engine.tick().unwrap();
        assert_eq!(activities.len(), 1);
        let has_exhausted = activities[0]
            .actions_taken
            .iter()
            .any(|a| a.contains("exhausted") || a.contains("Exhausted"));
        assert!(
            has_exhausted,
            "Expected budget exhaustion action, got: {:?}",
            activities[0].actions_taken
        );

        // Verify objective is now paused
        let obj = engine.get_status(&obj_id).unwrap().unwrap();
        assert_eq!(obj.status, ObjectiveStatus::Paused);
    }
}
