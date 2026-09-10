use super::super::AgentResult;
use super::{RunBudget, RunLimits, RuntimeError};
use crate::{
    context::Context,
    memory::Memory,
    tool::{ToolCall, ToolDefinition},
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub(crate) const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PauseReason {
    User,
    Budget,
    Model(String),
    Limit(String),
    ToolResultUnknown(String),
    PlanReady,
    GraphBlocked(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    Running,
    Paused(PauseReason),
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopPhase {
    Model,
    ModelInFlight,
    Tools,
    Waiting { request_id: String },
    ToolInFlight { call_id: String },
    FinishSession { summary: String },
    Done,
}

/// 一个根任务的一致检查点。会话、待调用工具和预算在同一提交中保存。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunState {
    #[serde(default)]
    pub(crate) collaboration: super::super::collaboration::CollaborationState,
    #[serde(default)]
    pub(crate) delegations_created: usize,
    #[serde(default)]
    pub(crate) graph: super::super::graph::GraphState,
    #[serde(default)]
    pub(crate) routing: super::super::routing::RoutingState,
    #[serde(default)]
    pub(crate) requested_node: Option<String>,
    #[serde(default)]
    pub(crate) last_tool_succeeded: Option<bool>,
    #[serde(default)]
    pub(crate) last_tool_operator: bool,
    #[serde(default)]
    pub(crate) work_revision: u64,
    #[serde(default)]
    pub(crate) goal: String,
    #[serde(default)]
    pub(crate) intent: super::WorkIntent,
    #[serde(default)]
    pub(crate) planning: bool,
    #[serde(default)]
    pub(crate) plans: super::super::planning::PlanHistory,
    #[serde(default)]
    pub(crate) blackboard: super::super::blackboard::Blackboard,
    pub(crate) format_version: u32,
    pub(crate) revision: u64,
    pub(crate) id: String,
    pub(crate) session_id: String,
    pub(crate) model_name: String,
    pub(crate) tools: Vec<ToolDefinition>,
    pub(crate) context: Context,
    pub(crate) memories: Vec<Memory>,
    pub(crate) limits: RunLimits,
    pub(crate) budget: RunBudget,
    pub(crate) phase: LoopPhase,
    pub(crate) status: RunStatus,
    pub(crate) pending: VecDeque<ToolCall>,
    pub(crate) result: Option<AgentResult>,
}

impl RunState {
    pub fn collaboration(&self) -> &super::super::collaboration::CollaborationState {
        &self.collaboration
    }
    pub fn actor(&self) -> String {
        self.graph
            .active_node()
            .map_or_else(|| "main".into(), |id| format!("node/{id}"))
    }
    pub(crate) fn agent_policy(&self) -> Option<&super::super::delegation::AgentPolicy> {
        self.graph
            .current()?
            .nodes
            .get(self.graph.active_node()?)?
            .policy
            .as_ref()
    }
    pub(crate) fn tool_allowed(&self, name: &str) -> bool {
        self.tools.iter().any(|tool| {
            tool.name() == name
                && (self.intent != super::WorkIntent::PlanOnly || tool.is_read_only())
        }) && self
            .agent_policy()
            .is_none_or(|policy| policy.tools.iter().any(|tool| tool == name))
    }
    /// 读取活动或历史节点上下文，解析跨版本结果引用。
    pub fn node_context(&self, mut version: u64, id: &str) -> Option<&Context> {
        for _ in 0..16 {
            let run = self
                .graph
                .history()
                .iter()
                .find(|run| run.plan_version == version)?;
            let node = run.nodes.get(id)?;
            if self.graph.active_node() == Some(id) && self.graph.current()?.plan_version == version
            {
                return Some(&self.context);
            }
            if let Some(previous) = node.reused_from {
                if previous >= version {
                    return None;
                }
                version = previous;
            } else {
                return Some(&node.context);
            }
        }
        None
    }
    pub fn graph(&self) -> &super::super::graph::GraphState {
        &self.graph
    }
    pub fn routing(&self) -> &super::super::routing::RoutingState {
        &self.routing
    }
    pub fn goal(&self) -> &str {
        &self.goal
    }
    pub fn intent(&self) -> super::WorkIntent {
        self.intent
    }
    pub fn plans(&self) -> &super::super::planning::PlanHistory {
        &self.plans
    }
    pub fn blackboard(&self) -> &super::super::blackboard::Blackboard {
        &self.blackboard
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn status(&self) -> &RunStatus {
        &self.status
    }
    pub fn phase(&self) -> &LoopPhase {
        &self.phase
    }
    pub fn budget(&self) -> &RunBudget {
        &self.budget
    }
    pub fn limits(&self) -> &RunLimits {
        &self.limits
    }
    pub fn context(&self) -> &Context {
        self.graph
            .coordinator_context
            .as_ref()
            .unwrap_or(&self.context)
    }
    pub fn result(&self) -> Option<&AgentResult> {
        self.result.as_ref()
    }
    pub fn pending_tool_calls(&self) -> impl Iterator<Item = &ToolCall> {
        self.pending.iter()
    }

    pub(crate) fn validate(&self) -> Result<(), RuntimeError> {
        if self.format_version != FORMAT_VERSION {
            return Err(RuntimeError::Invalid("不兼容的检查点格式版本".into()));
        }
        self.limits.validate()?;
        if let Some(id) = &self.graph.active {
            if self.graph.coordinator_context.is_none()
                || !self.graph.current().is_some_and(|graph| {
                    graph
                        .nodes
                        .get(id)
                        .is_some_and(|node| node.status == super::super::graph::NodeStatus::Running)
                })
            {
                return Err(RuntimeError::Invalid("图节点与协调者检查点不一致".into()));
            }
        } else if self.graph.coordinator_context.is_some() {
            return Err(RuntimeError::Invalid("缺少活动节点的协调者上下文".into()));
        }
        if self.status == RunStatus::Completed && self.graph.unfinished() {
            return Err(RuntimeError::Invalid("根任务完成但图节点尚未结算".into()));
        }
        if self.intent == super::WorkIntent::PlanOnly && !self.planning {
            return Err(RuntimeError::Invalid("只规划检查点缺少规划能力".into()));
        }
        if self.id.trim().is_empty() || self.session_id.trim().is_empty() {
            return Err(RuntimeError::Invalid("运行和会话 ID 不能为空".into()));
        }
        if self.pending.len() > self.limits.max_calls_per_response {
            return Err(RuntimeError::Invalid("工具批次数量超限".into()));
        }
        if let LoopPhase::ToolInFlight { call_id } = &self.phase
            && self.pending.front().map(ToolCall::id) != Some(call_id.as_str())
        {
            return Err(RuntimeError::Invalid(
                "执行中的工具与待执行队列不匹配".into(),
            ));
        }
        if let LoopPhase::Waiting { request_id } = &self.phase
            && (self.graph.active.is_none()
                || self.pending.is_empty()
                || self.collaboration.get(request_id).is_none())
        {
            return Err(RuntimeError::Invalid(
                "协作等待缺少节点、原调用或请求记录".into(),
            ));
        }
        if matches!(self.status, RunStatus::Completed)
            && (self.phase != LoopPhase::Done || self.result.is_none() || !self.pending.is_empty())
        {
            return Err(RuntimeError::Invalid("已完成任务的检查点不完整".into()));
        }
        Ok(())
    }
}
