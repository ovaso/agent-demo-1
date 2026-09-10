mod content;
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
use serde_json::Value;

use agent_core::model::{ModelError, ModelProvider, ModelRequest, ModelResponse};

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1";
const API_VERSION: &str = "2023-06-01";

/// Anthropic Messages API 的适配器。
pub(crate) struct AnthropicProvider {
    client: Client,
    api_key: String,
    model: String,
    max_tokens: u32,
    base_url: String,
    cache_ttl: Option<super::cache::CacheTtl>,
}

impl AnthropicProvider {
    pub(crate) fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.into(),
            model: model.into(),
            max_tokens: 1_024,
            base_url: DEFAULT_BASE_URL.to_owned(),
            cache_ttl: None,
        }
    }

    pub(crate) fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_owned();
        self
    }

    pub(crate) fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub(crate) fn with_cache_ttl(mut self, value: Option<&str>) -> Result<Self, ModelError> {
        self.cache_ttl = value.map(super::cache::CacheTtl::parse).transpose()?;
        Ok(self)
    }

    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    pub(crate) fn max_tokens(&self) -> u32 {
        self.max_tokens
    }
}

impl ModelProvider for AnthropicProvider {
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
            super::continuation::ANTHROPIC,
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
        let HttpResponse {
            response,
            request_bytes,
        } = self.send(&body, request.max_input_bytes())?;

        let mut parsed = parse_stream(BufReader::new(response), on_text_delta)?;
        parsed.bind_continuation(super::continuation::binding(
            super::continuation::ANTHROPIC,
            &self.model,
            &self.base_url,
        ));
        Ok(parsed.with_request_bytes(request_bytes))
    }
}
