use serde_json::json;

use super::{Agent, AgentError, AgentResult};
use crate::{
    context::{Context, ContextStore},
    memory::MemoryStore,
    model::{ModelProvider, ModelRequest},
    trace::{RunTrace, TraceEvent, TraceSink},
};

pub(super) fn run<M, C, S, T>(
    agent: &mut Agent<M, C, S, T>,
    session_id: &str,
    input: String,
) -> Result<AgentResult, AgentError>
where
    M: ModelProvider,
    C: ContextStore,
    S: MemoryStore,
    T: TraceSink,
{
    run_inner(agent, session_id, input, None)
}

pub(super) fn run_stream<M, C, S, T>(
    agent: &mut Agent<M, C, S, T>,
    session_id: &str,
    input: String,
    on_text_delta: &mut dyn FnMut(&str),
) -> Result<AgentResult, AgentError>
where
    M: ModelProvider,
    C: ContextStore,
    S: MemoryStore,
    T: TraceSink,
{
    run_inner(agent, session_id, input, Some(on_text_delta))
}

fn run_inner<M, C, S, T>(
    agent: &mut Agent<M, C, S, T>,
    session_id: &str,
    input: String,
    on_text_delta: Option<&mut dyn FnMut(&str)>,
) -> Result<AgentResult, AgentError>
where
    M: ModelProvider,
    C: ContextStore,
    S: MemoryStore,
    T: TraceSink,
{
    let mut trace = RunTrace::new(session_id);
    trace.start(&mut agent.trace_sink, "agent.run", json!({}))?;
    let result = execute(agent, session_id, input, on_text_delta, &mut trace);
    let recorded = trace.finish(&mut agent.trace_sink, result.as_ref().err());
    let value = result?;
    recorded?;
    Ok(value)
}

fn execute<M, C, S, T>(
    agent: &mut Agent<M, C, S, T>,
    session_id: &str,
    input: String,
    mut on_text_delta: Option<&mut dyn FnMut(&str)>,
    trace: &mut RunTrace,
) -> Result<AgentResult, AgentError>
where
    M: ModelProvider,
    C: ContextStore,
    S: MemoryStore,
    T: TraceSink,
{
    if agent.config.max_steps == 0 {
        return Err(AgentError::InvalidConfiguration(
            "max_steps 必须大于 0".to_owned(),
        ));
    }

    trace.record(
        &mut agent.trace_sink,
        TraceEvent::new("agent.run.started")
            .with_field("session_id", session_id)
            .with_field("input", input.clone()),
    )?;
    let mut context = trace
        .call(&mut agent.trace_sink, "context.load", || {
            agent.context_store.load(session_id)
        })?
        .unwrap_or_else(Context::new);
    context.push_user(input.clone());
    trace.call(&mut agent.trace_sink, "context.save", || {
        agent.context_store.save(session_id, &context)
    })?;

    let memories = trace.call(&mut agent.trace_sink, "memory.search", || {
        agent.memory_store.search(&input)
    })?;
    trace.record(
        &mut agent.trace_sink,
        TraceEvent::new("memory.search.completed")
            .with_field("session_id", session_id)
            .with_field("result_count", memories.len() as u64),
    )?;
    let tools = trace.call(&mut agent.trace_sink, "tools.definitions", || {
        Ok::<_, AgentError>(agent.tools.definitions())
    })?;

    for step in 1..=agent.config.max_steps {
        trace.start(
            &mut agent.trace_sink,
            "agent.step",
            json!({"loop_step": step}),
        )?;
        let request = trace.call(&mut agent.trace_sink, "model.prepare", || {
            Ok::<_, AgentError>(ModelRequest::new(context.snapshot(), &memories, &tools))
        })?;
        let response = super::model_step::stream(
            &mut agent.model,
            &mut agent.trace_sink,
            trace,
            super::model_step::ModelStep {
                actor: "main",
                request,
                session_id,
                step,
            },
            on_text_delta
                .as_deref_mut()
                .map(|callback| callback as &mut dyn FnMut(&str)),
        )?;
        if !response.stop_reason().is_complete() {
            if let Some(text) = response.text_content() {
                context.push_assistant(text);
            }
            agent.context_store.save(session_id, &context)?;
            return Err(crate::model::ModelError::new(response.stop_reason().description()).into());
        }
        let (text, calls) = response.into_parts();

        if calls.is_empty() {
            let text = text.ok_or(AgentError::EmptyModelResponse)?;
            context.push_assistant(&text);
            trace.call(&mut agent.trace_sink, "context.save", || {
                agent.context_store.save(session_id, &context)
            })?;
            trace.record(
                &mut agent.trace_sink,
                TraceEvent::new("agent.run.completed")
                    .with_field("session_id", session_id)
                    .with_field("loop_steps", step as u64)
                    .with_field("output", text.clone()),
            )?;

            return Ok(AgentResult {
                text,
                steps: step,
                session_finished: false,
            });
        }

        context.push_assistant_with_tool_calls(text.unwrap_or_default(), calls.clone());

        if let Some(result) =
            super::tool_calls::execute(agent, session_id, &mut context, calls, step, trace)?
        {
            return Ok(result);
        }

        trace.call(&mut agent.trace_sink, "context.save", || {
            agent.context_store.save(session_id, &context)
        })?;
        trace.end(&mut agent.trace_sink, None)?;
    }

    Err(AgentError::MaxStepsExceeded {
        max_steps: agent.config.max_steps,
    })
}
