use super::helpers::{arguments_from_object, required_string};
use super::usage;
use agent_core::{
    model::{ModelError, ModelResponse},
    tool::ToolCall,
};
use serde_json::Value;
pub(super) fn parse_response(response: &Value) -> Result<ModelResponse, ModelError> {
    let blocks = response
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| ModelError::new("Anthropic 响应缺少 content"))?;
    let mut texts = Vec::new();
    let mut calls = Vec::new();

    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    texts.push(text);
                }
            }
            Some("tool_use") => {
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
    Ok(ModelResponse::tool_calls(calls)
        .with_optional_text(text)
        .with_usage(usage::parse(response)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_text_and_tool_calls() {
        let response = json!({
            "content": [
                {"type": "text", "text": "正在查询。"},
                {"type": "tool_use", "id": "toolu-1", "name": "search", "input": {"query": "Rust"}}
            ]
        });

        let (text, calls) = parse_response(&response).unwrap().into_parts();
        assert_eq!(text.as_deref(), Some("正在查询。"));
        assert_eq!(calls[0].id(), "toolu-1");
        assert_eq!(calls[0].arguments().get("query"), Some("Rust"));
    }
    #[test]
    fn parses_non_streaming_usage_and_explicit_cache_miss() {
        let response = parse_response(&json!({"content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":5,"output_tokens":3,"cache_read_input_tokens":0,"cache_creation_input_tokens":7}})).unwrap();
        assert_eq!(response.usage().input_tokens, Some(12));
        assert_eq!(response.usage().total_tokens(), Some(15));
        assert_eq!(response.usage().cache_hit(), Some(false));
    }
}
