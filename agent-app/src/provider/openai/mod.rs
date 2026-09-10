mod helpers;
mod request;
mod response;
mod stream;
mod usage;

use request::{messages, tools};
use response::parse_response;
use stream::parse_stream;

use std::io::{BufReader, Read};

use reqwest::blocking::Client;
use serde_json::{Value, json};

use agent_core::model::{ModelError, ModelProvider, ModelRequest, ModelResponse};

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// 兼容 OpenAI Chat Completions API 的适配器。
///
/// 可用于 OpenAI，也可用于采用相同请求与响应格式的模型服务。
pub struct OpenAiCompatibleProvider {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
    stream_usage: bool,
}

impl OpenAiCompatibleProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.into(),
            model: model.into(),
            base_url: DEFAULT_BASE_URL.to_owned(),
            stream_usage: true,
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_owned();
        self
    }

    /// 某些兼容服务不支持 stream_options，可显式关闭用量请求。
    pub fn with_stream_usage(mut self, enabled: bool) -> Self {
        self.stream_usage = enabled;
        self
    }

    fn request_body(&self, request: &ModelRequest<'_>) -> Result<Value, ModelError> {
        super::continuation::validate(
            request,
            super::continuation::OPENAI,
            &super::continuation::binding(super::continuation::OPENAI, &self.model, &self.base_url),
        )?;
        Ok(json!({
            "model": self.model,
            "messages": messages(request.messages(), request.memories())?,
            "tools": tools(request.tools()),
            "tool_choice": "auto",
        }))
    }
}

impl ModelProvider for OpenAiCompatibleProvider {
    fn model_name(&self) -> &str {
        &self.model
    }

    fn complete(&mut self, request: ModelRequest<'_>) -> Result<ModelResponse, ModelError> {
        let body = self.request_body(&request)?;
        let body = super::input::encode(&body, request.max_input_bytes())?;
        let request_bytes = body.len();
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .map_err(ModelError::new)?
            .error_for_status()
            .map_err(ModelError::new)?;
        let response: Value =
            serde_json::from_reader(response.take(super::continuation::MAX_RESPONSE_BYTES))
                .map_err(ModelError::new)?;

        let mut parsed = parse_response(&response)?;
        parsed.bind_continuation(super::continuation::binding(
            super::continuation::OPENAI,
            &self.model,
            &self.base_url,
        ));
        Ok(parsed.with_request_bytes(request_bytes))
    }

    fn stream(
        &mut self,
        request: ModelRequest<'_>,
        on_text_delta: &mut dyn FnMut(&str),
    ) -> Result<ModelResponse, ModelError> {
        let mut body = self.request_body(&request)?;
        body["stream"] = Value::Bool(true);
        if self.stream_usage {
            body["stream_options"] = json!({"include_usage": true});
        }

        let body = super::input::encode(&body, request.max_input_bytes())?;
        let request_bytes = body.len();
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .map_err(ModelError::new)?
            .error_for_status()
            .map_err(ModelError::new)?;

        let mut parsed = parse_stream(BufReader::new(response), on_text_delta)?;
        parsed.bind_continuation(super::continuation::binding(
            super::continuation::OPENAI,
            &self.model,
            &self.base_url,
        ));
        Ok(parsed.with_request_bytes(request_bytes))
    }
}
