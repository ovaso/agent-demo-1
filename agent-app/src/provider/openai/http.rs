use super::super::transport::{self, HttpResponse};
use super::OpenAiCompatibleProvider;
use agent_core::model::ModelError;
use serde_json::Value;

impl OpenAiCompatibleProvider {
    pub(super) fn send(
        &self,
        body: &Value,
        max_input_bytes: usize,
    ) -> Result<HttpResponse, ModelError> {
        let request = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key);
        transport::send(request, body, max_input_bytes)
    }
}
