//! Durable reservations prevent failures and process restarts from resetting usage.
use super::{RunState, RunStore, Runtime, RuntimeError};
use crate::model::ModelUsage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TokenUsage {
    input: u64,
    output: u64,
    estimated: u64,
    unknown_requests: u64,
    unmetered_requests: u64,
    accounted_calls: u64,
    input_overhead: u64,
    pending: Option<Reservation>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Reservation {
    logical_bytes: u64,
    input: u64,
    output: Option<u64>,
}
pub(super) struct Allocation {
    pub output_limit: Option<u64>,
}

impl TokenUsage {
    pub fn input_tokens(&self) -> u64 {
        self.input
    }
    pub fn output_tokens(&self) -> u64 {
        self.output
    }
    pub fn estimated_tokens(&self) -> u64 {
        self.estimated
    }
    pub fn unknown_requests(&self) -> u64 {
        self.unknown_requests
    }
    pub fn unmetered_requests(&self) -> u64 {
        self.unmetered_requests
    }
    pub fn accounted_requests(&self) -> u64 {
        self.accounted_calls
            .saturating_add(u64::from(self.pending.is_some()))
    }
    pub(super) fn validate(&self, calls: u64, in_flight: bool) -> Result<(), RuntimeError> {
        if self.accounted_requests() > calls
            || (self.pending.is_some() && (!in_flight || self.accounted_requests() != calls))
        {
            return Err(RuntimeError::Invalid(
                "Token 预留与模型调用检查点不一致".into(),
            ));
        }
        Ok(())
    }
    pub fn reserved_tokens(&self) -> u64 {
        self.pending
            .as_ref()
            .map_or(0, |r| r.input.saturating_add(r.output.unwrap_or(0)))
    }
    pub fn total_tokens(&self) -> u64 {
        self.input
            .saturating_add(self.output)
            .saturating_add(self.estimated)
            .saturating_add(self.reserved_tokens())
    }
    pub(super) fn synchronize(&mut self, model_calls: u64) {
        if self.pending.is_none() && self.accounted_calls < model_calls {
            self.unmetered_requests = self
                .unmetered_requests
                .saturating_add(model_calls - self.accounted_calls);
            self.accounted_calls = model_calls;
        }
    }
    pub(super) fn allocate(
        &mut self,
        bytes: usize,
        requested: Option<u64>,
        ceiling: Option<u64>,
        calls: u64,
    ) -> Option<Allocation> {
        self.synchronize(calls);
        if self.pending.is_some() || (ceiling.is_some() && self.unmetered_requests > 0) {
            return None;
        }
        let input = (bytes as u64).saturating_add(self.input_overhead);
        let output = if let Some(ceiling) = ceiling {
            let remaining = ceiling.saturating_sub(self.total_tokens());
            if remaining <= input {
                return None;
            }
            Some(requested.unwrap_or(8192).min(remaining - input))
        } else {
            requested
        };
        self.pending = Some(Reservation {
            logical_bytes: bytes as u64,
            input,
            output,
        });
        Some(Allocation {
            output_limit: output,
        })
    }
    pub(super) fn settle(&mut self, usage: ModelUsage) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        self.accounted_calls = self.accounted_calls.saturating_add(1);
        self.input = self.input.saturating_add(usage.input_tokens.unwrap_or(0));
        self.output = self.output.saturating_add(usage.output_tokens.unwrap_or(0));
        if let Some(input) = usage.input_tokens {
            self.input_overhead = self
                .input_overhead
                .max(input.saturating_sub(pending.logical_bytes));
        } else {
            self.estimated = self.estimated.saturating_add(pending.input);
        }
        if usage.output_tokens.is_none() {
            self.estimated = self.estimated.saturating_add(pending.output.unwrap_or(0));
            if pending.output.is_none() {
                self.unmetered_requests = self.unmetered_requests.saturating_add(1);
            }
        }
        if usage.input_tokens.is_none() || usage.output_tokens.is_none() {
            self.unknown_requests = self.unknown_requests.saturating_add(1);
        }
    }
}

impl<
    M: crate::model::ModelProvider,
    R: RunStore,
    S: crate::memory::MemoryStore,
    T: crate::trace::TraceSink,
> Runtime<M, R, S, T>
{
    pub fn set_output_budget(&mut self, id: &str, limit: u64) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        if limit == 0 {
            return Err(RuntimeError::Invalid("单次输出上限必须大于零".into()));
        }
        state.limits.max_output_tokens = Some(limit);
        self.commit(&mut state)?;
        Ok(state)
    }

    pub fn set_token_budget(
        &mut self,
        id: &str,
        limit: Option<u64>,
    ) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        state
            .budget
            .token_usage
            .synchronize(state.budget.model_calls);
        if let Some(limit) = limit {
            if limit == 0 || limit <= state.budget.token_usage.total_tokens() {
                return Err(RuntimeError::Invalid(
                    "Token 总预算必须大于已用与已预留额度".into(),
                ));
            }
            if state.budget.token_usage.unmetered_requests() > 0 {
                return Err(RuntimeError::Invalid(
                    "历史请求缺少完整用量，不能恢复可靠的累计 Token 上限；请为新任务配置预算"
                        .into(),
                ));
            }
        }
        state.limits.max_total_tokens = limit;
        self.commit(&mut state)?;
        Ok(state)
    }
}
