use super::helpers::{arguments_from_object, required_string};
use super::usage;
use agent_core::{
    model::{ModelError, ModelResponse},
    tool::ToolCall,
};
use serde_json::Value;
pub(super) fn parse_response(response: &Value) -> Result<ModelResponse, ModelError> {
    let stop = super::stop_reason::parse(response.get("stop_reason").and_then(Value::as_str));
    let blocks = response
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| ModelError::new("Anthropic 响应缺少 content"))?;
    let mut texts = Vec::new();
    let mut reasoning = Vec::new();
    let mut calls = Vec::new();

    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    texts.push(text);
                }
            }
            Some("thinking") => {
                if let Some(text) = block.get("thinking").and_then(Value::as_str) {
                    reasoning.push(text);
                }
            }
            Some("tool_use") if stop.is_complete() => {
                let id = required_string(block, "id", "Anthropic 工具调用缺少 id")?;
                let name = required_string(block, "name", "Anthropic 工具调用缺少名称")?;
                let input = block
                    .get("input")
                    .and_then(Value::as_object)
                    .ok_or_else(|| ModelError::new("Anthropic 工具调用缺少 input"))?;
                calls.push(ToolCall::new(id, name, arguments_from_object(input)));
            }
            _ => {}
        }
    }

    let text = (!texts.is_empty()).then(|| texts.join("\n"));
    let continuation = (stop.is_complete()
        && blocks
            .iter()
            .any(|b| matches!(b["type"].as_str(), Some("thinking" | "redacted_thinking"))))
    .then(|| {
        agent_core::model::ModelContinuation::new(crate::PROTOCOL, Value::Array(blocks.clone()))
    });
    Ok(ModelResponse::tool_calls(calls)
        .with_reasoning((!reasoning.is_empty()).then(|| reasoning.join("\n")))
        .with_continuation(continuation)
        .with_response_model(
            response
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_owned),
        )
        .with_optional_text(text)
        .with_usage(usage::parse(response))
        .with_stop_reason(stop))
}
