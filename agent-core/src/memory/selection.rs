use super::Memory;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemorySearchLimits {
    pub max_results: usize,
    pub max_total_bytes: usize,
    pub max_entry_bytes: usize,
}
impl Default for MemorySearchLimits {
    fn default() -> Self {
        Self {
            max_results: 8,
            max_total_bytes: 64 * 1024,
            max_entry_bytes: 16 * 1024,
        }
    }
}
impl MemorySearchLimits {
    pub(crate) fn within(mut self, bytes: usize) -> Self {
        self.max_total_bytes = self.max_total_bytes.min(bytes);
        self.max_entry_bytes = self.max_entry_bytes.min(self.max_total_bytes);
        self.max_results = self.max_results.min(self.max_total_bytes);
        self
    }

    pub fn select(&self, memories: impl IntoIterator<Item = Memory>) -> MemorySelection {
        let mut selector = Selector::new(*self);
        for memory in memories {
            selector.add(memory);
        }
        selector.finish()
    }
}
#[derive(Debug, Default)]
pub struct MemorySelection {
    pub memories: Vec<Memory>,
    /// Some files were not inspected or some candidates did not fit the bounds.
    pub truncated: bool,
}

pub(super) struct Selector {
    limits: MemorySearchLimits,
    items: BTreeMap<String, Memory>,
    bytes: usize,
    pub(super) truncated: bool,
}
impl Selector {
    pub(super) fn new(limits: MemorySearchLimits) -> Self {
        Self {
            limits,
            items: BTreeMap::new(),
            bytes: 0,
            truncated: false,
        }
    }
    pub(super) fn add(&mut self, memory: Memory) {
        let size = memory.bytes();
        if size > self.limits.max_entry_bytes
            || size > self.limits.max_total_bytes
            || self.limits.max_results == 0
        {
            self.truncated = true;
            return;
        }
        // Deterministic selection even when directory iteration order changes.
        if let Some(previous) = self.items.get(memory.id()) {
            if previous.content() <= memory.content() {
                self.truncated = true;
                return;
            }
            self.bytes -= previous.bytes();
        }
        self.bytes += size;
        self.items.insert(memory.id().into(), memory);
        while self.items.len() > self.limits.max_results || self.bytes > self.limits.max_total_bytes
        {
            let (_, removed) = self.items.pop_last().expect("nonempty selection");
            self.bytes -= removed.bytes();
            self.truncated = true;
        }
    }
    pub(super) fn finish(self) -> MemorySelection {
        MemorySelection {
            memories: self.items.into_values().collect(),
            truncated: self.truncated,
        }
    }
}
