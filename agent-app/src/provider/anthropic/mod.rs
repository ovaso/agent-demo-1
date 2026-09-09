mod helpers;
mod request;
mod response;
mod stream;
mod usage;

use request::{messages, tools};
use response::parse_response;
use stream::parse_stream;

use std::io::BufReader;

use reqwest::{
    blocking::Client,
    header::{HeaderMap, HeaderValue},
};
use serde_json::{Value, json};

use agent_core::model::{ModelError, ModelProvider, ModelRequest, ModelResponse};

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
    fn model_name(&self) -> &str {
        &self.model
    }

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

        parse_stream(BufReader::new(response), on_text_delta)
    }
}
