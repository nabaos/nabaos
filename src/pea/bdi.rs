use crate::pea::objective::*;

// ---------------------------------------------------------------------------
// BdiAction — the output of BDI deliberation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum BdiAction {
    /// Continue current intention
    Persist,
    /// Commit to a new desire
    Commit { desire_id: String },
    /// Current desire done, move to next
    Advance {
        completed_desire_id: String,
        next_desire_id: Option<String>,
    },
    /// Current intention needs reconsideration
    Reconsider { reason: String },
    /// Stuck too long, trigger strategic review
    HegelianReview { stuck_ticks: u64 },
    /// All tasks done, run diagnostic before completing
    Diagnose { desire_id: String },
    /// All desires achieved
    ObjectiveComplete,
    /// No actionable desires remain
    ObjectiveFailed { reason: String },
}

// ---------------------------------------------------------------------------
// BdiEngine — manages motivational state and commitment
// ---------------------------------------------------------------------------

pub struct BdiEngine {
    pub stuck_threshold: u64,
}

impl Default for BdiEngine {
    fn default() -> Self {
        Self {
            stuck_threshold: 12, // 12 ticks × 5 min = 1 hour
        }
    }
}

impl BdiEngine {
    pub fn new(stuck_threshold: u64) -> Self {
        Self { stuck_threshold }
    }

    /// Main deliberation: decide what to do next based on current BDI state.
    pub fn deliberate(
        &self,
        objective: &Objective,
        desires: &[Desire],
        tasks: &[PeaTask],
    ) -> BdiAction {
        // 1. Find committed desire (Active + intention_id.is_some())
        let committed = desires
            .iter()
            .find(|d| d.status == DesireStatus::Active && d.intention_id.is_some());

        if let Some(committed_desire) = committed {
            // 2. Committed desire exists — check its tasks
            let desire_tasks: Vec<&PeaTask> = tasks
                .iter()
                .filter(|t| t.desire_id == committed_desire.id)
                .collect();

            // All tasks completed or skipped?
            let all_done = !desire_tasks.is_empty()
                && desire_tasks
                    .iter()
                    .all(|t| matches!(t.status, TaskStatus::Completed | TaskStatus::Skipped));

            if all_done {
                // Check if this desire has been diagnosed yet
                let diag_key = format!("diagnosed_{}", committed_desire.id);
                let already_diagnosed = objective
                    .beliefs
                    .get(&diag_key)
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if !already_diagnosed {
                    return BdiAction::Diagnose {
                        desire_id: committed_desire.id.clone(),
                    };
                }

                let next = self.pick_next_desire(desires, &committed_desire.id);
                let all_achieved = desires
                    .iter()
                    .all(|d| d.id == committed_desire.id || d.status == DesireStatus::Achieved);
                if all_achieved && next.is_none() {
                    return BdiAction::ObjectiveComplete;
                }
                return BdiAction::Advance {
                    completed_desire_id: committed_desire.id.clone(),
                    next_desire_id: next,
                };
            }

            // Any tasks failed AND none still runnable?
            let any_failed = desire_tasks
                .iter()
                .any(|t| matches!(t.status, TaskStatus::Failed));
            let any_runnable = desire_tasks.iter().any(|t| {
                matches!(
                    t.status,
                    TaskStatus::Pending | TaskStatus::Ready | TaskStatus::Running
                )
            });

            if any_failed && !any_runnable {
                return BdiAction::Reconsider {
                    reason: format!(
                        "all runnable tasks exhausted for desire '{}'",
                        committed_desire.id
                    ),
                };
            }

            // Check stuck
            let stuck_ticks = self.ticks_since_last_completion(objective, tasks);
            if stuck_ticks >= self.stuck_threshold {
                return BdiAction::HegelianReview { stuck_ticks };
            }

            BdiAction::Persist
        } else {
            // 3. No committed desire — pick one
            if let Some(desire_id) = self.pick_next_desire(desires, "") {
                BdiAction::Commit { desire_id }
            } else {
                // Check if all achieved
                let all_achieved = desires.iter().all(|d| d.status == DesireStatus::Achieved);
                if all_achieved {
                    BdiAction::ObjectiveComplete
                } else {
                    BdiAction::ObjectiveFailed {
                        reason: "no actionable desires remain".to_string(),
                    }
                }
            }
        }
    }

    /// Pick next desire: lowest priority number among active desires without intention_id,
    /// excluding `exclude_id`.
    pub fn pick_next_desire(&self, desires: &[Desire], exclude_id: &str) -> Option<String> {
        desires
            .iter()
            .filter(|d| {
                d.status == DesireStatus::Active && d.intention_id.is_none() && d.id != exclude_id
            })
            .min_by_key(|d| d.priority)
            .map(|d| d.id.clone())
    }

    /// Count ticks since last task completion.
    /// (now - max(completed_at)) / heartbeat_interval
    pub fn ticks_since_last_completion(&self, objective: &Objective, tasks: &[PeaTask]) -> u64 {
        let last_completed = tasks
            .iter()
            .filter_map(|t| t.completed_at)
            .max()
            .unwrap_or(objective.created_at);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let interval = objective.heartbeat_interval_secs.max(1);
        now.saturating_sub(last_completed) / interval
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    fn make_objective() -> Objective {
        Objective {
            id: "obj-1".into(),
            description: "Test objective".into(),
            status: ObjectiveStatus::Active,
            created_at: 1000,
            updated_at: 1000,
            beliefs: BeliefStore::default(),
            budget_usd: 10.0,
            spent_usd: 0.0,
            budget_strategy: BudgetStrategy::default(),
            progress_score: 0.0,
            milestones: vec![],
            heartbeat_interval_secs: 300,
            last_tick_at: 0,
        }
    }

    fn make_desire(
        id: &str,
        priority: i32,
        intention_id: Option<String>,
        status: DesireStatus,
    ) -> Desire {
        Desire {
            id: id.into(),
            objective_id: "obj-1".into(),
            description: format!("Desire {id}"),
            priority,
            status,
            completion_criteria: String::new(),
            intention_id,
            created_at: 1000,
        }
    }

    fn make_task(
        id: &str,
        desire_id: &str,
        status: TaskStatus,
        completed_at: Option<u64>,
    ) -> PeaTask {
        PeaTask {
            id: id.into(),
            objective_id: "obj-1".into(),
            desire_id: desire_id.into(),
            parent_task_id: None,
            description: format!("Task {id}"),
            task_type: TaskType::Primitive,
            status,
            ordering: 0,
            depends_on: vec![],
            capability_required: None,
            result_json: None,
            pramana_record_json: None,
            retry_count: 0,
            max_retries: 3,
            created_at: 1000,
            completed_at,
        }
    }

    #[test]
    fn test_deliberate_no_committed_desire_commits_highest_priority() {
        let engine = BdiEngine::default();
        let obj = make_objective();
        let desires = vec![
            make_desire("d-a", 2, None, DesireStatus::Active),
            make_desire("d-b", 0, None, DesireStatus::Active),
            make_desire("d-c", 1, None, DesireStatus::Active),
        ];
        let tasks: Vec<PeaTask> = vec![];

        let action = engine.deliberate(&obj, &desires, &tasks);
        match action {
            BdiAction::Commit { desire_id } => assert_eq!(desire_id, "d-b"),
            other => panic!("expected Commit, got {:?}", other),
        }
    }

    #[test]
    fn test_deliberate_all_tasks_done_advances_after_diagnosis() {
        let engine = BdiEngine::default();
        let mut obj = make_objective();
        // Pre-mark as diagnosed so we get Advance
        obj.beliefs.set("diagnosed_d-1", serde_json::json!(true), 1.0);
        let desires = vec![
            make_desire("d-1", 0, Some("int-1".into()), DesireStatus::Active),
            make_desire("d-2", 1, None, DesireStatus::Active),
        ];
        let tasks = vec![
            make_task("t-1", "d-1", TaskStatus::Completed, Some(2000)),
            make_task("t-2", "d-1", TaskStatus::Skipped, None),
        ];

        let action = engine.deliberate(&obj, &desires, &tasks);
        match action {
            BdiAction::Advance {
                completed_desire_id,
                next_desire_id,
            } => {
                assert_eq!(completed_desire_id, "d-1");
                assert_eq!(next_desire_id, Some("d-2".into()));
            }
            other => panic!("expected Advance, got {:?}", other),
        }
    }

    #[test]
    fn test_deliberate_all_desires_achieved_completes() {
        let engine = BdiEngine::default();
        let obj = make_objective();
        let desires = vec![
            make_desire("d-1", 0, None, DesireStatus::Achieved),
            make_desire("d-2", 1, None, DesireStatus::Achieved),
        ];
        let tasks: Vec<PeaTask> = vec![];

        let action = engine.deliberate(&obj, &desires, &tasks);
        match action {
            BdiAction::ObjectiveComplete => {}
            other => panic!("expected ObjectiveComplete, got {:?}", other),
        }
    }

    #[test]
    fn test_deliberate_persist_when_tasks_running() {
        let engine = BdiEngine::default();
        let obj = make_objective();
        let desires = vec![make_desire(
            "d-1",
            0,
            Some("int-1".into()),
            DesireStatus::Active,
        )];
        // One running task with a recent completion timestamp to avoid stuck detection
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let tasks = vec![
            make_task("t-1", "d-1", TaskStatus::Running, None),
            make_task("t-2", "d-1", TaskStatus::Completed, Some(now)),
        ];

        let action = engine.deliberate(&obj, &desires, &tasks);
        match action {
            BdiAction::Persist => {}
            other => panic!("expected Persist, got {:?}", other),
        }
    }

    #[test]
    fn test_deliberate_reconsider_on_all_tasks_failed() {
        let engine = BdiEngine::default();
        let obj = make_objective();
        let desires = vec![make_desire(
            "d-1",
            0,
            Some("int-1".into()),
            DesireStatus::Active,
        )];
        let tasks = vec![
            make_task("t-1", "d-1", TaskStatus::Failed, None),
            make_task("t-2", "d-1", TaskStatus::Failed, None),
        ];

        let action = engine.deliberate(&obj, &desires, &tasks);
        match action {
            BdiAction::Reconsider { reason } => {
                assert!(reason.contains("d-1"));
            }
            other => panic!("expected Reconsider, got {:?}", other),
        }
    }

    #[test]
    fn test_deliberate_all_done_triggers_diagnose_first() {
        let engine = BdiEngine::default();
        let obj = make_objective();
        let desires = vec![
            make_desire("d-1", 0, Some("int-1".into()), DesireStatus::Active),
            make_desire("d-2", 1, None, DesireStatus::Active),
        ];
        let tasks = vec![
            make_task("t-1", "d-1", TaskStatus::Completed, Some(2000)),
            make_task("t-2", "d-1", TaskStatus::Completed, Some(2000)),
        ];

        // First time: should return Diagnose (not yet diagnosed)
        let action = engine.deliberate(&obj, &desires, &tasks);
        match action {
            BdiAction::Diagnose { desire_id } => assert_eq!(desire_id, "d-1"),
            other => panic!("expected Diagnose, got {:?}", other),
        }
    }

    #[test]
    fn test_deliberate_after_diagnosis_advances() {
        let engine = BdiEngine::default();
        let mut obj = make_objective();
        // Mark desire as already diagnosed
        obj.beliefs.set("diagnosed_d-1", serde_json::json!(true), 1.0);
        let desires = vec![
            make_desire("d-1", 0, Some("int-1".into()), DesireStatus::Active),
            make_desire("d-2", 1, None, DesireStatus::Active),
        ];
        let tasks = vec![
            make_task("t-1", "d-1", TaskStatus::Completed, Some(2000)),
            make_task("t-2", "d-1", TaskStatus::Completed, Some(2000)),
        ];

        // Already diagnosed: should Advance
        let action = engine.deliberate(&obj, &desires, &tasks);
        match action {
            BdiAction::Advance {
                completed_desire_id,
                next_desire_id,
            } => {
                assert_eq!(completed_desire_id, "d-1");
                assert_eq!(next_desire_id, Some("d-2".into()));
            }
            other => panic!("expected Advance, got {:?}", other),
        }
    }

    #[test]
    fn test_deliberate_stuck_triggers_hegelian() {
        let engine = BdiEngine::new(0); // stuck_threshold = 0 → always stuck
        let obj = make_objective();
        let desires = vec![make_desire(
            "d-1",
            0,
            Some("int-1".into()),
            DesireStatus::Active,
        )];
        let tasks = vec![make_task("t-1", "d-1", TaskStatus::Pending, None)];

        let action = engine.deliberate(&obj, &desires, &tasks);
        match action {
            BdiAction::HegelianReview { stuck_ticks } => {
                // stuck_ticks is u64, always >= 0; just confirm we got here
                let _ = stuck_ticks;
            }
            other => panic!("expected HegelianReview, got {:?}", other),
        }
    }
}
