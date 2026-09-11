use super::AnthropicProvider;
use super::helpers::arguments_value;
use agent_core::{
    context::Message,
    model::{ModelError, ModelRequest},
    tool::ToolDefinition,
};
use serde_json::{Map, Value, json};
pub(super) fn messages(
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
            Message::User { content, .. } => {
                output.push(json!({"role": "user", "content": content}))
            }
            Message::Assistant {
                content: assistant_text,
                tool_calls,
                continuation,
            } => {
                if let Some(continuation) = continuation {
                    if continuation.protocol() != crate::PROTOCOL {
                        return Err(ModelError::new("模型续接协议不匹配"));
                    }
                    output.push(json!({"role":"assistant","content":continuation.data()}));
                    continue;
                }
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

pub(super) fn tools(definitions: &[ToolDefinition]) -> Vec<Value> {
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

impl AnthropicProvider {
    pub(super) fn request_body(&self, request: &ModelRequest<'_>) -> Result<Value, ModelError> {
        agent_vendor::continuation::validate(
            request,
            crate::PROTOCOL,
            &agent_vendor::continuation::binding(crate::PROTOCOL, &self.model, &self.base_url),
        )?;
        let (system, messages) = messages(request.messages(), request.memories())?;
        let mut body = json!({
            "model": self.model,
            "max_tokens": request.max_output_tokens().unwrap_or(self.max_tokens as u64),
            "system": system,
            "messages": messages,
            "tools": tools(request.tools()),
        });
        super::cache::anthropic(&mut body, self.cache_ttl, &self.base_url);
        Ok(body)
    }
}
