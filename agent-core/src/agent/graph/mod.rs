//! 有界的顺序任务图，保存节点上下文、结果与历史计划执行。
use super::{
    planning::{Plan, PlanTask, TaskAction, ToolCheck},
    runtime::{LoopPhase, RuntimeError},
};
use crate::{context::Context, tool::ToolCall};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeStatus {
    Pending,
    Running,
    Paused,
    Succeeded,
    Failed,
    Cancelled,
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
    fn new(task: PlanTask) -> Self {
        Self {
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
    pub plan_version: u64,
    pub nodes: BTreeMap<String, NodeRun>,
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
            run.nodes
                .values()
                .any(|node| node.status != NodeStatus::Succeeded)
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
    pub(crate) fn bind(
        &mut self,
        version: u64,
        plan: &Plan,
        work_revision: u64,
    ) -> Result<(), RuntimeError> {
        if self
            .current()
            .is_some_and(|run| run.plan_version == version)
        {
            return Ok(());
        }
        if self.active.is_some() {
            return Err(RuntimeError::Invalid(
                "活动节点未结算，不能替换图计划".into(),
            ));
        }
        plan.validate()?;
        if self.runs.len() >= 16 {
            return Err(RuntimeError::Invalid("图执行计划版本达到上限".into()));
        }
        // Reuse an identical successful task only when its prerequisites are reused.
        // Verification commands always run again after replanning.
        let mut nodes: BTreeMap<String, NodeRun> = plan
            .tasks
            .iter()
            .map(|task| (task.id.clone(), NodeRun::new(task.clone())))
            .collect();
        if let Some(previous) = self.current() {
            for (id, node) in &mut nodes {
                if let Some(old) = previous.nodes.get(id) {
                    node.prior_output.clone_from(&old.output);
                    if old.task == node.task
                        && matches!(old.status, NodeStatus::Paused | NodeStatus::Failed)
                    {
                        *node = old.clone();
                    }
                }
            }
            for _ in 0..nodes.len() {
                let reusable: Vec<_> = nodes
                    .iter()
                    .filter_map(|(id, node)| {
                        let old = previous.nodes.get(id)?;
                        (node.status == NodeStatus::Pending
                            && old.status == NodeStatus::Succeeded
                            && old.task == node.task
                            && ((!matches!(node.task.action, TaskAction::Agent { .. })
                                && !old.read_only)
                                || old.evidence_revision == work_revision)
                            && !matches!(
                                node.task.action,
                                TaskAction::Tool {
                                    check: ToolCheck::ExitCodeZero,
                                    ..
                                }
                            )
                            && node.task.depends_on.iter().all(|id| {
                                nodes
                                    .get(id)
                                    .is_some_and(|dep| dep.status == NodeStatus::Succeeded)
                            }))
                        .then(|| id.clone())
                    })
                    .collect();
                if reusable.is_empty() {
                    break;
                }
                for id in reusable {
                    let old = &previous.nodes[&id];
                    let node = nodes.get_mut(&id).expect("reused node");
                    node.status = old.status;
                    node.attempts = old.attempts;
                    node.output.clone_from(&old.output);
                    node.validation = old.validation;
                    node.evidence_revision = old.evidence_revision;
                    node.read_only = old.read_only;
                    node.reused_from = Some(previous.plan_version);
                    node.prior_output = String::new();
                }
            }
        }
        self.runs.push(GraphRun {
            plan_version: version,
            nodes,
        });
        Ok(())
    }
}
