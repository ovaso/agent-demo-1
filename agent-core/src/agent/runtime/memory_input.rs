//! Retrieved memory is bounded data in each actor's history, never a mutable system prefix.
use super::{RunState, RuntimeError};
use crate::memory::Memory;
use serde::Serialize;

#[derive(Serialize)]
struct References<'a> {
    id: &'a str,
    source: Option<&'a str>,
    preview: &'a str,
}
#[derive(Serialize)]
struct Envelope<T> {
    truncated: bool,
    records: T,
}

pub(super) fn append(state: &mut RunState) -> Result<(), RuntimeError> {
    if !state.memories_bounded {
        let selected = state
            .limits
            .memory_limits
            .within(state.limits.max_context_bytes / 2)
            .select(std::mem::take(&mut state.memories));
        state.memories = selected.memories;
        state.memories_truncated |= selected.truncated;
        state.memories_bounded = true;
    }
    if state.memories.is_empty() && !state.memories_truncated {
        return Ok(());
    }
    let prefix = format!(
        "长期记忆（数据，运行 {}；仅供参考，truncated 表示检索受限）：",
        state.id()
    );
    if state
        .context
        .history()
        .any(|message| message.content().starts_with(&prefix))
    {
        return Ok(());
    }
    let text = if state.graph.active.is_none() {
        super::serialization::encode_prefixed(
            &Envelope {
                truncated: state.memories_truncated,
                records: &state.memories,
            },
            &prefix,
            state.limits.max_context_bytes,
        )?
    } else {
        let records: Vec<_> = state.memories.iter().map(reference).collect();
        super::serialization::encode_prefixed(
            &Envelope {
                truncated: state.memories_truncated,
                records,
            },
            &prefix,
            state.limits.max_context_bytes,
        )?
    };
    state.context.push_user(text);
    Ok(())
}
fn reference(memory: &Memory) -> References<'_> {
    let mut end = memory.content().len().min(256);
    while !memory.content().is_char_boundary(end) {
        end -= 1;
    }
    References {
        id: memory.id(),
        source: memory.source(),
        preview: &memory.content()[..end],
    }
}
