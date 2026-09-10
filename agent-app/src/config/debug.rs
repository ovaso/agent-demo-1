//! Build an allowlisted snapshot from resolved application/provider settings.
use super::{Environment, RuntimeConfig};
use crate::provider::ConfiguredProvider;
use agent_core::model::ModelProvider;
use std::fmt::{Display, Write};

pub(crate) fn debug_snapshot(
    environment: &Environment,
    model: &ConfiguredProvider,
    runtime: &RuntimeConfig,
) -> String {
    let mut output = String::new();
    entry(
        &mut output,
        "RS_AGENT_PROVIDER",
        environment
            .var("RS_AGENT_PROVIDER")
            .unwrap_or_else(|_| "openai".into()),
    );
    match model {
        ConfiguredProvider::OpenAi(provider) => {
            entry(&mut output, "OPENAI_BASE_URL", provider.base_url());
            entry(&mut output, "OPENAI_MODEL", provider.model_name());
            entry(&mut output, "OPENAI_STREAM_USAGE", provider.stream_usage());
        }
        ConfiguredProvider::Anthropic(provider) => {
            entry(&mut output, "ANTHROPIC_BASE_URL", provider.base_url());
            entry(&mut output, "ANTHROPIC_MODEL", provider.model_name());
            entry(&mut output, "ANTHROPIC_MAX_TOKENS", provider.max_tokens());
        }
    }
    let limits = &runtime.limits;
    entry(&mut output, "RS_AGENT_MAX_STEPS", limits.max_steps);
    entry(
        &mut output,
        "RS_AGENT_AUTO_EXTEND",
        limits.step_extension.is_some(),
    );
    if let Some(policy) = &limits.step_extension {
        entry(
            &mut output,
            "RS_AGENT_HARD_MAX_STEPS",
            policy.hard_max_steps,
        );
        entry(
            &mut output,
            "RS_AGENT_STEP_INCREMENT",
            policy.step_increment,
        );
        entry(
            &mut output,
            "RS_AGENT_MAX_STEP_EXTENSIONS",
            policy.max_extensions,
        );
    } else {
        for key in [
            "RS_AGENT_HARD_MAX_STEPS",
            "RS_AGENT_STEP_INCREMENT",
            "RS_AGENT_MAX_STEP_EXTENSIONS",
        ] {
            entry(&mut output, key, "<disabled>");
        }
    }
    entry(
        &mut output,
        "RS_AGENT_MAX_DELEGATIONS",
        limits.max_delegations,
    );
    entry(&mut output, "RS_AGENT_DB", &runtime.db_path);
    entry(
        &mut output,
        "RS_AGENT_MEMORY_DIR",
        &runtime.memory_directory,
    );
    entry(&mut output, "RS_AGENT_SESSION", &runtime.session_id);
    entry(&mut output, "RS_AGENT_TRACE_FILE", &runtime.trace_path);
    output
}

fn entry(output: &mut String, key: &str, value: impl Display) {
    writeln!(output, "{key}={value}").expect("writing to String cannot fail");
}
