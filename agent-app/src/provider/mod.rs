//! 第三方模型服务适配器。

mod anthropic;
mod continuation;
mod openai;
mod stop_reason;

pub(crate) use anthropic::AnthropicProvider;
pub(crate) use openai::OpenAiCompatibleProvider;

use agent_core::model::{ModelError, ModelProvider, ModelRequest, ModelResponse};

/// The application uses one runtime instantiation for both protocols. Each
/// adapter retains its own reusable HTTP client and incremental stream parser.
pub(crate) enum ConfiguredProvider {
    OpenAi(OpenAiCompatibleProvider),
    Anthropic(AnthropicProvider),
}

impl ModelProvider for ConfiguredProvider {
    fn model_name(&self) -> &str {
        match self {
            Self::OpenAi(provider) => provider.model_name(),
            Self::Anthropic(provider) => provider.model_name(),
        }
    }

    fn complete(&mut self, request: ModelRequest<'_>) -> Result<ModelResponse, ModelError> {
        match self {
            Self::OpenAi(provider) => provider.complete(request),
            Self::Anthropic(provider) => provider.complete(request),
        }
    }

    fn stream(
        &mut self,
        request: ModelRequest<'_>,
        on_text_delta: &mut dyn FnMut(&str),
    ) -> Result<ModelResponse, ModelError> {
        match self {
            Self::OpenAi(provider) => provider.stream(request, on_text_delta),
            Self::Anthropic(provider) => provider.stream(request, on_text_delta),
        }
    }
}
