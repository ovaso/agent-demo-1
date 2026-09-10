mod memory;
pub use memory::MemoryRunStore;
#[cfg(feature = "sqlite")]
pub(super) mod sqlite;

use super::{RunState, RuntimeError};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// 覆盖模型和工具执行的排他所有权；释放所有权不撤销已产生的外部副作用。
pub struct RunLease {
    memory: Option<Arc<AtomicBool>>,
    #[cfg(feature = "sqlite")]
    file: Option<std::fs::File>,
}

impl Drop for RunLease {
    fn drop(&mut self) {
        if let Some(busy) = &self.memory {
            busy.store(false, Ordering::Release);
        }
        #[cfg(feature = "sqlite")]
        if let Some(file) = &self.file {
            let _ = file.unlock();
        }
    }
}

/// 原子保存完整执行检查点。自定义后端必须实现排他执行和版本比较。
pub trait RunStore {
    type Lease;
    fn acquire(&self) -> Result<Self::Lease, RuntimeError>;
    fn load(&self, run_id: &str) -> Result<Option<RunState>, RuntimeError>;
    fn create(&mut self, state: &RunState) -> Result<(), RuntimeError>;
    fn save(&mut self, state: &RunState, expected_revision: u64) -> Result<(), RuntimeError>;
}

fn check_size(state: &RunState) -> Result<(), RuntimeError> {
    state.validate()?;
    super::serialization::check(state, state.limits.max_checkpoint_bytes)
}
