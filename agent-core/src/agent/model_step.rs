use super::AgentError;
use crate::{
    model::{ModelProvider, ModelRequest, ModelResponse},
    trace::{RunTrace, TraceEvent, TraceSink},
};
use serde_json::json;
use std::time::Instant;

pub(super) struct ModelStep<'a> {
    pub actor: &'a str,
    pub request: ModelRequest<'a>,
    pub session_id: &'a str,
    pub step: usize,
}

pub(super) fn stream<M: ModelProvider, T: TraceSink>(
    model: &mut M,
    sink: &mut T,
    trace: &mut RunTrace,
    step: ModelStep<'_>,
    mut on_text_delta: Option<&mut dyn FnMut(&str)>,
) -> Result<ModelResponse, AgentError> {
    let ModelStep {
        actor,
        request,
        session_id,
        step,
    } = step;
    trace.start(
        sink,
        "model.request",
        json!({"provider": std::any::type_name::<M>(), "model": model.model_name(), "loop_step": step, "actor":actor}),
    )?;
    trace.model_usage(Default::default());
    trace.record(
        sink,
        TraceEvent::new("model.request.started")
            .with_field("session_id", session_id)
            .with_field("loop_step", step as u64)
            .with_field("message_count", request.messages().len() as u64)
            .with_field("tool_count", request.tools().len() as u64),
    )?;

    let model_started = Instant::now();
    let mut first_delta_at = None;
    let mut emitted_characters = 0usize;
    let response = {
        let mut emit = |delta: &str| {
            if first_delta_at.is_none() {
                first_delta_at = Some(Instant::now());
            }
            emitted_characters += delta.chars().count();
            if let Some(callback) = on_text_delta.as_deref_mut() {
                callback(delta);
            }
        };
        model.stream(request, &mut emit)?
    };
    trace.model_usage(response.usage());
    let model_elapsed = model_started.elapsed();
    trace.record(
        sink,
        TraceEvent::new("model.response.completed")
            .with_field("response_model", response.response_model())
            .with_field("stop_reason", format!("{:?}", response.stop_reason()))
            .with_field("session_id", session_id)
            .with_field("loop_step", step as u64)
            .with_field("duration_ms", model_elapsed.as_millis() as u64)
            .with_field("emitted_characters", emitted_characters as u64)
            .with_field(
                "characters_per_second",
                if model_elapsed.is_zero() {
                    0.0
                } else {
                    emitted_characters as f64 / model_elapsed.as_secs_f64()
                },
            ),
    )?;
    if let Some(first_delta_at) = first_delta_at {
        trace.record(
            sink,
            TraceEvent::new("model.first_text_delta")
                .with_field("session_id", session_id)
                .with_field("loop_step", step as u64)
                .with_field(
                    "latency_ms",
                    first_delta_at.duration_since(model_started).as_millis() as u64,
                ),
        )?;
    }
    trace.end(sink, None)?;
    Ok(response)
}
