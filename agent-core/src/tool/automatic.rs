use std::{any::Any, sync::Arc};

use super::{Registry, RegistryError, Tool};

type ToolFactory = fn(Option<&dyn Any>) -> Result<Box<dyn Tool>, RegistryError>;

/// Macro-generated metadata only. Tool instances are created when a group is enabled.
#[doc(hidden)]
pub struct ToolRegistration {
    pub created_at: u64,
    pub name: &'static str,
    pub group: &'static str,
    pub factory: ToolFactory,
}

inventory::collect!(ToolRegistration);

impl Registry {
    /// Register all linked `#[tool]` functions in a group, ordered by first introduction then name.
    /// The default macro group is `"default"`. Other groups remain disabled.
    /// A failure leaves this registry unchanged.
    pub fn register_group(&mut self, group: &str) -> Result<(), RegistryError> {
        self.register_group_inner(group, None)
    }

    /// Enable a group with shared application state for `#[context]` parameters.
    /// Stateless tools in the group are also registered. Each stateful tool must
    /// accept exactly `C`; state is shared through Arc, never read from model input.
    pub fn register_group_with_context<C: Send + Sync + 'static>(
        &mut self,
        group: &str,
        context: Arc<C>,
    ) -> Result<(), RegistryError> {
        self.register_group_inner(group, Some(&context))
    }

    fn register_group_inner(
        &mut self,
        group: &str,
        context: Option<&dyn Any>,
    ) -> Result<(), RegistryError> {
        let mut entries: Vec<_> = inventory::iter::<ToolRegistration>
            .into_iter()
            .filter(|entry| entry.group == group)
            .collect();
        entries.sort_unstable_by_key(|entry| (entry.created_at, entry.name));
        let mut staged = Self::new();
        for entry in entries {
            if self.contains(entry.name) || staged.contains(entry.name) {
                return Err(RegistryError::DuplicateTool {
                    name: entry.name.into(),
                });
            }
            let tool = (entry.factory)(context)?;
            if self.contains(tool.name()) {
                return Err(RegistryError::DuplicateTool {
                    name: tool.name().into(),
                });
            }
            staged.register_boxed(tool)?;
        }
        self.tools.append(&mut staged.tools);
        Ok(())
    }
}
