use std::collections::BTreeMap;

use super::{Context, ContextStore, ContextStoreError, store::validate_session_id};

/// 进程内上下文存储，适用于测试、命令行程序与短生命周期 Agent。
#[derive(Debug, Default)]
pub struct MemoryContextStore {
    contexts: BTreeMap<String, Context>,
}

impl MemoryContextStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.contexts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.contexts.is_empty()
    }
}

impl ContextStore for MemoryContextStore {
    fn load(&self, session_id: &str) -> Result<Option<Context>, ContextStoreError> {
        validate_session_id(session_id)?;
        Ok(self.contexts.get(session_id).cloned())
    }

    fn save(&mut self, session_id: &str, context: &Context) -> Result<(), ContextStoreError> {
        validate_session_id(session_id)?;
        self.contexts.insert(session_id.to_owned(), context.clone());
        Ok(())
    }

    fn delete(&mut self, session_id: &str) -> Result<bool, ContextStoreError> {
        validate_session_id(session_id)?;
        Ok(self.contexts.remove(session_id).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_loads_and_deletes_a_context() {
        let mut store = MemoryContextStore::new();
        let context = Context::with_system("保持简洁。");

        store.save("session-1", &context).unwrap();
        assert_eq!(store.load("session-1").unwrap(), Some(context));
        assert!(store.delete("session-1").unwrap());
        assert_eq!(store.load("session-1").unwrap(), None);
    }
}
