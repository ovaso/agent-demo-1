use super::{Agent, AgentError, AgentResult};
use crate::{
    context::{Context, ContextStore},
    memory::{Memory, MemoryStore},
    model::ModelProvider,
    tool::ToolCall,
    trace::{RunTrace, TraceEvent, TraceSink},
};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn execute<M, C, S, T>(
    agent: &mut Agent<M, C, S, T>,
    session_id: &str,
    context: &mut Context,
    calls: Vec<ToolCall>,
    step: usize,
    trace: &mut RunTrace,
) -> Result<Option<AgentResult>, AgentError>
where
    M: ModelProvider,
    C: ContextStore,
    S: MemoryStore,
    T: TraceSink,
{
    for call in calls {
        trace.start(
            &mut agent.trace_sink,
            "tool.call",
            json!({"tool_name": call.name(), "call_id": call.id()}),
        )?;
        trace.record(
            &mut agent.trace_sink,
            TraceEvent::new("tool.call.started")
                .with_field("session_id", session_id)
                .with_field("loop_step", step as u64)
                .with_field("call_id", call.id())
                .with_field("tool_name", call.name())
                .with_field("arguments", arguments_value(call.arguments())),
        )?;
        match agent.tools.invoke(call.name(), call.arguments()) {
            Ok(output) => {
                trace.record(
                    &mut agent.trace_sink,
                    TraceEvent::new("tool.call.completed")
                        .with_field("session_id", session_id)
                        .with_field("call_id", call.id())
                        .with_field("tool_name", call.name())
                        .with_field("output", output.content()),
                )?;
                trace.end(&mut agent.trace_sink, None)?;
                if output.finishes_session() {
                    let summary = output.content().to_owned();
                    context.push_tool(call.id(), call.name(), &summary);
                    let memory_id = format!(
                        "session-summary:{session_id}:{}",
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis()
                    );
                    trace.call(&mut agent.trace_sink, "memory.save", || {
                        agent
                            .memory_store
                            .save(Memory::new(memory_id, &summary).with_tag("session-summary"))
                    })?;
                    trace.call(&mut agent.trace_sink, "context.delete", || {
                        agent.context_store.delete(session_id)
                    })?;
                    trace.record(
                        &mut agent.trace_sink,
                        TraceEvent::new("agent.session.finished")
                            .with_field("session_id", session_id)
                            .with_field("loop_steps", step as u64)
                            .with_field("summary", summary.clone()),
                    )?;

                    return Ok(Some(AgentResult {
                        text: summary,
                        steps: step,
                        session_finished: true,
                    }));
                }
                context.push_tool(call.id(), call.name(), output.content())
            }
            Err(error) => {
                trace.record(
                    &mut agent.trace_sink,
                    TraceEvent::new("tool.call.failed")
                        .with_field("session_id", session_id)
                        .with_field("call_id", call.id())
                        .with_field("tool_name", call.name())
                        .with_field("error", error.to_string()),
                )?;
                context.push_tool(call.id(), call.name(), format!("工具调用失败：{error}"));
                trace.end(&mut agent.trace_sink, Some(&AgentError::Tool(error)))?;
            }
        }
    }

    Ok(None)
}

fn arguments_value(arguments: &crate::tool::Arguments) -> Value {
    json!(
        arguments
            .iter()
            .collect::<std::collections::BTreeMap<_, _>>()
    )
}
