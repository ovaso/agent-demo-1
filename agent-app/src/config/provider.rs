use super::Environment;
use crate::provider::{AnthropicProvider, ConfiguredProvider, OpenAiCompatibleProvider};
use std::{env, error::Error};

pub(crate) fn model_provider(
    environment: &Environment,
) -> Result<ConfiguredProvider, Box<dyn Error>> {
    let provider = environment
        .var("RS_AGENT_PROVIDER")
        .unwrap_or_else(|_| "openai".to_owned());

    match provider.as_str() {
        "openai" | "openai-compatible" => {
            let api_key = required_environment(environment, "OPENAI_API_KEY")?;
            let model = required_environment(environment, "OPENAI_MODEL")?;
            let stream_usage = match environment.var("OPENAI_STREAM_USAGE").as_deref() {
                Ok("0" | "false") => false,
                Ok("1" | "true") | Err(env::VarError::NotPresent) => true,
                _ => return Err("OPENAI_STREAM_USAGE 必须为 true/false 或 1/0".into()),
            };
            let mut provider =
                OpenAiCompatibleProvider::new(api_key, model).with_stream_usage(stream_usage);
            if let Ok(base_url) = environment.var("OPENAI_BASE_URL") {
                provider = provider.with_base_url(base_url);
            }
            provider =
                provider.with_max_tokens_field(environment.var("OPENAI_MAX_TOKENS_FIELD").ok())?;
            provider =
                provider.with_reasoning_effort(environment.var("OPENAI_REASONING_EFFORT").ok())?;
            Ok(ConfiguredProvider::OpenAi(provider))
        }
        "anthropic" => {
            let api_key = required_environment(environment, "ANTHROPIC_API_KEY")?;
            let model = required_environment(environment, "ANTHROPIC_MODEL")?;
            let max_tokens = environment
                .var("ANTHROPIC_MAX_TOKENS")
                .map_or(Ok(1_024), |value| value.parse::<u32>())
                .map_err(|error| format!("ANTHROPIC_MAX_TOKENS：{error}"))?;
            if max_tokens == 0 {
                return Err("ANTHROPIC_MAX_TOKENS 必须大于零".into());
            }
            let mut provider = AnthropicProvider::new(api_key, model).with_max_tokens(max_tokens);
            if let Ok(base_url) = environment.var("ANTHROPIC_BASE_URL") {
                provider = provider.with_base_url(base_url);
            }
            provider =
                provider.with_cache_ttl(environment.var("ANTHROPIC_CACHE_TTL").ok().as_deref())?;
            Ok(ConfiguredProvider::Anthropic(provider))
        }
        _ => Err(
            "不支持的 RS_AGENT_PROVIDER；可选值为 openai、openai-compatible 或 anthropic".into(),
        ),
    }
}

fn required_environment(environment: &Environment, name: &str) -> Result<String, Box<dyn Error>> {
    environment
        .var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("缺少配置 {name}，请在进程环境或 .env 中设置").into())
}
