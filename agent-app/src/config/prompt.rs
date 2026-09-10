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
