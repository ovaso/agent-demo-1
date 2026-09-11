//! 应用选择的模型集合；协议实现由独立 vendor crate 提供。
pub(crate) use agent_vendor_anthropic::AnthropicProvider;
pub(crate) use agent_vendor_deepseek::DeepSeekProvider;
pub(crate) use agent_vendor_openai::OpenAiCompatibleProvider;

use agent_core::model::{ModelError, ModelProvider, ModelRequest, ModelResponse, ModelStreamEvent};

/// The application uses one runtime instantiation for all protocols. Each
/// adapter retains its own reusable HTTP client and incremental stream parser.
pub(crate) enum ConfiguredProvider {
    DeepSeek(DeepSeekProvider),
    OpenAi(OpenAiCompatibleProvider),
    Anthropic(AnthropicProvider),
}

impl ModelProvider for ConfiguredProvider {
    fn model_name(&self) -> &str {
        match self {
            Self::DeepSeek(provider) => provider.model_name(),
            Self::OpenAi(provider) => provider.model_name(),
            Self::Anthropic(provider) => provider.model_name(),
        }
    }

    fn complete(&mut self, request: ModelRequest<'_>) -> Result<ModelResponse, ModelError> {
        match self {
            Self::DeepSeek(provider) => provider.complete(request),
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
            Self::DeepSeek(provider) => provider.stream(request, on_text_delta),
            Self::OpenAi(provider) => provider.stream(request, on_text_delta),
            Self::Anthropic(provider) => provider.stream(request, on_text_delta),
        }
    }
    fn stream_events(
        &mut self,
        request: ModelRequest<'_>,
        on_event: &mut dyn FnMut(ModelStreamEvent<'_>),
    ) -> Result<ModelResponse, ModelError> {
        match self {
            Self::DeepSeek(provider) => provider.stream_events(request, on_event),
            Self::OpenAi(provider) => provider.stream_events(request, on_event),
            Self::Anthropic(provider) => provider.stream_events(request, on_event),
        }
    }
}
