use std::collections::HashMap;
use std::path::Path;

use rusqlite::OptionalExtension;

use crate::chain::workflow::{
    WaitSpec, WorkflowCursor, WorkflowDef, WorkflowInstance, WorkflowStatus,
};
use crate::core::error::{NyayaError, Result};

/// SQLite-backed persistence for workflow definitions and instances.
pub struct WorkflowStore {
    conn: rusqlite::Connection,
}

impl WorkflowStore {
    /// Open (or create) the workflow database at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = rusqlite::Connection::open(path)
            .map_err(|e| NyayaError::Cache(format!("Workflow DB open failed: {}", e)))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS workflow_defs (
                workflow_id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                definition_json TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS workflow_instances (
                instance_id TEXT PRIMARY KEY,
                workflow_id TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'created',
                params_json TEXT NOT NULL DEFAULT '{}',
                outputs_json TEXT NOT NULL DEFAULT '{}',
                cursor_json TEXT NOT NULL,
                correlation_value TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                wait_started_at INTEGER,
                wait_deadline INTEGER,
                wait_spec_json TEXT,
                error TEXT,
                execution_ms INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_wfi_correlation
                ON workflow_instances(workflow_id, correlation_value)
                WHERE status = 'waiting';

            CREATE INDEX IF NOT EXISTS idx_wfi_deadline
                ON workflow_instances(wait_deadline)
                WHERE status = 'waiting';

            CREATE TABLE IF NOT EXISTS compensation_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                instance_id TEXT NOT NULL,
                compensated_node TEXT NOT NULL,
                ability TEXT NOT NULL,
                result TEXT,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS workflow_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                instance_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                node_id TEXT,
                detail_json TEXT,
                created_at INTEGER NOT NULL
            );",
        )
        .map_err(|e| NyayaError::Cache(format!("Workflow table creation failed: {}", e)))?;

        // Migration: add style_vars_json column if not present
        let _ = conn.execute(
            "ALTER TABLE workflow_instances ADD COLUMN style_vars_json TEXT",
            [],
        );

        // Migration: add kb_context_text column if not present
        let _ = conn.execute(
            "ALTER TABLE workflow_instances ADD COLUMN kb_context_text TEXT",
            [],
        );

        // Migration: add effective_permissions_json column if not present
        let _ = conn.execute(
            "ALTER TABLE workflow_instances ADD COLUMN effective_permissions_json TEXT",
            [],
        );

        // Migration: add compensation state columns if not present
        let _ = conn.execute("ALTER TABLE workflow_instances ADD COLUMN compensation_triggered INTEGER NOT NULL DEFAULT 0", []);
        let _ = conn.execute(
            "ALTER TABLE workflow_instances ADD COLUMN compensated_nodes_json TEXT",
            [],
        );

        Ok(Self { conn })
    }

    // -----------------------------------------------------------------------
    // Definitions
    // -----------------------------------------------------------------------

    /// Store (or replace) a workflow definition.
    pub fn store_def(&self, def: &WorkflowDef) -> Result<()> {
        let json = serde_json::to_string(def)?;
        let now = chrono::Utc::now().timestamp();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO workflow_defs (workflow_id, name, definition_json, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![def.id, def.name, json, now],
            )
            .map_err(|e| NyayaError::Cache(format!("store_def failed: {}", e)))?;
        Ok(())
    }

    /// Retrieve a workflow definition by ID.
    pub fn get_def(&self, workflow_id: &str) -> Result<Option<WorkflowDef>> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT definition_json FROM workflow_defs WHERE workflow_id = ?1",
                rusqlite::params![workflow_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| NyayaError::Cache(format!("get_def failed: {}", e)))?;

        match json {
            Some(j) => Ok(Some(serde_json::from_str(&j)?)),
            None => Ok(None),
        }
    }

    /// List all workflow definitions as (id, name) pairs.
    pub fn list_defs(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT workflow_id, name FROM workflow_defs ORDER BY created_at DESC")
            .map_err(|e| NyayaError::Cache(format!("list_defs failed: {}", e)))?;

        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| NyayaError::Cache(format!("list_defs query failed: {}", e)))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(rows)
    }

    // -----------------------------------------------------------------------
    // Instances
    // -----------------------------------------------------------------------

    /// Create a new workflow instance with status `Created`.
    /// Returns the generated instance ID.
    pub fn create_instance(
        &self,
        workflow_id: &str,
        params: &HashMap<String, String>,
        correlation_value: Option<&str>,
    ) -> Result<String> {
        let instance_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();
        let params_json = serde_json::to_string(params)?;
        let outputs_json = serde_json::to_string(&HashMap::<String, String>::new())?;
        let cursor = WorkflowCursor::start();
        let cursor_json = serde_json::to_string(&cursor)?;

        self.conn
            .execute(
                "INSERT INTO workflow_instances
                    (instance_id, workflow_id, status, params_json, outputs_json,
                     cursor_json, correlation_value, created_at, updated_at, execution_ms)
                 VALUES (?1, ?2, 'created', ?3, ?4, ?5, ?6, ?7, ?7, 0)",
                rusqlite::params![
                    instance_id,
                    workflow_id,
                    params_json,
                    outputs_json,
                    cursor_json,
                    correlation_value,
                    now,
                ],
            )
            .map_err(|e| NyayaError::Cache(format!("create_instance failed: {}", e)))?;

        Ok(instance_id)
    }

    /// Retrieve a workflow instance by its ID.
    pub fn get_instance(&self, instance_id: &str) -> Result<Option<WorkflowInstance>> {
        self.conn
            .query_row(
                "SELECT instance_id, workflow_id, status, params_json, outputs_json,
                        cursor_json, correlation_value, created_at, updated_at,
                        wait_started_at, wait_deadline, wait_spec_json, error, execution_ms,
                        style_vars_json, kb_context_text, effective_permissions_json,
                        compensation_triggered, compensated_nodes_json
                 FROM workflow_instances WHERE instance_id = ?1",
                rusqlite::params![instance_id],
                |row| Ok(row_to_instance(row)),
            )
            .optional()
            .map_err(|e| NyayaError::Cache(format!("get_instance failed: {}", e)))?
            .transpose()
    }

    /// Update all mutable fields of an existing instance.
    pub fn update_instance(&self, inst: &WorkflowInstance) -> Result<()> {
        let outputs_json = serde_json::to_string(&inst.outputs)?;
        let cursor_json = serde_json::to_string(&inst.cursor)?;
        let wait_spec_json = match &inst.wait_spec {
            Some(ws) => Some(serde_json::to_string(ws)?),
            None => None,
        };
        let style_vars_json = match &inst.style_vars {
            Some(sv) => Some(serde_json::to_string(sv)?),
            None => None,
        };
        let effective_permissions_json = match &inst.effective_permissions {
            Some(ep) => Some(serde_json::to_string(ep)?),
            None => None,
        };
        let compensated_nodes_json = if inst.compensated_nodes.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&inst.compensated_nodes)?)
        };
        let now = chrono::Utc::now().timestamp();

        self.conn
            .execute(
                "UPDATE workflow_instances SET
                    status = ?1,
                    outputs_json = ?2,
                    cursor_json = ?3,
                    correlation_value = ?4,
                    updated_at = ?5,
                    wait_started_at = ?6,
                    wait_deadline = ?7,
                    wait_spec_json = ?8,
                    error = ?9,
                    execution_ms = ?10,
                    style_vars_json = ?11,
                    kb_context_text = ?12,
                    effective_permissions_json = ?13,
                    compensation_triggered = ?14,
                    compensated_nodes_json = ?15
                 WHERE instance_id = ?16",
                rusqlite::params![
                    inst.status.to_string(),
                    outputs_json,
                    cursor_json,
                    inst.correlation_value,
                    now,
                    inst.wait_started_at,
                    inst.wait_deadline,
                    wait_spec_json,
                    inst.error,
                    inst.execution_ms,
                    style_vars_json,
                    inst.kb_context,
                    effective_permissions_json,
                    inst.compensation_triggered,
                    compensated_nodes_json,
                    inst.instance_id,
                ],
            )
            .map_err(|e| NyayaError::Cache(format!("update_instance failed: {}", e)))?;

        Ok(())
    }

    /// Find a waiting instance by workflow ID and correlation value.
    pub fn find_waiting_by_correlation(
        &self,
        workflow_id: &str,
        correlation_value: &str,
    ) -> Result<Option<WorkflowInstance>> {
        self.conn
            .query_row(
                "SELECT instance_id, workflow_id, status, params_json, outputs_json,
                        cursor_json, correlation_value, created_at, updated_at,
                        wait_started_at, wait_deadline, wait_spec_json, error, execution_ms,
                        style_vars_json, kb_context_text, effective_permissions_json,
                        compensation_triggered, compensated_nodes_json
                 FROM workflow_instances
                 WHERE workflow_id = ?1 AND correlation_value = ?2 AND status = 'waiting'
                 LIMIT 1",
                rusqlite::params![workflow_id, correlation_value],
                |row| Ok(row_to_instance(row)),
            )
            .optional()
            .map_err(|e| NyayaError::Cache(format!("find_waiting_by_correlation failed: {}", e)))?
            .transpose()
    }

    /// Find all waiting instances whose wait deadline has expired.
    pub fn find_expired_waits(&self, now: i64) -> Result<Vec<WorkflowInstance>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT instance_id, workflow_id, status, params_json, outputs_json,
                        cursor_json, correlation_value, created_at, updated_at,
                        wait_started_at, wait_deadline, wait_spec_json, error, execution_ms,
                        style_vars_json, kb_context_text, effective_permissions_json,
                        compensation_triggered, compensated_nodes_json
                 FROM workflow_instances
                 WHERE status = 'waiting' AND wait_deadline IS NOT NULL AND wait_deadline < ?1",
            )
            .map_err(|e| NyayaError::Cache(format!("find_expired_waits prepare failed: {}", e)))?;

        let rows = stmt
            .query_map(rusqlite::params![now], |row| Ok(row_to_instance(row)))
            .map_err(|e| NyayaError::Cache(format!("find_expired_waits query failed: {}", e)))?;

        let mut results = Vec::new();
        for r in rows {
            let inst =
                r.map_err(|e| NyayaError::Cache(format!("find_expired_waits row failed: {}", e)))?;
            results.push(inst?);
        }
        Ok(results)
    }

    /// Find waiting instances where a poll is due (next_poll_at <= now).
    pub fn find_due_polls(&self, now: i64) -> Result<Vec<WorkflowInstance>> {
        // We select all waiting instances that have a wait_spec_json containing poll data.
        // The next_poll_at is stored inside the JSON, so we use json_extract.
        let mut stmt = self
            .conn
            .prepare(
                "SELECT instance_id, workflow_id, status, params_json, outputs_json,
                        cursor_json, correlation_value, created_at, updated_at,
                        wait_started_at, wait_deadline, wait_spec_json, error, execution_ms,
                        style_vars_json, kb_context_text, effective_permissions_json,
                        compensation_triggered, compensated_nodes_json
                 FROM workflow_instances
                 WHERE status = 'waiting'
                   AND wait_spec_json IS NOT NULL
                   AND json_extract(wait_spec_json, '$.kind') = 'poll'
                   AND json_extract(wait_spec_json, '$.next_poll_at') <= ?1",
            )
            .map_err(|e| NyayaError::Cache(format!("find_due_polls prepare failed: {}", e)))?;

        let rows = stmt
            .query_map(rusqlite::params![now], |row| Ok(row_to_instance(row)))
            .map_err(|e| NyayaError::Cache(format!("find_due_polls query failed: {}", e)))?;

        let mut results = Vec::new();
        for r in rows {
            let inst =
                r.map_err(|e| NyayaError::Cache(format!("find_due_polls row failed: {}", e)))?;
            results.push(inst?);
        }
        Ok(results)
    }

    /// Log an event for auditing / debugging.
    pub fn log_event(
        &self,
        instance_id: &str,
        event_type: &str,
        node_id: Option<&str>,
        detail: Option<&str>,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn
            .execute(
                "INSERT INTO workflow_events (instance_id, event_type, node_id, detail_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![instance_id, event_type, node_id, detail, now],
            )
            .map_err(|e| NyayaError::Cache(format!("log_event failed: {}", e)))?;
        Ok(())
    }

    /// Log a compensation action.
    pub fn log_compensation(
        &self,
        instance_id: &str,
        compensated_node: &str,
        ability: &str,
        result: Option<&str>,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn
            .execute(
                "INSERT INTO compensation_log (instance_id, compensated_node, ability, result, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![instance_id, compensated_node, ability, result, now],
            )
            .map_err(|e| NyayaError::Cache(format!("Log compensation: {}", e)))?;
        Ok(())
    }

    /// List all instances for a given workflow definition, ordered by most recent first.
    pub fn list_instances_for_workflow(&self, workflow_id: &str) -> Result<Vec<WorkflowInstance>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT instance_id, workflow_id, status, params_json, outputs_json,
                        cursor_json, correlation_value, created_at, updated_at,
                        wait_started_at, wait_deadline, wait_spec_json, error, execution_ms,
                        style_vars_json, kb_context_text, effective_permissions_json,
                        compensation_triggered, compensated_nodes_json
                 FROM workflow_instances
                 WHERE workflow_id = ?1
                 ORDER BY updated_at DESC",
            )
            .map_err(|e| {
                NyayaError::Cache(format!("list_instances_for_workflow prepare failed: {}", e))
            })?;

        let rows = stmt
            .query_map(rusqlite::params![workflow_id], |row| {
                Ok(row_to_instance(row))
            })
            .map_err(|e| {
                NyayaError::Cache(format!("list_instances_for_workflow query failed: {}", e))
            })?;

        let mut results = Vec::new();
        for r in rows {
            let inst = r.map_err(|e| {
                NyayaError::Cache(format!("list_instances_for_workflow row failed: {}", e))
            })?;
            results.push(inst?);
        }
        Ok(results)
    }

    /// Count non-terminal (active) instances of a given workflow.
    pub fn count_active_instances(&self, workflow_id: &str) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM workflow_instances
                 WHERE workflow_id = ?1
                   AND status NOT IN ('completed', 'failed', 'cancelled', 'timed_out', 'compensated')",
                rusqlite::params![workflow_id],
                |row| row.get(0),
            )
            .map_err(|e| NyayaError::Cache(format!("count_active_instances failed: {}", e)))?;
        Ok(count as usize)
    }
}

/// Parse a row into a WorkflowInstance.  Called from within a query_row closure
/// which already returns `rusqlite::Result`, so we do the JSON parsing here
/// and return our own Result.
fn row_to_instance(row: &rusqlite::Row<'_>) -> Result<WorkflowInstance> {
    let status_str: String = row.get(2).map_err(|e| NyayaError::Cache(e.to_string()))?;
    let params_json: String = row.get(3).map_err(|e| NyayaError::Cache(e.to_string()))?;
    let outputs_json: String = row.get(4).map_err(|e| NyayaError::Cache(e.to_string()))?;
    let cursor_json: String = row.get(5).map_err(|e| NyayaError::Cache(e.to_string()))?;
    let wait_spec_json: Option<String> =
        row.get(11).map_err(|e| NyayaError::Cache(e.to_string()))?;

    let status: WorkflowStatus = status_str.parse()?;
    let params: HashMap<String, String> = serde_json::from_str(&params_json)?;
    let outputs: HashMap<String, String> = serde_json::from_str(&outputs_json)?;
    let cursor: WorkflowCursor = serde_json::from_str(&cursor_json)?;
    let wait_spec: Option<WaitSpec> = match wait_spec_json {
        Some(ref j) if !j.is_empty() => Some(serde_json::from_str(j)?),
        _ => None,
    };

    Ok(WorkflowInstance {
        instance_id: row.get(0).map_err(|e| NyayaError::Cache(e.to_string()))?,
        workflow_id: row.get(1).map_err(|e| NyayaError::Cache(e.to_string()))?,
        status,
        params,
        outputs,
        cursor,
        correlation_value: row.get(6).map_err(|e| NyayaError::Cache(e.to_string()))?,
        created_at: row.get(7).map_err(|e| NyayaError::Cache(e.to_string()))?,
        updated_at: row.get(8).map_err(|e| NyayaError::Cache(e.to_string()))?,
        wait_started_at: row.get(9).map_err(|e| NyayaError::Cache(e.to_string()))?,
        wait_deadline: row.get(10).map_err(|e| NyayaError::Cache(e.to_string()))?,
        wait_spec,
        error: row.get(12).map_err(|e| NyayaError::Cache(e.to_string()))?,
        execution_ms: row.get(13).map_err(|e| NyayaError::Cache(e.to_string()))?,
        receipt_ids: Vec::new(), // Not stored in DB -- reconstructed from events if needed
        style_vars: {
            let style_vars_json: Option<String> =
                row.get(14).map_err(|e| NyayaError::Cache(e.to_string()))?;
            match style_vars_json {
                Some(ref j) if !j.is_empty() => Some(serde_json::from_str(j)?),
                _ => None,
            }
        },
        kb_context: {
            let kb_context_text: Option<String> =
                row.get(15).map_err(|e| NyayaError::Cache(e.to_string()))?;
            kb_context_text
        },
        effective_permissions: {
            let ep_json: Option<String> =
                row.get(16).map_err(|e| NyayaError::Cache(e.to_string()))?;
            match ep_json {
                Some(ref j) if !j.is_empty() => Some(serde_json::from_str(j)?),
                _ => None,
            }
        },
        compensation_triggered: {
            let v: i64 = row.get(17).map_err(|e| NyayaError::Cache(e.to_string()))?;
            v != 0
        },
        compensated_nodes: {
            let cn_json: Option<String> =
                row.get(18).map_err(|e| NyayaError::Cache(e.to_string()))?;
            match cn_json {
                Some(ref j) if !j.is_empty() => serde_json::from_str(j)?,
                _ => vec![],
            }
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::dsl::{ConditionOp, ParamDef, ParamType, StepCondition};
    use crate::chain::workflow::*;

    fn test_def() -> WorkflowDef {
        WorkflowDef {
            id: "wf_order".into(),
            name: "Order Workflow".into(),
            description: "Process orders".into(),
            params: vec![ParamDef {
                name: "order_id".into(),
                param_type: ParamType::Text,
                description: "Order ID".into(),
                required: true,
                default: None,
            }],
            nodes: vec![WorkflowNode::Action(ActionNode {
                id: "validate".into(),
                ability: "order.validate".into(),
                args: HashMap::from([("id".into(), "{{order_id}}".into())]),
                output_key: Some("result".into()),
                condition: None,
                on_failure: None,
            })],
            global_timeout_secs: 3600,
            max_instances: 10,
            correlation_key: Some("{{order_id}}".into()),
            style: None,
            kb_context: None,
            channel_permissions: None,
        }
    }

    fn open_temp_store() -> (WorkflowStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkflowStore::open(&dir.path().join("workflows.db")).unwrap();
        (store, dir)
    }

    #[test]
    fn test_store_and_get_def() {
        let (store, _dir) = open_temp_store();
        let def = test_def();

        store.store_def(&def).unwrap();
        let loaded = store.get_def("wf_order").unwrap().unwrap();
        assert_eq!(loaded.id, "wf_order");
        assert_eq!(loaded.name, "Order Workflow");
        assert_eq!(loaded.nodes.len(), 1);
        assert_eq!(loaded.global_timeout_secs, 3600);
    }

    #[test]
    fn test_get_def_missing() {
        let (store, _dir) = open_temp_store();
        assert!(store.get_def("nonexistent").unwrap().is_none());
    }

    #[test]
    fn test_list_defs() {
        let (store, _dir) = open_temp_store();
        store.store_def(&test_def()).unwrap();

        let mut def2 = test_def();
        def2.id = "wf_invoice".into();
        def2.name = "Invoice Workflow".into();
        store.store_def(&def2).unwrap();

        let list = store.list_defs().unwrap();
        assert_eq!(list.len(), 2);
        // Both should be present
        let ids: Vec<&str> = list.iter().map(|(id, _)| id.as_str()).collect();
        assert!(ids.contains(&"wf_order"));
        assert!(ids.contains(&"wf_invoice"));
    }

    #[test]
    fn test_create_and_get_instance() {
        let (store, _dir) = open_temp_store();
        store.store_def(&test_def()).unwrap();

        let params = HashMap::from([("order_id".into(), "ORD-001".into())]);
        let instance_id = store
            .create_instance("wf_order", &params, Some("ORD-001"))
            .unwrap();

        let inst = store.get_instance(&instance_id).unwrap().unwrap();
        assert_eq!(inst.workflow_id, "wf_order");
        assert_eq!(inst.status, WorkflowStatus::Created);
        assert_eq!(inst.params.get("order_id").unwrap(), "ORD-001");
        assert_eq!(inst.correlation_value, Some("ORD-001".into()));
        assert_eq!(inst.cursor.node_index, 0);
        assert!(inst.outputs.is_empty());
        assert!(inst.error.is_none());
        assert_eq!(inst.execution_ms, 0);
    }

    #[test]
    fn test_update_instance() {
        let (store, _dir) = open_temp_store();
        store.store_def(&test_def()).unwrap();

        let params = HashMap::from([("order_id".into(), "ORD-002".into())]);
        let instance_id = store
            .create_instance("wf_order", &params, Some("ORD-002"))
            .unwrap();

        let mut inst = store.get_instance(&instance_id).unwrap().unwrap();
        inst.status = WorkflowStatus::Running;
        inst.outputs.insert("result".into(), "validated".into());
        inst.cursor.node_index = 1;
        inst.execution_ms = 150;
        store.update_instance(&inst).unwrap();

        let updated = store.get_instance(&instance_id).unwrap().unwrap();
        assert_eq!(updated.status, WorkflowStatus::Running);
        assert_eq!(updated.outputs.get("result").unwrap(), "validated");
        assert_eq!(updated.cursor.node_index, 1);
        assert_eq!(updated.execution_ms, 150);
    }

    #[test]
    fn test_find_waiting_by_correlation() {
        let (store, _dir) = open_temp_store();
        store.store_def(&test_def()).unwrap();

        let params = HashMap::from([("order_id".into(), "ORD-003".into())]);
        let instance_id = store
            .create_instance("wf_order", &params, Some("ORD-003"))
            .unwrap();

        // Should not find it yet -- status is Created
        assert!(store
            .find_waiting_by_correlation("wf_order", "ORD-003")
            .unwrap()
            .is_none());

        // Update to Waiting
        let mut inst = store.get_instance(&instance_id).unwrap().unwrap();
        inst.status = WorkflowStatus::Waiting;
        inst.wait_spec = Some(WaitSpec::Event {
            event_type: "payment.confirmed".into(),
            filter: None,
            output_key: Some("payment".into()),
            on_timeout: "fail".into(),
        });
        store.update_instance(&inst).unwrap();

        // Now it should be found
        let found = store
            .find_waiting_by_correlation("wf_order", "ORD-003")
            .unwrap()
            .unwrap();
        assert_eq!(found.instance_id, instance_id);
    }

    #[test]
    fn test_find_expired_waits() {
        let (store, _dir) = open_temp_store();
        store.store_def(&test_def()).unwrap();

        let params = HashMap::new();
        let id1 = store.create_instance("wf_order", &params, None).unwrap();
        let id2 = store.create_instance("wf_order", &params, None).unwrap();

        // Set id1 to waiting with deadline in the past
        let mut inst1 = store.get_instance(&id1).unwrap().unwrap();
        inst1.status = WorkflowStatus::Waiting;
        inst1.wait_deadline = Some(1000);
        inst1.wait_spec = Some(WaitSpec::Delay { resume_at: 1000 });
        store.update_instance(&inst1).unwrap();

        // Set id2 to waiting with deadline in the future
        let mut inst2 = store.get_instance(&id2).unwrap().unwrap();
        inst2.status = WorkflowStatus::Waiting;
        inst2.wait_deadline = Some(9999999999);
        inst2.wait_spec = Some(WaitSpec::Delay {
            resume_at: 9999999999,
        });
        store.update_instance(&inst2).unwrap();

        // now = 2000 -- only id1 should be expired
        let expired = store.find_expired_waits(2000).unwrap();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].instance_id, id1);
    }

    #[test]
    fn test_find_due_polls() {
        let (store, _dir) = open_temp_store();
        store.store_def(&test_def()).unwrap();

        let params = HashMap::new();
        let id1 = store.create_instance("wf_order", &params, None).unwrap();

        let mut inst = store.get_instance(&id1).unwrap().unwrap();
        inst.status = WorkflowStatus::Waiting;
        inst.wait_spec = Some(WaitSpec::Poll {
            ability: "order.status".into(),
            args: HashMap::new(),
            output_key: Some("status".into()),
            until: StepCondition {
                ref_key: "status".into(),
                op: ConditionOp::Equals,
                value: "shipped".into(),
            },
            poll_interval_secs: 60,
            next_poll_at: 5000,
            on_timeout: "fail".into(),
        });
        inst.wait_deadline = Some(100000);
        store.update_instance(&inst).unwrap();

        // now = 4000 -- not due yet
        let due = store.find_due_polls(4000).unwrap();
        assert_eq!(due.len(), 0);

        // now = 5001 -- due
        let due = store.find_due_polls(5001).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].instance_id, id1);
    }

    #[test]
    fn test_log_event() {
        let (store, _dir) = open_temp_store();
        store.store_def(&test_def()).unwrap();

        let params = HashMap::new();
        let id = store.create_instance("wf_order", &params, None).unwrap();

        store
            .log_event(&id, "node_started", Some("validate"), Some("{\"t\":1}"))
            .unwrap();
        store
            .log_event(&id, "node_completed", Some("validate"), None)
            .unwrap();

        // Verify events exist (query directly)
        let count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM workflow_events WHERE instance_id = ?1",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_count_active_instances() {
        let (store, _dir) = open_temp_store();
        store.store_def(&test_def()).unwrap();

        let params = HashMap::new();
        let id1 = store.create_instance("wf_order", &params, None).unwrap();
        let id2 = store.create_instance("wf_order", &params, None).unwrap();
        let id3 = store.create_instance("wf_order", &params, None).unwrap();

        assert_eq!(store.count_active_instances("wf_order").unwrap(), 3);

        // Complete one
        let mut inst1 = store.get_instance(&id1).unwrap().unwrap();
        inst1.status = WorkflowStatus::Completed;
        store.update_instance(&inst1).unwrap();

        assert_eq!(store.count_active_instances("wf_order").unwrap(), 2);

        // Fail another
        let mut inst2 = store.get_instance(&id2).unwrap().unwrap();
        inst2.status = WorkflowStatus::Failed;
        inst2.error = Some("something went wrong".into());
        store.update_instance(&inst2).unwrap();

        assert_eq!(store.count_active_instances("wf_order").unwrap(), 1);

        // Cancel the last one
        let mut inst3 = store.get_instance(&id3).unwrap().unwrap();
        inst3.status = WorkflowStatus::Cancelled;
        store.update_instance(&inst3).unwrap();

        assert_eq!(store.count_active_instances("wf_order").unwrap(), 0);
    }

    #[test]
    fn test_instance_with_wait_spec_roundtrip() {
        let (store, _dir) = open_temp_store();
        store.store_def(&test_def()).unwrap();

        let params = HashMap::new();
        let id = store.create_instance("wf_order", &params, None).unwrap();

        let mut inst = store.get_instance(&id).unwrap().unwrap();
        inst.status = WorkflowStatus::Waiting;
        inst.wait_started_at = Some(1000);
        inst.wait_deadline = Some(2000);
        inst.wait_spec = Some(WaitSpec::ParallelJoin {
            parallel_node_id: "p1".into(),
            branch_statuses: HashMap::from([
                ("b1".into(), "completed".into()),
                ("b2".into(), "running".into()),
            ]),
            join: JoinStrategy::All,
        });
        store.update_instance(&inst).unwrap();

        let loaded = store.get_instance(&id).unwrap().unwrap();
        assert_eq!(loaded.status, WorkflowStatus::Waiting);
        assert_eq!(loaded.wait_started_at, Some(1000));
        assert_eq!(loaded.wait_deadline, Some(2000));
        match loaded.wait_spec.unwrap() {
            WaitSpec::ParallelJoin {
                parallel_node_id,
                branch_statuses,
                join,
            } => {
                assert_eq!(parallel_node_id, "p1");
                assert_eq!(branch_statuses.len(), 2);
                assert_eq!(join, JoinStrategy::All);
            }
            _ => panic!("Expected ParallelJoin"),
        }
    }

    #[test]
    fn test_store_def_upsert() {
        let (store, _dir) = open_temp_store();
        let mut def = test_def();
        store.store_def(&def).unwrap();

        def.name = "Updated Order Workflow".into();
        store.store_def(&def).unwrap();

        let loaded = store.get_def("wf_order").unwrap().unwrap();
        assert_eq!(loaded.name, "Updated Order Workflow");

        // Should still be just one
        let list = store.list_defs().unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_workflow_instance_store_roundtrip_with_permissions() {
        use crate::security::channel_permissions::{AccessLevel, ChannelPermissions};

        let (store, _dir) = open_temp_store();
        store.store_def(&test_def()).unwrap();

        let params = HashMap::from([("order_id".into(), "ORD-PERM".into())]);
        let id = store
            .create_instance("wf_order", &params, Some("ORD-PERM"))
            .unwrap();

        // Instance starts with no permissions
        let inst = store.get_instance(&id).unwrap().unwrap();
        assert!(inst.effective_permissions.is_none());

        // Set effective permissions and update
        let mut inst = inst;
        let perms = ChannelPermissions {
            default_access: AccessLevel::Restricted,
            channels: HashMap::new(),
        };
        inst.effective_permissions = Some(perms);
        store.update_instance(&inst).unwrap();

        // Reload and verify
        let loaded = store.get_instance(&id).unwrap().unwrap();
        assert!(loaded.effective_permissions.is_some());
        let loaded_perms = loaded.effective_permissions.unwrap();
        assert_eq!(loaded_perms.default_access, AccessLevel::Restricted);
        assert!(loaded_perms.channels.is_empty());
    }

    #[test]
    fn test_compensation_state_roundtrip() {
        let (store, _dir) = open_temp_store();
        store.store_def(&test_def()).unwrap();

        let params = HashMap::new();
        let id = store.create_instance("wf_order", &params, None).unwrap();

        // Initially compensation is not triggered
        let inst = store.get_instance(&id).unwrap().unwrap();
        assert!(!inst.compensation_triggered);
        assert!(inst.compensated_nodes.is_empty());

        // Trigger compensation and record compensated nodes
        let mut inst = inst;
        inst.compensation_triggered = true;
        inst.compensated_nodes = vec!["node_a".into(), "node_b".into()];
        inst.status = WorkflowStatus::Compensated;
        store.update_instance(&inst).unwrap();

        // Reload and verify
        let loaded = store.get_instance(&id).unwrap().unwrap();
        assert!(loaded.compensation_triggered);
        assert_eq!(loaded.compensated_nodes, vec!["node_a", "node_b"]);
        assert_eq!(loaded.status, WorkflowStatus::Compensated);
    }
}
