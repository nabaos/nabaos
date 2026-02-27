use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::chain::dsl::{ChainDef, ChainStep, ParamDef, StepCondition};
use crate::core::error::{NyayaError, Result};

// ---------------------------------------------------------------------------
// Workflow Definition (static blueprint)
// ---------------------------------------------------------------------------

/// A workflow definition -- a DAG of nodes that can pause, branch, and fan-out.
/// Superset of `ChainDef`: every chain is a linear workflow of Action nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDef {
    /// Unique workflow identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Description of what this workflow does.
    pub description: String,
    /// Parameter schema.
    pub params: Vec<ParamDef>,
    /// Ordered list of workflow nodes.
    pub nodes: Vec<WorkflowNode>,
    /// Global timeout in seconds (0 = no timeout).
    #[serde(default)]
    pub global_timeout_secs: u64,
    /// Maximum concurrent instances of this workflow (0 = unlimited).
    #[serde(default)]
    pub max_instances: u64,
    /// Correlation key template, e.g. "{{order_id}}".
    /// Used to match incoming events to waiting instances.
    #[serde(default)]
    pub correlation_key: Option<String>,
    /// Default style name for this workflow (e.g. "children", "technical").
    #[serde(default)]
    pub style: Option<String>,
    /// Inline KB context or "file:path" reference for this workflow.
    #[serde(default)]
    pub kb_context: Option<String>,
    /// Channel-level permissions for this workflow (capabilities, contact/group ACLs).
    #[serde(default)]
    pub channel_permissions: Option<crate::security::channel_permissions::ChannelPermissions>,
}

impl WorkflowDef {
    /// Parse a workflow definition from YAML.
    pub fn from_yaml(yaml: &str) -> Result<Self> {
        serde_yaml::from_str(yaml)
            .map_err(|e| NyayaError::Config(format!("Workflow YAML parse error: {}", e)))
    }

    /// Serialize to YAML.
    pub fn to_yaml(&self) -> Result<String> {
        serde_yaml::to_string(self)
            .map_err(|e| NyayaError::Config(format!("Workflow YAML serialize error: {}", e)))
    }
}

/// Convert a ChainDef into a WorkflowDef -- every ChainStep becomes an ActionNode.
impl From<ChainDef> for WorkflowDef {
    fn from(chain: ChainDef) -> Self {
        let nodes = chain
            .steps
            .into_iter()
            .map(|step| WorkflowNode::Action(ActionNode::from(step)))
            .collect();
        WorkflowDef {
            id: chain.id,
            name: chain.name,
            description: chain.description,
            params: chain.params,
            nodes,
            global_timeout_secs: 0,
            max_instances: 0,
            correlation_key: None,
            style: None,
            kb_context: None,
            channel_permissions: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Workflow Nodes
// ---------------------------------------------------------------------------

/// A compensating action to undo a previously completed action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompensationDef {
    /// Node ID for this compensation action.
    pub id: String,
    /// The node ID this compensates (the original action).
    pub compensates_node: String,
    /// Ability to call for compensation.
    pub ability: String,
    /// Arguments for the compensation call.
    #[serde(default)]
    pub args: HashMap<String, String>,
    /// Description for logging.
    #[serde(default)]
    pub description: Option<String>,
}

/// A single node in a workflow graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowNode {
    /// Invoke an ability (same semantics as ChainStep).
    Action(ActionNode),
    /// Pause until an external event arrives (webhook, bus message).
    WaitEvent(WaitEventNode),
    /// Pause for a fixed duration.
    Delay(DelayNode),
    /// Poll an ability repeatedly until a condition is met.
    WaitPoll(WaitPollNode),
    /// Fork into parallel branches, then join.
    Parallel(ParallelNode),
    /// Conditional branching (if/else on outputs).
    Branch(BranchNode),
    /// Compensating action — automatically triggered on downstream failure.
    Compensate(CompensationDef),
}

impl WorkflowNode {
    /// Return the node's ID regardless of variant.
    pub fn id(&self) -> &str {
        match self {
            WorkflowNode::Action(n) => &n.id,
            WorkflowNode::WaitEvent(n) => &n.id,
            WorkflowNode::Delay(n) => &n.id,
            WorkflowNode::WaitPoll(n) => &n.id,
            WorkflowNode::Parallel(n) => &n.id,
            WorkflowNode::Branch(n) => &n.id,
            WorkflowNode::Compensate(n) => &n.id,
        }
    }
}

// -- Action -----------------------------------------------------------------

/// Invoke an ability -- direct equivalent of `ChainStep`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionNode {
    pub id: String,
    /// The ability to invoke.
    pub ability: String,
    /// Arguments (values may contain `{{param}}` templates).
    #[serde(default)]
    pub args: HashMap<String, String>,
    /// Store the result under this key.
    #[serde(default)]
    pub output_key: Option<String>,
    /// Conditional execution.
    #[serde(default)]
    pub condition: Option<StepCondition>,
    /// Jump to this node ID on failure instead of aborting.
    #[serde(default)]
    pub on_failure: Option<String>,
}

impl From<ChainStep> for ActionNode {
    fn from(step: ChainStep) -> Self {
        ActionNode {
            id: step.id,
            ability: step.ability,
            args: step.args,
            output_key: step.output_key,
            condition: step.condition,
            on_failure: step.on_failure,
        }
    }
}

// -- WaitEvent --------------------------------------------------------------

/// Pause execution until a matching external event arrives.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitEventNode {
    pub id: String,
    /// The event type to wait for (e.g. "order.shipped").
    pub event_type: String,
    /// JSONPath filter applied to the incoming event payload.
    #[serde(default)]
    pub filter: Option<String>,
    /// Store the matched event payload under this key.
    #[serde(default)]
    pub output_key: Option<String>,
    /// Timeout in seconds. 0 = wait forever.
    #[serde(default)]
    pub timeout_secs: u64,
    /// Action to take on timeout: "fail" (default) or a node ID to jump to.
    #[serde(default = "default_on_timeout")]
    pub on_timeout: String,
}

fn default_on_timeout() -> String {
    "fail".to_string()
}

// -- Delay ------------------------------------------------------------------

/// Pause execution for a fixed duration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelayNode {
    pub id: String,
    /// Duration to wait in seconds.
    pub duration_secs: u64,
}

// -- WaitPoll ---------------------------------------------------------------

/// Poll an ability at regular intervals until a condition is met.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitPollNode {
    pub id: String,
    /// The ability to poll.
    pub ability: String,
    /// Arguments for the ability call.
    #[serde(default)]
    pub args: HashMap<String, String>,
    /// Store the ability result under this key.
    #[serde(default)]
    pub output_key: Option<String>,
    /// Condition that must be true for the poll to succeed.
    pub until: StepCondition,
    /// Seconds between poll attempts.
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
    /// Maximum time to poll before giving up, in seconds. 0 = no limit.
    #[serde(default)]
    pub timeout_secs: u64,
    /// Action on timeout: "fail" (default) or a node ID.
    #[serde(default = "default_on_timeout")]
    pub on_timeout: String,
}

fn default_poll_interval() -> u64 {
    30
}

// -- Parallel ---------------------------------------------------------------

/// Fork execution into parallel branches, then join.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelNode {
    pub id: String,
    /// Each branch is a sequence of WorkflowNodes.
    pub branches: Vec<ParallelBranch>,
    /// Join strategy.
    #[serde(default)]
    pub join: JoinStrategy,
}

/// A named branch within a parallel fork.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelBranch {
    pub branch_id: String,
    pub nodes: Vec<WorkflowNode>,
}

/// How to join parallel branches.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum JoinStrategy {
    /// Wait for all branches to complete (default).
    #[default]
    All,
    /// Wait for any one branch to complete, cancel the rest.
    Any,
    /// Wait for N branches to complete.
    N(usize),
}

// -- Branch -----------------------------------------------------------------

/// Conditional branching: evaluate conditions on outputs and pick a path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchNode {
    pub id: String,
    /// Ordered list of condition/node-list pairs. First match wins.
    pub conditions: Vec<BranchArm>,
    /// Fallback nodes if no condition matches.
    #[serde(default)]
    pub otherwise: Vec<WorkflowNode>,
}

/// One arm of a Branch node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchArm {
    pub condition: StepCondition,
    pub nodes: Vec<WorkflowNode>,
}

// ---------------------------------------------------------------------------
// Workflow Instance (runtime state)
// ---------------------------------------------------------------------------

/// Runtime state of a single workflow execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInstance {
    /// Unique instance ID (UUID).
    pub instance_id: String,
    /// The workflow definition ID this is an instance of.
    pub workflow_id: String,
    /// Current status.
    pub status: WorkflowStatus,
    /// Input parameters supplied when the workflow was started.
    pub params: HashMap<String, String>,
    /// Accumulated outputs from completed nodes.
    pub outputs: HashMap<String, String>,
    /// Current execution position.
    pub cursor: WorkflowCursor,
    /// Resolved correlation value (if the def has a correlation_key).
    pub correlation_value: Option<String>,
    /// When the current wait started (epoch seconds).
    pub wait_started_at: Option<i64>,
    /// When the current wait expires (epoch seconds).
    pub wait_deadline: Option<i64>,
    /// Details of what we are waiting for.
    pub wait_spec: Option<WaitSpec>,
    /// Error message if status is Failed.
    pub error: Option<String>,
    /// When this instance was created (epoch seconds).
    pub created_at: i64,
    /// Last update time (epoch seconds).
    pub updated_at: i64,
    /// Total execution time in milliseconds (excludes wait time).
    pub execution_ms: i64,
    /// Receipt IDs from completed action nodes.
    #[serde(default)]
    pub receipt_ids: Vec<String>,
    /// Style template variables resolved from the workflow's style setting.
    #[serde(default)]
    pub style_vars: Option<HashMap<String, String>>,
    /// Resolved KB context content for this workflow run.
    #[serde(default)]
    pub kb_context: Option<String>,
    /// Effective channel permissions resolved from the workflow definition.
    #[serde(default)]
    pub effective_permissions: Option<crate::security::channel_permissions::ChannelPermissions>,
    /// Whether this instance is currently running compensation.
    #[serde(default)]
    pub compensation_triggered: bool,
    /// Node IDs that have been compensated.
    #[serde(default)]
    pub compensated_nodes: Vec<String>,
}

/// Tracks the current execution position inside a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowCursor {
    /// Index of the current node in the top-level nodes list.
    pub node_index: usize,
    /// If we are inside a parallel or branch node, track the sub-position.
    #[serde(default)]
    pub sub_cursor: Option<Box<SubCursor>>,
}

impl WorkflowCursor {
    /// Create a cursor pointing at the first node.
    pub fn start() -> Self {
        WorkflowCursor {
            node_index: 0,
            sub_cursor: None,
        }
    }
}

/// Sub-cursor for parallel and branch nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubCursor {
    /// Which branch we are inside (index into branches or conditions).
    pub branch_index: usize,
    /// Position within that branch's node list.
    pub node_index: usize,
}

// ---------------------------------------------------------------------------
// Workflow Status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStatus {
    Created,
    Running,
    Waiting,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Compensated,
}

impl WorkflowStatus {
    /// Whether this status is terminal (no further transitions).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            WorkflowStatus::Completed
                | WorkflowStatus::Failed
                | WorkflowStatus::Cancelled
                | WorkflowStatus::TimedOut
                | WorkflowStatus::Compensated
        )
    }
}

impl std::fmt::Display for WorkflowStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            WorkflowStatus::Created => "created",
            WorkflowStatus::Running => "running",
            WorkflowStatus::Waiting => "waiting",
            WorkflowStatus::Completed => "completed",
            WorkflowStatus::Failed => "failed",
            WorkflowStatus::Cancelled => "cancelled",
            WorkflowStatus::TimedOut => "timed_out",
            WorkflowStatus::Compensated => "compensated",
        };
        write!(f, "{}", s)
    }
}

impl std::str::FromStr for WorkflowStatus {
    type Err = NyayaError;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "created" => Ok(WorkflowStatus::Created),
            "running" => Ok(WorkflowStatus::Running),
            "waiting" => Ok(WorkflowStatus::Waiting),
            "completed" => Ok(WorkflowStatus::Completed),
            "failed" => Ok(WorkflowStatus::Failed),
            "cancelled" => Ok(WorkflowStatus::Cancelled),
            "timed_out" => Ok(WorkflowStatus::TimedOut),
            "compensated" => Ok(WorkflowStatus::Compensated),
            _ => Err(NyayaError::Config(format!(
                "Unknown workflow status: {}",
                s
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Wait Specification (what a Waiting instance is waiting for)
// ---------------------------------------------------------------------------

/// Describes what a waiting workflow instance needs before it can resume.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WaitSpec {
    /// Waiting for an external event.
    Event {
        event_type: String,
        #[serde(default)]
        filter: Option<String>,
        #[serde(default)]
        output_key: Option<String>,
        #[serde(default = "default_on_timeout")]
        on_timeout: String,
    },
    /// Waiting for a fixed delay to elapse.
    Delay {
        /// Epoch seconds when the delay expires.
        resume_at: i64,
    },
    /// Polling an ability at intervals.
    Poll {
        ability: String,
        #[serde(default)]
        args: HashMap<String, String>,
        #[serde(default)]
        output_key: Option<String>,
        until: StepCondition,
        poll_interval_secs: u64,
        /// Epoch seconds of the next scheduled poll.
        next_poll_at: i64,
        #[serde(default = "default_on_timeout")]
        on_timeout: String,
    },
    /// Waiting for parallel branches to complete.
    ParallelJoin {
        parallel_node_id: String,
        /// branch_id -> status string (e.g. "running", "completed", "failed").
        branch_statuses: HashMap<String, String>,
        join: JoinStrategy,
    },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::dsl::{ChainStep, ConditionOp, ParamDef, ParamType};

    #[test]
    fn test_chain_def_to_workflow_def() {
        let chain = ChainDef {
            id: "check_weather".into(),
            name: "Check Weather".into(),
            description: "Fetch weather".into(),
            params: vec![ParamDef {
                name: "city".into(),
                param_type: ParamType::Text,
                description: "City name".into(),
                required: true,
                default: None,
            }],
            steps: vec![
                ChainStep {
                    id: "fetch".into(),
                    ability: "data.fetch_url".into(),
                    args: HashMap::from([(
                        "url".into(),
                        "https://api.weather.com/{{city}}".into(),
                    )]),
                    output_key: Some("weather_data".into()),
                    condition: None,
                    on_failure: None,
                },
                ChainStep {
                    id: "notify".into(),
                    ability: "notify.user".into(),
                    args: HashMap::from([("message".into(), "Weather: {{weather_data}}".into())]),
                    output_key: None,
                    condition: None,
                    on_failure: None,
                },
            ],
        };

        let wf: WorkflowDef = chain.into();
        assert_eq!(wf.id, "check_weather");
        assert_eq!(wf.nodes.len(), 2);
        assert_eq!(wf.params.len(), 1);
        assert_eq!(wf.global_timeout_secs, 0);
        assert_eq!(wf.max_instances, 0);
        assert!(wf.correlation_key.is_none());
        assert!(wf.style.is_none());

        // Verify nodes are ActionNodes
        match &wf.nodes[0] {
            WorkflowNode::Action(a) => {
                assert_eq!(a.id, "fetch");
                assert_eq!(a.ability, "data.fetch_url");
            }
            _ => panic!("Expected Action node"),
        }
    }

    #[test]
    fn test_workflow_yaml_roundtrip() {
        let wf = WorkflowDef {
            id: "order_flow".into(),
            name: "Order Flow".into(),
            description: "Process an order".into(),
            params: vec![ParamDef {
                name: "order_id".into(),
                param_type: ParamType::Text,
                description: "Order ID".into(),
                required: true,
                default: None,
            }],
            nodes: vec![
                WorkflowNode::Action(ActionNode {
                    id: "validate".into(),
                    ability: "order.validate".into(),
                    args: HashMap::from([("id".into(), "{{order_id}}".into())]),
                    output_key: Some("validation".into()),
                    condition: None,
                    on_failure: None,
                }),
                WorkflowNode::Delay(DelayNode {
                    id: "cool_off".into(),
                    duration_secs: 60,
                }),
                WorkflowNode::WaitEvent(WaitEventNode {
                    id: "wait_payment".into(),
                    event_type: "payment.confirmed".into(),
                    filter: Some("$.order_id == '{{order_id}}'".into()),
                    output_key: Some("payment".into()),
                    timeout_secs: 3600,
                    on_timeout: "fail".into(),
                }),
            ],
            global_timeout_secs: 7200,
            max_instances: 100,
            correlation_key: Some("{{order_id}}".into()),
            style: None,
            kb_context: None,
            channel_permissions: None,
        };

        let yaml = wf.to_yaml().unwrap();
        let parsed = WorkflowDef::from_yaml(&yaml).unwrap();
        assert_eq!(parsed.id, "order_flow");
        assert_eq!(parsed.nodes.len(), 3);
        assert_eq!(parsed.global_timeout_secs, 7200);
        assert_eq!(parsed.max_instances, 100);
        assert_eq!(parsed.correlation_key, Some("{{order_id}}".into()));
    }

    #[test]
    fn test_workflow_status_roundtrip() {
        for status in &[
            WorkflowStatus::Created,
            WorkflowStatus::Running,
            WorkflowStatus::Waiting,
            WorkflowStatus::Completed,
            WorkflowStatus::Failed,
            WorkflowStatus::Cancelled,
            WorkflowStatus::TimedOut,
            WorkflowStatus::Compensated,
        ] {
            let s = status.to_string();
            let parsed: WorkflowStatus = s.parse().unwrap();
            assert_eq!(&parsed, status);
        }
    }

    #[test]
    fn test_status_terminal() {
        assert!(!WorkflowStatus::Created.is_terminal());
        assert!(!WorkflowStatus::Running.is_terminal());
        assert!(!WorkflowStatus::Waiting.is_terminal());
        assert!(WorkflowStatus::Completed.is_terminal());
        assert!(WorkflowStatus::Failed.is_terminal());
        assert!(WorkflowStatus::Cancelled.is_terminal());
        assert!(WorkflowStatus::TimedOut.is_terminal());
    }

    #[test]
    fn test_node_id_accessor() {
        let action = WorkflowNode::Action(ActionNode {
            id: "a1".into(),
            ability: "test".into(),
            args: HashMap::new(),
            output_key: None,
            condition: None,
            on_failure: None,
        });
        assert_eq!(action.id(), "a1");

        let delay = WorkflowNode::Delay(DelayNode {
            id: "d1".into(),
            duration_secs: 10,
        });
        assert_eq!(delay.id(), "d1");
    }

    #[test]
    fn test_cursor_start() {
        let cursor = WorkflowCursor::start();
        assert_eq!(cursor.node_index, 0);
        assert!(cursor.sub_cursor.is_none());
    }

    #[test]
    fn test_wait_spec_serde() {
        let spec = WaitSpec::Poll {
            ability: "order.status".into(),
            args: HashMap::from([("id".into(), "123".into())]),
            output_key: Some("status".into()),
            until: StepCondition {
                ref_key: "status".into(),
                op: ConditionOp::Equals,
                value: "shipped".into(),
            },
            poll_interval_secs: 60,
            next_poll_at: 1700000000,
            on_timeout: "fail".into(),
        };

        let json = serde_json::to_string(&spec).unwrap();
        let parsed: WaitSpec = serde_json::from_str(&json).unwrap();
        match parsed {
            WaitSpec::Poll {
                ability,
                poll_interval_secs,
                ..
            } => {
                assert_eq!(ability, "order.status");
                assert_eq!(poll_interval_secs, 60);
            }
            _ => panic!("Expected Poll variant"),
        }
    }

    #[test]
    fn test_parallel_node_yaml() {
        let wf = WorkflowDef {
            id: "par_test".into(),
            name: "Parallel Test".into(),
            description: "Test parallel".into(),
            params: vec![],
            nodes: vec![WorkflowNode::Parallel(ParallelNode {
                id: "p1".into(),
                branches: vec![
                    ParallelBranch {
                        branch_id: "b1".into(),
                        nodes: vec![WorkflowNode::Action(ActionNode {
                            id: "b1_a1".into(),
                            ability: "task.a".into(),
                            args: HashMap::new(),
                            output_key: None,
                            condition: None,
                            on_failure: None,
                        })],
                    },
                    ParallelBranch {
                        branch_id: "b2".into(),
                        nodes: vec![WorkflowNode::Action(ActionNode {
                            id: "b2_a1".into(),
                            ability: "task.b".into(),
                            args: HashMap::new(),
                            output_key: None,
                            condition: None,
                            on_failure: None,
                        })],
                    },
                ],
                join: JoinStrategy::All,
            })],
            global_timeout_secs: 0,
            max_instances: 0,
            correlation_key: None,
            style: None,
            kb_context: None,
            channel_permissions: None,
        };

        let yaml = wf.to_yaml().unwrap();
        let parsed = WorkflowDef::from_yaml(&yaml).unwrap();
        assert_eq!(parsed.nodes.len(), 1);
        match &parsed.nodes[0] {
            WorkflowNode::Parallel(p) => {
                assert_eq!(p.branches.len(), 2);
                assert_eq!(p.join, JoinStrategy::All);
            }
            _ => panic!("Expected Parallel node"),
        }
    }

    #[test]
    fn test_branch_node_yaml() {
        let wf = WorkflowDef {
            id: "branch_test".into(),
            name: "Branch Test".into(),
            description: "Test branching".into(),
            params: vec![],
            nodes: vec![WorkflowNode::Branch(BranchNode {
                id: "br1".into(),
                conditions: vec![BranchArm {
                    condition: StepCondition {
                        ref_key: "status".into(),
                        op: ConditionOp::Equals,
                        value: "premium".into(),
                    },
                    nodes: vec![WorkflowNode::Action(ActionNode {
                        id: "premium_path".into(),
                        ability: "premium.process".into(),
                        args: HashMap::new(),
                        output_key: None,
                        condition: None,
                        on_failure: None,
                    })],
                }],
                otherwise: vec![WorkflowNode::Action(ActionNode {
                    id: "default_path".into(),
                    ability: "basic.process".into(),
                    args: HashMap::new(),
                    output_key: None,
                    condition: None,
                    on_failure: None,
                })],
            })],
            global_timeout_secs: 0,
            max_instances: 0,
            correlation_key: None,
            style: None,
            kb_context: None,
            channel_permissions: None,
        };

        let yaml = wf.to_yaml().unwrap();
        let parsed = WorkflowDef::from_yaml(&yaml).unwrap();
        match &parsed.nodes[0] {
            WorkflowNode::Branch(b) => {
                assert_eq!(b.conditions.len(), 1);
                assert_eq!(b.otherwise.len(), 1);
            }
            _ => panic!("Expected Branch node"),
        }
    }

    #[test]
    fn test_workflow_def_with_style_yaml_roundtrip() {
        let wf = WorkflowDef {
            id: "styled_flow".into(),
            name: "Styled Flow".into(),
            description: "A workflow with a style".into(),
            params: vec![],
            nodes: vec![WorkflowNode::Action(ActionNode {
                id: "step1".into(),
                ability: "test.action".into(),
                args: HashMap::new(),
                output_key: None,
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

        let yaml = wf.to_yaml().unwrap();
        let parsed = WorkflowDef::from_yaml(&yaml).unwrap();
        assert_eq!(parsed.style, Some("children".into()));
        assert_eq!(parsed.id, "styled_flow");
    }

    #[test]
    fn test_workflow_def_without_style() {
        let yaml = r#"
id: no_style_flow
name: No Style Flow
description: A workflow without style
params: []
nodes:
  - type: action
    id: step1
    ability: test.action
"#;
        let parsed = WorkflowDef::from_yaml(yaml).unwrap();
        assert!(parsed.style.is_none());
        assert_eq!(parsed.id, "no_style_flow");
    }

    #[test]
    fn test_workflow_def_with_kb_context_yaml_roundtrip() {
        let yaml = r#"
id: kb_flow
name: KB Flow
description: A workflow with kb context
params: []
nodes:
  - type: action
    id: step1
    ability: test.action
kb_context: "Customer tier: Enterprise"
"#;
        let parsed = WorkflowDef::from_yaml(yaml).unwrap();
        assert_eq!(parsed.kb_context, Some("Customer tier: Enterprise".into()));
        let yaml_out = parsed.to_yaml().unwrap();
        let reparsed = WorkflowDef::from_yaml(&yaml_out).unwrap();
        assert_eq!(
            reparsed.kb_context,
            Some("Customer tier: Enterprise".into())
        );
    }

    #[test]
    fn test_workflow_def_without_kb_context() {
        let yaml = r#"
id: no_kb_flow
name: No KB Flow
description: A workflow without kb context
params: []
nodes:
  - type: action
    id: step1
    ability: test.action
"#;
        let parsed = WorkflowDef::from_yaml(yaml).unwrap();
        assert!(parsed.kb_context.is_none());
    }

    #[test]
    fn test_chain_def_to_workflow_def_has_none_kb_context() {
        let chain = ChainDef {
            id: "test_chain".into(),
            name: "Test".into(),
            description: "Test".into(),
            params: vec![],
            steps: vec![],
        };
        let wf: WorkflowDef = chain.into();
        assert!(wf.kb_context.is_none());
    }

    #[test]
    fn test_workflow_def_with_channel_permissions() {
        let yaml = r#"
id: perm_flow
name: Perm Flow
description: A workflow with channel permissions
params: []
nodes:
  - type: action
    id: step1
    ability: test.action
channel_permissions:
  default_access: full
  channels:
    telegram:
      access: restricted
"#;
        let parsed = WorkflowDef::from_yaml(yaml).unwrap();
        assert!(parsed.channel_permissions.is_some());
        let perms = parsed.channel_permissions.unwrap();
        assert_eq!(
            perms.default_access,
            crate::security::channel_permissions::AccessLevel::Full
        );
        assert!(perms.channels.contains_key("telegram"));
    }

    #[test]
    fn test_workflow_def_without_channel_permissions() {
        let yaml = r#"
id: no_perm_flow
name: No Perm Flow
description: A workflow without channel permissions
params: []
nodes:
  - type: action
    id: step1
    ability: test.action
"#;
        let parsed = WorkflowDef::from_yaml(yaml).unwrap();
        assert!(parsed.channel_permissions.is_none());
    }

    #[test]
    fn test_chain_to_workflow_has_no_permissions() {
        let chain = ChainDef {
            id: "test_chain".into(),
            name: "Test".into(),
            description: "Test".into(),
            params: vec![],
            steps: vec![],
        };
        let wf: WorkflowDef = chain.into();
        assert!(wf.channel_permissions.is_none());
    }

    #[test]
    fn test_compensation_def_serde() {
        let yaml = r#"
type: compensate
id: comp_1
compensates_node: action_1
ability: refund_payment
args:
  order_id: "{{order_id}}"
description: "Refund payment if shipping fails"
"#;
        let node: WorkflowNode = serde_yaml::from_str(yaml).unwrap();
        match &node {
            WorkflowNode::Compensate(def) => {
                assert_eq!(def.id, "comp_1");
                assert_eq!(def.compensates_node, "action_1");
                assert_eq!(def.ability, "refund_payment");
            }
            other => panic!("Expected Compensate, got {:?}", other),
        }
    }

    #[test]
    fn test_workflow_status_compensated() {
        let status = WorkflowStatus::Compensated;
        assert!(status.is_terminal());
        assert_eq!(format!("{}", status), "compensated");
    }

    #[test]
    fn test_compensation_node_id() {
        let node = WorkflowNode::Compensate(CompensationDef {
            id: "comp_x".into(),
            compensates_node: "action_x".into(),
            ability: "undo_action".into(),
            args: HashMap::new(),
            description: None,
        });
        assert_eq!(node.id(), "comp_x");
    }
}
