use super::super::{LoopPhase, RunState, RunStore, Runtime, RuntimeError};
use crate::agent::runtime::budget;
use crate::agent::{
    collaboration::{CollaborationMessage, MAX_MESSAGE_BYTES, MAX_MESSAGES, MessageStatus},
    graph::NodeStatus,
};
use crate::{memory::MemoryStore, model::ModelProvider, trace::TraceSink};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{SystemTime, UNIX_EPOCH},
};

impl<M: ModelProvider, R: RunStore, S: MemoryStore, T: TraceSink> Runtime<M, R, S, T> {
    pub fn send_notice(
        &mut self,
        id: &str,
        to: &str,
        body: &str,
    ) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        send(&mut state, to, body, None, false, true, now_ms())?;
        self.commit(&mut state)?;
        Ok(state)
    }
    pub fn answer_request(
        &mut self,
        id: &str,
        request: &str,
        body: &str,
    ) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        reply(&mut state, request, body, false, true, now_ms())?;
        self.commit(&mut state)?;
        Ok(state)
    }
}

pub(in crate::agent::runtime) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(in crate::agent::runtime) fn binding(
    state: &RunState,
    actor: &str,
) -> Result<u32, RuntimeError> {
    if matches!(actor, "main" | "operator") {
        return Ok(0);
    }
    let id = actor
        .strip_prefix("node/")
        .ok_or_else(|| RuntimeError::Invalid("地址须为 main 或 node/节点ID".into()))?;
    let graph = state
        .graph
        .current()
        .ok_or_else(|| RuntimeError::NotFound(actor.into()))?;
    let node = graph
        .nodes
        .get(id)
        .ok_or_else(|| RuntimeError::NotFound(actor.into()))?;
    if matches!(
        node.status,
        NodeStatus::Succeeded | NodeStatus::Failed | NodeStatus::Cancelled
    ) {
        return Err(RuntimeError::Invalid(
            "接收者或发送者已经结束，不会因消息自动重启".into(),
        ));
    }
    if !graph.engaged && node.origin == crate::agent::delegation::NodeOrigin::Planned {
        return Err(RuntimeError::Invalid("该计划节点尚未启用执行".into()));
    }
    Ok(node.attempts + u32::from(node.status == NodeStatus::Pending))
}

pub(in crate::agent::runtime) fn send(
    state: &mut RunState,
    to: &str,
    body: &str,
    timeout_ms: Option<u64>,
    wait: bool,
    operator: bool,
    now: u64,
) -> Result<String, RuntimeError> {
    if to == "operator" {
        return Err(RuntimeError::Invalid(
            "请向协调者 main 或 node/节点ID 发送消息".into(),
        ));
    }
    if body.is_empty() || body.len() > MAX_MESSAGE_BYTES || state.id.len() > 256 {
        return Err(RuntimeError::Invalid(
            "消息为空、超过 2048 字节或运行 ID 过长".into(),
        ));
    }
    if timeout_ms.is_some_and(|timeout| {
        timeout == 0 || timeout > crate::agent::collaboration::MAX_TIMEOUT_MS
    }) {
        return Err(RuntimeError::Invalid(
            "请求超时须为 1..=3600000 毫秒".into(),
        ));
    }
    if state.collaboration.messages.len() >= MAX_MESSAGES {
        return Err(RuntimeError::Invalid("消息总数达到 256 条上限".into()));
    }
    let from = if operator {
        "operator".into()
    } else {
        state.actor()
    };
    if from == to {
        return Err(RuntimeError::Invalid("不能向自己发送协作消息".into()));
    }
    if wait && (state.graph.active.is_none() || timeout_ms.is_none()) {
        return Err(RuntimeError::Invalid("只有节点请求可进入协作等待".into()));
    }
    let from_attempt = binding(state, &from)?;
    let to_attempt = binding(state, to)?;
    if wait {
        check_wait(state, &from, to)?;
    }
    state.collaboration.sequence += 1;
    state.work_revision += 1;
    let sequence = state.collaboration.sequence;
    let id = format!("{}:m{sequence}", state.id);
    state.collaboration.messages.insert(
        id.clone(),
        CollaborationMessage {
            id: id.clone(),
            sequence,
            from,
            to: to.into(),
            from_attempt,
            to_attempt,
            plan_version: state.graph.current().map_or(0, |run| run.plan_version),
            body: body.into(),
            deadline_ms: timeout_ms.map(|timeout| now.saturating_add(timeout)),
            status: if timeout_ms.is_some() {
                MessageStatus::Pending
            } else {
                MessageStatus::Notice
            },
            delivered: false,
            response_seen: false,
        },
    );
    Ok(id)
}

pub(in crate::agent::runtime) fn reply(
    state: &mut RunState,
    id: &str,
    body: &str,
    declined: bool,
    operator: bool,
    now: u64,
) -> Result<(), RuntimeError> {
    if body.len() > MAX_MESSAGE_BYTES {
        return Err(RuntimeError::Invalid("答复超过 2048 字节".into()));
    }
    let actor = if operator {
        "operator".into()
    } else {
        state.actor()
    };
    let message = state
        .collaboration
        .get(id)
        .ok_or_else(|| RuntimeError::NotFound(id.into()))?;
    if !operator
        && (message.to != actor
            || binding(state, &actor)? != message.to_attempt
            || state.graph.current().map_or(0, |run| run.plan_version) != message.plan_version)
    {
        return Err(RuntimeError::Invalid("答复者或任务版本与请求不匹配".into()));
    }
    if let MessageStatus::Answered {
        by,
        body: existing,
        declined: existing_declined,
    } = &message.status
    {
        return if by == &actor && existing == body && *existing_declined == declined {
            Ok(())
        } else {
            Err(RuntimeError::Conflict)
        };
    }
    if message.status != MessageStatus::Pending
        || message.deadline_ms.is_some_and(|deadline| now >= deadline)
    {
        return Err(RuntimeError::Invalid("请求已结束或超时".into()));
    }
    state.collaboration.sequence += 1;
    state.work_revision += 1;
    let sequence = state.collaboration.sequence;
    let message = state
        .collaboration
        .messages
        .get_mut(id)
        .expect("request exists");
    message.status = MessageStatus::Answered {
        by: actor,
        body: body.into(),
        declined,
    };
    message.sequence = sequence;
    budget::steps::record_control(state, "reply", id.as_bytes());
    Ok(())
}

pub(in crate::agent::runtime) fn check_wait(
    state: &RunState,
    from: &str,
    to: &str,
) -> Result<(), RuntimeError> {
    let mut edges: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if let Some(graph) = state.graph.current() {
        for (id, node) in &graph.nodes {
            if !matches!(node.status, NodeStatus::Succeeded | NodeStatus::Cancelled) {
                for dependency in &node.task.depends_on {
                    if graph
                        .nodes
                        .get(dependency)
                        .is_some_and(|dep| dep.status != NodeStatus::Succeeded)
                    {
                        edges
                            .entry(format!("node/{id}"))
                            .or_default()
                            .push(format!("node/{dependency}"));
                    }
                }
            }
            let phase = if state.graph.active_node() == Some(id) {
                &state.phase
            } else {
                &node.phase
            };
            if let LoopPhase::Waiting { request_id } = phase
                && let Some(request) = state.collaboration.get(request_id)
                && request.status == MessageStatus::Pending
            {
                edges
                    .entry(format!("node/{id}"))
                    .or_default()
                    .push(request.to.clone());
            }
        }
    }
    let mut stack = vec![to.to_owned()];
    let mut visited = BTreeSet::new();
    while let Some(node) = stack.pop() {
        if node == from {
            return Err(RuntimeError::Invalid(format!(
                "协作等待形成环：{from} → {to} → … → {from}"
            )));
        }
        if visited.insert(node.clone())
            && let Some(next) = edges.get(&node)
        {
            stack.extend(next.iter().cloned());
        }
    }
    Ok(())
}
