//! The application's ordinary toolset, shared by CLI and other application hosts.
//! Tool implementations use `#[tool]`; the host enables the toolset explicitly.

mod check;
mod process_output;
mod read;
mod run_cmd;
mod search;
mod session_finish;
mod write_file;

use agent_core::tool::{Registry, RegistryError};

/// Register the linked ordinary tools in the default group, ordered by fixed creation timestamp and name.
/// Debug tools and runtime control capabilities are enabled separately by the host.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register_group("default")
}
