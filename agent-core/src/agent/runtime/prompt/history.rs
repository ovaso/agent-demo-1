//! Per-actor observations. Model-visible state is appended, never replaced in history.
use super::super::{RunState, RuntimeError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub(in crate::agent::runtime) const SNAPSHOT_PREFIX: &str = "运行状态（数据）：";
pub(in crate::agent::runtime) const DELTA_PREFIX: &str = "运行状态增量（数据）：";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::agent::runtime) struct PromptHistory {
    #[serde(default)]
    version: u8,
    #[serde(default)]
    frames: BTreeMap<String, Frame>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Frame {
    generation: u64,
    value: Value,
}
#[derive(Serialize)]
pub(in crate::agent::runtime) struct Change {
    actor: String,
    full_snapshot: bool,
    history_generation: u64,
    fields: Vec<String>,
}

impl PromptHistory {
    pub(in crate::agent::runtime) fn validate(&self) -> Result<(), RuntimeError> {
        if self.version > 1
            || self.frames.len() > 65
            || self.frames.values().any(|f| !f.value.is_object())
        {
            return Err(RuntimeError::Invalid("提示词历史元数据无效".into()));
        }
        Ok(())
    }
}

pub(in crate::agent::runtime) fn append(
    state: &mut RunState,
) -> Result<Option<Change>, RuntimeError> {
    if !state.planning && state.graph.current().is_none() {
        return Ok(None);
    }
    if state.limits.context_window.is_none() && state.context.history_limit() < 2 {
        return Err(RuntimeError::Invalid(
            "规划上下文至少需要保留两条历史消息".into(),
        ));
    }
    if state.prompt_history.version > 1 {
        return Err(RuntimeError::Invalid("不支持的提示词历史版本".into()));
    }
    let graph = state.graph.current();
    state.prompt_history.frames.retain(|actor, _| {
        actor == "main"
            || actor
                .strip_prefix("node/")
                .is_some_and(|id| graph.is_some_and(|g| g.nodes.contains_key(id)))
    });
    let actor = state.actor();
    let next = super::view::value(state)?;
    let old = state.prompt_history.frames.get(&actor);
    let generation = state.context.generation();
    let has_snapshot = state
        .context
        .history()
        .any(|m| m.content().starts_with(SNAPSHOT_PREFIX));
    let mut full = !has_snapshot || old.is_none_or(|f| f.generation != generation);
    let patch = if full {
        next.clone()
    } else {
        difference(
            &old.ok_or_else(|| RuntimeError::Invalid("增量提示词缺少历史帧".into()))?
                .value,
            &next,
        )?
    };
    if patch.as_object().is_some_and(Map::is_empty) {
        return Ok(None);
    }
    let fields = patch
        .as_object()
        .ok_or_else(|| RuntimeError::Invalid("当前运行视图不是 JSON 对象".into()))?
        .keys()
        .cloned()
        .collect();
    let prefix = if full { SNAPSHOT_PREFIX } else { DELTA_PREFIX };
    let message = super::super::serialization::encode_prefixed(
        &patch,
        prefix,
        state.limits.max_context_bytes,
    )?;
    state.context.push_observation(message);
    // Appending the delta itself can cross the count boundary. Establish a new
    // complete view in the same request if that removed its supporting history.
    if !full && state.context.generation() != generation {
        state
            .context
            .push_observation(super::super::serialization::encode_prefixed(
                &next,
                SNAPSHOT_PREFIX,
                state.limits.max_context_bytes,
            )?);
        full = true;
    }
    state.prompt_history.version = 1;
    state.prompt_history.frames.insert(
        actor.clone(),
        Frame {
            generation: state.context.generation(),
            value: next,
        },
    );
    Ok(Some(Change {
        actor,
        full_snapshot: full,
        history_generation: state.context.generation(),
        fields,
    }))
}

fn difference(old: &Value, next: &Value) -> Result<Value, RuntimeError> {
    let old = old
        .as_object()
        .ok_or_else(|| RuntimeError::Invalid("保存的运行视图不是 JSON 对象".into()))?;
    let next = next
        .as_object()
        .ok_or_else(|| RuntimeError::Invalid("当前运行视图不是 JSON 对象".into()))?;
    let mut patch = Map::new();
    for (key, value) in next {
        if old.get(key) != Some(value) {
            patch.insert(key.clone(), value.clone());
        }
    }
    for key in old.keys() {
        if !next.contains_key(key) {
            patch.insert(key.clone(), Value::Null);
        }
    }
    Ok(Value::Object(patch))
}
