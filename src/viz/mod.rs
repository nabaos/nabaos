#[cfg(feature = "tui")]
pub mod tui;

pub mod charts;
pub mod dot;
pub mod mermaid;

use crate::chain::workflow::{WorkflowInstance, WorkflowNode, WorkflowStatus};

// ---------------------------------------------------------------------------
// Shared helpers for visualization renderers
// ---------------------------------------------------------------------------

/// Render status of a workflow node, used for coloring in diagrams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodeRenderStatus {
    Pending,
    Running,
    Completed,
    Waiting,
    Failed,
}

/// Extract the id string from any `WorkflowNode` variant.
pub(crate) fn get_node_id(node: &WorkflowNode) -> String {
    node.id().to_owned()
}

/// Extract the `on_failure` target from node variants that support it.
pub(crate) fn get_on_failure(node: &WorkflowNode) -> Option<String> {
    match node {
        WorkflowNode::Action(n) => n.on_failure.clone(),
        // WaitEvent and WaitPoll have on_timeout which acts as a failure target
        // when its value is not "fail" (a literal node ID to jump to).
        WorkflowNode::WaitEvent(n) => {
            if n.on_timeout != "fail" {
                Some(n.on_timeout.clone())
            } else {
                None
            }
        }
        WorkflowNode::WaitPoll(n) => {
            if n.on_timeout != "fail" {
                Some(n.on_timeout.clone())
            } else {
                None
            }
        }
        WorkflowNode::Delay(_)
        | WorkflowNode::Parallel(_)
        | WorkflowNode::Branch(_)
        | WorkflowNode::Compensate(_) => None,
    }
}

/// Determine the render status of a node at the given index based on the
/// runtime cursor position and workflow instance status.
pub(crate) fn node_status(index: usize, instance: &WorkflowInstance) -> NodeRenderStatus {
    let cursor_index = instance.cursor.node_index;

    if index < cursor_index {
        // Nodes before the cursor have already been executed.
        NodeRenderStatus::Completed
    } else if index == cursor_index {
        // The node at the cursor depends on the overall workflow status.
        match instance.status {
            WorkflowStatus::Running => NodeRenderStatus::Running,
            WorkflowStatus::Waiting => NodeRenderStatus::Waiting,
            WorkflowStatus::Completed => NodeRenderStatus::Completed,
            WorkflowStatus::Failed | WorkflowStatus::TimedOut | WorkflowStatus::Compensated => {
                NodeRenderStatus::Failed
            }
            WorkflowStatus::Created | WorkflowStatus::Cancelled => NodeRenderStatus::Pending,
        }
    } else {
        NodeRenderStatus::Pending
    }
}
