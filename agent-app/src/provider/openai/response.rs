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

    let continuation = message
        .get("reasoning_content")
        .filter(|v| v.is_string())
        .map(|v| {
            agent_core::model::ModelContinuation::new(
                super::super::continuation::OPENAI,
                serde_json::json!({"reasoning_content":v}),
            )
        });
    Ok(ModelResponse::tool_calls(calls)
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
