//! Progress-gated grants within an operator-authorized ceiling.
use super::super::{RunState, RunStore, Runtime, RuntimeError};
use crate::{memory::MemoryStore, model::ModelProvider, tool::ToolCall, trace::TraceSink};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

const MAX_EXTENSIONS: usize = 16;
const RECENT_RESULTS: usize = 64;
const PROGRESS_WINDOW: u64 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepExtensionPolicy {
    pub hard_max_steps: u64,
    pub step_increment: u64,
    pub max_extensions: usize,
}

impl Default for StepExtensionPolicy {
    fn default() -> Self {
        Self {
            hard_max_steps: 32,
            step_increment: 8,
            max_extensions: 3,
        }
    }
}

impl StepExtensionPolicy {
    /// Validate a policy against the currently authorized cumulative allocation.
    pub fn validate(&self, granted: u64) -> Result<(), RuntimeError> {
        if self.hard_max_steps < granted
            || self.step_increment == 0
            || self.max_extensions == 0
            || self.max_extensions > MAX_EXTENSIONS
        {
            return Err(RuntimeError::Invalid(
                "续期硬上限不能小于已获准额度，单次增量须大于零，次数须为 1..=16".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepExtension {
    pub at_model_call: u64,
    pub previous_limit: u64,
    pub granted_limit: u64,
    pub new_progress: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepExtensionBlock {
    Disabled,
    HardLimit,
    ExtensionLimit,
    NoRecentProgress,
}

impl StepExtensionBlock {
    pub fn description(self) -> &'static str {
        match self {
            Self::Disabled => "自动续期未启用",
            Self::HardLimit => "已达到模型步数硬上限",
            Self::ExtensionLimit => "自动续期次数已用完",
            Self::NoRecentProgress => "最近没有可用于续期的新进展",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::agent::runtime) struct StepProgress {
    pub(in crate::agent::runtime) extensions: Vec<StepExtension>,
    sequence: u64,
    consumed_sequence: u64,
    last_model_call: u64,
    recent: VecDeque<u64>,
}

impl RunState {
    /// None means a grant is eligible once the current allocation is exhausted.
    pub fn step_extension_block(&self) -> Option<StepExtensionBlock> {
        let Some(policy) = &self.limits.step_extension else {
            return Some(StepExtensionBlock::Disabled);
        };
        if self.limits.max_steps >= policy.hard_max_steps {
            return Some(StepExtensionBlock::HardLimit);
        }
        if self.budget.step_extensions().len() >= policy.max_extensions {
            return Some(StepExtensionBlock::ExtensionLimit);
        }
        if self.budget.step_progress.as_ref().is_none_or(|progress| {
            progress.sequence <= progress.consumed_sequence
                || self
                    .budget
                    .model_calls
                    .saturating_sub(progress.last_model_call)
                    >= PROGRESS_WINDOW
        }) {
            return Some(StepExtensionBlock::NoRecentProgress);
        }
        None
    }
}

impl<M: ModelProvider, R: RunStore, S: MemoryStore, T: TraceSink> Runtime<M, R, S, T> {
    /// Operator configuration; keeps pause state, spent calls and prior grants.
    pub fn set_step_extension_policy(
        &mut self,
        id: &str,
        policy: StepExtensionPolicy,
    ) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        policy.validate(state.limits.max_steps)?;
        if policy.max_extensions < state.budget.step_extensions().len() {
            return Err(RuntimeError::Invalid("不能抹除已经发生的续期次数".into()));
        }
        state.limits.step_extension = Some(policy);
        if state.budget.step_progress.is_none() {
            // Older checkpoints contain no progress ledger. An operator may
            // adopt their last confirmed successful receipt once, at a safe boundary.
            let receipt = if state.phase == super::super::LoopPhase::Model
                && state.last_tool_succeeded == Some(true)
            {
                state.context.last().and_then(|last| {
                    let id = last.tool_call_id()?;
                    state
                        .context
                        .history()
                        .filter_map(|message| message.tool_calls())
                        .flatten()
                        .find(|call| {
                            call.id() == id
                                && !(state.planning && call.name().starts_with("runtime_"))
                        })
                        .map(|call| (call.clone(), last.content().to_owned()))
                })
            } else {
                None
            };
            if let Some((call, output)) = receipt {
                record_tool(&mut state, &call, &output);
            } else {
                state.budget.step_progress = Some(StepProgress::default());
            }
        }
        self.commit(&mut state)?;
        Ok(state)
    }
}

pub(in crate::agent::runtime) fn extend(state: &mut RunState) -> Option<StepExtension> {
    if state.budget.model_calls < state.limits.max_steps || state.step_extension_block().is_some() {
        return None;
    }
    let policy = state
        .limits
        .step_extension
        .as_ref()
        .expect("eligible policy");
    let progress = state
        .budget
        .step_progress
        .as_mut()
        .expect("eligible progress");
    let grant = StepExtension {
        at_model_call: state.budget.model_calls,
        previous_limit: state.limits.max_steps,
        granted_limit: state
            .limits
            .max_steps
            .saturating_add(policy.step_increment)
            .min(policy.hard_max_steps),
        new_progress: progress.sequence - progress.consumed_sequence,
    };
    state.limits.max_steps = grant.granted_limit;
    progress.consumed_sequence = progress.sequence;
    progress.extensions.push(grant.clone());
    Some(grant)
}

pub(in crate::agent::runtime) fn record_tool(state: &mut RunState, call: &ToolCall, output: &str) {
    if state.limits.step_extension.is_none() {
        return;
    }
    // Fixed FNV-1a fingerprints deduplicate observations across process restarts.
    // They are novelty hints, not a security boundary or proof of task completion.
    let mut hash = fingerprint(b"tool", call.name().as_bytes());
    for (name, value) in call.arguments().iter() {
        add(&mut hash, name.as_bytes());
        add(&mut hash, value.as_bytes());
    }
    add(&mut hash, output.as_bytes());
    record(state, hash);
}

pub(in crate::agent::runtime) fn record_node(state: &mut RunState, id: &str, version: u64) {
    if state.limits.step_extension.is_none() {
        return;
    }
    let mut hash = fingerprint(b"node", id.as_bytes());
    add(&mut hash, &version.to_le_bytes());
    record(state, hash);
}

pub(in crate::agent::runtime) fn record_control(state: &mut RunState, kind: &str, identity: &[u8]) {
    if state.limits.step_extension.is_some() {
        record(state, fingerprint(kind.as_bytes(), identity));
    }
}

fn record(state: &mut RunState, hash: u64) {
    let progress = state
        .budget
        .step_progress
        .get_or_insert_with(StepProgress::default);
    if progress.recent.contains(&hash) {
        return;
    }
    if progress.recent.len() == RECENT_RESULTS {
        progress.recent.pop_front();
    }
    progress.recent.push_back(hash);
    progress.sequence = progress.sequence.saturating_add(1);
    progress.last_model_call = state.budget.model_calls;
}

fn fingerprint(kind: &[u8], value: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325;
    add(&mut hash, kind);
    add(&mut hash, value);
    hash
}

fn add(hash: &mut u64, bytes: &[u8]) {
    for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
        *hash = (*hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
}

pub(in crate::agent::runtime) fn validate(state: &RunState) -> Result<(), RuntimeError> {
    let Some(progress) = &state.budget.step_progress else {
        return Ok(());
    };
    if progress.recent.len() > RECENT_RESULTS
        || progress.extensions.len() > MAX_EXTENSIONS
        || progress.consumed_sequence > progress.sequence
        || progress.last_model_call > state.budget.model_calls
        || progress.extensions.iter().any(|grant| {
            grant.previous_limit >= grant.granted_limit
                || grant.at_model_call < grant.previous_limit
                || grant.at_model_call > state.budget.model_calls
                || grant.new_progress == 0
        })
        || state
            .limits
            .step_extension
            .as_ref()
            .is_some_and(|policy| progress.extensions.len() > policy.max_extensions)
    {
        return Err(RuntimeError::Invalid("预算续期记录无效".into()));
    }
    Ok(())
}
