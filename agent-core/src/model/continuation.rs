use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Opaque provider-owned reply data needed to continue a conversation.
/// Protocol conversion belongs to the provider; persistence belongs to Context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelContinuation {
    protocol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    binding: Option<String>,
    data: Value,
}

impl ModelContinuation {
    pub fn new(protocol: impl Into<String>, data: Value) -> Self {
        Self {
            protocol: protocol.into(),
            binding: None,
            data,
        }
    }
    pub fn protocol(&self) -> &str {
        &self.protocol
    }
    pub fn binding(&self) -> Option<&str> {
        self.binding.as_deref()
    }
    pub fn data(&self) -> &Value {
        &self.data
    }
    pub(crate) fn bind(&mut self, binding: String) {
        self.binding = Some(binding);
    }
}
