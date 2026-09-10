use super::helpers::parse_arguments;
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
    let mut reasoning: Option<String> = None;
    let mut response_model = None;
    let mut usage = usage::Usage::default();
    let mut finished = false;
    let mut stop = agent_core::model::StopReason::Complete;
    let mut calls = BTreeMap::<usize, PartialToolCall>::new();

    for line in response
        .take(super::super::continuation::MAX_RESPONSE_BYTES)
        .lines()
    {
        let line = line.map_err(ModelError::new)?;
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        if data == "[DONE]" {
            finished = true;
            break;
        }

        let chunk: Value = serde_json::from_str(data).map_err(ModelError::new)?;
        if let Some(error) = chunk.get("error") {
            return Err(ModelError::new(error));
        }
        usage.update(&chunk);
        if let Some(model) = chunk.get("model").and_then(Value::as_str) {
            response_model = Some(model.to_owned());
        }
        if let Some(reason) = chunk
            .pointer("/choices/0/finish_reason")
            .and_then(Value::as_str)
        {
            stop = super::super::stop_reason::openai(Some(reason));
        }
        let Some(delta) = chunk.pointer("/choices/0/delta") else {
            continue;
        };

        if let Some(part) = delta.get("reasoning_content").and_then(Value::as_str) {
            reasoning.get_or_insert_with(String::new).push_str(part);
        }
        if let Some(content) = delta.get("content").and_then(Value::as_str) {
            text.push_str(content);
            on_text_delta(content);
        }

        for call in delta
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let partial = calls.entry(index).or_default();
            if let Some(id) = call.get("id").and_then(Value::as_str) {
                partial.id.push_str(id);
            }
            if let Some(function) = call.get("function") {
                if let Some(name) = function.get("name").and_then(Value::as_str) {
                    partial.name.push_str(name);
                }
                if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                    partial.arguments.push_str(arguments);
                }
            }
        }
    }

    if !finished {
        return Err(ModelError::new("openai 流在结束标记之前中断"));
    }
    if !stop.is_complete() {
        calls.clear();
    }
    let calls = calls
        .into_values()
        .map(|call| {
            if call.id.is_empty() || call.name.is_empty() {
                return Err(ModelError::new("OpenAI 流式工具调用不完整"));
            }
            Ok(ToolCall::new(
                call.id,
                call.name,
                parse_arguments(&call.arguments)?,
            ))
        })
        .collect::<Result<Vec<_>, ModelError>>()?;

    let continuation = reasoning.map(|text| {
        agent_core::model::ModelContinuation::new(
            super::super::continuation::OPENAI,
            serde_json::json!({"reasoning_content":text}),
        )
    });
    let response = ModelResponse::tool_calls(calls)
        .with_continuation(continuation)
        .with_response_model(response_model)
        .with_usage(usage.finish())
        .with_stop_reason(stop);
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
    fn reads_usage_only_chunk_and_forwards_deltas() {
        let stream = [
            json!({"choices":[{"delta":{"content":"Hello"}}], "usage":null}),
            json!({"choices":[{"delta":{"content":"!", "tool_calls":[{"index":0,"id":"c1","function":{"name":"echo","arguments":"{}"}}]}}]}),
            json!({"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":20,"prompt_tokens_details":{"cached_tokens":60},"completion_tokens_details":{"reasoning_tokens":5}}}),
        ].iter().map(|event| format!("data: {event}\n\n")).collect::<String>() + "data: [DONE]\n\n";
        let mut deltas = Vec::new();
        let response = parse_stream(stream.as_bytes(), &mut |delta| {
            deltas.push(delta.to_owned())
        })
        .unwrap();
        assert_eq!(deltas, ["Hello", "!"]);
        let usage = response.usage();
        assert_eq!(usage.total_tokens(), Some(120));
        assert_eq!(usage.cached_input_tokens, Some(60));
        assert_eq!(usage.reasoning_tokens, Some(5));
        assert_eq!(usage.cache_hit(), Some(true));
        assert_eq!(response.into_parts().1[0].name(), "echo");
    }

    #[test]
    fn missing_usage_stays_unknown_and_interrupted_stream_fails() {
        let stream = "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n";
        assert!(parse_stream(stream.as_bytes(), &mut |_| {}).is_err());
        let response = parse_stream(
            (stream.to_owned() + "data: [DONE]\n").as_bytes(),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(response.usage().total_tokens(), None);
        assert_eq!(response.usage().cache_hit(), None);
        assert!(
            parse_stream(
                b"data: {\"error\":{\"message\":\"failed\"}}\n".as_slice(),
                &mut |_| {}
            )
            .is_err()
        );
    }
}
