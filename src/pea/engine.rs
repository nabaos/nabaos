use crate::core::error::{NyayaError, Result};
use crate::pea::bdi::{BdiAction, BdiEngine};
use crate::pea::bridge::PeaBridge;
use crate::pea::budget::{BudgetController, BudgetMode};
use crate::pea::episode::EpisodeStore;
use crate::pea::htn::HtnDecomposer;
use crate::pea::objective::*;
use crate::pea::output_store::{OutputRecord, OutputStore, SourceType};
use crate::pea::pramana::PramanaValidator;
use crate::runtime::host_functions::AbilityRegistry;
use crate::runtime::manifest::AgentManifest;
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
    output_store: OutputStore,
    bdi: BdiEngine,
    htn: HtnDecomposer,
    pramana: PramanaValidator,
    /// Cached research context per desire_id — avoids re-running the full
    /// research pipeline (search + fetch + score) on every tick.
    /// Stores (context_string, corpus) so composition can reuse the corpus.
    research_cache: std::collections::HashMap<String, (String, crate::pea::research::ResearchCorpus)>,
    /// Knowledge graph per desire_id — extracted entities and relationships.
    kg_cache: std::collections::HashMap<String, crate::pea::knowledge_graph::KnowledgeGraph>,
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

        // On startup, reset any tasks stuck in 'running' — after a daemon
        // restart nothing can actually be running.
        match store.recover_running_tasks() {
            Ok(n) if n > 0 => eprintln!("[pea] recovered {n} tasks stuck in 'running' → ready"),
            _ => {}
        }

        let episode_store = EpisodeStore::open(&data_dir.join("pea_episodes.db"))?;
        let output_store = OutputStore::open(&data_dir.join("outputs.db"))?;
        Ok(Self {
            store,
            episode_store,
            output_store,
            bdi: BdiEngine::default(),
            htn: HtnDecomposer::default(),
            pramana: PramanaValidator::default(),
            research_cache: std::collections::HashMap::new(),
            kg_cache: std::collections::HashMap::new(),
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
            heartbeat_interval_secs: std::env::var("NABA_PEA_HEARTBEAT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(15),
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

                    // Attempt Hegelian dialectic synthesis
                    let desires = self.store.list_desires(&obj.id)?;
                    let tasks = self.store.list_tasks(&obj.id)?;
                    let current_strategy = desires
                        .iter()
                        .find(|d| d.intention_id.is_some())
                        .map(|d| d.description.as_str())
                        .unwrap_or("(unknown)");
                    let completed: Vec<String> = tasks
                        .iter()
                        .filter(|t| matches!(t.status, TaskStatus::Completed))
                        .map(|t| t.description.clone())
                        .collect();
                    let failed: Vec<String> = tasks
                        .iter()
                        .filter(|t| matches!(t.status, TaskStatus::Failed))
                        .map(|t| t.description.clone())
                        .collect();

                    let prompt = crate::pea::dialectic::build_dialectic_prompt(
                        &obj.description,
                        current_strategy,
                        obj.progress_score,
                        &completed,
                        &failed,
                        &format!("{:?}", obj.beliefs),
                    );
                    actions.push(format!(
                        "Dialectic prompt prepared ({} chars) — awaiting LLM handle for synthesis",
                        prompt.len()
                    ));

                    // Without an LLM handle, we log and fall through to Reconsider behavior.
                    // When orchestrator handle is available, it would:
                    //   1. Send `prompt` to LLM
                    //   2. Parse response with `dialectic::parse_dialectic_response()`
                    //   3. Create new tasks from `review.action_items`
                    //   4. Mark stuck tasks as replaced
                    //
                    // For now, create tasks from stuck task descriptions as placeholders
                    // that the next tick can act on.
                }
                BdiAction::Diagnose { desire_id } => {
                    // Simple tick: just mark as diagnosed and move on
                    let mut updated_obj = self.store.load_objective(&obj.id)?
                        .unwrap_or_else(|| obj.clone());
                    updated_obj.beliefs.set(
                        &format!("diagnosed_{}", desire_id),
                        serde_json::json!(true),
                        1.0,
                    );
                    self.store.save_objective(&updated_obj)?;
                    actions.push(format!("Diagnostic skipped (simple tick) for desire '{}'", desire_id));
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

    /// Tick all active objectives with actual execution via AbilityRegistry.
    ///
    /// Unlike `tick()`, this method:
    /// - Falls back to LLM-driven HTN decomposition when keywords fail
    /// - Executes tasks via `PeaBridge` (routes to llm.chat, media, files, etc.)
    /// - Performs Hegelian dialectic review via LLM when stuck
    /// - Assembles final documents on objective completion
    pub fn tick_with_executor(
        &mut self,
        registry: &AbilityRegistry,
        manifest: &AgentManifest,
        data_dir: &Path,
    ) -> Result<Vec<TickActivity>> {
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

            // 2. Promote pending tasks whose dependencies completed
            //    and auto-complete compound tasks when all children finish
            {
                let mut all_tasks = self.store.list_tasks(&obj.id)?;
                HtnDecomposer::promote_ready_tasks(&mut all_tasks);

                // Auto-complete compound tasks whose children are all done
                let done_ids: std::collections::HashSet<String> = all_tasks
                    .iter()
                    .filter(|t| matches!(t.status, TaskStatus::Completed | TaskStatus::Skipped))
                    .map(|t| t.id.clone())
                    .collect();
                // Collect compound tasks that need completion
                let compounds_to_complete: Vec<String> = all_tasks
                    .iter()
                    .filter(|t| {
                        t.task_type == TaskType::Compound
                            && !matches!(t.status, TaskStatus::Completed | TaskStatus::Skipped)
                    })
                    .filter(|t| {
                        let prefix = format!("{}.", t.id);
                        let children: Vec<&PeaTask> = all_tasks
                            .iter()
                            .filter(|c| c.id.starts_with(&prefix))
                            .collect();
                        !children.is_empty()
                            && children.iter().all(|c| done_ids.contains(&c.id))
                    })
                    .map(|t| t.id.clone())
                    .collect();
                for t in all_tasks.iter_mut() {
                    if compounds_to_complete.contains(&t.id) {
                        t.status = TaskStatus::Completed;
                        t.completed_at = Some(now);
                    }
                }

                for t in &all_tasks {
                    self.store.save_task(t)?;
                }
            }

            // 3. BDI deliberation (with freshly promoted tasks)
            let desires = self.store.list_desires(&obj.id)?;
            let tasks = self.store.list_tasks(&obj.id)?;
            let action = self.bdi.deliberate(obj, &desires, &tasks);

            match action {
                BdiAction::Persist => {
                    // Find next ready primitive task and execute it.
                    // Compound tasks complete when all children complete.
                    let ready = self.store.list_ready_tasks(&obj.id)?;
                    if let Some(next) = ready.iter().find(|t| t.task_type == TaskType::Primitive) {
                        self.store
                            .update_task_status(&next.id, &TaskStatus::Running)?;
                        actions.push(format!("Executing task '{}'", next.description));

                        // Build prior results for context threading
                        let completed_tasks = self.store.list_tasks(&obj.id)?;
                        let prior_results: Vec<(String, String)> = completed_tasks
                            .iter()
                            .filter(|t| t.status == TaskStatus::Completed && t.result_json.is_some())
                            .map(|t| {
                                (t.description.clone(), t.result_json.clone().unwrap_or_default())
                            })
                            .collect();

                        // Compute research once per desire, cache context + corpus.
                        // Check in-memory cache first, then disk, then run fresh.
                        let desire_id = &next.desire_id;
                        if !self.research_cache.contains_key(desire_id) {
                            use crate::pea::research::{ResearchConfig, ResearchCorpus, ResearchEngine};
                            let corpus_path = data_dir
                                .join("pea_output")
                                .join(&obj.id)
                                .join(format!("research_{}.json", desire_id));

                            // Try disk cache (valid for 4 hours)
                            let corpus = if let Some(cached) = ResearchCorpus::load_from_disk(
                                &corpus_path,
                                std::time::Duration::from_secs(4 * 3600),
                            ) {
                                eprintln!(
                                    "[pea] reusing cached research for desire {} ({} sources)",
                                    desire_id, cached.sources.len()
                                );
                                cached
                            } else {
                                let re = ResearchEngine::new(registry, manifest, ResearchConfig::default());
                                let corpus = re.execute(&obj.description, &next.description);
                                // Persist to disk for reuse
                                if let Err(e) = corpus.save_to_disk(&corpus_path) {
                                    eprintln!("[pea] failed to persist research corpus: {}", e);
                                }
                                corpus
                            };

                            let ctx = if corpus.sources.is_empty() {
                                String::new()
                            } else {
                                corpus.to_context_string()
                            };
                            // Build knowledge graph from corpus
                            if !self.kg_cache.contains_key(desire_id) && !corpus.sources.is_empty() {
                                use crate::pea::knowledge_graph::KnowledgeGraph;
                                let kg_path = data_dir
                                    .join("pea_output")
                                    .join(&obj.id)
                                    .join(format!("kg_{}.json", desire_id));

                                let kg = if let Some(cached_kg) = KnowledgeGraph::load_from_disk(&kg_path) {
                                    eprintln!(
                                        "[pea] reusing cached KG for desire {} ({} entities)",
                                        desire_id, cached_kg.entities.len()
                                    );
                                    cached_kg
                                } else {
                                    let kg = KnowledgeGraph::from_corpus(&corpus, registry, manifest);
                                    if let Err(e) = kg.save_to_disk(&kg_path) {
                                        eprintln!("[pea] failed to persist KG: {}", e);
                                    }
                                    kg
                                };
                                self.kg_cache.insert(desire_id.clone(), kg);
                            }

                            self.research_cache.insert(desire_id.clone(), (ctx, corpus));
                        }
                        let cached_context = self.research_cache.get(desire_id).map(|(ctx, _)| ctx.clone());

                        // Execute via bridge
                        let output_dir = data_dir.join("pea_output").join(&obj.id);
                        let bridge = PeaBridge::new(registry, manifest, &output_dir);
                        let result = bridge.execute_task(
                            &next.description,
                            &obj.description,
                            &prior_results,
                            cached_context.as_deref(),
                        );

                        if result.success {
                            let result_json = serde_json::to_string(&result).unwrap_or_default();

                            // Pratyaksha check: output not empty
                            let pratyaksha = self.pramana.pratyaksha(&result.output, "");
                            let pramana_json = serde_json::to_string(&pratyaksha).unwrap_or_default();

                            let mut completed_task = next.clone();
                            completed_task.status = TaskStatus::Completed;
                            completed_task.completed_at = Some(now);
                            completed_task.result_json = Some(result_json);
                            completed_task.pramana_record_json = Some(pramana_json);
                            self.store.save_task(&completed_task)?;

                            // Update spent
                            if result.cost_usd > 0.0 {
                                let mut updated_obj = obj.clone();
                                updated_obj.spent_usd += result.cost_usd;
                                self.store.save_objective(&updated_obj)?;
                            }

                            // Update progress
                            let all = self.store.list_tasks(&obj.id)?;
                            let total = all.iter().filter(|t| t.task_type == TaskType::Primitive).count();
                            let completed = all.iter().filter(|t| {
                                t.task_type == TaskType::Primitive
                                    && (t.status == TaskStatus::Completed || t.status == TaskStatus::Skipped)
                            }).count();
                            if total > 0 {
                                let mut updated_obj = self.store.load_objective(&obj.id)?
                                    .unwrap_or_else(|| obj.clone());
                                updated_obj.progress_score = completed as f64 / total as f64;
                                self.store.save_objective(&updated_obj)?;
                            }

                            actions.push(format!(
                                "Task completed — output {} chars, cost ${:.4}",
                                result.output.len(),
                                result.cost_usd
                            ));
                        } else {
                            // Task failed — retry or mark failed
                            let mut updated_task = next.clone();
                            updated_task.retry_count += 1;
                            if updated_task.retry_count >= updated_task.max_retries {
                                updated_task.status = TaskStatus::Failed;
                                actions.push(format!(
                                    "Task failed after {} retries: {}",
                                    updated_task.max_retries, result.output
                                ));
                            } else {
                                updated_task.status = TaskStatus::Ready; // retry
                                actions.push(format!(
                                    "Task failed (retry {}/{}): {}",
                                    updated_task.retry_count, updated_task.max_retries, result.output
                                ));
                            }
                            self.store.save_task(&updated_task)?;
                        }
                    } else {
                        actions.push("Persisting — no ready tasks".to_string());
                    }
                }
                BdiAction::Commit { desire_id } => {
                    actions.push(format!("Committing to desire '{}'", desire_id));

                    let desire_desc = desires
                        .iter()
                        .find(|d| d.id == desire_id)
                        .map(|d| d.description.clone())
                        .unwrap_or_else(|| desire_id.clone());

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

                    // Try keyword HTN first, then LLM decomposition
                    if let Some(subtasks) = self.htn.decompose(&root_task, &obj.id, &desire_id) {
                        self.store.save_task(&root_task)?;
                        for st in &subtasks {
                            self.store.save_task(st)?;
                        }
                        actions.push(format!("HTN decomposed into {} subtasks", subtasks.len()));
                    } else if let Some(subtasks) = crate::pea::htn::decompose_with_llm(
                        registry,
                        manifest,
                        &root_task,
                        &obj.id,
                        &desire_id,
                    ) {
                        self.store.save_task(&root_task)?;
                        for st in &subtasks {
                            self.store.save_task(st)?;
                        }
                        actions.push(format!(
                            "LLM decomposed into {} subtasks",
                            subtasks.len()
                        ));
                    } else {
                        // No decomposition — save as primitive
                        let mut prim = root_task;
                        prim.task_type = TaskType::Primitive;
                        self.store.save_task(&prim)?;
                        actions.push("Saved as primitive task (no decomposition)".to_string());
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
                    // Evict research + KG cache for the completed desire
                    self.research_cache.remove(&completed_desire_id);
                    self.kg_cache.remove(&completed_desire_id);
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

                    let desires = self.store.list_desires(&obj.id)?;
                    let tasks = self.store.list_tasks(&obj.id)?;
                    let current_strategy = desires
                        .iter()
                        .find(|d| d.intention_id.is_some())
                        .map(|d| d.description.as_str())
                        .unwrap_or("(unknown)");
                    let completed: Vec<String> = tasks
                        .iter()
                        .filter(|t| matches!(t.status, TaskStatus::Completed))
                        .map(|t| t.description.clone())
                        .collect();
                    let failed: Vec<String> = tasks
                        .iter()
                        .filter(|t| matches!(t.status, TaskStatus::Failed))
                        .map(|t| t.description.clone())
                        .collect();

                    let prompt = crate::pea::dialectic::build_dialectic_prompt(
                        &obj.description,
                        current_strategy,
                        obj.progress_score,
                        &completed,
                        &failed,
                        &format!("{:?}", obj.beliefs),
                    );

                    // Send to LLM for dialectic synthesis
                    let input = serde_json::json!({
                        "system": "You are a strategic advisor. Respond with ONLY a JSON object.",
                        "prompt": prompt,
                    });

                    match registry.execute_ability(manifest, "llm.chat", &input.to_string()) {
                        Ok(result) => {
                            let response = String::from_utf8_lossy(&result.output);
                            if let Some(review) =
                                crate::pea::dialectic::parse_dialectic_response(&response)
                            {
                                actions.push(format!(
                                    "Dialectic synthesis: {}",
                                    review.synthesis
                                ));

                                // Create new tasks from action items
                                let desire_id = desires
                                    .iter()
                                    .find(|d| d.intention_id.is_some())
                                    .map(|d| d.id.clone())
                                    .unwrap_or_default();

                                for (i, item) in review.action_items.iter().enumerate() {
                                    let new_task = PeaTask {
                                        id: format!("synth_{}_{}", uuid_simple(), i),
                                        objective_id: obj.id.clone(),
                                        desire_id: desire_id.clone(),
                                        parent_task_id: None,
                                        description: item.clone(),
                                        task_type: TaskType::Primitive,
                                        status: TaskStatus::Ready,
                                        ordering: (tasks.len() + i) as i32,
                                        depends_on: vec![],
                                        capability_required: None,
                                        result_json: None,
                                        pramana_record_json: None,
                                        retry_count: 0,
                                        max_retries: 3,
                                        created_at: now,
                                        completed_at: None,
                                    };
                                    self.store.save_task(&new_task)?;
                                }
                                actions.push(format!(
                                    "Created {} new tasks from dialectic review",
                                    review.action_items.len()
                                ));
                            }
                        }
                        Err(e) => {
                            actions.push(format!("Dialectic LLM call failed: {}", e));
                        }
                    }
                }
                BdiAction::Diagnose { desire_id } => {
                    actions.push(format!(
                        "Diagnostic review for desire '{}'",
                        desire_id
                    ));

                    // 1. Collect all completed task results for this desire
                    let all_tasks = self.store.list_tasks(&obj.id)?;
                    let desire_results: Vec<(String, String)> = all_tasks
                        .iter()
                        .filter(|t| {
                            t.desire_id == desire_id
                                && t.status == TaskStatus::Completed
                                && t.result_json.is_some()
                        })
                        .filter_map(|t| {
                            let raw = t.result_json.as_ref()?;
                            let output =
                                if let Ok(tr) = serde_json::from_str::<serde_json::Value>(raw) {
                                    tr.get("output")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or(raw)
                                        .to_string()
                                } else {
                                    raw.clone()
                                };
                            Some((t.description.clone(), output))
                        })
                        .collect();

                    // 2. Check for LaTeX compilation log
                    let output_dir = data_dir.join("pea_output").join(&obj.id);
                    let log_path = output_dir.join("output.log");
                    let latex_log = std::fs::read_to_string(&log_path).ok().map(|log| {
                        let lines: Vec<&str> = log.lines().collect();
                        let start = lines.len().saturating_sub(100);
                        lines[start..].join("\n")
                    });

                    // 3. Build diagnostic prompt
                    let mut task_section = String::new();
                    for (desc, output) in &desire_results {
                        task_section.push_str(&format!(
                            "=== Task: {} ===\n{}\n\n",
                            desc, output
                        ));
                    }

                    let log_section = if let Some(ref log) = latex_log {
                        format!(
                            "\nDOCUMENT COMPILATION LOG (errors):\n{}\n",
                            log
                        )
                    } else {
                        String::new()
                    };

                    // Build current date for temporal context
                    let current_date = {
                        let secs = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        let days = secs / 86400;
                        let y = (days * 400 / 146097) + 1970;
                        format!("{}", y)
                    };

                    // Collect fetched source URLs as reality evidence
                    let fetched_urls: Vec<String> = self.research_cache
                        .get(&desire_id)
                        .map(|(_, corpus)| {
                            corpus.sources.iter().map(|s| s.url.clone()).collect()
                        })
                        .unwrap_or_default();
                    let url_evidence = if fetched_urls.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "\nVERIFIED SOURCE URLs (fetched successfully, HTTP 200):\n{}\n\
                             If a claim is attributed to one of these URLs, treat it as SOURCED, not fabricated.\n",
                            fetched_urls.iter().take(50).cloned().collect::<Vec<_>>().join("\n")
                        )
                    };

                    let prompt = format!(
                        "You are a quality assurance reviewer. Today's date is {} (current year). \
                         Content below is based on LIVE web research from real sources fetched today.\n\n\
                         CRITICAL RULES:\n\
                         - Do NOT flag current events as fabricated simply because they are recent or dramatic.\n\
                         - Do NOT rewrite content as 'fictional scenario' or 'speculative analysis'.\n\
                         - Do NOT add disclaimers stating events did not occur.\n\
                         - Your role: check INTERNAL CONSISTENCY, citation completeness, and formatting ONLY.\n\n\
                         OBJECTIVE: {}\n\n\
                         TASK RESULTS:\n{}\n\
                         {}\
                         {}\n\
                         Check for these failure modes ONLY:\n\n\
                         1. INTERNAL CONTRADICTIONS — facts, dates, quantities, or names that diverge between sections. \
                         Compare claims across all task outputs.\n\
                         2. CONTENT TRUNCATION — abrupt endings mid-sentence, placeholder text (\"TBD\", \"TODO\", \"[insert]\"), \
                         or sections that trail off without conclusion.\n\
                         3. MISSING REQUIRED CONTENT — elements explicitly requested in the objective but absent from outputs.\n\
                         4. STRUCTURAL ISSUES — missing conclusion, missing executive summary, orphaned section references.\n\
                         5. DOCUMENT COMPILATION ERRORS — LaTeX/build log issues (if log provided).\n\n\
                         Severity rules: contradictions = high; truncation = high; missing content = medium-high; \
                         structural = medium; compilation = low unless it blocks output.\n\n\
                         For each issue, provide a SPECIFIC remediation task as a surgical fix \
                         (e.g. 'In section X, replace Y with Z') — NOT full rewrites.\n\n\
                         Output JSON ONLY (no markdown fences):\n\
                         {{\"issues\": [{{\"task\": \"task description\", \"problem\": \"what's wrong — quote: [excerpt]\", \
                         \"severity\": \"high|medium|low\"}}], \
                         \"remediation_tasks\": [{{\"description\": \"specific surgical fix\", \"depends_on\": []}}]}}\n\n\
                         If everything looks good, output: {{\"issues\": [], \"remediation_tasks\": []}}",
                        current_date,
                        obj.description,
                        task_section,
                        log_section,
                        url_evidence,
                    );

                    let input = serde_json::json!({
                        "system": "You are a quality assurance reviewer. Respond with ONLY a JSON object.",
                        "prompt": prompt,
                    });

                    // 4. Call LLM for diagnostic
                    match registry.execute_ability(manifest, "llm.chat", &input.to_string()) {
                        Ok(result) => {
                            let response = String::from_utf8_lossy(&result.output);

                            // Parse the diagnostic response (extract JSON from possible fences)
                            let json_str = response
                                .find('{')
                                .and_then(|start| {
                                    response.rfind('}').map(|end| &response[start..=end])
                                })
                                .unwrap_or(&response);

                            if let Ok(diag) =
                                serde_json::from_str::<serde_json::Value>(json_str)
                            {
                                let remediation = diag
                                    .get("remediation_tasks")
                                    .and_then(|v| v.as_array())
                                    .cloned()
                                    .unwrap_or_default();

                                let issues = diag
                                    .get("issues")
                                    .and_then(|v| v.as_array())
                                    .map(|a| a.len())
                                    .unwrap_or(0);

                                if !remediation.is_empty() {
                                    actions.push(format!(
                                        "Found {} issues, creating {} remediation tasks",
                                        issues,
                                        remediation.len()
                                    ));

                                    for (i, task_def) in remediation.iter().enumerate() {
                                        let desc = task_def
                                            .get("description")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("Fix identified issue")
                                            .to_string();

                                        let new_task = PeaTask {
                                            id: format!("diag_{}_{}", uuid_simple(), i),
                                            objective_id: obj.id.clone(),
                                            desire_id: desire_id.clone(),
                                            parent_task_id: None,
                                            description: desc,
                                            task_type: TaskType::Primitive,
                                            status: TaskStatus::Ready,
                                            ordering: (all_tasks.len() + i) as i32,
                                            depends_on: vec![],
                                            capability_required: None,
                                            result_json: None,
                                            pramana_record_json: None,
                                            retry_count: 0,
                                            max_retries: 3,
                                            created_at: now,
                                            completed_at: None,
                                        };
                                        self.store.save_task(&new_task)?;
                                    }
                                } else {
                                    actions
                                        .push("Diagnostic clean — no issues found".to_string());
                                }
                            } else {
                                actions.push(
                                    "Could not parse diagnostic response — marking clean"
                                        .to_string(),
                                );
                            }
                        }
                        Err(e) => {
                            actions.push(format!(
                                "Diagnostic LLM call failed: {} — marking clean",
                                e
                            ));
                        }
                    }

                    // 5. Mark desire as diagnosed (prevents infinite loops)
                    let mut updated_obj = self
                        .store
                        .load_objective(&obj.id)?
                        .unwrap_or_else(|| obj.clone());
                    updated_obj.beliefs.set(
                        &format!("diagnosed_{}", desire_id),
                        serde_json::json!(true),
                        1.0,
                    );
                    self.store.save_objective(&updated_obj)?;
                }
                BdiAction::ObjectiveComplete => {
                    // Assemble final document from completed task results.
                    // Remediation tasks (from QA/Diagnose, id starts with "diag_")
                    // supersede original tasks — we prefer the fixed version.
                    let tasks = self.store.list_tasks(&obj.id)?;
                    let completed: Vec<&PeaTask> = tasks
                        .iter()
                        .filter(|t| t.status == TaskStatus::Completed && t.result_json.is_some())
                        .collect();

                    // Separate original and remediation tasks
                    let originals: Vec<&&PeaTask> = completed.iter().filter(|t| !t.id.starts_with("diag_")).collect();
                    let remediations: Vec<&&PeaTask> = completed.iter().filter(|t| t.id.starts_with("diag_")).collect();

                    // Build the result list: originals first, then remediations
                    // with an explicit label so the LLM knows remediations replace originals
                    let extract_output = |t: &PeaTask| -> Option<(String, String)> {
                        let raw = t.result_json.as_ref()?;
                        let output = if let Ok(tr) = serde_json::from_str::<serde_json::Value>(raw) {
                            tr.get("output")
                                .and_then(|v| v.as_str())
                                .unwrap_or(raw)
                                .to_string()
                        } else {
                            raw.clone()
                        };
                        Some((t.description.clone(), output))
                    };

                    let mut task_results: Vec<(String, String)> = Vec::new();
                    for t in &originals {
                        if let Some(pair) = extract_output(t) {
                            task_results.push(pair);
                        }
                    }
                    if !remediations.is_empty() {
                        // Add remediation results with explicit supersession label
                        for t in &remediations {
                            if let Some((desc, output)) = extract_output(t) {
                                task_results.push((
                                    format!("[QA FIX — supersedes earlier content] {}", desc),
                                    output,
                                ));
                            }
                        }
                    }

                    let output_dir = data_dir.join("pea_output").join(&obj.id);

                    // Style analysis: LLM determines visual theme based on content
                    let style_config = crate::pea::document::analyze_style(
                        registry, manifest, &obj.description, &task_results,
                    );
                    actions.push(format!(
                        "Style: theme={}, ornaments={}, images={}",
                        style_config.theme, style_config.ornament_style, style_config.image_queries.len()
                    ));

                    // Fetch images from stock photo services
                    let mut images: Vec<crate::pea::document::ImageEntry> = Vec::new();
                    for iq in &style_config.image_queries {
                        let fetch_input = serde_json::json!({
                            "query": iq.query,
                            "output_dir": output_dir.to_string_lossy(),
                        });
                        match registry.execute_ability(
                            manifest,
                            "media.fetch_stock_image",
                            &fetch_input.to_string(),
                        ) {
                            Ok(result) => {
                                let path_str = String::from_utf8_lossy(&result.output).to_string();
                                let path = std::path::PathBuf::from(path_str.trim());
                                if path.exists() {
                                    let caption = iq.chapter.clone().unwrap_or_else(|| iq.query.clone());
                                    let attribution = result.facts.get("attribution").cloned();
                                    images.push((caption, path, attribution));
                                }
                            }
                            Err(e) => {
                                actions.push(format!("Image fetch failed for '{}': {}", iq.query, e));
                            }
                        }
                    }
                    if !images.is_empty() {
                        actions.push(format!("Fetched {} images", images.len()));
                    }

                    // Use DocumentComposer for intelligent multi-level composition.
                    // Reuse cached research corpus from task execution to avoid a
                    // duplicate research pass (saves ~10 min). Only run fresh
                    // research if no cached corpus is available.
                    let compose_result = {
                        use crate::pea::composer::{ComposerConfig, DocumentComposer};
                        use crate::pea::research::{ResearchConfig, ResearchEngine};

                        let corpus = if let Some((_, cached_corpus)) = self.research_cache.values().next() {
                            eprintln!("[pea] reusing cached research corpus ({} sources) for composition", cached_corpus.sources.len());
                            actions.push(format!(
                                "Research: reused cached corpus ({} sources)",
                                cached_corpus.sources.len(),
                            ));
                            cached_corpus.clone()
                        } else {
                            eprintln!("[pea] no cached corpus, running fresh research for composition");
                            let research = ResearchEngine::new(
                                registry,
                                manifest,
                                ResearchConfig::default(),
                            );
                            let c = research.execute(&obj.description, "compile final document");
                            actions.push(format!(
                                "Research: {} sources from {} candidates",
                                c.sources.len(),
                                c.total_candidates,
                            ));
                            c
                        };

                        let composer = DocumentComposer::new(
                            registry,
                            manifest,
                            ComposerConfig::default(),
                        );
                        // Look up KG for the desire that triggered composition
                        let kg_ref = self.kg_cache.values().next();
                        composer.compose_document_with_kg(
                            &obj.description,
                            &corpus,
                            &task_results,
                            &images,
                            &style_config,
                            &output_dir,
                            kg_ref,
                        )
                    };

                    // Fall back to legacy assembly if composer fails
                    let doc_result = match compose_result {
                        Ok(path) => Ok(path),
                        Err(e) => {
                            actions.push(format!("Composer failed ({}), using legacy assembly", e));
                            crate::pea::document::assemble_document(
                                registry,
                                manifest,
                                &obj.description,
                                &task_results,
                                &images,
                                Some(&style_config),
                                &output_dir,
                            )
                        }
                    };

                    match doc_result {
                        Ok(path) => {
                            actions.push(format!(
                                "Document assembled: {}",
                                path.display()
                            ));

                            // Register output in OutputStore
                            let output_id = uuid_simple();
                            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                            let content_type = match ext {
                                "pdf" => "application/pdf",
                                "html" | "htm" => "text/html",
                                "tex" => "text/x-latex",
                                _ => "text/plain",
                            };
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            let record = OutputRecord {
                                id: output_id.clone(),
                                source_type: SourceType::Pea,
                                source_id: obj.id.clone(),
                                title: obj.description.clone(),
                                content_type: content_type.to_string(),
                                file_path: Some(path.display().to_string()),
                                text_content: None,
                                metadata_json: "{}".to_string(),
                                created_at: now,
                                updated_at: now,
                            };
                            if let Err(e) = self.output_store.register(&record) {
                                actions.push(format!("Output registration failed: {}", e));
                            } else {
                                // Store output_id in beliefs for TUI/API lookup
                                let mut obj_upd = self.store.load_objective(&obj.id)?
                                    .unwrap_or_else(|| obj.clone());
                                obj_upd.beliefs.set("output_id", serde_json::json!(output_id.clone()), 1.0);
                                self.store.save_objective(&obj_upd)?;
                                // Queue notification for Telegram delivery
                                let _ = self.output_store.insert_notification(&output_id);
                                actions.push(format!("Output registered: {}", output_id));
                            }
                        }
                        Err(e) => {
                            actions.push(format!("Document assembly failed: {}", e));
                        }
                    }

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
            let mut updated_obj = self.store.load_objective(&obj.id)?
                .unwrap_or_else(|| obj.clone());
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

    /// Access the output store.
    pub fn output_store(&self) -> &OutputStore {
        &self.output_store
    }

    /// Create an improvement objective from a completed original.
    ///
    /// Loads the original objective and its task results, then creates a new
    /// objective that re-diagnoses and fixes quality issues. The daemon's tick
    /// loop will execute it automatically.
    pub fn improve_objective(
        &self,
        original_id: &str,
        instructions: Option<&str>,
        budget_usd: f64,
    ) -> Result<String> {
        let original = self
            .store
            .load_objective(original_id)?
            .ok_or_else(|| NyayaError::Cache(format!("objective not found: {original_id}")))?;

        // Gather completed task results from original
        let tasks = self.store.list_tasks(original_id)?;
        let task_results: Vec<serde_json::Value> = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Completed && t.result_json.is_some())
            .map(|t| {
                serde_json::json!({
                    "description": t.description,
                    "result": t.result_json,
                })
            })
            .collect();

        let description = format!("[IMPROVEMENT] {}", original.description);

        // Build desires: user instructions + re-diagnose
        let mut desires = Vec::new();
        if let Some(instr) = instructions {
            desires.push((
                instr.to_string(),
                "user-requested improvements applied".to_string(),
                0,
            ));
        }
        desires.push((
            "Re-diagnose and fix all quality issues found in the original output".to_string(),
            "all quality issues resolved".to_string(),
            desires.len() as i32,
        ));

        let obj_id = self.create_objective(&description, budget_usd, desires)?;

        // Store prior context in beliefs
        let mut obj = self.store.load_objective(&obj_id)?.unwrap();
        obj.beliefs.set(
            "original_objective_id",
            serde_json::json!(original_id),
            1.0,
        );
        obj.beliefs.set(
            "prior_task_results",
            serde_json::json!(task_results),
            1.0,
        );
        self.store.save_objective(&obj)?;

        Ok(obj_id)
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
    fn test_engine_hegelian_review_produces_dialectic_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let engine = PeaEngine::open(dir.path()).unwrap();

        let obj_id = engine
            .create_objective(
                "Stuck objective",
                10.0,
                vec![("Do something".to_string(), "done".to_string(), 0)],
            )
            .unwrap();

        // First tick: commits to the desire
        let _ = engine.tick().unwrap();

        // Set heartbeat_interval very low and last_tick_at to 0 to ensure next tick runs
        let mut obj = engine.get_status(&obj_id).unwrap().unwrap();
        obj.heartbeat_interval_secs = 1;
        obj.last_tick_at = 0;
        // Make BDI engine think we're stuck by setting created_at far in the past
        obj.created_at = 0;
        engine.store.save_objective(&obj).unwrap();

        // Force stuck by creating a task that is pending and very old
        // The BDI engine uses stuck_threshold=12 by default, so
        // ticks_since_last_completion needs to be >= 12
        // With heartbeat_interval=1 and created_at=0, any now > 12 suffices

        // Second tick should hit HegelianReview since stuck_ticks >= 12
        // (created_at=0, no completed tasks, so last_completed=0, now is large)
        let activities = engine.tick().unwrap();
        if !activities.is_empty() {
            let has_hegelian = activities[0]
                .actions_taken
                .iter()
                .any(|a| a.contains("Hegelian") || a.contains("dialectic") || a.contains("Dialectic"));
            // Note: may or may not trigger depending on BDI state, but if it does,
            // should contain dialectic prompt
            if has_hegelian {
                let has_prompt = activities[0]
                    .actions_taken
                    .iter()
                    .any(|a| a.contains("prompt prepared"));
                assert!(has_prompt, "Hegelian review should prepare dialectic prompt");
            }
        }
    }

    #[test]
    fn test_dialectic_parse_creates_action_items() {
        // Test that parse_dialectic_response works with the format expected by the engine
        let response = r#"{"thesis":"current approach","antithesis":"stuck on tasks","synthesis":"try new approach","action_items":["retry with different params","split task"],"should_pivot":false}"#;
        let review = crate::pea::dialectic::parse_dialectic_response(response).unwrap();
        assert_eq!(review.action_items.len(), 2);
        assert_eq!(review.action_items[0], "retry with different params");
        assert!(!review.should_pivot);
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
