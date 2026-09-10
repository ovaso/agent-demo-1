//! Application environment parsing. Provider protocols and CLI rendering live elsewhere.
use crate::provider::{AnthropicProvider, ConfiguredProvider, OpenAiCompatibleProvider};
use agent_core::agent::runtime::RunLimits;
use std::{env, error::Error};

pub(crate) struct RuntimeConfig {
    pub(crate) db_path: String,
    pub(crate) memory_directory: String,
    pub(crate) trace_path: String,
    pub(crate) session_id: String,
    pub(crate) limits: RunLimits,
}

impl RuntimeConfig {
    pub(crate) fn from_environment() -> Result<Self, Box<dyn Error>> {
        let max_steps =
            env::var("RS_AGENT_MAX_STEPS").map_or(Ok(8), |value| value.parse::<u64>())?;
        if max_steps == 0 {
            return Err("RS_AGENT_MAX_STEPS 必须大于零".into());
        }
        Ok(Self {
            db_path: db_path(),
            memory_directory: env::var("RS_AGENT_MEMORY_DIR").unwrap_or_else(|_| "memories".into()),
            trace_path: trace_path(),
            session_id: session_id(),
            limits: RunLimits {
                max_delegations: env::var("RS_AGENT_MAX_DELEGATIONS")
                    .map_or(Ok(8), |value| value.parse::<usize>())?,
                ..RunLimits::new(max_steps)
            },
        })
    }
}

pub(crate) fn db_path() -> String {
    env::var("RS_AGENT_DB").unwrap_or_else(|_| "agent-context.sqlite3".into())
}

pub(crate) fn trace_path() -> String {
    env::var("RS_AGENT_TRACE_FILE").unwrap_or_else(|_| "agent-trace.jsonl".into())
}

pub(crate) fn session_id() -> String {
    env::var("RS_AGENT_SESSION").unwrap_or_else(|_| "default".into())
}

pub(crate) fn model_provider() -> Result<ConfiguredProvider, Box<dyn Error>> {
    let provider = env::var("RS_AGENT_PROVIDER").unwrap_or_else(|_| "openai".to_owned());

    match provider.as_str() {
        "openai" | "openai-compatible" => {
            let api_key = required_environment("OPENAI_API_KEY")?;
            let model = required_environment("OPENAI_MODEL")?;
            let stream_usage = match env::var("OPENAI_STREAM_USAGE").as_deref() {
                Ok("0" | "false") => false,
                Ok("1" | "true") | Err(env::VarError::NotPresent) => true,
                _ => return Err("OPENAI_STREAM_USAGE 必须为 true/false 或 1/0".into()),
            };
            let mut provider =
                OpenAiCompatibleProvider::new(api_key, model).with_stream_usage(stream_usage);
            if let Ok(base_url) = env::var("OPENAI_BASE_URL") {
                provider = provider.with_base_url(base_url);
            }
            Ok(ConfiguredProvider::OpenAi(provider))
        }
        "anthropic" => {
            let api_key = required_environment("ANTHROPIC_API_KEY")?;
            let model = required_environment("ANTHROPIC_MODEL")?;
            let max_tokens = env::var("ANTHROPIC_MAX_TOKENS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(1_024);
            let mut provider = AnthropicProvider::new(api_key, model).with_max_tokens(max_tokens);
            if let Ok(base_url) = env::var("ANTHROPIC_BASE_URL") {
                provider = provider.with_base_url(base_url);
            }
            Ok(ConfiguredProvider::Anthropic(provider))
        }
        other => Err(format!(
            "不支持的 RS_AGENT_PROVIDER：{other}；可选值为 openai、openai-compatible 或 anthropic"
        )
        .into()),
    }
}

fn required_environment(name: &str) -> Result<String, Box<dyn Error>> {
    env::var(name).map_err(|_| format!("缺少环境变量 {name}").into())
}
