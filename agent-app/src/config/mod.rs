//! Application environment parsing. Provider protocols and CLI rendering live elsewhere.
use agent_core::agent::runtime::{RunLimits, StepExtensionPolicy};
use std::{env, error::Error};

mod debug;
mod environment;
mod prompt;
mod provider;
pub(crate) use debug::debug_snapshot;
pub(crate) use environment::Environment;
pub(crate) use provider::model_provider;

pub(crate) struct RuntimeConfig {
    pub(crate) db_path: String,
    pub(crate) memory_directory: String,
    pub(crate) trace_path: String,
    pub(crate) session_id: String,
    pub(crate) limits: RunLimits,
}

impl RuntimeConfig {
    pub(crate) fn from_environment(environment: &Environment) -> Result<Self, Box<dyn Error>> {
        let max_steps = environment
            .var("RS_AGENT_MAX_STEPS")
            .map_or(Ok(8), |value| value.parse::<u64>())?;
        if max_steps == 0 {
            return Err("RS_AGENT_MAX_STEPS 必须大于零".into());
        }
        Ok(Self {
            db_path: db_path(environment),
            memory_directory: environment
                .var("RS_AGENT_MEMORY_DIR")
                .unwrap_or_else(|_| "memories".into()),
            trace_path: trace_path(environment),
            session_id: session_id(environment),
            limits: RunLimits {
                step_extension: step_extension_policy(environment, max_steps)?,
                memory_limits: prompt::memory_limits(environment)?,
                context_window: prompt::context_window(environment)?,
                max_total_tokens: prompt::total_tokens(environment)?,
                max_output_tokens: Some(prompt::output_tokens(environment)?),
                max_context_bytes: prompt::number(
                    environment,
                    "RS_AGENT_MAX_CONTEXT_BYTES",
                    1024 * 1024,
                )?,
                max_delegations: environment
                    .var("RS_AGENT_MAX_DELEGATIONS")
                    .map_or(Ok(8), |value| value.parse::<usize>())?,
                ..RunLimits::new(max_steps)
            },
        })
    }
}

fn step_extension_policy(
    environment: &Environment,
    granted: u64,
) -> Result<Option<StepExtensionPolicy>, Box<dyn Error>> {
    let enabled = match environment.var("RS_AGENT_AUTO_EXTEND").as_deref() {
        Ok("0" | "false") => false,
        Ok("1" | "true") | Err(env::VarError::NotPresent) => true,
        _ => return Err("RS_AGENT_AUTO_EXTEND 必须为 true/false 或 1/0".into()),
    };
    if !enabled {
        return Ok(None);
    }
    let number = |name: &str, default: u64| -> Result<u64, String> {
        environment
            .var(name)
            .map_or(Ok(default), |value| value.parse::<u64>())
            .map_err(|error| format!("{name}：{error}"))
    };
    let defaults = StepExtensionPolicy::default();
    let policy = StepExtensionPolicy {
        hard_max_steps: number(
            "RS_AGENT_HARD_MAX_STEPS",
            granted.max(defaults.hard_max_steps),
        )?,
        step_increment: number("RS_AGENT_STEP_INCREMENT", defaults.step_increment)?,
        max_extensions: usize::try_from(number(
            "RS_AGENT_MAX_STEP_EXTENSIONS",
            defaults.max_extensions as u64,
        )?)?,
    };
    policy.validate(granted)?;
    Ok(Some(policy))
}

pub(crate) fn db_path(environment: &Environment) -> String {
    environment
        .var("RS_AGENT_DB")
        .unwrap_or_else(|_| "agent-context.sqlite3".into())
}

pub(crate) fn trace_path(environment: &Environment) -> String {
    environment
        .var("RS_AGENT_TRACE_FILE")
        .unwrap_or_else(|_| "agent-trace.jsonl".into())
}

pub(crate) fn session_id(environment: &Environment) -> String {
    environment
        .var("RS_AGENT_SESSION")
        .unwrap_or_else(|_| "default".into())
}
