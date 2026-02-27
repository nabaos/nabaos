use super::{get_node_id, get_on_failure, node_status, NodeRenderStatus};
use crate::chain::workflow::{WorkflowDef, WorkflowInstance, WorkflowNode};

/// Render a `WorkflowDef` as a Graphviz DOT digraph string.
///
/// When an optional `WorkflowInstance` is provided, nodes are colour-filled
/// according to their execution status. Without an instance every node uses
/// a white background.
pub fn workflow_to_dot(def: &WorkflowDef, instance: Option<&WorkflowInstance>) -> String {
    let mut lines: Vec<String> = Vec::new();

    lines.push(format!("digraph \"{}\" {{", def.id));
    lines.push("    rankdir=TD;".to_string());
    lines.push(format!("    label=\"{}\";", def.name));
    lines.push("    node [fontname=\"Helvetica\"];".to_string());

    if def.nodes.is_empty() {
        lines.push("}".to_string());
        return lines.join("\n");
    }

    lines.push(String::new()); // blank separator

    // Node declarations.
    for (i, node) in def.nodes.iter().enumerate() {
        let id = get_node_id(node);
        let label = node_label(node);
        let shape = node_shape(node);
        let fillcolor = match instance {
            Some(inst) => status_color(node_status(i, inst)),
            None => "#ffffff",
        };
        lines.push(format!(
            "    \"{}\" [label=\"{}\" shape={} style=filled fillcolor=\"{}\"];",
            id, label, shape, fillcolor
        ));
    }

    lines.push(String::new()); // blank separator

    // Edges.
    for (i, node) in def.nodes.iter().enumerate() {
        let id = get_node_id(node);

        // Sequential edge to next node.
        if i + 1 < def.nodes.len() {
            let next_id = get_node_id(&def.nodes[i + 1]);
            lines.push(format!("    \"{}\" -> \"{}\";", id, next_id));
        }

        // Failure / timeout edge (dashed, red).
        if let Some(target) = get_on_failure(node) {
            lines.push(format!(
                "    \"{}\" -> \"{}\" [style=dashed label=\"failure\" color=red];",
                id, target
            ));
        }
    }

    lines.push("}".to_string());
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Build the DOT label text for a node. Uses `\n` (literal backslash-n in DOT)
/// to produce a two-line label with the node id and a description.
fn node_label(node: &WorkflowNode) -> String {
    match node {
        WorkflowNode::Action(a) => format!("{}\\n{}", a.id, a.ability),
        WorkflowNode::WaitEvent(w) => format!("{}\\nwait {}", w.id, w.event_type),
        WorkflowNode::Delay(d) => format!("{}\\ndelay {}s", d.id, d.duration_secs),
        WorkflowNode::WaitPoll(p) => format!("{}\\npoll {}", p.id, p.ability),
        WorkflowNode::Parallel(p) => format!("{}\\nfork/join", p.id),
        WorkflowNode::Branch(b) => format!("{}\\nbranch", b.id),
        WorkflowNode::Compensate(c) => format!("{}\\ncompensate {}", c.id, c.compensates_node),
    }
}

/// Map a `WorkflowNode` variant to the corresponding DOT shape name.
fn node_shape(node: &WorkflowNode) -> &'static str {
    match node {
        WorkflowNode::Action(_) => "box",
        WorkflowNode::WaitEvent(_) => "hexagon",
        WorkflowNode::Delay(_) => "ellipse",
        WorkflowNode::WaitPoll(_) => "trapezium",
        WorkflowNode::Parallel(_) => "diamond",
        WorkflowNode::Branch(_) => "diamond",
        WorkflowNode::Compensate(_) => "ellipse",
    }
}

/// Map a `NodeRenderStatus` to a hex fill colour.
fn status_color(status: NodeRenderStatus) -> &'static str {
    match status {
        NodeRenderStatus::Completed => "#22c55e",
        NodeRenderStatus::Running => "#3b82f6",
        NodeRenderStatus::Waiting => "#eab308",
        NodeRenderStatus::Failed => "#ef4444",
        NodeRenderStatus::Pending => "#d1d5db",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
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
            name: "Test Workflow".into(),
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
    fn test_dot_basic_output() {
        let def = make_def(vec![
            action("step1", "notify.user"),
            WorkflowNode::Delay(DelayNode {
                id: "wait".into(),
                duration_secs: 60,
            }),
            action("step2", "email.send"),
        ]);

        let output = workflow_to_dot(&def, None);

        assert!(output.starts_with("digraph"), "should start with digraph");
        assert!(output.contains("shape=box"), "action should have box shape");
        assert!(
            output.contains("shape=ellipse"),
            "delay should have ellipse shape"
        );
        assert!(
            output.contains("\"step1\" -> \"wait\""),
            "should have edge step1->wait"
        );
        assert!(
            output.contains("\"wait\" -> \"step2\""),
            "should have edge wait->step2"
        );
        assert!(
            output.contains("notify.user"),
            "should contain ability name"
        );
        assert!(
            output.contains("delay 60s"),
            "should contain delay description"
        );
        assert!(
            output.contains("fillcolor=\"#ffffff\""),
            "no instance means white fill"
        );
    }

    #[test]
    fn test_dot_with_status_colors() {
        let def = make_def(vec![
            action("a", "x.a"),
            action("b", "x.b"),
            action("c", "x.c"),
        ]);
        // Cursor at index 1, status Running => a=completed, b=running, c=pending
        let inst = make_instance(1, WorkflowStatus::Running);
        let output = workflow_to_dot(&def, Some(&inst));

        assert!(
            output.contains("fillcolor=\"#22c55e\""),
            "completed node should be green, got:\n{}",
            output
        );
        assert!(
            output.contains("fillcolor=\"#3b82f6\""),
            "running node should be blue, got:\n{}",
            output
        );
        assert!(
            output.contains("fillcolor=\"#d1d5db\""),
            "pending node should be gray, got:\n{}",
            output
        );
    }

    #[test]
    fn test_dot_failure_edges() {
        let nodes = vec![
            WorkflowNode::Action(ActionNode {
                id: "risky".into(),
                ability: "deploy.push".into(),
                args: HashMap::new(),
                output_key: None,
                condition: None,
                on_failure: Some("error_handler".into()),
            }),
            action("error_handler", "deploy.rollback"),
        ];
        let def = make_def(nodes);
        let output = workflow_to_dot(&def, None);

        assert!(
            output.contains("style=dashed"),
            "failure edge should be dashed, got:\n{}",
            output
        );
        assert!(
            output.contains("label=\"failure\""),
            "failure edge should have failure label, got:\n{}",
            output
        );
        assert!(
            output.contains("color=red"),
            "failure edge should be red, got:\n{}",
            output
        );
        assert!(
            output.contains("\"risky\" -> \"error_handler\""),
            "failure edge should connect risky to error_handler, got:\n{}",
            output
        );
    }

    #[test]
    fn test_dot_empty_workflow() {
        let def = make_def(vec![]);
        let output = workflow_to_dot(&def, None);

        assert!(output.starts_with("digraph"), "should start with digraph");
        assert!(output.contains("}"), "should close the graph");
        // Should not panic and should be valid DOT.
    }
}
