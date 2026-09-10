mod helpers;
mod http;
mod request;
mod response;
mod stream;
mod usage;

use response::parse_response;
use stream::parse_stream;

use super::transport::{self, HttpResponse};
use std::io::BufReader;

use reqwest::blocking::Client;
use serde_json::{Value, json};

use agent_core::model::{ModelError, ModelProvider, ModelRequest, ModelResponse};

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// 兼容 OpenAI Chat Completions API 的适配器。
///
/// 可用于 OpenAI，也可用于采用相同请求与响应格式的模型服务。
pub(crate) struct OpenAiCompatibleProvider {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
    stream_usage: bool,
    max_tokens_field: Option<String>,
    reasoning_effort: Option<String>,
}

impl OpenAiCompatibleProvider {
    pub(crate) fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.into(),
            model: model.into(),
            base_url: DEFAULT_BASE_URL.to_owned(),
            stream_usage: true,
            max_tokens_field: None,
            reasoning_effort: None,
        }
    }

    pub(crate) fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_owned();
        self
    }

    /// 某些兼容服务不支持 stream_options，可显式关闭用量请求。
    pub(crate) fn with_stream_usage(mut self, enabled: bool) -> Self {
        self.stream_usage = enabled;
        self
    }

    pub(crate) fn with_max_tokens_field(
        mut self,
        field: Option<String>,
    ) -> Result<Self, ModelError> {
        if field
            .as_deref()
            .is_some_and(|f| !matches!(f, "max_tokens" | "max_completion_tokens"))
        {
            return Err(ModelError::new(
                "OPENAI_MAX_TOKENS_FIELD 必须为 max_tokens 或 max_completion_tokens",
            ));
        }
        self.max_tokens_field = field;
        Ok(self)
    }

    pub(crate) fn with_reasoning_effort(
        mut self,
        effort: Option<String>,
    ) -> Result<Self, ModelError> {
        super::cache::validate_effort(effort.as_deref())?;
        self.reasoning_effort = effort;
        Ok(self)
    }

    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    pub(crate) fn stream_usage(&self) -> bool {
        self.stream_usage
    }
}

impl ModelProvider for OpenAiCompatibleProvider {
    fn model_name(&self) -> &str {
        &self.model
    }

    fn complete(&mut self, request: ModelRequest<'_>) -> Result<ModelResponse, ModelError> {
        let body = self.request_body(&request)?;
        let HttpResponse {
            response,
            request_bytes,
        } = self.send(&body, request.max_input_bytes())?;
        let response = transport::read_json(response)?;

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

        let HttpResponse {
            response,
            request_bytes,
        } = self.send(&body, request.max_input_bytes())?;

        let mut parsed = parse_stream(BufReader::new(response), on_text_delta)?;
        parsed.bind_continuation(super::continuation::binding(
            super::continuation::OPENAI,
            &self.model,
            &self.base_url,
        ));
        Ok(parsed.with_request_bytes(request_bytes))
    }
}
