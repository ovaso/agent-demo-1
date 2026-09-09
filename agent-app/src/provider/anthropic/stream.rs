use super::helpers::{arguments_from_json, required_string};
use super::usage;
use agent_core::{
    model::{ModelError, ModelResponse},
    tool::ToolCall,
};
use serde_json::Value;
use std::{collections::BTreeMap, io::BufRead};
#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

pub(super) fn parse_stream(
    response: impl BufRead,
    on_text_delta: &mut dyn FnMut(&str),
) -> Result<ModelResponse, ModelError> {
    let mut text = String::new();
    let mut usage = usage::Usage::default();
    let mut finished = false;
    let mut calls = BTreeMap::<usize, PartialToolCall>::new();

    for line in response.lines() {
        let line = line.map_err(ModelError::new)?;
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        let event: Value = serde_json::from_str(data).map_err(ModelError::new)?;

        match event.get("type").and_then(Value::as_str) {
            Some("message_start") => usage.update(event.get("message").unwrap_or(&Value::Null)),
            Some("message_delta") => usage.update(&event),
            Some("error") => return Err(ModelError::new(event.get("error").unwrap_or(&event))),
            Some("content_block_start") => {
                let index = event.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let block = event
                    .get("content_block")
                    .ok_or_else(|| ModelError::new("Anthropic 流式事件缺少内容块"))?;
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    let partial = calls.entry(index).or_default();
                    partial.id =
                        required_string(block, "id", "Anthropic 工具调用缺少 id")?.to_owned();
                    partial.name =
                        required_string(block, "name", "Anthropic 工具调用缺少名称")?.to_owned();
                }
            }
            Some("content_block_delta") => {
                let index = event.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let delta = event
                    .get("delta")
                    .ok_or_else(|| ModelError::new("Anthropic 流式事件缺少增量"))?;
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        if let Some(delta) = delta.get("text").and_then(Value::as_str) {
                            text.push_str(delta);
                            on_text_delta(delta);
                        }
                    }
                    Some("input_json_delta") => {
                        if let Some(delta) = delta.get("partial_json").and_then(Value::as_str) {
                            calls.entry(index).or_default().arguments.push_str(delta);
                        }
                    }
                    _ => {}
                }
            }
            Some("message_stop") => {
                finished = true;
                break;
            }
            _ => {}
        }
    }

    if !finished {
        return Err(ModelError::new("anthropic 流在结束标记之前中断"));
    }
    let calls = calls
        .into_values()
        .map(|call| {
            if call.id.is_empty() || call.name.is_empty() {
                return Err(ModelError::new("Anthropic 流式工具调用不完整"));
            }
            Ok(ToolCall::new(
                call.id,
                call.name,
                arguments_from_json(&call.arguments)?,
            ))
        })
        .collect::<Result<Vec<_>, ModelError>>()?;

    let response = ModelResponse::tool_calls(calls).with_usage(usage.finish());
    if text.is_empty() {
        Ok(response)
    } else {
        Ok(response.with_text(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merges_cumulative_usage_without_double_counting() {
        let events = [
            json!({"type":"message_start", "message":{"usage":{"input_tokens":10,"output_tokens":1,"cache_read_input_tokens":30,"cache_creation_input_tokens":20}}}),
            json!({"type":"content_block_delta", "delta":{"type":"text_delta","text":"Hello"}}),
            json!({"type":"message_delta", "usage":{"output_tokens":3}}),
            json!({"type":"message_delta", "usage":{"input_tokens":12,"output_tokens":5}}),
            json!({"type":"message_stop"}),
        ];
        let stream = events
            .iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect::<String>();
        let mut text = String::new();
        let response = parse_stream(stream.as_bytes(), &mut |delta| text.push_str(delta)).unwrap();
        assert_eq!(text, "Hello");
        assert_eq!(response.usage().input_tokens, Some(62));
        assert_eq!(response.usage().output_tokens, Some(5));
        assert_eq!(response.usage().total_tokens(), Some(67));
        assert_eq!(response.usage().cache_write_input_tokens, Some(20));
    }

    #[test]
    fn missing_usage_is_unknown_and_errors_or_truncation_fail() {
        let response = parse_stream(
            b"data: {\"type\":\"message_stop\"}\n".as_slice(),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(response.usage().total_tokens(), None);
        assert_eq!(response.usage().cache_hit(), None);
        assert!(
            parse_stream(
                b"data: {\"type\":\"message_start\"}\n".as_slice(),
                &mut |_| {}
            )
            .is_err()
        );
        assert!(
            parse_stream(
                b"data: {\"type\":\"error\",\"error\":{\"message\":\"failed\"}}\n".as_slice(),
                &mut |_| {}
            )
            .is_err()
        );
    }
}
