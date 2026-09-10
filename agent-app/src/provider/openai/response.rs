use super::helpers::parse_arguments;
use super::usage;
use agent_core::{
    model::{ModelError, ModelResponse},
    tool::ToolCall,
};
use serde_json::Value;
pub(super) fn parse_response(response: &Value) -> Result<ModelResponse, ModelError> {
    let stop = super::super::stop_reason::openai(
        response
            .pointer("/choices/0/finish_reason")
            .and_then(Value::as_str),
    );
    let message = response
        .pointer("/choices/0/message")
        .ok_or_else(|| ModelError::new("OpenAI 响应缺少 choices[0].message"))?;
    let text = message
        .get("content")
        .and_then(Value::as_str)
        .filter(|content| !content.is_empty())
        .map(str::to_owned);
    let calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .filter(|_| stop.is_complete())
        .map(|calls| {
            calls
                .iter()
                .map(|call| {
                    let id = call
                        .get("id")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ModelError::new("OpenAI 工具调用缺少 id"))?;
                    let function = call
                        .get("function")
                        .ok_or_else(|| ModelError::new("OpenAI 工具调用缺少 function"))?;
                    let name = function
                        .get("name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ModelError::new("OpenAI 工具调用缺少名称"))?;
                    let arguments = function
                        .get("arguments")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ModelError::new("OpenAI 工具调用缺少参数"))?;
                    Ok(ToolCall::new(id, name, parse_arguments(arguments)?))
                })
                .collect::<Result<Vec<_>, ModelError>>()
        })
        .transpose()?
        .unwrap_or_default();

    Ok(ModelResponse::tool_calls(calls)
        .with_optional_text(text)
        .with_usage(usage::parse(response))
        .with_stop_reason(stop))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_text_and_tool_calls() {
        let response = json!({
            "choices": [{
                "message": {
                    "content": "正在查询。",
                    "tool_calls": [{
                        "id": "call-1",
                        "function": {"name": "search", "arguments": "{\"query\":\"Rust\"}"}
                    }]
                }
            }]
        });

        let (text, calls) = parse_response(&response).unwrap().into_parts();
        assert_eq!(text.as_deref(), Some("正在查询。"));
        assert_eq!(calls[0].id(), "call-1");
        assert_eq!(calls[0].arguments().get("query"), Some("Rust"));
    }
    #[test]
    fn parses_non_streaming_usage_and_explicit_cache_miss() {
        let response = parse_response(&json!({"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":12,"completion_tokens":3,"prompt_tokens_details":{"cached_tokens":0}}})).unwrap();
        assert_eq!(response.usage().input_tokens, Some(12));
        assert_eq!(response.usage().total_tokens(), Some(15));
        assert_eq!(response.usage().cache_hit(), Some(false));
    }
}
