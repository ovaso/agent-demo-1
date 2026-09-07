use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader},
};

use reqwest::blocking::Client;
use serde_json::{Map, Value, json};

use agent_core::{
    context::Message,
    model::{ModelError, ModelProvider, ModelRequest, ModelResponse},
    tool::{Arguments, ToolCall, ToolDefinition},
};

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// 兼容 OpenAI Chat Completions API 的适配器。
///
/// 可用于 OpenAI，也可用于采用相同请求与响应格式的模型服务。
pub struct OpenAiCompatibleProvider {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenAiCompatibleProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.into(),
            model: model.into(),
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_owned();
        self
    }

    fn request_body(&self, request: &ModelRequest<'_>) -> Result<Value, ModelError> {
        Ok(json!({
            "model": self.model,
            "messages": messages(request.messages(), request.memories())?,
            "tools": tools(request.tools()),
            "tool_choice": "auto",
        }))
    }
}

impl ModelProvider for OpenAiCompatibleProvider {
    fn complete(&mut self, request: ModelRequest<'_>) -> Result<ModelResponse, ModelError> {
        let body = self.request_body(&request)?;
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
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

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
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
        if data == "[DONE]" {
            break;
        }

        let chunk: Value = serde_json::from_str(data).map_err(ModelError::new)?;
        let Some(delta) = chunk.pointer("/choices/0/delta") else {
            continue;
        };

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
) -> Result<Vec<Value>, ModelError> {
    let mut output = Vec::new();
    let memory_text = memories
        .iter()
        .map(|memory| format!("[{}] {}", memory.id(), memory.content()))
        .collect::<Vec<_>>()
        .join("\n");

    if !memory_text.is_empty() {
        output.push(json!({
            "role": "system",
            "content": format!("可参考以下长期记忆：\n{memory_text}"),
        }));
    }

    for message in messages {
        match message {
            Message::System { content } => {
                output.push(json!({"role": "system", "content": content}))
            }
            Message::User { content } => output.push(json!({"role": "user", "content": content})),
            Message::Assistant {
                content,
                tool_calls,
            } => {
                let mut value = json!({"role": "assistant", "content": content});
                if !tool_calls.is_empty() {
                    value["tool_calls"] = Value::Array(
                        tool_calls
                            .iter()
                            .map(|call| {
                                json!({
                                    "id": call.id(),
                                    "type": "function",
                                    "function": {
                                        "name": call.name(),
                                        "arguments": arguments_json(call.arguments()),
                                    }
                                })
                            })
                            .collect(),
                    );
                }
                output.push(value);
            }
            Message::Tool {
                call_id, content, ..
            } => output.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": content,
            })),
        }
    }

    Ok(output)
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
                "type": "function",
                "function": {
                    "name": definition.name(),
                    "description": definition.description(),
                    "parameters": {
                        "type": "object",
                        "properties": properties,
                        "required": required,
                        "additionalProperties": false,
                    }
                }
            })
        })
        .collect()
}

fn parse_response(response: &Value) -> Result<ModelResponse, ModelError> {
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

    Ok(ModelResponse::tool_calls(calls).with_optional_text(text))
}

fn arguments_json(arguments: &Arguments) -> String {
    Value::Object(
        arguments
            .iter()
            .map(|(name, value)| (name.to_owned(), Value::String(value.to_owned())))
            .collect(),
    )
    .to_string()
}

fn parse_arguments(arguments: &str) -> Result<Arguments, ModelError> {
    let value: Value = serde_json::from_str(arguments).map_err(ModelError::new)?;
    let object = value
        .as_object()
        .ok_or_else(|| ModelError::new("工具参数必须是 JSON 对象"))?;

    Ok(object
        .iter()
        .fold(Arguments::new(), |arguments, (name, value)| {
            arguments.with(
                name,
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            )
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
