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
    ToolInFlight { call_id: String },
    FinishSession { summary: String },
    Done,
}

/// 一个根任务的一致检查点。会话、待调用工具和预算在同一提交中保存。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunState {
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
        &self.context
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
        if matches!(self.status, RunStatus::Completed)
            && (self.phase != LoopPhase::Done || self.result.is_none() || !self.pending.is_empty())
        {
            return Err(RuntimeError::Invalid("已完成任务的检查点不完整".into()));
        }
        Ok(())
    }
}
