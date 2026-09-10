//! Debug tools. The application controls registration and supplies safe snapshots.

mod echo;
mod show_config;

use agent_core::tool::{Registry, RegistryError};
use std::sync::Arc;

struct DebugContext {
    config_snapshot: String,
}

/// Register debug tools with an application-provided, non-secret startup snapshot.
/// Call only when the application explicitly enables debug mode.
pub fn register(registry: &mut Registry, config_snapshot: String) -> Result<(), RegistryError> {
    registry.register_group_with_context("debug", Arc::new(DebugContext { config_snapshot }))
}
