use super::{get_node_id, get_on_failure, node_status, NodeRenderStatus};
use crate::chain::workflow::{WorkflowDef, WorkflowInstance, WorkflowNode};

/// Render a `WorkflowDef` as a Mermaid flowchart string.
///
/// When an optional `WorkflowInstance` is provided the output includes
/// `classDef` declarations and class assignments so that nodes are coloured
/// according to their execution status.
pub fn workflow_to_mermaid(def: &WorkflowDef, instance: Option<&WorkflowInstance>) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("graph TD".to_string());

    if def.nodes.is_empty() {
        return lines.join("\n");
    }

    // Track per-status node ids for class assignment.
    let mut completed_ids: Vec<String> = Vec::new();
    let mut running_ids: Vec<String> = Vec::new();
    let mut waiting_ids: Vec<String> = Vec::new();
    let mut failed_ids: Vec<String> = Vec::new();
    let mut pending_ids: Vec<String> = Vec::new();

    // Render each node declaration.
    for (i, node) in def.nodes.iter().enumerate() {
        let id = get_node_id(node);
        let shape = render_node_shape(node);
        lines.push(format!("    {}", shape));

        // Collect status class membership.
        if let Some(inst) = instance {
            match node_status(i, inst) {
                NodeRenderStatus::Completed => completed_ids.push(id.clone()),
                NodeRenderStatus::Running => running_ids.push(id.clone()),
                NodeRenderStatus::Waiting => waiting_ids.push(id.clone()),
                NodeRenderStatus::Failed => failed_ids.push(id.clone()),
                NodeRenderStatus::Pending => pending_ids.push(id.clone()),
            }
        }

        // Sequential edge to next node.
        if i + 1 < def.nodes.len() {
            let next_id = get_node_id(&def.nodes[i + 1]);
            lines.push(format!("    {} --> {}", id, next_id));
        }

        // Failure / timeout edge (dashed).
        if let Some(target) = get_on_failure(node) {
            lines.push(format!("    {} -.->|failure| {}", id, target));
        }

        // Branch-specific labelled edges.
        if let WorkflowNode::Branch(b) = node {
            for arm in &b.conditions {
                if let Some(first) = arm.nodes.first() {
                    let first_id = get_node_id(first);
                    let label = format!(
                        "{} {} {}",
                        arm.condition.ref_key,
                        condition_op_str(&arm.condition.op),
                        arm.condition.value
                    );
                    lines.push(format!("    {} -->|{}| {}", id, label, first_id));
                }
            }
            if let Some(first_otherwise) = b.otherwise.first() {
                let first_id = get_node_id(first_otherwise);
                lines.push(format!("    {} -->|otherwise| {}", id, first_id));
            }
        }
    }

    // classDef + class assignments when an instance is provided.
    if instance.is_some() {
        lines.push(String::new());
        lines.push("    classDef completed fill:#28a745,stroke:#1e7e34,color:#fff".to_string());
        lines.push("    classDef running fill:#007bff,stroke:#0056b3,color:#fff".to_string());
        lines.push("    classDef waiting fill:#ffc107,stroke:#d39e00,color:#000".to_string());
        lines.push("    classDef failed fill:#dc3545,stroke:#bd2130,color:#fff".to_string());
        lines.push("    classDef pending fill:#6c757d,stroke:#545b62,color:#fff".to_string());

        emit_class_line(&mut lines, "completed", &completed_ids);
        emit_class_line(&mut lines, "running", &running_ids);
        emit_class_line(&mut lines, "waiting", &waiting_ids);
        emit_class_line(&mut lines, "failed", &failed_ids);
        emit_class_line(&mut lines, "pending", &pending_ids);
    }

    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Produce a Mermaid node declaration with the appropriate shape.
fn render_node_shape(node: &WorkflowNode) -> String {
    match node {
        WorkflowNode::Action(a) => {
            // Rectangle: id[label]
            format!("{}[\"{}: {}\"]", a.id, a.id, a.ability)
        }
        WorkflowNode::WaitEvent(w) => {
            // Hexagon: id{{label}}
            format!("{}{{\"{}: wait {}\"}}", w.id, w.id, w.event_type)
        }
        WorkflowNode::Delay(d) => {
            // Rounded: id(label)
            format!("{}(\"{}: delay {}s\")", d.id, d.id, d.duration_secs)
        }
        WorkflowNode::WaitPoll(p) => {
            // Trapezoid: id[/label\]
            format!("{}[/\"{}: poll {}\"\\ ]", p.id, p.id, p.ability)
        }
        WorkflowNode::Parallel(p) => {
            // Diamond: id{label}
            format!("{}{{\"{}: fork/join\"}}", p.id, p.id)
        }
        WorkflowNode::Branch(b) => {
            // Diamond: id{label}
            format!("{}{{\"{}: branch\"}}", b.id, b.id)
        }
        WorkflowNode::Compensate(c) => {
            // Rounded: id(label)
            format!("{}(\"{}: compensate {}\")", c.id, c.id, c.compensates_node)
        }
    }
}

/// Convert a `ConditionOp` to a compact display string.
fn condition_op_str(op: &crate::chain::dsl::ConditionOp) -> &'static str {
    match op {
        crate::chain::dsl::ConditionOp::Equals => "==",
        crate::chain::dsl::ConditionOp::NotEquals => "!=",
        crate::chain::dsl::ConditionOp::Contains => "contains",
        crate::chain::dsl::ConditionOp::GreaterThan => ">",
        crate::chain::dsl::ConditionOp::LessThan => "<",
        crate::chain::dsl::ConditionOp::IsEmpty => "is_empty",
        crate::chain::dsl::ConditionOp::IsNotEmpty => "is_not_empty",
    }
}

/// Emit a `class` assignment line if the id list is non-empty.
fn emit_class_line(lines: &mut Vec<String>, class_name: &str, ids: &[String]) {
    if !ids.is_empty() {
        lines.push(format!("    class {} {}", ids.join(","), class_name));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::dsl::{ConditionOp, StepCondition};
    use crate::chain::workflow::*;
    use std::collections::HashMap;

    /// Helper: build a simple ActionNode.
    fn action(id: &str, ability: &str) -> WorkflowNode {
        WorkflowNode::Action(ActionNode {
            id: id.into(),
            ability: ability.into(),
            args: HashMap::new(),
            output_key: None,
            condition: None,
            on_failure: None,
        })
    }

    /// Helper: minimal WorkflowDef around a set of nodes.
    fn make_def(nodes: Vec<WorkflowNode>) -> WorkflowDef {
        WorkflowDef {
            id: "test_wf".into(),
            name: "Test".into(),
            description: "test workflow".into(),
            params: vec![],
            nodes,
            global_timeout_secs: 0,
            max_instances: 0,
            correlation_key: None,
            style: None,
            kb_context: None,
            channel_permissions: None,
        }
    }

    /// Helper: build a WorkflowInstance at a given cursor position with a status.
    fn make_instance(node_index: usize, status: WorkflowStatus) -> WorkflowInstance {
        WorkflowInstance {
            instance_id: "inst-1".into(),
            workflow_id: "test_wf".into(),
            status,
            params: HashMap::new(),
            outputs: HashMap::new(),
            cursor: WorkflowCursor {
                node_index,
                sub_cursor: None,
            },
            correlation_value: None,
            wait_started_at: None,
            wait_deadline: None,
            wait_spec: None,
            error: None,
            created_at: 0,
            updated_at: 0,
            execution_ms: 0,
            receipt_ids: vec![],
            style_vars: None,
            kb_context: None,
            effective_permissions: None,
            compensation_triggered: false,
            compensated_nodes: vec![],
        }
    }

    #[test]
    fn test_mermaid_basic_output() {
        let def = make_def(vec![
            action("step1", "email.fetch"),
            action("step2", "llm.summarize"),
            action("step3", "notify.user"),
        ]);

        let output = workflow_to_mermaid(&def, None);

        assert!(output.starts_with("graph TD"), "should start with graph TD");
        assert!(output.contains("step1"), "should contain step1");
        assert!(output.contains("step2"), "should contain step2");
        assert!(output.contains("step3"), "should contain step3");
        assert!(
            output.contains("email.fetch"),
            "should contain ability name"
        );
        assert!(output.contains("step1 --> step2"), "should have edge 1->2");
        assert!(output.contains("step2 --> step3"), "should have edge 2->3");
    }

    #[test]
    fn test_mermaid_on_failure_edge() {
        let nodes = vec![
            WorkflowNode::Action(ActionNode {
                id: "risky".into(),
                ability: "deploy.push".into(),
                args: HashMap::new(),
                output_key: None,
                condition: None,
                on_failure: Some("rollback".into()),
            }),
            action("rollback", "deploy.rollback"),
        ];
        let def = make_def(nodes);
        let output = workflow_to_mermaid(&def, None);

        assert!(
            output.contains("risky -.->|failure| rollback"),
            "should contain dashed failure edge, got:\n{}",
            output
        );
    }

    #[test]
    fn test_mermaid_with_instance_coloring() {
        let def = make_def(vec![
            action("a", "x.a"),
            action("b", "x.b"),
            action("c", "x.c"),
        ]);
        // Cursor at index 1, status Running => a=completed, b=running, c=pending
        let inst = make_instance(1, WorkflowStatus::Running);
        let output = workflow_to_mermaid(&def, Some(&inst));

        assert!(
            output.contains("classDef completed"),
            "should have completed classDef"
        );
        assert!(
            output.contains("classDef running"),
            "should have running classDef"
        );
        assert!(
            output.contains("classDef pending"),
            "should have pending classDef"
        );
        assert!(output.contains("fill:#28a745"), "completed should be green");
        assert!(output.contains("fill:#007bff"), "running should be blue");
        assert!(output.contains("fill:#6c757d"), "pending should be gray");
        assert!(
            output.contains("class a completed"),
            "node a should be completed"
        );
        assert!(
            output.contains("class b running"),
            "node b should be running"
        );
        assert!(
            output.contains("class c pending"),
            "node c should be pending"
        );
    }

    #[test]
    fn test_mermaid_empty_workflow() {
        let def = make_def(vec![]);
        let output = workflow_to_mermaid(&def, None);
        assert_eq!(output, "graph TD");
    }

    #[test]
    fn test_mermaid_all_node_types() {
        let nodes = vec![
            action("act", "do.thing"),
            WorkflowNode::WaitEvent(WaitEventNode {
                id: "we".into(),
                event_type: "order.shipped".into(),
                filter: None,
                output_key: None,
                timeout_secs: 0,
                on_timeout: "fail".into(),
            }),
            WorkflowNode::Delay(DelayNode {
                id: "dl".into(),
                duration_secs: 30,
            }),
            WorkflowNode::WaitPoll(WaitPollNode {
                id: "wp".into(),
                ability: "status.check".into(),
                args: HashMap::new(),
                output_key: None,
                until: StepCondition {
                    ref_key: "done".into(),
                    op: ConditionOp::Equals,
                    value: "true".into(),
                },
                poll_interval_secs: 10,
                timeout_secs: 0,
                on_timeout: "fail".into(),
            }),
            WorkflowNode::Parallel(ParallelNode {
                id: "par".into(),
                branches: vec![ParallelBranch {
                    branch_id: "b1".into(),
                    nodes: vec![action("sub1", "sub.a")],
                }],
                join: JoinStrategy::All,
            }),
            WorkflowNode::Branch(BranchNode {
                id: "br".into(),
                conditions: vec![BranchArm {
                    condition: StepCondition {
                        ref_key: "tier".into(),
                        op: ConditionOp::Equals,
                        value: "premium".into(),
                    },
                    nodes: vec![action("prem", "premium.flow")],
                }],
                otherwise: vec![action("basic", "basic.flow")],
            }),
        ];

        let def = make_def(nodes);
        let output = workflow_to_mermaid(&def, None);

        // Each node type should produce a declaration.
        assert!(output.contains("act[\"act: do.thing\"]"), "action shape");
        assert!(
            output.contains("we{\"we: wait order.shipped\"}"),
            "wait event shape"
        );
        assert!(output.contains("dl(\"dl: delay 30s\")"), "delay shape");
        assert!(
            output.contains("wp[/\"wp: poll status.check\"\\"),
            "wait poll shape"
        );
        assert!(output.contains("par{\"par: fork/join\"}"), "parallel shape");
        assert!(output.contains("br{\"br: branch\"}"), "branch shape");

        // Branch edges.
        assert!(
            output.contains("br -->|tier == premium| prem"),
            "branch arm edge"
        );
        assert!(output.contains("br -->|otherwise| basic"), "otherwise edge");
    }
}
