use super::Environment;
use agent_core::memory::MemorySearchLimits;

pub(super) fn number(
    environment: &Environment,
    name: &str,
    default: usize,
) -> Result<usize, String> {
    environment
        .var(name)
        .map_or(Ok(default), |value| value.parse::<usize>())
        .map_err(|_| format!("{name} 必须是非负整数"))
}
pub(super) fn memory_limits(environment: &Environment) -> Result<MemorySearchLimits, String> {
    let defaults = MemorySearchLimits::default();
    let total = number(
        environment,
        "RS_AGENT_MEMORY_MAX_BYTES",
        defaults.max_total_bytes,
    )?;
    Ok(MemorySearchLimits {
        max_results: number(
            environment,
            "RS_AGENT_MEMORY_MAX_RESULTS",
            defaults.max_results,
        )?,
        max_total_bytes: total,
        max_entry_bytes: number(
            environment,
            "RS_AGENT_MEMORY_ENTRY_BYTES",
            defaults.max_entry_bytes.min(total),
        )?,
    })
}

pub(super) fn context_window(
    environment: &Environment,
) -> Result<Option<agent_core::context::ContextWindow>, String> {
    match environment.var("RS_AGENT_CONTEXT_COMPACTION").as_deref() {
        Ok("0" | "false") => return Ok(None),
        Ok("1" | "true") | Err(std::env::VarError::NotPresent) => {}
        _ => return Err("RS_AGENT_CONTEXT_COMPACTION 必须为 true/false 或 1/0".into()),
    }
    let hard = number(environment, "RS_AGENT_MAX_CONTEXT_BYTES", 1024 * 1024)?;
    let high = number(
        environment,
        "RS_AGENT_CONTEXT_HIGH_BYTES",
        (256 * 1024).min(hard.saturating_mul(3) / 4),
    )?;
    let low = number(environment, "RS_AGENT_CONTEXT_LOW_BYTES", high / 2)?;
    let window = agent_core::context::ContextWindow {
        high_bytes: high,
        low_bytes: low,
        max_messages: number(environment, "RS_AGENT_HISTORY_MAX_MESSAGES", 512)?,
        summary_bytes: number(environment, "RS_AGENT_SUMMARY_MAX_BYTES", 8192.min(low / 4))?,
    };
    window.validate(hard).map_err(|e| e.to_string())?;
    Ok(Some(window))
}
