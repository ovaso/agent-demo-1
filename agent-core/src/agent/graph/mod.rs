//! 有界的顺序任务图，保存节点上下文、结果与历史计划执行。
use super::{planning::PlanTask, runtime::LoopPhase};
use crate::{context::Context, tool::ToolCall};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

mod plan_binding;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeStatus {
    Pending,
    Running,
    Paused,
    Succeeded,
    Failed,
    Cancelled,
    BudgetExceeded,
    NeedsCoordinator,
    Waiting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeAttempt {
    pub attempt: u32,
    pub status: NodeStatus,
    pub output: String,
    pub validation: Option<ValidationKind>,
    pub context: Context,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationKind {
    ModelReported,
    ToolSucceeded,
    ExitCodeZero,
    OperatorVerified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeRun {
    #[serde(default)]
    pub(crate) superseded: bool,
    #[serde(default)]
    pub origin: super::delegation::NodeOrigin,
    #[serde(default)]
    pub policy: Option<super::delegation::AgentPolicy>,
    #[serde(default)]
    pub reused_from: Option<u64>,
    #[serde(default)]
    pub evidence_revision: u64,
    #[serde(default)]
    pub prior_output: String,
    #[serde(default)]
    pub(crate) read_only: bool,
    #[serde(default)]
    pub(crate) last_tool_succeeded: Option<bool>,
    #[serde(default)]
    pub(crate) last_tool_operator: bool,
    #[serde(default)]
    pub history: Vec<NodeAttempt>,
    pub task: PlanTask,
    pub status: NodeStatus,
    pub attempts: u32,
    pub output: String,
    pub validation: Option<ValidationKind>,
    pub(crate) context: Context,
    pub(crate) phase: LoopPhase,
    pub(crate) pending: VecDeque<ToolCall>,
}

impl NodeRun {
    pub(crate) fn new(task: PlanTask) -> Self {
        Self {
            superseded: false,
            origin: Default::default(),
            policy: None,
            reused_from: None,
            evidence_revision: 0,
            prior_output: String::new(),
            read_only: false,
            last_tool_succeeded: None,
            last_tool_operator: false,
            history: Vec::new(),
            task,
            status: NodeStatus::Pending,
            attempts: 0,
            output: String::new(),
            validation: None,
            context: Context::new(),
            phase: LoopPhase::Model,
            pending: VecDeque::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphRun {
    #[serde(default = "engaged_default")]
    pub(crate) engaged: bool,
    pub plan_version: u64,
    pub nodes: BTreeMap<String, NodeRun>,
}

fn engaged_default() -> bool {
    true
}

impl GraphRun {
    pub fn is_engaged(&self) -> bool {
        self.engaged
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphState {
    pub(crate) runs: Vec<GraphRun>,
    pub(crate) active: Option<String>,
    pub(crate) coordinator_context: Option<Context>,
}

impl GraphState {
    pub fn current(&self) -> Option<&GraphRun> {
        self.runs.last()
    }
    pub fn active_node(&self) -> Option<&str> {
        self.active.as_deref()
    }
    pub fn history(&self) -> &[GraphRun] {
        &self.runs
    }
    pub(crate) fn current_mut(&mut self) -> Option<&mut GraphRun> {
        self.runs.last_mut()
    }
    pub(crate) fn unfinished(&self) -> bool {
        self.current().is_some_and(|run| {
            run.nodes.values().any(|node| {
                if node.origin == super::delegation::NodeOrigin::Delegated {
                    !matches!(node.status, NodeStatus::Succeeded | NodeStatus::Cancelled)
                } else {
                    run.engaged && node.status != NodeStatus::Succeeded
                }
            })
        })
    }
    pub(crate) fn ready_delegation(&self) -> Option<&str> {
        let graph = self.current()?;
        graph
            .nodes
            .iter()
            .find(|(_, node)| {
                node.origin == super::delegation::NodeOrigin::Delegated
                    && matches!(node.status, NodeStatus::Pending | NodeStatus::Paused)
                    && node.task.depends_on.iter().all(|id| {
                        graph
                            .nodes
                            .get(id)
                            .is_some_and(|dep| dep.status == NodeStatus::Succeeded)
                    })
            })
            .map(|(id, _)| id.as_str())
    }
    pub(crate) fn has_open_delegations(&self) -> bool {
        self.current().is_some_and(|run| {
            run.nodes.values().any(|node| {
                node.origin == super::delegation::NodeOrigin::Delegated
                    && !matches!(node.status, NodeStatus::Succeeded | NodeStatus::Cancelled)
            })
        })
    }
    pub(crate) fn ready(&self) -> Option<&str> {
        let graph = self.current()?;
        graph
            .nodes
            .iter()
            .find(|(_, node)| {
                matches!(node.status, NodeStatus::Pending | NodeStatus::Paused)
                    && node.task.depends_on.iter().all(|id| {
                        graph
                            .nodes
                            .get(id)
                            .is_some_and(|node| node.status == NodeStatus::Succeeded)
                    })
            })
            .map(|(id, _)| id.as_str())
    }
}
