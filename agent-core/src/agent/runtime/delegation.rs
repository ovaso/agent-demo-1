use super::super::{
    delegation::{AgentPolicy, AgentSpec, NodeOrigin},
    graph::{GraphRun, NodeRun, NodeStatus},
    planning::{PlanTask, TaskAction},
};
use super::{RunState, RunStore, Runtime, RuntimeError, WorkIntent};
use crate::{memory::MemoryStore, model::ModelProvider, trace::TraceSink};
use std::collections::{BTreeMap, BTreeSet};

impl<M: ModelProvider, R: RunStore, S: MemoryStore, T: TraceSink> Runtime<M, R, S, T> {
    pub fn delegate(&mut self, id: &str, spec: AgentSpec) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        create(&mut state, spec)?;
        self.commit(&mut state)?;
        Ok(state)
    }
    pub fn set_agent_budget(
        &mut self,
        id: &str,
        agent: &str,
        max_steps: u64,
    ) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        budget(&mut state, agent, max_steps, true)?;
        self.commit(&mut state)?;
        Ok(state)
    }
    pub fn cancel_agent(&mut self, id: &str, agent: &str) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        cancel(&mut state, agent, true)?;
        super::message_delivery::tick(&mut state, super::collaboration::now_ms());
        self.commit(&mut state)?;
        Ok(state)
    }
}

pub(super) fn create(state: &mut RunState, spec: AgentSpec) -> Result<(), RuntimeError> {
    if state.graph.active.is_some() {
        return Err(RuntimeError::Invalid(
            "当前由协调者统一创建子 Agent，请交回协调者分派".into(),
        ));
    }
    if state.delegations_created >= state.limits.max_delegations {
        return Err(RuntimeError::Invalid("委托总数达到上限".into()));
    }
    if spec.name.is_empty()
        || spec.name.len() > 64
        || spec.name.contains(char::is_whitespace)
        || spec.instruction.trim().is_empty()
        || spec.instruction.len() > 8192
        || spec.acceptance.is_empty()
        || spec.acceptance.len() > 16
        || spec
            .acceptance
            .iter()
            .any(|text| text.trim().is_empty() || text.len() > 1024)
        || spec.max_steps == 0
        || spec.max_steps > state.limits.max_steps
    {
        return Err(RuntimeError::Invalid(
            "Agent 名称、任务、验收或局部预算无效".into(),
        ));
    }
    if state
        .graph
        .history()
        .iter()
        .any(|run| run.nodes.contains_key(&spec.name))
    {
        return Err(RuntimeError::Invalid(
            "名称已被工作节点使用，请读取已有结果或换一个名称".into(),
        ));
    }
    let mut tools = spec.tools.unwrap_or_else(|| {
        state
            .tools
            .iter()
            .filter(|tool| state.intent != WorkIntent::PlanOnly || tool.is_read_only())
            .map(|tool| tool.name().to_owned())
            .collect()
    });
    tools.sort();
    tools.dedup();
    if tools.iter().any(|name| !state.tool_allowed(name)) {
        return Err(RuntimeError::Invalid(
            "子 Agent 请求了父任务未允许的工具".into(),
        ));
    }
    let dependencies: BTreeSet<_> = spec.depends_on.iter().collect();
    if dependencies.len() != spec.depends_on.len()
        || dependencies.iter().any(|id| {
            !state
                .graph
                .current()
                .is_some_and(|graph| graph.nodes.contains_key(*id))
        })
    {
        return Err(RuntimeError::Invalid("委托依赖重复或不存在".into()));
    }
    if state.graph.current().is_none() {
        state.graph.runs.push(GraphRun {
            plan_version: 0,
            engaged: false,
            nodes: BTreeMap::new(),
        });
    }
    let graph = state.graph.current_mut().expect("graph exists");
    if graph.nodes.len() >= 64 {
        return Err(RuntimeError::Invalid("工作节点总数达到上限".into()));
    }
    let task = PlanTask {
        id: spec.name.clone(),
        description: spec.instruction.chars().take(120).collect(),
        depends_on: spec.depends_on,
        acceptance: spec.acceptance,
        action: TaskAction::Agent {
            prompt: spec.instruction,
        },
    };
    let mut node = NodeRun::new(task);
    node.origin = NodeOrigin::Delegated;
    node.policy = Some(AgentPolicy {
        tools,
        max_steps: spec.max_steps,
        model_calls: 0,
        budget_changes: Vec::new(),
    });
    graph.nodes.insert(spec.name, node);
    state.delegations_created += 1;
    let sequence = state.delegations_created as u64;
    super::step_budget::record_control(state, "delegate", &sequence.to_le_bytes());
    Ok(())
}

pub(super) fn budget(
    state: &mut RunState,
    id: &str,
    max_steps: u64,
    operator: bool,
) -> Result<(), RuntimeError> {
    if !operator && state.graph.active.is_some() {
        return Err(RuntimeError::Invalid(
            "只有协调者可调整子 Agent 额度".into(),
        ));
    }
    let node = state
        .graph
        .current_mut()
        .and_then(|run| run.nodes.get_mut(id))
        .ok_or_else(|| RuntimeError::NotFound(id.into()))?;
    let policy = node
        .policy
        .as_mut()
        .ok_or_else(|| RuntimeError::Invalid("此节点没有独立委托预算".into()))?;
    if max_steps <= policy.model_calls
        || max_steps > state.limits.max_steps
        || policy.budget_changes.len() >= 16
        || matches!(node.status, NodeStatus::Succeeded | NodeStatus::Cancelled)
    {
        return Err(RuntimeError::Invalid(
            "新额度必须大于已消耗步数且不超过根额度；已结束委托不可调整".into(),
        ));
    }
    policy.budget_changes.push((policy.max_steps, max_steps));
    policy.max_steps = max_steps;
    if node.status == NodeStatus::BudgetExceeded {
        node.status = NodeStatus::Paused;
    }
    Ok(())
}

pub(super) fn cancel(state: &mut RunState, id: &str, operator: bool) -> Result<(), RuntimeError> {
    if !operator && state.graph.active.is_some() {
        return Err(RuntimeError::Invalid(
            "需先在安全边界交回协调者，再取消子任务".into(),
        ));
    }
    if state.graph.active_node() == Some(id) {
        if let super::LoopPhase::ToolInFlight { call_id } = &state.phase {
            return Err(RuntimeError::NeedsResolution(call_id.clone()));
        }
        let status = state.status.clone();
        for call in state.pending.drain(..) {
            state
                .context
                .push_tool(call.id(), call.name(), "未执行：操作者取消任务");
        }
        super::graph_execution::finish_node(state, NodeStatus::Cancelled, None, None)?;
        state.status = status;
        return Ok(());
    }
    let node = state
        .graph
        .current_mut()
        .and_then(|run| run.nodes.get_mut(id))
        .ok_or_else(|| RuntimeError::NotFound(id.into()))?;
    if matches!(node.status, NodeStatus::Succeeded | NodeStatus::Cancelled) {
        return Err(RuntimeError::Invalid("节点已结束".into()));
    }
    for call in node.pending.drain(..) {
        node.context
            .push_tool(call.id(), call.name(), "未执行：协调者取消任务");
    }
    node.status = NodeStatus::Cancelled;
    if state.requested_node.as_deref() == Some(id) {
        state.requested_node = None;
    }
    Ok(())
}
