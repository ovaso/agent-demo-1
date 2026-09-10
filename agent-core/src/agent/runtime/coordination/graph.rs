use super::super::{LoopPhase, RunState, RunStore, Runtime, RuntimeError, WorkIntent};
use crate::agent::runtime::execution;
use crate::agent::{
    graph::NodeStatus,
    routing::{ExecutionMode, RouteChange},
};
use crate::{memory::MemoryStore, model::ModelProvider, trace::TraceSink};

impl<M: ModelProvider, R: RunStore, S: MemoryStore, T: TraceSink> Runtime<M, R, S, T> {
    /// 在当前工具批次结算后的边界切换；不会重置预算。
    pub fn route(
        &mut self,
        id: &str,
        mode: ExecutionMode,
        reason: &str,
    ) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        if let LoopPhase::ToolInFlight { call_id } = &state.phase {
            return Err(RuntimeError::NeedsResolution(call_id.clone()));
        }
        let status = state.status.clone();
        if state.intent == WorkIntent::PlanOnly && mode == ExecutionMode::Graph {
            if state.plans.current().is_none() {
                return Err(RuntimeError::Invalid("尚无计划".into()));
            }
            state.routing.request(mode, reason)?;
        } else {
            request_route(&mut state, mode, reason)?;
            apply_route(&mut state)?;
        }
        state.status = status;
        self.commit(&mut state)?;
        Ok(state)
    }

    /// 操作者显式重试已知失败的节点；未知工具结果仍需先核实。
    pub fn retry_node(&mut self, id: &str, node: &str) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        request_retry(&mut state, node, true)?;
        self.commit(&mut state)?;
        Ok(state)
    }
}

pub(in crate::agent::runtime) fn request_route(
    state: &mut RunState,
    mode: ExecutionMode,
    reason: &str,
) -> Result<(), RuntimeError> {
    if mode == ExecutionMode::Graph
        && (state.intent == WorkIntent::PlanOnly || state.plans.current().is_none())
    {
        return Err(RuntimeError::Invalid(
            "进入 Graph 需要可执行任务和已保存计划".into(),
        ));
    }
    state.routing.request(mode, reason)?;
    if mode == ExecutionMode::Loop
        && let Some(id) = state.graph.active.clone()
        && let Some(node) = state
            .graph
            .current_mut()
            .and_then(|run| run.nodes.get_mut(&id))
    {
        node.output = format!("请求协调者处理：{reason}");
    }
    Ok(())
}

pub(in crate::agent::runtime) fn apply_route(state: &mut RunState) -> Result<(), RuntimeError> {
    if state.phase != LoopPhase::Model || !state.pending.is_empty() {
        return Ok(());
    }
    let Some((mode, reason)) = state.routing.pending.clone() else {
        return Ok(());
    };
    if state.intent == WorkIntent::PlanOnly && mode == ExecutionMode::Graph {
        return Ok(());
    }
    if mode == state.routing.mode {
        if mode == ExecutionMode::Loop
            && state.graph.active.is_some()
            && state.agent_policy().is_some()
        {
            execution::graph::finish_node(state, NodeStatus::NeedsCoordinator, None, None)?;
        }
        state.routing.pending = None;
        return Ok(());
    }
    if mode == ExecutionMode::Graph {
        if state.graph.has_open_delegations() {
            return Ok(());
        }
        let plan = state
            .plans
            .current()
            .ok_or_else(|| RuntimeError::Invalid("缺少图计划".into()))?;
        state
            .graph
            .bind(state.plans.revision(), plan, state.work_revision)?;
        state.graph.current_mut().expect("bound graph").engaged = true;
    } else if state.graph.active.is_some() {
        let status = if state.agent_policy().is_some() {
            NodeStatus::NeedsCoordinator
        } else {
            NodeStatus::Paused
        };
        execution::graph::finish_node(state, status, None, None)?;
    }
    state.routing.history.push(RouteChange {
        from: state.routing.mode,
        to: mode,
        reason,
        plan_version: state.plans.revision(),
        model_calls_used: state.budget.model_calls,
    });
    state.routing.mode = mode;
    state.routing.pending = None;
    Ok(())
}

pub(in crate::agent::runtime) fn request_node(
    state: &mut RunState,
    id: &str,
) -> Result<(), RuntimeError> {
    if state.requested_node.is_some() {
        return Err(RuntimeError::Invalid("已有节点排队，请等待其调度".into()));
    }
    if state.graph.active.is_some() {
        return Err(RuntimeError::Invalid("只能由执行中的协调者选择节点".into()));
    }
    if !state
        .graph
        .current()
        .is_some_and(|run| run.nodes.contains_key(id))
    {
        let plan = state
            .plans
            .current()
            .ok_or_else(|| RuntimeError::Invalid("尚无工作节点或计划".into()))?;
        state
            .graph
            .bind(state.plans.revision(), plan, state.work_revision)?;
    }
    let graph = state
        .graph
        .current()
        .ok_or_else(|| RuntimeError::Invalid("尚无图".into()))?;
    let node = graph
        .nodes
        .get(id)
        .ok_or_else(|| RuntimeError::NotFound(id.into()))?;
    if state.intent == WorkIntent::PlanOnly
        && node.origin == crate::agent::delegation::NodeOrigin::Planned
    {
        return Err(RuntimeError::Invalid(
            "只规划模式不能启动执行计划节点".into(),
        ));
    }
    if !matches!(
        node.status,
        NodeStatus::Pending | NodeStatus::Paused | NodeStatus::NeedsCoordinator
    ) || !node.task.depends_on.iter().all(|id| {
        graph
            .nodes
            .get(id)
            .is_some_and(|node| node.status == NodeStatus::Succeeded)
    }) {
        return Err(RuntimeError::Invalid("节点尚未就绪或已经结束".into()));
    }
    let planned = node.origin == crate::agent::delegation::NodeOrigin::Planned;
    state.requested_node = Some(id.into());
    if planned {
        state.graph.current_mut().expect("graph").engaged = true;
    }
    Ok(())
}

pub(in crate::agent::runtime) fn request_retry(
    state: &mut RunState,
    id: &str,
    operator: bool,
) -> Result<(), RuntimeError> {
    if state.requested_node.is_some() {
        return Err(RuntimeError::Invalid("已有节点排队".into()));
    }
    use crate::agent::planning::{TaskAction, ToolCheck};
    if state.intent == WorkIntent::PlanOnly || state.graph.active.is_some() {
        return Err(RuntimeError::Invalid("当前不能重试图节点".into()));
    }
    let graph = state
        .graph
        .current_mut()
        .ok_or_else(|| RuntimeError::Invalid("尚无图运行".into()))?;
    let node = graph
        .nodes
        .get_mut(id)
        .ok_or_else(|| RuntimeError::NotFound(id.into()))?;
    if node.status != NodeStatus::Failed || node.attempts >= 3 {
        return Err(RuntimeError::Invalid(
            "只能重试已知失败节点，每节点最多 3 次尝试".into(),
        ));
    }
    if !operator
        && let TaskAction::Tool { name, check, .. } = &node.task.action
        && *check != ToolCheck::ExitCodeZero
        && !state
            .tools
            .iter()
            .any(|tool| tool.name() == name && tool.is_read_only())
    {
        return Err(RuntimeError::Invalid(
            "有写入能力的失败节点需操作者明确重试或重新规划".into(),
        ));
    }
    node.history.push(crate::agent::graph::NodeAttempt {
        attempt: node.attempts,
        status: node.status,
        output: std::mem::take(&mut node.output),
        validation: node.validation.take(),
        context: std::mem::take(&mut node.context),
    });
    node.status = NodeStatus::Pending;
    state.requested_node = Some(id.into());
    Ok(())
}
