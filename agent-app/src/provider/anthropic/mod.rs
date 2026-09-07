use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader},
};

use reqwest::{
    blocking::Client,
    header::{HeaderMap, HeaderValue},
};
use serde_json::{Map, Value, json};

use agent_core::{
    context::Message,
    model::{ModelError, ModelProvider, ModelRequest, ModelResponse},
    tool::{Arguments, ToolCall, ToolDefinition},
};

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1";
const API_VERSION: &str = "2023-06-01";

/// Anthropic Messages API 的适配器。
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    model: String,
    max_tokens: u32,
    base_url: String,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.into(),
            model: model.into(),
            max_tokens: 1_024,
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_owned();
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    fn request_body(&self, request: &ModelRequest<'_>) -> Result<Value, ModelError> {
        let (system, messages) = messages(request.messages(), request.memories())?;
        Ok(json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": system,
            "messages": messages,
            "tools": tools(request.tools()),
        }))
    }
}

impl ModelProvider for AnthropicProvider {
    fn complete(&mut self, request: ModelRequest<'_>) -> Result<ModelResponse, ModelError> {
        let body = self.request_body(&request)?;
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&self.api_key).map_err(ModelError::new)?,
        );
        headers.insert("anthropic-version", HeaderValue::from_static(API_VERSION));

        let response = self
            .client
            .post(format!("{}/messages", self.base_url))
            .headers(headers)
            .json(&body)
            .send()
            .map_err(ModelError::new)?
            .error_for_status()
            .map_err(ModelError::new)?
            .json::<Value>()
            .map_err(ModelError::new)?;

        parse_response(&response)
    }

    fn stream(
        &mut self,
        request: ModelRequest<'_>,
        on_text_delta: &mut dyn FnMut(&str),
    ) -> Result<ModelResponse, ModelError> {
        let mut body = self.request_body(&request)?;
        body["stream"] = Value::Bool(true);
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&self.api_key).map_err(ModelError::new)?,
        );
        headers.insert("anthropic-version", HeaderValue::from_static(API_VERSION));

        let response = self
            .client
            .post(format!("{}/messages", self.base_url))
            .headers(headers)
            .json(&body)
            .send()
            .map_err(ModelError::new)?
            .error_for_status()
            .map_err(ModelError::new)?;

        parse_stream(response, on_text_delta)
    }
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

fn parse_stream(
    response: reqwest::blocking::Response,
    on_text_delta: &mut dyn FnMut(&str),
) -> Result<ModelResponse, ModelError> {
    let mut text = String::new();
    let mut calls = BTreeMap::<usize, PartialToolCall>::new();

    for line in BufReader::new(response).lines() {
        let line = line.map_err(ModelError::new)?;
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        let event: Value = serde_json::from_str(data).map_err(ModelError::new)?;

        match event.get("type").and_then(Value::as_str) {
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
            Some("message_stop") => break,
            _ => {}
        }
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

    let response = ModelResponse::tool_calls(calls);
    if text.is_empty() {
        Ok(response)
    } else {
        Ok(response.with_text(text))
    }
}

fn messages(
    messages: &[Message],
    memories: &[agent_core::memory::Memory],
) -> Result<(String, Vec<Value>), ModelError> {
    let mut system_parts = memories
        .iter()
        .map(|memory| format!("[{}] {}", memory.id(), memory.content()))
        .collect::<Vec<_>>();
    let mut output = Vec::new();

    for message in messages {
        match message {
            Message::System { content } => system_parts.push(content.clone()),
            Message::User { content } => output.push(json!({"role": "user", "content": content})),
            Message::Assistant {
                content: assistant_text,
                tool_calls,
            } => {
                let mut blocks = Vec::new();
                if !assistant_text.is_empty() {
                    blocks.push(json!({"type": "text", "text": assistant_text}));
                }
                for call in tool_calls {
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": call.id(),
                        "name": call.name(),
                        "input": arguments_value(call.arguments()),
                    }));
                }
                output.push(json!({"role": "assistant", "content": blocks}));
            }
            Message::Tool {
                call_id, content, ..
            } => output.push(json!({
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": call_id,
                    "content": content,
                }]
            })),
        }
    }

    Ok((system_parts.join("\n\n"), output))
}

fn tools(definitions: &[ToolDefinition]) -> Vec<Value> {
    definitions
        .iter()
        .map(|definition| {
            let required = definition
                .parameters()
                .iter()
                .filter(|parameter| parameter.is_required())
                .map(|parameter| Value::String(parameter.name().to_owned()))
                .collect::<Vec<_>>();
            let properties = definition
                .parameters()
                .iter()
                .map(|parameter| {
                    (
                        parameter.name().to_owned(),
                        json!({
                            "type": "string",
                            "description": parameter.description(),
                        }),
                    )
                })
                .collect::<Map<String, Value>>();
            json!({
                "name": definition.name(),
                "description": definition.description(),
                "input_schema": {
                    "type": "object",
                    "properties": properties,
                    "required": required,
                    "additionalProperties": false,
                }
            })
        })
        .collect()
}

fn parse_response(response: &Value) -> Result<ModelResponse, ModelError> {
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
    Ok(ModelResponse::tool_calls(calls).with_optional_text(text))
}

fn required_string<'a>(
    value: &'a Value,
    field: &str,
    message: &str,
) -> Result<&'a str, ModelError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| ModelError::new(message))
}

fn arguments_value(arguments: &Arguments) -> Value {
    Value::Object(
        arguments
            .iter()
            .map(|(name, value)| (name.to_owned(), Value::String(value.to_owned())))
            .collect(),
    )
}

fn arguments_from_object(object: &Map<String, Value>) -> Arguments {
    object
        .iter()
        .fold(Arguments::new(), |arguments, (name, value)| {
            arguments.with(
                name,
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            )
        })
}

fn arguments_from_json(arguments: &str) -> Result<Arguments, ModelError> {
    let value: Value = serde_json::from_str(arguments).map_err(ModelError::new)?;
    let object = value
        .as_object()
        .ok_or_else(|| ModelError::new("工具参数必须是 JSON 对象"))?;
    Ok(arguments_from_object(object))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
