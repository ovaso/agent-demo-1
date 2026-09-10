//! 计划通过入口校验后的共同状态转换；与操作者/模型各自的权限校验分离。
use super::delivery;
use crate::agent::{
    planning::Plan,
    runtime::{RunState, RuntimeError, budget},
};

pub(in crate::agent::runtime) fn save(
    state: &mut RunState,
    expected_revision: u64,
    plan: Plan,
) -> Result<u64, RuntimeError> {
    let revision = state.plans.propose(expected_revision, plan)?;
    delivery::supersede(state);
    if state.graph.current().is_some() {
        state.graph.bind(
            revision,
            state.plans.current().expect("saved plan"),
            state.work_revision,
        )?;
    }
    budget::steps::record_control(state, "plan", &revision.to_le_bytes());
    Ok(revision)
}
