//! Workflow Engine Core — executes workflow definitions as stateful, pausable
//! processes with support for delays, external events, polling, branching,
//! and parallel execution (currently sequential).
//!
//! The engine drives a `WorkflowInstance` through its `WorkflowDef` node graph,
//! persisting state to a `WorkflowStore` after every step so that workflows
//! survive process restarts.

use std::collections::HashMap;

use crate::chain::circuit_breaker::{BreakerAction, BreakerRegistry};
use crate::chain::dsl::ChainDef;
#[cfg(test)]
use crate::chain::dsl::StepCondition;
use crate::chain::workflow::{
    ActionNode, BranchNode, ParallelNode, WaitSpec, WorkflowDef, WorkflowInstance, WorkflowNode,
    WorkflowStatus,
};
use crate::chain::workflow_store::WorkflowStore;
use crate::runtime::host_functions::AbilityRegistry;
use crate::runtime::manifest::AgentManifest;
use crate::security::constitution::ConstitutionEnforcer;

/// Maximum steps per single `advance()` call to prevent runaway execution.
const MAX_STEPS_PER_ADVANCE: usize = 50;

/// The workflow engine — orchestrates workflow instance lifecycles.
pub struct WorkflowEngine {
    store: WorkflowStore,
}

impl WorkflowEngine {
    /// Create a new engine backed by the given store.
    pub fn new(store: WorkflowStore) -> Self {
        Self { store }
    }

    /// Access the underlying store (e.g. for registering definitions).
    pub fn store(&self) -> &WorkflowStore {
        &self.store
    }

    // -----------------------------------------------------------------------
    // Start
    // -----------------------------------------------------------------------

    /// Start a new workflow instance.
    ///
    /// Enforces `max_instances`, resolves the correlation key from params,
    /// creates the instance, logs a WorkflowStarted event, and returns the
    /// instance ID.
    pub fn start(
        &self,
        workflow_id: &str,
        params: HashMap<String, String>,
    ) -> Result<String, String> {
        // Load definition
        let def = self
            .store
            .get_def(workflow_id)
            .map_err(|e| format!("Failed to load workflow def: {}", e))?
            .ok_or_else(|| format!("Workflow definition not found: {}", workflow_id))?;

        // Enforce max_instances
        if def.max_instances > 0 {
            let active = self
                .store
                .count_active_instances(workflow_id)
                .map_err(|e| format!("Failed to count active instances: {}", e))?;
            if active as u64 >= def.max_instances {
                return Err(format!(
                    "Max instances ({}) reached for workflow '{}'",
                    def.max_instances, workflow_id
                ));
            }
        }

        // Resolve correlation key
        let correlation_value = def.correlation_key.as_ref().map(|tmpl| {
            let mut resolved = tmpl.clone();
            for (k, v) in &params {
                resolved = resolved.replace(&format!("{{{{{}}}}}", k), v);
            }
            resolved
        });

        // Create instance
        let instance_id = self
            .store
            .create_instance(workflow_id, &params, correlation_value.as_deref())
            .map_err(|e| format!("Failed to create instance: {}", e))?;

        // Resolve style vars if the workflow has a default style
        if let Some(ref style_name) = def.style {
            if let Some(preset) = crate::persona::conditional::parse_builtin_preset(style_name) {
                let profile = crate::persona::conditional::StyleProfile::from_audience(&preset);
                let vars = profile.to_template_vars();
                // Update the instance with style vars
                if let Ok(Some(mut inst)) = self.store.get_instance(&instance_id) {
                    inst.style_vars = Some(vars);
                    let _ = self.store.update_instance(&inst);
                }
            }
        }

        // Resolve effective channel permissions from the workflow definition
        if def.channel_permissions.is_some() {
            if let Ok(Some(mut inst)) = self.store.get_instance(&instance_id) {
                inst.effective_permissions = def.channel_permissions.clone();
                let _ = self.store.update_instance(&inst);
            }
        }

        // Resolve KB context if the workflow has one
        if let Some(ref kb_ctx) = def.kb_context {
            let resolved_content = if kb_ctx.starts_with("file:") {
                let raw_path = std::path::Path::new(&kb_ctx[5..]);
                // Security: reject path traversal (../) per CLAUDE.md coding standards
                let path_str = raw_path.to_string_lossy();
                if path_str.contains("..") {
                    "[KB file rejected: path traversal not allowed]".to_string()
                } else {
                    std::fs::read_to_string(raw_path).unwrap_or_else(|e| {
                        format!("[KB file not found: {} — {}]", raw_path.display(), e)
                    })
                }
            } else {
                kb_ctx.clone()
            };
            if let Ok(Some(mut inst)) = self.store.get_instance(&instance_id) {
                inst.kb_context = Some(resolved_content);
                let _ = self.store.update_instance(&inst);
            }
        }

        // Log event
        let _ = self.store.log_event(
            &instance_id,
            "workflow_started",
            None,
            Some(&format!(
                "{{\"params\":{}}}",
                serde_json::to_string(&params).unwrap_or_default()
            )),
        );

        Ok(instance_id)
    }

    // -----------------------------------------------------------------------
    // Advance
    // -----------------------------------------------------------------------

    /// Advance a workflow instance — execute nodes until a wait point or completion.
    ///
    /// Returns the resulting status after advancing.
    pub fn advance(
        &self,
        instance_id: &str,
        ability_registry: &AbilityRegistry,
        manifest: &AgentManifest,
        breakers: Option<&BreakerRegistry>,
        constitution: Option<&ConstitutionEnforcer>,
    ) -> Result<WorkflowStatus, String> {
        let mut inst = self.load_instance(instance_id)?;
        let def = self.load_def(&inst.workflow_id)?;

        // Don't advance terminal instances
        if inst.status.is_terminal() {
            return Ok(inst.status);
        }

        // Set to running
        inst.status = WorkflowStatus::Running;
        inst.wait_spec = None;
        inst.wait_started_at = None;
        inst.wait_deadline = None;

        // Global timeout check
        if def.global_timeout_secs > 0 {
            let now = chrono::Utc::now().timestamp();
            let elapsed = now - inst.created_at;
            if elapsed > def.global_timeout_secs as i64 {
                inst.status = WorkflowStatus::TimedOut;
                inst.error = Some("Global workflow timeout exceeded".into());
                self.save_instance(&inst)?;
                let _ = self
                    .store
                    .log_event(instance_id, "workflow_timed_out", None, None);
                return Ok(WorkflowStatus::TimedOut);
            }
        }

        // Core execution loop
        for _step_count in 0..MAX_STEPS_PER_ADVANCE {
            // Check if cursor is past all nodes
            if inst.cursor.node_index >= def.nodes.len() {
                inst.status = WorkflowStatus::Completed;
                self.save_instance(&inst)?;
                let _ = self
                    .store
                    .log_event(instance_id, "workflow_completed", None, None);
                return Ok(WorkflowStatus::Completed);
            }

            // Get current node
            let node = &def.nodes[inst.cursor.node_index];
            let node_id = node.id().to_string();

            let _ = self
                .store
                .log_event(instance_id, "node_entered", Some(&node_id), None);

            match node {
                WorkflowNode::Action(action) => {
                    let result = self.execute_action(
                        &mut inst,
                        action,
                        &def,
                        ability_registry,
                        manifest,
                        breakers,
                        constitution,
                    )?;
                    match result {
                        ActionOutcome::Completed => {
                            inst.cursor.node_index += 1;
                            inst.cursor.sub_cursor = None;
                        }
                        ActionOutcome::Skipped => {
                            inst.cursor.node_index += 1;
                            inst.cursor.sub_cursor = None;
                        }
                        ActionOutcome::Failed(err) => {
                            // Check on_failure jump
                            if let Some(ref target) = action.on_failure {
                                if let Some(idx) = def.nodes.iter().position(|n| n.id() == target) {
                                    inst.outputs.insert(format!("{}_error", action.id), err);
                                    inst.cursor.node_index = idx;
                                    inst.cursor.sub_cursor = None;
                                    continue;
                                }
                            }
                            inst.status = WorkflowStatus::Failed;
                            inst.error = Some(err);
                            self.save_instance(&inst)?;
                            let _ = self.store.log_event(
                                instance_id,
                                "workflow_failed",
                                Some(&node_id),
                                None,
                            );
                            return Ok(WorkflowStatus::Failed);
                        }
                    }
                }

                WorkflowNode::WaitEvent(wait_event) => {
                    let now = chrono::Utc::now().timestamp();
                    let deadline = if wait_event.timeout_secs > 0 {
                        Some(now + wait_event.timeout_secs as i64)
                    } else {
                        None
                    };

                    inst.status = WorkflowStatus::Waiting;
                    inst.wait_started_at = Some(now);
                    inst.wait_deadline = deadline;
                    inst.wait_spec = Some(WaitSpec::Event {
                        event_type: wait_event.event_type.clone(),
                        filter: wait_event.filter.clone(),
                        output_key: wait_event.output_key.clone(),
                        on_timeout: wait_event.on_timeout.clone(),
                    });
                    self.save_instance(&inst)?;
                    let _ = self.store.log_event(
                        instance_id,
                        "waiting_event",
                        Some(&node_id),
                        Some(&format!("{{\"event_type\":\"{}\"}}", wait_event.event_type)),
                    );
                    return Ok(WorkflowStatus::Waiting);
                }

                WorkflowNode::Delay(delay) => {
                    let now = chrono::Utc::now().timestamp();
                    let resume_at = now + delay.duration_secs as i64;

                    inst.status = WorkflowStatus::Waiting;
                    inst.wait_started_at = Some(now);
                    inst.wait_deadline = Some(resume_at);
                    inst.wait_spec = Some(WaitSpec::Delay { resume_at });
                    self.save_instance(&inst)?;
                    let _ = self.store.log_event(
                        instance_id,
                        "waiting_delay",
                        Some(&node_id),
                        Some(&format!("{{\"duration_secs\":{}}}", delay.duration_secs)),
                    );
                    return Ok(WorkflowStatus::Waiting);
                }

                WorkflowNode::WaitPoll(poll) => {
                    let now = chrono::Utc::now().timestamp();
                    let next_poll_at = now + poll.poll_interval_secs as i64;
                    let deadline = if poll.timeout_secs > 0 {
                        Some(now + poll.timeout_secs as i64)
                    } else {
                        None
                    };

                    // Resolve poll args for storage
                    let resolved_args =
                        ChainDef::resolve_args(&poll.args, &inst.params, &inst.outputs);

                    inst.status = WorkflowStatus::Waiting;
                    inst.wait_started_at = Some(now);
                    inst.wait_deadline = deadline;
                    inst.wait_spec = Some(WaitSpec::Poll {
                        ability: poll.ability.clone(),
                        args: resolved_args,
                        output_key: poll.output_key.clone(),
                        until: poll.until.clone(),
                        poll_interval_secs: poll.poll_interval_secs,
                        next_poll_at,
                        on_timeout: poll.on_timeout.clone(),
                    });
                    self.save_instance(&inst)?;
                    let _ = self.store.log_event(
                        instance_id,
                        "waiting_poll",
                        Some(&node_id),
                        Some(&format!("{{\"ability\":\"{}\"}}", poll.ability)),
                    );
                    return Ok(WorkflowStatus::Waiting);
                }

                WorkflowNode::Branch(branch) => {
                    let chosen_nodes = self.evaluate_branch(branch, &inst.outputs);
                    // Execute chosen branch nodes inline by expanding them
                    // For simplicity: execute each sub-node sequentially.
                    // If the branch is empty, just advance.
                    let result = self.execute_branch_nodes(
                        &mut inst,
                        chosen_nodes,
                        &def,
                        ability_registry,
                        manifest,
                        breakers,
                        constitution,
                    )?;
                    match result {
                        BranchOutcome::Completed => {
                            inst.cursor.node_index += 1;
                            inst.cursor.sub_cursor = None;
                        }
                        BranchOutcome::Waiting => {
                            // Instance already saved inside execute_branch_nodes
                            return Ok(WorkflowStatus::Waiting);
                        }
                        BranchOutcome::Failed(err) => {
                            inst.status = WorkflowStatus::Failed;
                            inst.error = Some(err);
                            self.save_instance(&inst)?;
                            return Ok(WorkflowStatus::Failed);
                        }
                    }
                }

                WorkflowNode::Parallel(parallel) => {
                    // Sequential execution of branches (true parallelism is future work)
                    let result = self.execute_parallel_sequential(
                        &mut inst,
                        parallel,
                        &def,
                        ability_registry,
                        manifest,
                        breakers,
                        constitution,
                    )?;
                    match result {
                        BranchOutcome::Completed => {
                            inst.cursor.node_index += 1;
                            inst.cursor.sub_cursor = None;
                        }
                        BranchOutcome::Waiting => {
                            return Ok(WorkflowStatus::Waiting);
                        }
                        BranchOutcome::Failed(err) => {
                            inst.status = WorkflowStatus::Failed;
                            inst.error = Some(err);
                            self.save_instance(&inst)?;
                            return Ok(WorkflowStatus::Failed);
                        }
                    }
                }

                WorkflowNode::Compensate(_) => {
                    // Compensation nodes are not executed during normal flow;
                    // they are triggered by the compensation engine on failure.
                    inst.cursor.node_index += 1;
                }
            }
        }

        // Hit max steps limit — save and return current status
        // The workflow is still Running; the caller should call advance() again.
        self.save_instance(&inst)?;
        let _ = self.store.log_event(
            instance_id,
            "max_steps_reached",
            None,
            Some(&format!("{{\"limit\":{}}}", MAX_STEPS_PER_ADVANCE)),
        );
        Ok(inst.status.clone())
    }

    // -----------------------------------------------------------------------
    // Resume with event
    // -----------------------------------------------------------------------

    /// Resume a waiting instance with an event payload.
    pub fn resume_with_event(
        &self,
        instance_id: &str,
        payload: &str,
        ability_registry: &AbilityRegistry,
        manifest: &AgentManifest,
        breakers: Option<&BreakerRegistry>,
        constitution: Option<&ConstitutionEnforcer>,
    ) -> Result<WorkflowStatus, String> {
        let mut inst = self.load_instance(instance_id)?;

        if inst.status != WorkflowStatus::Waiting {
            return Err(format!(
                "Instance {} is not waiting (status: {})",
                instance_id, inst.status
            ));
        }

        // Store event payload in outputs if the wait spec has an output_key
        if let Some(ref spec) = inst.wait_spec {
            if let WaitSpec::Event { output_key, .. } = spec {
                if let Some(key) = output_key {
                    inst.outputs.insert(key.clone(), payload.to_string());
                }
            }
        }

        // Clear wait state and advance cursor past the wait node
        inst.status = WorkflowStatus::Running;
        inst.wait_spec = None;
        inst.wait_started_at = None;
        inst.wait_deadline = None;
        inst.cursor.node_index += 1;
        inst.cursor.sub_cursor = None;
        self.save_instance(&inst)?;

        let _ = self
            .store
            .log_event(instance_id, "event_received", None, Some(payload));

        // Continue advancing
        self.advance(
            instance_id,
            ability_registry,
            manifest,
            breakers,
            constitution,
        )
    }

    // -----------------------------------------------------------------------
    // Deliver event (correlation-based)
    // -----------------------------------------------------------------------

    /// Deliver an event to a workflow by correlation value.
    /// Finds the waiting instance, matches event type, and resumes it.
    pub fn deliver_event(
        &self,
        workflow_id: &str,
        correlation_value: &str,
        event_type: &str,
        payload: &str,
        ability_registry: &AbilityRegistry,
        manifest: &AgentManifest,
        breakers: Option<&BreakerRegistry>,
        constitution: Option<&ConstitutionEnforcer>,
    ) -> Result<Option<WorkflowStatus>, String> {
        let inst = self
            .store
            .find_waiting_by_correlation(workflow_id, correlation_value)
            .map_err(|e| format!("Failed to find waiting instance: {}", e))?;

        let inst = match inst {
            Some(i) => i,
            None => return Ok(None),
        };

        // Check that the wait spec matches the event type
        if let Some(ref spec) = inst.wait_spec {
            match spec {
                WaitSpec::Event {
                    event_type: expected_type,
                    ..
                } => {
                    if expected_type != event_type {
                        return Ok(None);
                    }
                }
                _ => return Ok(None), // Not waiting for an event
            }
        } else {
            return Ok(None);
        }

        let status = self.resume_with_event(
            &inst.instance_id,
            payload,
            ability_registry,
            manifest,
            breakers,
            constitution,
        )?;

        Ok(Some(status))
    }

    // -----------------------------------------------------------------------
    // Tick
    // -----------------------------------------------------------------------

    /// Periodic tick -- handle expired delays, expired waits, and due polls.
    pub fn tick(
        &self,
        ability_registry: &AbilityRegistry,
        manifest: &AgentManifest,
        breakers: Option<&BreakerRegistry>,
        constitution: Option<&ConstitutionEnforcer>,
    ) -> Vec<(String, Result<WorkflowStatus, String>)> {
        let now = chrono::Utc::now().timestamp();
        let mut results = Vec::new();

        // 1. Handle expired delays
        if let Ok(expired) = self.store.find_expired_waits(now) {
            for inst in expired {
                let instance_id = inst.instance_id.clone();
                let result = match &inst.wait_spec {
                    Some(WaitSpec::Delay { .. }) => {
                        // Delay expired: clear wait state, advance cursor, continue
                        self.resume_from_delay(
                            &instance_id,
                            ability_registry,
                            manifest,
                            breakers,
                            constitution,
                        )
                    }
                    Some(WaitSpec::Event { on_timeout, .. }) => self.handle_wait_timeout(
                        &instance_id,
                        on_timeout,
                        ability_registry,
                        manifest,
                        breakers,
                        constitution,
                    ),
                    Some(WaitSpec::Poll { on_timeout, .. }) => self.handle_wait_timeout(
                        &instance_id,
                        on_timeout,
                        ability_registry,
                        manifest,
                        breakers,
                        constitution,
                    ),
                    _ => continue,
                };
                results.push((instance_id, result));
            }
        }

        // 2. Handle due polls
        if let Ok(due_polls) = self.store.find_due_polls(now) {
            for inst in due_polls {
                // Skip if already processed above as expired
                if results.iter().any(|(id, _)| id == &inst.instance_id) {
                    continue;
                }
                let instance_id = inst.instance_id.clone();
                let result =
                    self.execute_poll(&inst, ability_registry, manifest, breakers, constitution);
                results.push((instance_id, result));
            }
        }

        results
    }

    // -----------------------------------------------------------------------
    // Cancel
    // -----------------------------------------------------------------------

    /// Cancel a running or waiting workflow.
    pub fn cancel(&self, instance_id: &str) -> Result<(), String> {
        let mut inst = self.load_instance(instance_id)?;

        if inst.status.is_terminal() {
            return Err(format!(
                "Cannot cancel instance {} (status: {})",
                instance_id, inst.status
            ));
        }

        inst.status = WorkflowStatus::Cancelled;
        inst.wait_spec = None;
        inst.wait_started_at = None;
        inst.wait_deadline = None;
        self.save_instance(&inst)?;
        let _ = self
            .store
            .log_event(instance_id, "workflow_cancelled", None, None);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Status
    // -----------------------------------------------------------------------

    /// Get workflow instance (if it exists).
    pub fn status(&self, instance_id: &str) -> Result<Option<WorkflowInstance>, String> {
        self.store
            .get_instance(instance_id)
            .map_err(|e| format!("Failed to get instance: {}", e))
    }

    // =======================================================================
    // Internal helpers
    // =======================================================================

    fn load_instance(&self, instance_id: &str) -> Result<WorkflowInstance, String> {
        self.store
            .get_instance(instance_id)
            .map_err(|e| format!("Failed to load instance: {}", e))?
            .ok_or_else(|| format!("Instance not found: {}", instance_id))
    }

    fn load_def(&self, workflow_id: &str) -> Result<WorkflowDef, String> {
        self.store
            .get_def(workflow_id)
            .map_err(|e| format!("Failed to load workflow def: {}", e))?
            .ok_or_else(|| format!("Workflow definition not found: {}", workflow_id))
    }

    fn save_instance(&self, inst: &WorkflowInstance) -> Result<(), String> {
        self.store
            .update_instance(inst)
            .map_err(|e| format!("Failed to save instance: {}", e))
    }

    /// Execute a single Action node.
    fn execute_action(
        &self,
        inst: &mut WorkflowInstance,
        action: &ActionNode,
        def: &WorkflowDef,
        ability_registry: &AbilityRegistry,
        manifest: &AgentManifest,
        breakers: Option<&BreakerRegistry>,
        constitution: Option<&ConstitutionEnforcer>,
    ) -> Result<ActionOutcome, String> {
        // Check condition
        if let Some(ref condition) = action.condition {
            if !condition.test(&inst.outputs) {
                let _ = self.store.log_event(
                    &inst.instance_id,
                    "node_skipped",
                    Some(&action.id),
                    Some("{\"reason\":\"condition_false\"}"),
                );
                return Ok(ActionOutcome::Skipped);
            }
        }

        // Constitution check
        if let Some(constitution) = constitution {
            let check = constitution.check_ability(&action.ability);
            if !check.allowed {
                let reason = check
                    .reason
                    .unwrap_or_else(|| "not allowed by constitution".to_string());
                return Ok(ActionOutcome::Failed(format!(
                    "Constitution blocked ability '{}': {}",
                    action.ability, reason
                )));
            }
        }

        // Circuit breaker check
        if let Some(breakers) = breakers {
            let check = breakers.evaluate(&def.id, &inst.outputs, &action.ability);
            if !check.proceed {
                let reasons: Vec<String> = check
                    .fired
                    .iter()
                    .filter(|f| {
                        f.action == BreakerAction::Abort || f.action == BreakerAction::Confirm
                    })
                    .map(|f| f.reason.clone())
                    .collect();
                return Ok(ActionOutcome::Failed(format!(
                    "Circuit breaker halted at '{}': {}",
                    action.id,
                    reasons.join("; ")
                )));
            }
        }

        // Resolve template arguments
        let resolved_args = ChainDef::resolve_args(&action.args, &inst.params, &inst.outputs);

        // Inject style template variables into resolved args
        let resolved_args = if let Some(ref style_vars) = inst.style_vars {
            resolved_args
                .into_iter()
                .map(|(k, mut v)| {
                    for (svar_key, svar_val) in style_vars {
                        v = v.replace(&format!("{{{{{}}}}}", svar_key), svar_val);
                    }
                    (k, v)
                })
                .collect()
        } else {
            resolved_args
        };

        // Inject KB context template variable
        let resolved_args = if let Some(ref kb_ctx) = inst.kb_context {
            resolved_args
                .into_iter()
                .map(|(k, v)| (k, v.replace("{{_kb_context}}", kb_ctx)))
                .collect()
        } else {
            resolved_args
        };

        let input_json = serde_json::to_string(&resolved_args)
            .map_err(|e| format!("Failed to serialize args: {}", e))?;

        // Execute ability
        let start = std::time::Instant::now();
        match ability_registry.execute_ability(manifest, &action.ability, &input_json) {
            Ok(result) => {
                let elapsed_ms = start.elapsed().as_millis() as i64;
                inst.execution_ms += elapsed_ms;

                // Store output
                if let Some(ref key) = action.output_key {
                    let output_str = String::from_utf8_lossy(&result.output).to_string();
                    inst.outputs.insert(key.clone(), output_str);
                }

                // Track receipt
                inst.receipt_ids.push(result.receipt.id.clone());

                let _ = self.store.log_event(
                    &inst.instance_id,
                    "node_completed",
                    Some(&action.id),
                    Some(&format!("{{\"ms\":{}}}", elapsed_ms)),
                );

                Ok(ActionOutcome::Completed)
            }
            Err(err) => {
                let _ = self.store.log_event(
                    &inst.instance_id,
                    "node_failed",
                    Some(&action.id),
                    Some(&format!("{{\"error\":\"{}\"}}", err.replace('"', "\\\""))),
                );
                Ok(ActionOutcome::Failed(err))
            }
        }
    }

    /// Evaluate a branch node's conditions against outputs.
    /// Returns the list of nodes from the matching arm (or `otherwise`).
    fn evaluate_branch<'a>(
        &self,
        branch: &'a BranchNode,
        outputs: &HashMap<String, String>,
    ) -> &'a [WorkflowNode] {
        for arm in &branch.conditions {
            if arm.condition.test(outputs) {
                return &arm.nodes;
            }
        }
        &branch.otherwise
    }

    /// Execute a sequence of sub-nodes (from a branch).
    /// Returns Completed if all finish, Waiting if a wait node is hit.
    fn execute_branch_nodes(
        &self,
        inst: &mut WorkflowInstance,
        nodes: &[WorkflowNode],
        def: &WorkflowDef,
        ability_registry: &AbilityRegistry,
        manifest: &AgentManifest,
        breakers: Option<&BreakerRegistry>,
        constitution: Option<&ConstitutionEnforcer>,
    ) -> Result<BranchOutcome, String> {
        for node in nodes {
            match node {
                WorkflowNode::Action(action) => {
                    let result = self.execute_action(
                        inst,
                        action,
                        def,
                        ability_registry,
                        manifest,
                        breakers,
                        constitution,
                    )?;
                    match result {
                        ActionOutcome::Completed | ActionOutcome::Skipped => continue,
                        ActionOutcome::Failed(err) => return Ok(BranchOutcome::Failed(err)),
                    }
                }
                WorkflowNode::Delay(delay) => {
                    let now = chrono::Utc::now().timestamp();
                    let resume_at = now + delay.duration_secs as i64;
                    inst.status = WorkflowStatus::Waiting;
                    inst.wait_started_at = Some(now);
                    inst.wait_deadline = Some(resume_at);
                    inst.wait_spec = Some(WaitSpec::Delay { resume_at });
                    self.save_instance(inst)?;
                    return Ok(BranchOutcome::Waiting);
                }
                WorkflowNode::WaitEvent(we) => {
                    let now = chrono::Utc::now().timestamp();
                    let deadline = if we.timeout_secs > 0 {
                        Some(now + we.timeout_secs as i64)
                    } else {
                        None
                    };
                    inst.status = WorkflowStatus::Waiting;
                    inst.wait_started_at = Some(now);
                    inst.wait_deadline = deadline;
                    inst.wait_spec = Some(WaitSpec::Event {
                        event_type: we.event_type.clone(),
                        filter: we.filter.clone(),
                        output_key: we.output_key.clone(),
                        on_timeout: we.on_timeout.clone(),
                    });
                    self.save_instance(inst)?;
                    return Ok(BranchOutcome::Waiting);
                }
                WorkflowNode::WaitPoll(poll) => {
                    let now = chrono::Utc::now().timestamp();
                    let next_poll_at = now + poll.poll_interval_secs as i64;
                    let deadline = if poll.timeout_secs > 0 {
                        Some(now + poll.timeout_secs as i64)
                    } else {
                        None
                    };
                    let resolved_args =
                        ChainDef::resolve_args(&poll.args, &inst.params, &inst.outputs);
                    inst.status = WorkflowStatus::Waiting;
                    inst.wait_started_at = Some(now);
                    inst.wait_deadline = deadline;
                    inst.wait_spec = Some(WaitSpec::Poll {
                        ability: poll.ability.clone(),
                        args: resolved_args,
                        output_key: poll.output_key.clone(),
                        until: poll.until.clone(),
                        poll_interval_secs: poll.poll_interval_secs,
                        next_poll_at,
                        on_timeout: poll.on_timeout.clone(),
                    });
                    self.save_instance(inst)?;
                    return Ok(BranchOutcome::Waiting);
                }
                WorkflowNode::Branch(sub_branch) => {
                    let chosen = self.evaluate_branch(sub_branch, &inst.outputs);
                    let result = self.execute_branch_nodes(
                        inst,
                        chosen,
                        def,
                        ability_registry,
                        manifest,
                        breakers,
                        constitution,
                    )?;
                    match result {
                        BranchOutcome::Completed => continue,
                        other => return Ok(other),
                    }
                }
                WorkflowNode::Parallel(parallel) => {
                    let result = self.execute_parallel_sequential(
                        inst,
                        parallel,
                        def,
                        ability_registry,
                        manifest,
                        breakers,
                        constitution,
                    )?;
                    match result {
                        BranchOutcome::Completed => continue,
                        other => return Ok(other),
                    }
                }
                WorkflowNode::Compensate(_) => {
                    // Compensation nodes are skipped during normal branch execution.
                    continue;
                }
            }
        }
        Ok(BranchOutcome::Completed)
    }

    /// Execute parallel branches sequentially (true parallelism is future work).
    fn execute_parallel_sequential(
        &self,
        inst: &mut WorkflowInstance,
        parallel: &ParallelNode,
        def: &WorkflowDef,
        ability_registry: &AbilityRegistry,
        manifest: &AgentManifest,
        breakers: Option<&BreakerRegistry>,
        constitution: Option<&ConstitutionEnforcer>,
    ) -> Result<BranchOutcome, String> {
        for branch in &parallel.branches {
            let result = self.execute_branch_nodes(
                inst,
                &branch.nodes,
                def,
                ability_registry,
                manifest,
                breakers,
                constitution,
            )?;
            match result {
                BranchOutcome::Completed => continue,
                other => return Ok(other),
            }
        }
        Ok(BranchOutcome::Completed)
    }

    /// Resume from an expired delay.
    fn resume_from_delay(
        &self,
        instance_id: &str,
        ability_registry: &AbilityRegistry,
        manifest: &AgentManifest,
        breakers: Option<&BreakerRegistry>,
        constitution: Option<&ConstitutionEnforcer>,
    ) -> Result<WorkflowStatus, String> {
        let mut inst = self.load_instance(instance_id)?;

        inst.status = WorkflowStatus::Running;
        inst.wait_spec = None;
        inst.wait_started_at = None;
        inst.wait_deadline = None;
        inst.cursor.node_index += 1;
        inst.cursor.sub_cursor = None;
        self.save_instance(&inst)?;

        let _ = self
            .store
            .log_event(instance_id, "delay_expired", None, None);

        self.advance(
            instance_id,
            ability_registry,
            manifest,
            breakers,
            constitution,
        )
    }

    /// Handle a timed-out wait (event or poll).
    fn handle_wait_timeout(
        &self,
        instance_id: &str,
        on_timeout: &str,
        ability_registry: &AbilityRegistry,
        manifest: &AgentManifest,
        breakers: Option<&BreakerRegistry>,
        constitution: Option<&ConstitutionEnforcer>,
    ) -> Result<WorkflowStatus, String> {
        let mut inst = self.load_instance(instance_id)?;

        if on_timeout == "fail" {
            inst.status = WorkflowStatus::TimedOut;
            inst.error = Some("Wait timed out".into());
            inst.wait_spec = None;
            inst.wait_started_at = None;
            inst.wait_deadline = None;
            self.save_instance(&inst)?;
            let _ = self
                .store
                .log_event(instance_id, "wait_timed_out", None, None);
            return Ok(WorkflowStatus::TimedOut);
        }

        // on_timeout is a node ID — try to jump to it
        let def = self.load_def(&inst.workflow_id)?;
        if let Some(idx) = def.nodes.iter().position(|n| n.id() == on_timeout) {
            inst.status = WorkflowStatus::Running;
            inst.wait_spec = None;
            inst.wait_started_at = None;
            inst.wait_deadline = None;
            inst.cursor.node_index = idx;
            inst.cursor.sub_cursor = None;
            self.save_instance(&inst)?;
            let _ = self.store.log_event(
                instance_id,
                "wait_timeout_jump",
                None,
                Some(&format!("{{\"target\":\"{}\"}}", on_timeout)),
            );
            return self.advance(
                instance_id,
                ability_registry,
                manifest,
                breakers,
                constitution,
            );
        }

        // Unknown on_timeout target — treat as failure
        inst.status = WorkflowStatus::TimedOut;
        inst.error = Some(format!(
            "Wait timed out; unknown on_timeout target: {}",
            on_timeout
        ));
        inst.wait_spec = None;
        inst.wait_started_at = None;
        inst.wait_deadline = None;
        self.save_instance(&inst)?;
        Ok(WorkflowStatus::TimedOut)
    }

    /// Execute a poll: run the ability, check condition, resume or reschedule.
    fn execute_poll(
        &self,
        inst: &WorkflowInstance,
        ability_registry: &AbilityRegistry,
        manifest: &AgentManifest,
        breakers: Option<&BreakerRegistry>,
        constitution: Option<&ConstitutionEnforcer>,
    ) -> Result<WorkflowStatus, String> {
        let (ability, args, output_key, until, poll_interval_secs, _on_timeout) =
            match &inst.wait_spec {
                Some(WaitSpec::Poll {
                    ability,
                    args,
                    output_key,
                    until,
                    poll_interval_secs,
                    on_timeout,
                    ..
                }) => (
                    ability.clone(),
                    args.clone(),
                    output_key.clone(),
                    until.clone(),
                    *poll_interval_secs,
                    on_timeout.clone(),
                ),
                _ => return Err("Instance is not in poll wait state".into()),
            };

        let input_json = serde_json::to_string(&args)
            .map_err(|e| format!("Failed to serialize poll args: {}", e))?;

        match ability_registry.execute_ability(manifest, &ability, &input_json) {
            Ok(result) => {
                let output_str = String::from_utf8_lossy(&result.output).to_string();
                let mut test_outputs = inst.outputs.clone();
                if let Some(ref key) = output_key {
                    test_outputs.insert(key.clone(), output_str.clone());
                }

                if until.test(&test_outputs) {
                    // Condition met — resume the workflow
                    let mut inst = self.load_instance(&inst.instance_id)?;
                    if let Some(ref key) = output_key {
                        inst.outputs.insert(key.clone(), output_str);
                    }
                    inst.receipt_ids.push(result.receipt.id.clone());
                    inst.status = WorkflowStatus::Running;
                    inst.wait_spec = None;
                    inst.wait_started_at = None;
                    inst.wait_deadline = None;
                    inst.cursor.node_index += 1;
                    inst.cursor.sub_cursor = None;
                    self.save_instance(&inst)?;

                    let _ =
                        self.store
                            .log_event(&inst.instance_id, "poll_condition_met", None, None);

                    self.advance(
                        &inst.instance_id,
                        ability_registry,
                        manifest,
                        breakers,
                        constitution,
                    )
                } else {
                    // Condition not met — reschedule next poll
                    let mut inst = self.load_instance(&inst.instance_id)?;
                    let now = chrono::Utc::now().timestamp();
                    let next = now + poll_interval_secs as i64;

                    if let Some(spec) = &mut inst.wait_spec {
                        if let WaitSpec::Poll {
                            next_poll_at,
                            ..
                        } = spec
                        {
                            *next_poll_at = next;
                        }
                    }
                    self.save_instance(&inst)?;

                    let _ = self.store.log_event(
                        &inst.instance_id,
                        "poll_rescheduled",
                        None,
                        Some(&format!("{{\"next_poll_at\":{}}}", next)),
                    );

                    Ok(WorkflowStatus::Waiting)
                }
            }
            Err(err) => {
                // Poll ability failed — log but keep polling (transient error)
                let _ = self.store.log_event(
                    &inst.instance_id,
                    "poll_error",
                    None,
                    Some(&format!("{{\"error\":\"{}\"}}", err.replace('"', "\\\""))),
                );
                // Reschedule
                let mut inst = self.load_instance(&inst.instance_id)?;
                let now = chrono::Utc::now().timestamp();
                let next = now + poll_interval_secs as i64;
                if let Some(spec) = &mut inst.wait_spec {
                    if let WaitSpec::Poll {
                        next_poll_at,
                        ..
                    } = spec
                    {
                        *next_poll_at = next;
                    }
                }
                self.save_instance(&inst)?;
                Ok(WorkflowStatus::Waiting)
            }
        }
    }
}

/// Internal outcome of executing an action node.
enum ActionOutcome {
    Completed,
    Skipped,
    Failed(String),
}

/// Internal outcome of executing a branch/parallel sequence.
enum BranchOutcome {
    Completed,
    Waiting,
    Failed(String),
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::dsl::{ConditionOp, ParamDef, ParamType};
    use crate::chain::workflow::*;
    use crate::runtime::receipt::ReceiptSigner;

    fn test_store() -> (WorkflowStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkflowStore::open(&dir.path().join("wf_engine.db")).unwrap();
        (store, dir)
    }

    fn test_manifest() -> AgentManifest {
        AgentManifest {
            name: "test-agent".into(),
            version: "0.1.0".into(),
            description: "test".into(),
            permissions: vec![
                "data.fetch_url".into(),
                "notify.user".into(),
                "flow.stop".into(),
                "order.validate".into(),
                "order.process".into(),
                "order.status".into(),
            ],
            memory_limit_mb: 64,
            fuel_limit: 1_000_000,
            kv_namespace: None,
            author: None,
            intent_filters: vec![],
            resources: None,
            background: false,
            subscriptions: vec![],
            data_namespace: None,
            signature: None,
        }
    }

    fn simple_two_step_def() -> WorkflowDef {
        WorkflowDef {
            id: "two_step".into(),
            name: "Two Step".into(),
            description: "Simple two-step action workflow".into(),
            params: vec![ParamDef {
                name: "city".into(),
                param_type: ParamType::Text,
                description: "City".into(),
                required: true,
                default: None,
            }],
            nodes: vec![
                WorkflowNode::Action(ActionNode {
                    id: "fetch".into(),
                    ability: "data.fetch_url".into(),
                    args: HashMap::from([(
                        "url".into(),
                        "https://api.weather.com/{{city}}".into(),
                    )]),
                    output_key: Some("weather".into()),
                    condition: None,
                    on_failure: None,
                }),
                WorkflowNode::Action(ActionNode {
                    id: "notify".into(),
                    ability: "notify.user".into(),
                    args: HashMap::from([("message".into(), "Weather: {{weather}}".into())]),
                    output_key: None,
                    condition: None,
                    on_failure: None,
                }),
            ],
            global_timeout_secs: 0,
            max_instances: 0,
            correlation_key: None,
            style: None,
            kb_context: None,
            channel_permissions: None,
        }
    }

    fn delay_workflow_def() -> WorkflowDef {
        WorkflowDef {
            id: "delay_wf".into(),
            name: "Delay Workflow".into(),
            description: "Workflow with a delay node".into(),
            params: vec![],
            nodes: vec![
                WorkflowNode::Action(ActionNode {
                    id: "step1".into(),
                    ability: "flow.stop".into(),
                    args: HashMap::new(),
                    output_key: Some("step1_out".into()),
                    condition: None,
                    on_failure: None,
                }),
                WorkflowNode::Delay(DelayNode {
                    id: "wait_30s".into(),
                    duration_secs: 30,
                }),
                WorkflowNode::Action(ActionNode {
                    id: "step2".into(),
                    ability: "flow.stop".into(),
                    args: HashMap::new(),
                    output_key: Some("step2_out".into()),
                    condition: None,
                    on_failure: None,
                }),
            ],
            global_timeout_secs: 0,
            max_instances: 0,
            correlation_key: None,
            style: None,
            kb_context: None,
            channel_permissions: None,
        }
    }

    // -----------------------------------------------------------------------
    // Test: simple action-only workflow (2 steps)
    // -----------------------------------------------------------------------

    #[test]
    fn test_simple_two_step_workflow() {
        let (store, _dir) = test_store();
        let def = simple_two_step_def();
        store.store_def(&def).unwrap();

        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let manifest = test_manifest();
        let engine = WorkflowEngine::new(store);

        let instance_id = engine
            .start("two_step", HashMap::from([("city".into(), "NYC".into())]))
            .unwrap();

        let status = engine
            .advance(&instance_id, &registry, &manifest, None, None)
            .unwrap();

        assert_eq!(status, WorkflowStatus::Completed);

        let inst = engine.status(&instance_id).unwrap().unwrap();
        assert_eq!(inst.status, WorkflowStatus::Completed);
        assert!(inst.outputs.contains_key("weather"));
        // Note: receipt_ids are not persisted in the DB (reconstructed from events if needed)
        // so we verify completion and outputs instead.
    }

    // -----------------------------------------------------------------------
    // Test: workflow with delay node
    // -----------------------------------------------------------------------

    #[test]
    fn test_delay_workflow() {
        let (store, _dir) = test_store();
        let def = delay_workflow_def();
        store.store_def(&def).unwrap();

        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let manifest = test_manifest();
        let engine = WorkflowEngine::new(store);

        let instance_id = engine.start("delay_wf", HashMap::new()).unwrap();

        // First advance: executes step1, hits delay, returns Waiting
        let status = engine
            .advance(&instance_id, &registry, &manifest, None, None)
            .unwrap();
        assert_eq!(status, WorkflowStatus::Waiting);

        let inst = engine.status(&instance_id).unwrap().unwrap();
        assert!(inst.outputs.contains_key("step1_out"));
        assert!(matches!(inst.wait_spec, Some(WaitSpec::Delay { .. })));

        // Simulate delay expiring via tick
        // We need to manipulate the deadline to be in the past.
        // Use the store directly.
        let mut inst = inst;
        inst.wait_deadline = Some(1); // epoch second 1 = definitely in the past
        if let Some(WaitSpec::Delay { ref mut resume_at }) = inst.wait_spec {
            *resume_at = 1;
        }
        engine.store().update_instance(&inst).unwrap();

        // Tick should resume the workflow
        let tick_results = engine.tick(&registry, &manifest, None, None);
        assert_eq!(tick_results.len(), 1);
        let (id, result) = &tick_results[0];
        assert_eq!(id, &instance_id);
        assert_eq!(result.as_ref().unwrap(), &WorkflowStatus::Completed);

        let final_inst = engine.status(&instance_id).unwrap().unwrap();
        assert_eq!(final_inst.status, WorkflowStatus::Completed);
        assert!(final_inst.outputs.contains_key("step2_out"));
    }

    // -----------------------------------------------------------------------
    // Test: workflow cancellation
    // -----------------------------------------------------------------------

    #[test]
    fn test_cancel_workflow() {
        let (store, _dir) = test_store();
        let def = delay_workflow_def();
        store.store_def(&def).unwrap();

        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let manifest = test_manifest();
        let engine = WorkflowEngine::new(store);

        let instance_id = engine.start("delay_wf", HashMap::new()).unwrap();

        // Advance to waiting state
        let status = engine
            .advance(&instance_id, &registry, &manifest, None, None)
            .unwrap();
        assert_eq!(status, WorkflowStatus::Waiting);

        // Cancel
        engine.cancel(&instance_id).unwrap();

        let inst = engine.status(&instance_id).unwrap().unwrap();
        assert_eq!(inst.status, WorkflowStatus::Cancelled);
        assert!(inst.wait_spec.is_none());

        // Cannot cancel again
        let err = engine.cancel(&instance_id);
        assert!(err.is_err());
    }

    // -----------------------------------------------------------------------
    // Test: max steps limit
    // -----------------------------------------------------------------------

    #[test]
    fn test_max_steps_limit() {
        // Create a workflow with more nodes than MAX_STEPS_PER_ADVANCE
        let mut nodes = Vec::new();
        for i in 0..(MAX_STEPS_PER_ADVANCE + 10) {
            nodes.push(WorkflowNode::Action(ActionNode {
                id: format!("step_{}", i),
                ability: "flow.stop".into(),
                args: HashMap::new(),
                output_key: Some(format!("out_{}", i)),
                condition: None,
                on_failure: None,
            }));
        }

        let def = WorkflowDef {
            id: "many_steps".into(),
            name: "Many Steps".into(),
            description: "Tests max steps limit".into(),
            params: vec![],
            nodes,
            global_timeout_secs: 0,
            max_instances: 0,
            correlation_key: None,
            style: None,
            kb_context: None,
            channel_permissions: None,
        };

        let (store, _dir) = test_store();
        store.store_def(&def).unwrap();

        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let manifest = test_manifest();
        let engine = WorkflowEngine::new(store);

        let instance_id = engine.start("many_steps", HashMap::new()).unwrap();

        // First advance — should execute MAX_STEPS_PER_ADVANCE steps and return Running
        let status = engine
            .advance(&instance_id, &registry, &manifest, None, None)
            .unwrap();
        assert_eq!(status, WorkflowStatus::Running);

        let inst = engine.status(&instance_id).unwrap().unwrap();
        assert_eq!(inst.cursor.node_index, MAX_STEPS_PER_ADVANCE);

        // Second advance — should complete the remaining steps
        let status = engine
            .advance(&instance_id, &registry, &manifest, None, None)
            .unwrap();
        assert_eq!(status, WorkflowStatus::Completed);
    }

    // -----------------------------------------------------------------------
    // Test: global timeout
    // -----------------------------------------------------------------------

    #[test]
    fn test_global_timeout() {
        let def = WorkflowDef {
            id: "timeout_wf".into(),
            name: "Timeout WF".into(),
            description: "Tests global timeout".into(),
            params: vec![],
            nodes: vec![WorkflowNode::Action(ActionNode {
                id: "step1".into(),
                ability: "flow.stop".into(),
                args: HashMap::new(),
                output_key: None,
                condition: None,
                on_failure: None,
            })],
            global_timeout_secs: 1, // 1 second timeout
            max_instances: 0,
            correlation_key: None,
            style: None,
            kb_context: None,
            channel_permissions: None,
        };

        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("wf_timeout.db");
        let store = WorkflowStore::open(&db_path).unwrap();
        store.store_def(&def).unwrap();

        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let manifest = test_manifest();
        let engine = WorkflowEngine::new(store);

        let instance_id = engine.start("timeout_wf", HashMap::new()).unwrap();

        // Backdate created_at via a separate DB connection (update_instance doesn't touch created_at)
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            let old_ts = chrono::Utc::now().timestamp() - 100;
            conn.execute(
                "UPDATE workflow_instances SET created_at = ?1 WHERE instance_id = ?2",
                rusqlite::params![old_ts, instance_id],
            )
            .unwrap();
        }

        // Advance should detect global timeout
        let status = engine
            .advance(&instance_id, &registry, &manifest, None, None)
            .unwrap();
        assert_eq!(status, WorkflowStatus::TimedOut);

        let inst = engine.status(&instance_id).unwrap().unwrap();
        assert_eq!(inst.status, WorkflowStatus::TimedOut);
        assert!(inst.error.as_ref().unwrap().contains("timeout"));
    }

    // -----------------------------------------------------------------------
    // Test: max instances enforcement
    // -----------------------------------------------------------------------

    #[test]
    fn test_max_instances() {
        let def = WorkflowDef {
            id: "limited".into(),
            name: "Limited".into(),
            description: "Max 2 instances".into(),
            params: vec![],
            nodes: vec![WorkflowNode::Action(ActionNode {
                id: "step".into(),
                ability: "flow.stop".into(),
                args: HashMap::new(),
                output_key: None,
                condition: None,
                on_failure: None,
            })],
            global_timeout_secs: 0,
            max_instances: 2,
            correlation_key: None,
            style: None,
            kb_context: None,
            channel_permissions: None,
        };

        let (store, _dir) = test_store();
        store.store_def(&def).unwrap();

        let engine = WorkflowEngine::new(store);

        // Create two instances (both are Created = active)
        engine.start("limited", HashMap::new()).unwrap();
        engine.start("limited", HashMap::new()).unwrap();

        // Third should fail
        let err = engine.start("limited", HashMap::new());
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("Max instances"));
    }

    // -----------------------------------------------------------------------
    // Test: correlation key resolution
    // -----------------------------------------------------------------------

    #[test]
    fn test_correlation_key() {
        let def = WorkflowDef {
            id: "correlated".into(),
            name: "Correlated".into(),
            description: "Test".into(),
            params: vec![ParamDef {
                name: "order_id".into(),
                param_type: ParamType::Text,
                description: "Order".into(),
                required: true,
                default: None,
            }],
            nodes: vec![
                WorkflowNode::Action(ActionNode {
                    id: "step1".into(),
                    ability: "flow.stop".into(),
                    args: HashMap::new(),
                    output_key: Some("result".into()),
                    condition: None,
                    on_failure: None,
                }),
                WorkflowNode::WaitEvent(WaitEventNode {
                    id: "wait".into(),
                    event_type: "payment.confirmed".into(),
                    filter: None,
                    output_key: Some("payment".into()),
                    timeout_secs: 3600,
                    on_timeout: "fail".into(),
                }),
                WorkflowNode::Action(ActionNode {
                    id: "step2".into(),
                    ability: "flow.stop".into(),
                    args: HashMap::new(),
                    output_key: None,
                    condition: None,
                    on_failure: None,
                }),
            ],
            global_timeout_secs: 0,
            max_instances: 0,
            correlation_key: Some("{{order_id}}".into()),
            style: None,
            kb_context: None,
            channel_permissions: None,
        };

        let (store, _dir) = test_store();
        store.store_def(&def).unwrap();

        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let manifest = test_manifest();
        let engine = WorkflowEngine::new(store);

        let instance_id = engine
            .start(
                "correlated",
                HashMap::from([("order_id".into(), "ORD-42".into())]),
            )
            .unwrap();

        // Advance: step1 completes, hits WaitEvent
        let status = engine
            .advance(&instance_id, &registry, &manifest, None, None)
            .unwrap();
        assert_eq!(status, WorkflowStatus::Waiting);

        // Deliver event by correlation
        let result = engine
            .deliver_event(
                "correlated",
                "ORD-42",
                "payment.confirmed",
                "{\"amount\":100}",
                &registry,
                &manifest,
                None,
                None,
            )
            .unwrap();

        assert_eq!(result, Some(WorkflowStatus::Completed));

        let inst = engine.status(&instance_id).unwrap().unwrap();
        assert_eq!(inst.status, WorkflowStatus::Completed);
        assert_eq!(
            inst.outputs.get("payment"),
            Some(&"{\"amount\":100}".to_string())
        );
    }

    // -----------------------------------------------------------------------
    // Test: branch node
    // -----------------------------------------------------------------------

    #[test]
    fn test_branch_workflow() {
        let def = WorkflowDef {
            id: "branch_wf".into(),
            name: "Branch WF".into(),
            description: "Tests branching".into(),
            params: vec![],
            nodes: vec![
                WorkflowNode::Action(ActionNode {
                    id: "setup".into(),
                    ability: "flow.stop".into(),
                    args: HashMap::new(),
                    output_key: Some("status".into()),
                    condition: None,
                    on_failure: None,
                }),
                WorkflowNode::Branch(BranchNode {
                    id: "decide".into(),
                    conditions: vec![BranchArm {
                        condition: StepCondition {
                            ref_key: "nonexistent_key".into(),
                            op: ConditionOp::Equals,
                            value: "premium".into(),
                        },
                        nodes: vec![WorkflowNode::Action(ActionNode {
                            id: "premium_action".into(),
                            ability: "flow.stop".into(),
                            args: HashMap::new(),
                            output_key: Some("path".into()),
                            condition: None,
                            on_failure: None,
                        })],
                    }],
                    otherwise: vec![WorkflowNode::Action(ActionNode {
                        id: "default_action".into(),
                        ability: "flow.stop".into(),
                        args: HashMap::new(),
                        output_key: Some("path".into()),
                        condition: None,
                        on_failure: None,
                    })],
                }),
            ],
            global_timeout_secs: 0,
            max_instances: 0,
            correlation_key: None,
            style: None,
            kb_context: None,
            channel_permissions: None,
        };

        let (store, _dir) = test_store();
        store.store_def(&def).unwrap();

        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let manifest = test_manifest();
        let engine = WorkflowEngine::new(store);

        let instance_id = engine.start("branch_wf", HashMap::new()).unwrap();
        let status = engine
            .advance(&instance_id, &registry, &manifest, None, None)
            .unwrap();
        assert_eq!(status, WorkflowStatus::Completed);

        let inst = engine.status(&instance_id).unwrap().unwrap();
        // Should have taken the otherwise path
        assert!(inst.outputs.contains_key("path"));
    }

    // -----------------------------------------------------------------------
    // Test: advancing a terminal instance is a no-op
    // -----------------------------------------------------------------------

    #[test]
    fn test_advance_terminal_noop() {
        let (store, _dir) = test_store();
        let def = simple_two_step_def();
        store.store_def(&def).unwrap();

        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let manifest = test_manifest();
        let engine = WorkflowEngine::new(store);

        let instance_id = engine
            .start("two_step", HashMap::from([("city".into(), "SF".into())]))
            .unwrap();

        // Complete the workflow
        let status = engine
            .advance(&instance_id, &registry, &manifest, None, None)
            .unwrap();
        assert_eq!(status, WorkflowStatus::Completed);

        // Advance again — should just return Completed without doing anything
        let status = engine
            .advance(&instance_id, &registry, &manifest, None, None)
            .unwrap();
        assert_eq!(status, WorkflowStatus::Completed);
    }

    // -----------------------------------------------------------------------
    // Test: deliver_event with wrong event type returns None
    // -----------------------------------------------------------------------

    #[test]
    fn test_deliver_event_wrong_type() {
        let def = WorkflowDef {
            id: "event_wf".into(),
            name: "Event WF".into(),
            description: "Test".into(),
            params: vec![ParamDef {
                name: "id".into(),
                param_type: ParamType::Text,
                description: "ID".into(),
                required: true,
                default: None,
            }],
            nodes: vec![WorkflowNode::WaitEvent(WaitEventNode {
                id: "wait".into(),
                event_type: "order.shipped".into(),
                filter: None,
                output_key: Some("data".into()),
                timeout_secs: 0,
                on_timeout: "fail".into(),
            })],
            global_timeout_secs: 0,
            max_instances: 0,
            correlation_key: Some("{{id}}".into()),
            style: None,
            kb_context: None,
            channel_permissions: None,
        };

        let (store, _dir) = test_store();
        store.store_def(&def).unwrap();

        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let manifest = test_manifest();
        let engine = WorkflowEngine::new(store);

        let instance_id = engine
            .start("event_wf", HashMap::from([("id".into(), "X1".into())]))
            .unwrap();
        engine
            .advance(&instance_id, &registry, &manifest, None, None)
            .unwrap();

        // Deliver wrong event type
        let result = engine
            .deliver_event(
                "event_wf",
                "X1",
                "order.cancelled", // wrong type
                "{}",
                &registry,
                &manifest,
                None,
                None,
            )
            .unwrap();
        assert_eq!(result, None);
    }

    // -----------------------------------------------------------------------
    // Test: style vars injected into instance on start
    // -----------------------------------------------------------------------

    #[test]
    fn test_style_vars_injected_into_action_args() {
        let def = WorkflowDef {
            id: "styled_wf".into(),
            name: "Styled WF".into(),
            description: "Workflow with style".into(),
            params: vec![],
            nodes: vec![WorkflowNode::Action(ActionNode {
                id: "step1".into(),
                ability: "flow.stop".into(),
                args: HashMap::from([("msg".into(), "Hello {{_style_audience}}!".into())]),
                output_key: Some("result".into()),
                condition: None,
                on_failure: None,
            })],
            global_timeout_secs: 0,
            max_instances: 0,
            correlation_key: None,
            style: Some("children".into()),
            kb_context: None,
            channel_permissions: None,
        };

        let (store, _dir) = test_store();
        store.store_def(&def).unwrap();

        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let manifest = test_manifest();
        let engine = WorkflowEngine::new(store);

        let instance_id = engine.start("styled_wf", HashMap::new()).unwrap();
        let inst = engine.status(&instance_id).unwrap().unwrap();
        assert!(inst.style_vars.is_some());
        let vars = inst.style_vars.unwrap();
        assert_eq!(vars.get("_style_audience").unwrap(), "children");
        assert_eq!(vars.get("_style_name").unwrap(), "children");

        // Advance to verify it completes (style vars don't break execution)
        let status = engine
            .advance(&instance_id, &registry, &manifest, None, None)
            .unwrap();
        assert_eq!(status, WorkflowStatus::Completed);
    }

    // -----------------------------------------------------------------------
    // Test: no style means no style_vars
    // -----------------------------------------------------------------------

    #[test]
    fn test_no_style_no_vars() {
        let def = WorkflowDef {
            id: "plain_wf".into(),
            name: "Plain WF".into(),
            description: "Workflow without style".into(),
            params: vec![],
            nodes: vec![WorkflowNode::Action(ActionNode {
                id: "step1".into(),
                ability: "flow.stop".into(),
                args: HashMap::new(),
                output_key: None,
                condition: None,
                on_failure: None,
            })],
            global_timeout_secs: 0,
            max_instances: 0,
            correlation_key: None,
            style: None,
            kb_context: None,
            channel_permissions: None,
        };

        let (store, _dir) = test_store();
        store.store_def(&def).unwrap();

        let engine = WorkflowEngine::new(store);

        let instance_id = engine.start("plain_wf", HashMap::new()).unwrap();
        let inst = engine.status(&instance_id).unwrap().unwrap();
        assert!(inst.style_vars.is_none());
    }

    #[test]
    fn test_kb_context_inline_injected() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkflowStore::open(&dir.path().join("wf.db")).unwrap();

        let def = WorkflowDef {
            id: "kb_inline_test".into(),
            name: "KB Inline Test".into(),
            description: "Test inline KB context".into(),
            params: vec![],
            nodes: vec![WorkflowNode::Action(ActionNode {
                id: "step1".into(),
                ability: "test.action".into(),
                args: std::collections::HashMap::from([(
                    "prompt".into(),
                    "Context: {{_kb_context}}".into(),
                )]),
                output_key: None,
                condition: None,
                on_failure: None,
            })],
            global_timeout_secs: 0,
            max_instances: 0,
            correlation_key: None,
            style: None,
            kb_context: Some("Customer tier: Enterprise".into()),
            channel_permissions: None,
        };

        store.store_def(&def).unwrap();
        let engine = WorkflowEngine::new(store);
        let instance_id = engine
            .start("kb_inline_test", std::collections::HashMap::new())
            .unwrap();

        let inst = engine.store.get_instance(&instance_id).unwrap().unwrap();
        assert_eq!(inst.kb_context, Some("Customer tier: Enterprise".into()));
    }

    #[test]
    fn test_kb_context_file_ref() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkflowStore::open(&dir.path().join("wf.db")).unwrap();

        // Write a temp KB file
        let kb_file = dir.path().join("kb_content.md");
        std::fs::write(&kb_file, "Enterprise SLA: 99.9%").unwrap();

        let def = WorkflowDef {
            id: "kb_file_test".into(),
            name: "KB File Test".into(),
            description: "Test file ref KB context".into(),
            params: vec![],
            nodes: vec![WorkflowNode::Action(ActionNode {
                id: "step1".into(),
                ability: "test.action".into(),
                args: std::collections::HashMap::new(),
                output_key: None,
                condition: None,
                on_failure: None,
            })],
            global_timeout_secs: 0,
            max_instances: 0,
            correlation_key: None,
            style: None,
            kb_context: Some(format!("file:{}", kb_file.display())),
            channel_permissions: None,
        };

        store.store_def(&def).unwrap();
        let engine = WorkflowEngine::new(store);
        let instance_id = engine
            .start("kb_file_test", std::collections::HashMap::new())
            .unwrap();

        let inst = engine.store.get_instance(&instance_id).unwrap().unwrap();
        assert_eq!(inst.kb_context, Some("Enterprise SLA: 99.9%".into()));
    }

    #[test]
    fn test_no_kb_context_no_change() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkflowStore::open(&dir.path().join("wf.db")).unwrap();

        let def = WorkflowDef {
            id: "no_kb_test".into(),
            name: "No KB Test".into(),
            description: "Test no KB context".into(),
            params: vec![],
            nodes: vec![WorkflowNode::Action(ActionNode {
                id: "step1".into(),
                ability: "test.action".into(),
                args: std::collections::HashMap::new(),
                output_key: None,
                condition: None,
                on_failure: None,
            })],
            global_timeout_secs: 0,
            max_instances: 0,
            correlation_key: None,
            style: None,
            kb_context: None,
            channel_permissions: None,
        };

        store.store_def(&def).unwrap();
        let engine = WorkflowEngine::new(store);
        let instance_id = engine
            .start("no_kb_test", std::collections::HashMap::new())
            .unwrap();

        let inst = engine.store.get_instance(&instance_id).unwrap().unwrap();
        assert!(inst.kb_context.is_none());
    }
}
