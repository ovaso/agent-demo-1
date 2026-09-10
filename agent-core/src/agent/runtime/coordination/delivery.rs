use super::super::{LoopPhase, RunState, RuntimeError};
use crate::agent::{
    collaboration::{CollaborationMessage, MessageStatus},
    graph::NodeStatus,
};
use serde_json::{Value, json};

fn matches_actor(state: &RunState, actor: &str, attempt: u32, version: u64) -> bool {
    if matches!(actor, "main" | "operator") {
        return true;
    }
    state
        .graph
        .current()
        .is_some_and(|graph| graph.plan_version == version)
        && super::messages::binding(state, actor).ok() == Some(attempt)
}

pub(in crate::agent::runtime) fn tick(state: &mut RunState, now: u64) -> Result<(), RuntimeError> {
    let changes: Vec<_> = state
        .collaboration
        .messages
        .values()
        .filter_map(|message| {
            let status = match &message.status {
                MessageStatus::Pending
                    if message.deadline_ms.is_some_and(|deadline| now >= deadline) =>
                {
                    Some(MessageStatus::Expired)
                }
                MessageStatus::Pending
                    if !matches_actor(
                        state,
                        &message.from,
                        message.from_attempt,
                        message.plan_version,
                    ) || !matches_actor(
                        state,
                        &message.to,
                        message.to_attempt,
                        message.plan_version,
                    ) =>
                {
                    Some(MessageStatus::Cancelled {
                        reason: "参与者已结束或计划版本已改变".into(),
                    })
                }
                MessageStatus::Notice
                    if !message.delivered
                        && !matches_actor(
                            state,
                            &message.to,
                            message.to_attempt,
                            message.plan_version,
                        ) =>
                {
                    Some(MessageStatus::Cancelled {
                        reason: "接收者不可用".into(),
                    })
                }
                _ => None,
            };
            status.map(|status| (message.id.clone(), status))
        })
        .collect();
    for (id, status) in changes {
        let message = state
            .collaboration
            .messages
            .get_mut(&id)
            .ok_or_else(|| RuntimeError::Invalid(format!("更新协作消息 {id} 时记录不存在")))?;
        state.collaboration.sequence += 1;
        message.status = status;
        message.sequence = state.collaboration.sequence;
    }
    if let Some(graph) = state.graph.current_mut() {
        for node in graph.nodes.values_mut() {
            if node.status == NodeStatus::Waiting
                && let LoopPhase::Waiting { request_id } = &node.phase
                && state
                    .collaboration
                    .get(request_id)
                    .is_some_and(|request| request.status != MessageStatus::Pending)
            {
                node.status = NodeStatus::Paused;
            }
        }
    }
    Ok(())
}

pub(in crate::agent::runtime) fn view(message: &CollaborationMessage, actor: &str) -> Value {
    let (body, by) = if message.from == actor {
        match &message.status {
            MessageStatus::Answered { by, body, .. } => (body.as_str(), by.as_str()),
            MessageStatus::Cancelled { reason } => (reason.as_str(), "runtime"),
            MessageStatus::Expired => ("请求已超时", "runtime"),
            _ => (message.body.as_str(), message.from.as_str()),
        }
    } else {
        (message.body.as_str(), message.from.as_str())
    };
    json!({"id":message.id,"sequence":message.sequence,"from":message.from,"to":message.to,"plan_version":message.plan_version,"status":message.status.label(),"body":body,"by":by,"deadline_ms":message.deadline_ms})
}

pub(in crate::agent::runtime) fn inbox(state: &RunState, after: u64, unseen: bool) -> Vec<Value> {
    let actor = state.actor();
    let mut messages: Vec<_> = state
        .collaboration
        .messages
        .values()
        .filter(|message| {
            let incoming = message.to == actor
                && matches_actor(state, &actor, message.to_attempt, message.plan_version)
                && (!unseen || !message.delivered)
                && matches!(
                    message.status,
                    MessageStatus::Pending | MessageStatus::Notice
                );
            let outgoing = message.from == actor
                && matches_actor(state, &actor, message.from_attempt, message.plan_version)
                && (!unseen || !message.response_seen)
                && !matches!(
                    message.status,
                    MessageStatus::Pending | MessageStatus::Notice
                );
            message.sequence > after && (incoming || outgoing)
        })
        .collect();
    messages.sort_by_key(|message| message.sequence);
    let mut bytes = 256;
    let mut result = Vec::new();
    for message in messages.into_iter().take(16) {
        let value = view(message, &actor);
        let size = value.to_string().len();
        if bytes + size > state.limits.max_tool_output_bytes {
            break;
        }
        bytes += size;
        result.push(value);
    }
    result
}

pub(in crate::agent::runtime) fn mark_seen(state: &mut RunState, views: &[Value]) {
    let actor = state.actor();
    for view in views {
        if let Some(id) = view["id"].as_str()
            && let Some(message) = state.collaboration.messages.get_mut(id)
        {
            if message.to == actor {
                message.delivered = true;
            }
            if message.from == actor {
                message.response_seen = true;
            }
        }
    }
}

pub(in crate::agent::runtime) struct WaitResult {
    pub text: String,
    pub succeeded: bool,
}

pub(in crate::agent::runtime) fn wait_result(
    state: &mut RunState,
    id: &str,
) -> Result<Option<WaitResult>, RuntimeError> {
    let actor = state.actor();
    let message = state
        .collaboration
        .get(id)
        .ok_or_else(|| RuntimeError::NotFound(id.into()))?;
    if message.from != actor
        || !matches_actor(state, &actor, message.from_attempt, message.plan_version)
    {
        return Err(RuntimeError::Invalid("请求不属于当前节点及计划版本".into()));
    }
    if message.deadline_ms.is_none() {
        return Err(RuntimeError::Invalid("通知没有答复等待状态".into()));
    }
    if message.status == MessageStatus::Pending {
        return Ok(None);
    }
    let succeeded = matches!(
        message.status,
        MessageStatus::Answered {
            declined: false,
            ..
        }
    );
    let result = view(message, &actor).to_string();
    if result.len() > state.limits.max_tool_output_bytes {
        return Err(RuntimeError::Invalid("答复超过工具结果上限".into()));
    }
    state
        .collaboration
        .messages
        .get_mut(id)
        .ok_or_else(|| RuntimeError::Invalid(format!("接纳答复 {id} 时记录不存在")))?
        .response_seen = true;
    Ok(Some(WaitResult {
        text: result,
        succeeded,
    }))
}

pub(in crate::agent::runtime) fn cancel_all(state: &mut RunState, reason: &str) {
    for message in state.collaboration.messages.values_mut() {
        if message.status == MessageStatus::Pending {
            state.collaboration.sequence += 1;
            message.status = MessageStatus::Cancelled {
                reason: reason.into(),
            };
            message.sequence = state.collaboration.sequence;
        }
    }
}

pub(in crate::agent::runtime) fn supersede(state: &mut RunState) {
    cancel_all(state, "计划已修订，旧请求失效");
    let settled: Vec<_> = state
        .graph
        .current()
        .into_iter()
        .flat_map(|run| &run.nodes)
        .filter_map(|(id, node)| {
            let LoopPhase::Waiting { request_id } = &node.phase else {
                return None;
            };
            let text = state
                .collaboration
                .get(request_id)
                .map(|message| view(message, &format!("node/{id}")).to_string())
                .unwrap_or_default();
            Some((id.clone(), format!("计划已修订，以下仅作历史记录：{text}")))
        })
        .collect();
    if let Some(graph) = state.graph.current_mut() {
        for (id, text) in settled {
            if let Some(node) = graph.nodes.get_mut(&id) {
                for (index, call) in node.pending.drain(..).enumerate() {
                    node.context.push_tool(
                        call.id(),
                        call.name(),
                        if index == 0 {
                            text.as_str()
                        } else {
                            "未执行：计划已修订"
                        },
                    );
                }
                node.status = NodeStatus::Cancelled;
                node.superseded = true;
                node.phase = LoopPhase::Model;
            }
        }
    }
}
