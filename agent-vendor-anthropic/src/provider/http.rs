use super::AnthropicProvider;
use agent_core::model::ModelError;
use agent_vendor::http::{self as transport, HttpResponse};
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;

impl AnthropicProvider {
    pub(super) fn send(
        &self,
        body: &Value,
        max_input_bytes: usize,
    ) -> Result<HttpResponse, ModelError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&self.api_key).map_err(ModelError::new)?,
        );
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static(super::API_VERSION),
        );
        let request = self
            .client
            .post(format!("{}/messages", self.base_url))
            .headers(headers);
        transport::send(request, body, max_input_bytes)
    }
}
