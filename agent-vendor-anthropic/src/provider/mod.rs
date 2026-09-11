mod cache;
mod content;
mod helpers;
mod http;
mod request;
mod response;
mod stop_reason;
mod stream;
mod usage;

use response::parse_response;
use stream::parse_stream;

use agent_vendor::http::{self as transport, HttpResponse};
use std::io::BufReader;

use reqwest::blocking::Client;
use serde_json::Value;

use agent_core::model::{ModelError, ModelProvider, ModelRequest, ModelResponse, ModelStreamEvent};

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1";
const API_VERSION: &str = "2023-06-01";

/// Anthropic Messages API 的适配器。
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    model: String,
    max_tokens: u32,
    base_url: String,
    cache_ttl: Option<cache::CacheTtl>,
}

impl AnthropicProvider {
    /// 使用默认 HTTP Client；需要自定义超时、代理或连接策略时使用 with_client。
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self::with_client(Client::new(), api_key, model)
    }

    /// 显式注入可复用 Client。Provider 不读取应用配置或创建逐请求 Client。
    pub fn with_client(
        client: Client,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client,
            api_key: api_key.into(),
            model: model.into(),
            base_url: DEFAULT_BASE_URL.to_owned(),
            max_tokens: 1_024,
            cache_ttl: None,
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

    pub fn with_cache_ttl(mut self, value: Option<&str>) -> Result<Self, ModelError> {
        self.cache_ttl = value.map(cache::CacheTtl::parse).transpose()?;
        Ok(self)
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn max_tokens(&self) -> u32 {
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
        parsed.bind_continuation(agent_vendor::continuation::binding(
            crate::PROTOCOL,
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
        self.stream_events(request, &mut |event| {
            if let ModelStreamEvent::TextDelta(text) = event {
                on_text_delta(text);
            }
        })
    }

    fn stream_events(
        &mut self,
        request: ModelRequest<'_>,
        on_event: &mut dyn FnMut(ModelStreamEvent<'_>),
    ) -> Result<ModelResponse, ModelError> {
        let mut body = self.request_body(&request)?;
        body["stream"] = Value::Bool(true);
        let HttpResponse {
            response,
            request_bytes,
        } = self.send(&body, request.max_input_bytes())?;

        let mut parsed = parse_stream(BufReader::new(response), on_event)?;
        parsed.bind_continuation(agent_vendor::continuation::binding(
            crate::PROTOCOL,
            &self.model,
            &self.base_url,
        ));
        Ok(parsed.with_request_bytes(request_bytes))
    }
}
