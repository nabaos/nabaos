use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::core::error::{NyayaError, Result};

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveStatus {
    Active,
    Paused,
    Completed,
    Failed,
}

impl fmt::Display for ObjectiveStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Paused => write!(f, "paused"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DesireStatus {
    Active,
    Achieved,
    Abandoned,
}

impl fmt::Display for DesireStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Achieved => write!(f, "achieved"),
            Self::Abandoned => write!(f, "abandoned"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Ready,
    Running,
    Completed,
    Failed,
    Blocked,
    Skipped,
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Ready => write!(f, "ready"),
            Self::Running => write!(f, "running"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Blocked => write!(f, "blocked"),
            Self::Skipped => write!(f, "skipped"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    Compound,
    Primitive,
}

impl fmt::Display for TaskType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compound => write!(f, "compound"),
            Self::Primitive => write!(f, "primitive"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum BudgetStrategy {
    Aggressive,
    #[default]
    Adaptive,
    Conservative,
    Minimal,
}

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BeliefStore {
    pub facts: HashMap<String, serde_json::Value>,
    pub confidence: HashMap<String, f64>,
    pub last_updated: HashMap<String, u64>,
}

impl BeliefStore {
    pub fn set(&mut self, key: &str, value: serde_json::Value, confidence: f64) {
        self.facts.insert(key.to_string(), value);
        self.confidence.insert(key.to_string(), confidence);
        self.last_updated.insert(
            key.to_string(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );
    }

    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.facts.get(key)
    }

    pub fn confidence_of(&self, key: &str) -> f64 {
        self.confidence.get(key).copied().unwrap_or(0.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Milestone {
    pub description: String,
    pub criteria: String,
    pub achieved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Objective {
    pub id: String,
    pub description: String,
    pub status: ObjectiveStatus,
    pub created_at: u64,
    pub updated_at: u64,
    pub beliefs: BeliefStore,
    pub budget_usd: f64,
    pub spent_usd: f64,
    pub budget_strategy: BudgetStrategy,
    pub progress_score: f64,
    pub milestones: Vec<Milestone>,
    pub heartbeat_interval_secs: u64,
    pub last_tick_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Desire {
    pub id: String,
    pub objective_id: String,
    pub description: String,
    pub priority: i32,
    pub status: DesireStatus,
    pub completion_criteria: String,
    pub intention_id: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeaTask {
    pub id: String,
    pub objective_id: String,
    pub desire_id: String,
    pub parent_task_id: Option<String>,
    pub description: String,
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub ordering: i32,
    pub depends_on: Vec<String>,
    pub capability_required: Option<String>,
    pub result_json: Option<String>,
    pub pramana_record_json: Option<String>,
    pub retry_count: i32,
    pub max_retries: i32,
    pub created_at: u64,
    pub completed_at: Option<u64>,
}

// ---------------------------------------------------------------------------
// ObjectiveStore — SQLite persistence
// ---------------------------------------------------------------------------

pub struct ObjectiveStore {
    conn: Connection,
}

impl ObjectiveStore {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| NyayaError::Cache(format!("failed to open objective store: {e}")))?;

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS pea_objectives (
                id              TEXT PRIMARY KEY,
                description     TEXT NOT NULL,
                status          TEXT NOT NULL DEFAULT 'active',
                created_at      INTEGER NOT NULL,
                updated_at      INTEGER NOT NULL,
                beliefs_json    TEXT NOT NULL DEFAULT '{}',
                budget_usd      REAL NOT NULL DEFAULT 0.0,
                spent_usd       REAL NOT NULL DEFAULT 0.0,
                budget_strategy TEXT NOT NULL DEFAULT 'adaptive',
                progress_score  REAL NOT NULL DEFAULT 0.0,
                milestones_json TEXT NOT NULL DEFAULT '[]',
                heartbeat_interval_secs INTEGER NOT NULL DEFAULT 300,
                last_tick_at    INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS pea_desires (
                id                  TEXT PRIMARY KEY,
                objective_id        TEXT NOT NULL,
                description         TEXT NOT NULL,
                priority            INTEGER NOT NULL DEFAULT 0,
                status              TEXT NOT NULL DEFAULT 'active',
                completion_criteria TEXT NOT NULL DEFAULT '',
                intention_id        TEXT,
                created_at          INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_desires_objective ON pea_desires(objective_id);

            CREATE TABLE IF NOT EXISTS pea_tasks (
                id                  TEXT PRIMARY KEY,
                objective_id        TEXT NOT NULL,
                desire_id           TEXT NOT NULL,
                parent_task_id      TEXT,
                description         TEXT NOT NULL,
                task_type           TEXT NOT NULL DEFAULT 'primitive',
                status              TEXT NOT NULL DEFAULT 'pending',
                ordering            INTEGER NOT NULL DEFAULT 0,
                depends_on_json     TEXT NOT NULL DEFAULT '[]',
                capability_required TEXT,
                result_json         TEXT,
                pramana_record_json TEXT,
                retry_count         INTEGER NOT NULL DEFAULT 0,
                max_retries         INTEGER NOT NULL DEFAULT 3,
                created_at          INTEGER NOT NULL,
                completed_at        INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_tasks_objective ON pea_tasks(objective_id);
            CREATE INDEX IF NOT EXISTS idx_tasks_status    ON pea_tasks(status);
            ",
        )
        .map_err(|e| NyayaError::Cache(format!("failed to create PEA tables: {e}")))?;

        Ok(Self { conn })
    }

    // -- Objectives ----------------------------------------------------------

    pub fn save_objective(&self, obj: &Objective) -> Result<()> {
        let beliefs_json = serde_json::to_string(&obj.beliefs)
            .map_err(|e| NyayaError::Cache(format!("serialize beliefs: {e}")))?;
        let milestones_json = serde_json::to_string(&obj.milestones)
            .map_err(|e| NyayaError::Cache(format!("serialize milestones: {e}")))?;
        let status_str = obj.status.to_string();
        let strategy_str = serde_json::to_value(&obj.budget_strategy)
            .map_err(|e| NyayaError::Cache(format!("serialize budget_strategy: {e}")))?
            .as_str()
            .unwrap_or("adaptive")
            .to_string();

        self.conn
            .execute(
                "INSERT OR REPLACE INTO pea_objectives
                 (id, description, status, created_at, updated_at,
                  beliefs_json, budget_usd, spent_usd, budget_strategy,
                  progress_score, milestones_json, heartbeat_interval_secs, last_tick_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                rusqlite::params![
                    obj.id,
                    obj.description,
                    status_str,
                    obj.created_at,
                    obj.updated_at,
                    beliefs_json,
                    obj.budget_usd,
                    obj.spent_usd,
                    strategy_str,
                    obj.progress_score,
                    milestones_json,
                    obj.heartbeat_interval_secs,
                    obj.last_tick_at,
                ],
            )
            .map_err(|e| NyayaError::Cache(format!("save objective: {e}")))?;
        Ok(())
    }

    pub fn load_objective(&self, id: &str) -> Result<Option<Objective>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, description, status, created_at, updated_at,
                        beliefs_json, budget_usd, spent_usd, budget_strategy,
                        progress_score, milestones_json, heartbeat_interval_secs, last_tick_at
                 FROM pea_objectives WHERE id = ?1",
            )
            .map_err(|e| NyayaError::Cache(format!("prepare load_objective: {e}")))?;

        let mut rows = stmt
            .query_map(rusqlite::params![id], |row| {
                Ok(ObjectiveRow {
                    id: row.get(0)?,
                    description: row.get(1)?,
                    status: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    beliefs_json: row.get(5)?,
                    budget_usd: row.get(6)?,
                    spent_usd: row.get(7)?,
                    budget_strategy: row.get(8)?,
                    progress_score: row.get(9)?,
                    milestones_json: row.get(10)?,
                    heartbeat_interval_secs: row.get(11)?,
                    last_tick_at: row.get(12)?,
                })
            })
            .map_err(|e| NyayaError::Cache(format!("query load_objective: {e}")))?;

        match rows.next() {
            Some(Ok(r)) => Ok(Some(row_to_objective(r)?)),
            Some(Err(e)) => Err(NyayaError::Cache(format!("read objective row: {e}"))),
            None => Ok(None),
        }
    }

    pub fn list_active_objectives(&self) -> Result<Vec<Objective>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, description, status, created_at, updated_at,
                        beliefs_json, budget_usd, spent_usd, budget_strategy,
                        progress_score, milestones_json, heartbeat_interval_secs, last_tick_at
                 FROM pea_objectives WHERE status = 'active'",
            )
            .map_err(|e| NyayaError::Cache(format!("prepare list_active: {e}")))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(ObjectiveRow {
                    id: row.get(0)?,
                    description: row.get(1)?,
                    status: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    beliefs_json: row.get(5)?,
                    budget_usd: row.get(6)?,
                    spent_usd: row.get(7)?,
                    budget_strategy: row.get(8)?,
                    progress_score: row.get(9)?,
                    milestones_json: row.get(10)?,
                    heartbeat_interval_secs: row.get(11)?,
                    last_tick_at: row.get(12)?,
                })
            })
            .map_err(|e| NyayaError::Cache(format!("query list_active: {e}")))?;

        let mut result = Vec::new();
        for r in rows {
            let r = r.map_err(|e| NyayaError::Cache(format!("read active row: {e}")))?;
            result.push(row_to_objective(r)?);
        }
        Ok(result)
    }

    pub fn update_objective_status(&self, id: &str, status: &ObjectiveStatus) -> Result<()> {
        self.conn
            .execute(
                "UPDATE pea_objectives SET status = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![
                    status.to_string(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    id,
                ],
            )
            .map_err(|e| NyayaError::Cache(format!("update objective status: {e}")))?;
        Ok(())
    }

    // -- Desires -------------------------------------------------------------

    pub fn save_desire(&self, desire: &Desire) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO pea_desires
                 (id, objective_id, description, priority, status,
                  completion_criteria, intention_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    desire.id,
                    desire.objective_id,
                    desire.description,
                    desire.priority,
                    desire.status.to_string(),
                    desire.completion_criteria,
                    desire.intention_id,
                    desire.created_at,
                ],
            )
            .map_err(|e| NyayaError::Cache(format!("save desire: {e}")))?;
        Ok(())
    }

    pub fn list_desires(&self, objective_id: &str) -> Result<Vec<Desire>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, objective_id, description, priority, status,
                        completion_criteria, intention_id, created_at
                 FROM pea_desires WHERE objective_id = ?1 ORDER BY priority",
            )
            .map_err(|e| NyayaError::Cache(format!("prepare list_desires: {e}")))?;

        let rows = stmt
            .query_map(rusqlite::params![objective_id], |row| {
                let status_str: String = row.get(4)?;
                let status: DesireStatus =
                    serde_json::from_value(serde_json::Value::String(status_str))
                        .unwrap_or(DesireStatus::Active);
                Ok(Desire {
                    id: row.get(0)?,
                    objective_id: row.get(1)?,
                    description: row.get(2)?,
                    priority: row.get(3)?,
                    status,
                    completion_criteria: row.get(5)?,
                    intention_id: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })
            .map_err(|e| NyayaError::Cache(format!("query list_desires: {e}")))?;

        let mut result = Vec::new();
        for r in rows {
            result.push(r.map_err(|e| NyayaError::Cache(format!("read desire row: {e}")))?);
        }
        Ok(result)
    }

    pub fn update_desire_status(&self, id: &str, status: &DesireStatus) -> Result<()> {
        self.conn
            .execute(
                "UPDATE pea_desires SET status = ?1 WHERE id = ?2",
                rusqlite::params![status.to_string(), id],
            )
            .map_err(|e| NyayaError::Cache(format!("update desire status: {e}")))?;
        Ok(())
    }

    // -- Tasks ---------------------------------------------------------------

    pub fn save_task(&self, task: &PeaTask) -> Result<()> {
        let depends_on_json = serde_json::to_string(&task.depends_on)
            .map_err(|e| NyayaError::Cache(format!("serialize depends_on: {e}")))?;

        self.conn
            .execute(
                "INSERT OR REPLACE INTO pea_tasks
                 (id, objective_id, desire_id, parent_task_id, description,
                  task_type, status, ordering, depends_on_json,
                  capability_required, result_json, pramana_record_json,
                  retry_count, max_retries, created_at, completed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                rusqlite::params![
                    task.id,
                    task.objective_id,
                    task.desire_id,
                    task.parent_task_id,
                    task.description,
                    task.task_type.to_string(),
                    task.status.to_string(),
                    task.ordering,
                    depends_on_json,
                    task.capability_required,
                    task.result_json,
                    task.pramana_record_json,
                    task.retry_count,
                    task.max_retries,
                    task.created_at,
                    task.completed_at,
                ],
            )
            .map_err(|e| NyayaError::Cache(format!("save task: {e}")))?;
        Ok(())
    }

    pub fn list_tasks(&self, objective_id: &str) -> Result<Vec<PeaTask>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, objective_id, desire_id, parent_task_id, description,
                        task_type, status, ordering, depends_on_json,
                        capability_required, result_json, pramana_record_json,
                        retry_count, max_retries, created_at, completed_at
                 FROM pea_tasks WHERE objective_id = ?1 ORDER BY ordering",
            )
            .map_err(|e| NyayaError::Cache(format!("prepare list_tasks: {e}")))?;

        let rows = stmt
            .query_map(rusqlite::params![objective_id], row_to_task)
            .map_err(|e| NyayaError::Cache(format!("query list_tasks: {e}")))?;

        let mut result = Vec::new();
        for r in rows {
            result.push(r.map_err(|e| NyayaError::Cache(format!("read task row: {e}")))?);
        }
        Ok(result)
    }

    pub fn list_ready_tasks(&self, objective_id: &str) -> Result<Vec<PeaTask>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, objective_id, desire_id, parent_task_id, description,
                        task_type, status, ordering, depends_on_json,
                        capability_required, result_json, pramana_record_json,
                        retry_count, max_retries, created_at, completed_at
                 FROM pea_tasks WHERE objective_id = ?1 AND status = 'ready' ORDER BY ordering",
            )
            .map_err(|e| NyayaError::Cache(format!("prepare list_ready: {e}")))?;

        let rows = stmt
            .query_map(rusqlite::params![objective_id], row_to_task)
            .map_err(|e| NyayaError::Cache(format!("query list_ready: {e}")))?;

        let mut result = Vec::new();
        for r in rows {
            result.push(r.map_err(|e| NyayaError::Cache(format!("read ready row: {e}")))?);
        }
        Ok(result)
    }

    /// Reset any tasks stuck in `running` back to `ready`.
    ///
    /// On daemon restart no task can actually be running, so we recover them
    /// to avoid permanently blocking downstream dependents.  Returns the
    /// number of tasks recovered.
    pub fn recover_running_tasks(&self) -> Result<usize> {
        let count = self
            .conn
            .execute(
                "UPDATE pea_tasks SET status = 'ready' WHERE status = 'running'",
                [],
            )
            .map_err(|e| NyayaError::Cache(format!("recover running tasks: {e}")))?;
        Ok(count)
    }

    pub fn update_task_status(&self, id: &str, status: &TaskStatus) -> Result<()> {
        self.conn
            .execute(
                "UPDATE pea_tasks SET status = ?1 WHERE id = ?2",
                rusqlite::params![status.to_string(), id],
            )
            .map_err(|e| NyayaError::Cache(format!("update task status: {e}")))?;
        Ok(())
    }

    pub fn update_task_result(
        &self,
        id: &str,
        result_json: &str,
        pramana_json: &str,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE pea_tasks SET result_json = ?1, pramana_record_json = ?2 WHERE id = ?3",
                rusqlite::params![result_json, pramana_json, id],
            )
            .map_err(|e| NyayaError::Cache(format!("update task result: {e}")))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

struct ObjectiveRow {
    id: String,
    description: String,
    status: String,
    created_at: u64,
    updated_at: u64,
    beliefs_json: String,
    budget_usd: f64,
    spent_usd: f64,
    budget_strategy: String,
    progress_score: f64,
    milestones_json: String,
    heartbeat_interval_secs: u64,
    last_tick_at: u64,
}

fn row_to_objective(r: ObjectiveRow) -> Result<Objective> {
    let status: ObjectiveStatus = serde_json::from_value(serde_json::Value::String(r.status))
        .map_err(|e| NyayaError::Cache(format!("deserialize objective status: {e}")))?;
    let beliefs: BeliefStore = serde_json::from_str(&r.beliefs_json)
        .map_err(|e| NyayaError::Cache(format!("deserialize beliefs: {e}")))?;
    let milestones: Vec<Milestone> = serde_json::from_str(&r.milestones_json)
        .map_err(|e| NyayaError::Cache(format!("deserialize milestones: {e}")))?;
    let budget_strategy: BudgetStrategy =
        serde_json::from_value(serde_json::Value::String(r.budget_strategy))
            .map_err(|e| NyayaError::Cache(format!("deserialize budget_strategy: {e}")))?;

    Ok(Objective {
        id: r.id,
        description: r.description,
        status,
        created_at: r.created_at,
        updated_at: r.updated_at,
        beliefs,
        budget_usd: r.budget_usd,
        spent_usd: r.spent_usd,
        budget_strategy,
        progress_score: r.progress_score,
        milestones,
        heartbeat_interval_secs: r.heartbeat_interval_secs,
        last_tick_at: r.last_tick_at,
    })
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<PeaTask> {
    let task_type_str: String = row.get(5)?;
    let status_str: String = row.get(6)?;
    let depends_on_str: String = row.get(8)?;

    let task_type: TaskType = serde_json::from_value(serde_json::Value::String(task_type_str))
        .unwrap_or(TaskType::Primitive);
    let status: TaskStatus = serde_json::from_value(serde_json::Value::String(status_str))
        .unwrap_or(TaskStatus::Pending);
    let depends_on: Vec<String> = serde_json::from_str(&depends_on_str).unwrap_or_default();

    Ok(PeaTask {
        id: row.get(0)?,
        objective_id: row.get(1)?,
        desire_id: row.get(2)?,
        parent_task_id: row.get(3)?,
        description: row.get(4)?,
        task_type,
        status,
        ordering: row.get(7)?,
        depends_on,
        capability_required: row.get(9)?,
        result_json: row.get(10)?,
        pramana_record_json: row.get(11)?,
        retry_count: row.get(12)?,
        max_retries: row.get(13)?,
        created_at: row.get(14)?,
        completed_at: row.get(15)?,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_belief_store_set_get() {
        let mut bs = BeliefStore::default();
        bs.set("weather", serde_json::json!("sunny"), 0.9);
        assert_eq!(bs.get("weather"), Some(&serde_json::json!("sunny")));
        assert!((bs.confidence_of("weather") - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn test_belief_store_confidence_default() {
        let bs = BeliefStore::default();
        assert!((bs.confidence_of("unknown_key") - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_objective_status_display() {
        assert_eq!(ObjectiveStatus::Active.to_string(), "active");
        assert_eq!(ObjectiveStatus::Paused.to_string(), "paused");
        assert_eq!(ObjectiveStatus::Completed.to_string(), "completed");
        assert_eq!(ObjectiveStatus::Failed.to_string(), "failed");
    }

    #[test]
    fn test_task_status_display() {
        assert_eq!(TaskStatus::Pending.to_string(), "pending");
        assert_eq!(TaskStatus::Ready.to_string(), "ready");
        assert_eq!(TaskStatus::Running.to_string(), "running");
        assert_eq!(TaskStatus::Completed.to_string(), "completed");
        assert_eq!(TaskStatus::Failed.to_string(), "failed");
        assert_eq!(TaskStatus::Blocked.to_string(), "blocked");
        assert_eq!(TaskStatus::Skipped.to_string(), "skipped");
    }

    #[test]
    fn test_budget_strategy_default() {
        assert_eq!(BudgetStrategy::default(), BudgetStrategy::Adaptive);
    }

    #[test]
    fn test_objective_store_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = ObjectiveStore::open(&db_path).unwrap();

        let obj = Objective {
            id: "obj-1".into(),
            description: "Test objective".into(),
            status: ObjectiveStatus::Active,
            created_at: 1000,
            updated_at: 1001,
            beliefs: BeliefStore::default(),
            budget_usd: 10.0,
            spent_usd: 2.5,
            budget_strategy: BudgetStrategy::Conservative,
            progress_score: 0.42,
            milestones: vec![Milestone {
                description: "First milestone".into(),
                criteria: "Done when X".into(),
                achieved: false,
            }],
            heartbeat_interval_secs: 60,
            last_tick_at: 999,
        };

        store.save_objective(&obj).unwrap();
        let loaded = store.load_objective("obj-1").unwrap().unwrap();

        assert_eq!(loaded.id, "obj-1");
        assert_eq!(loaded.description, "Test objective");
        assert_eq!(loaded.status, ObjectiveStatus::Active);
        assert_eq!(loaded.created_at, 1000);
        assert_eq!(loaded.updated_at, 1001);
        assert!((loaded.budget_usd - 10.0).abs() < f64::EPSILON);
        assert!((loaded.spent_usd - 2.5).abs() < f64::EPSILON);
        assert_eq!(loaded.budget_strategy, BudgetStrategy::Conservative);
        assert!((loaded.progress_score - 0.42).abs() < f64::EPSILON);
        assert_eq!(loaded.milestones.len(), 1);
        assert_eq!(loaded.milestones[0].description, "First milestone");
        assert_eq!(loaded.heartbeat_interval_secs, 60);
        assert_eq!(loaded.last_tick_at, 999);
    }

    #[test]
    fn test_objective_store_list_active() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = ObjectiveStore::open(&db_path).unwrap();

        let make_obj = |id: &str, status: ObjectiveStatus| Objective {
            id: id.into(),
            description: format!("Objective {id}"),
            status,
            created_at: 100,
            updated_at: 100,
            beliefs: BeliefStore::default(),
            budget_usd: 5.0,
            spent_usd: 0.0,
            budget_strategy: BudgetStrategy::default(),
            progress_score: 0.0,
            milestones: vec![],
            heartbeat_interval_secs: 300,
            last_tick_at: 0,
        };

        store
            .save_objective(&make_obj("a", ObjectiveStatus::Active))
            .unwrap();
        store
            .save_objective(&make_obj("b", ObjectiveStatus::Active))
            .unwrap();
        store
            .save_objective(&make_obj("c", ObjectiveStatus::Paused))
            .unwrap();

        let active = store.list_active_objectives().unwrap();
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn test_task_store_list_ready() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = ObjectiveStore::open(&db_path).unwrap();

        let make_task = |id: &str, status: TaskStatus, deps: Vec<String>| PeaTask {
            id: id.into(),
            objective_id: "obj-1".into(),
            desire_id: "d-1".into(),
            parent_task_id: None,
            description: format!("Task {id}"),
            task_type: TaskType::Primitive,
            status,
            ordering: 0,
            depends_on: deps,
            capability_required: None,
            result_json: None,
            pramana_record_json: None,
            retry_count: 0,
            max_retries: 3,
            created_at: 100,
            completed_at: None,
        };

        store
            .save_task(&make_task("t1", TaskStatus::Ready, vec![]))
            .unwrap();
        store
            .save_task(&make_task("t2", TaskStatus::Pending, vec!["t1".into()]))
            .unwrap();
        store
            .save_task(&make_task("t3", TaskStatus::Ready, vec![]))
            .unwrap();
        store
            .save_task(&make_task("t4", TaskStatus::Running, vec![]))
            .unwrap();

        let ready = store.list_ready_tasks("obj-1").unwrap();
        assert_eq!(ready.len(), 2);
        let ids: Vec<&str> = ready.iter().map(|t| t.id.as_str()).collect();
        assert!(ids.contains(&"t1"));
        assert!(ids.contains(&"t3"));
    }
}
