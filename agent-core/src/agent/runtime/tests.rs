use super::*;
use crate::{
    context::Context,
    memory::{Memory, MemoryStore, MemoryStoreError},
    model::{ModelError, ModelProvider, ModelRequest, ModelResponse},
    tool::{Arguments, Parameter, Registry, Tool, ToolCall, ToolError, ToolOutput},
};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Default)]
pub(super) struct Memories(BTreeMap<String, Memory>);
impl MemoryStore for Memories {
    fn get(&self, id: &str) -> Result<Option<Memory>, MemoryStoreError> {
        Ok(self.0.get(id).cloned())
    }
    fn save(&mut self, memory: Memory) -> Result<(), MemoryStoreError> {
        self.0.insert(memory.id().into(), memory);
        Ok(())
    }
    fn list(&self) -> Result<Vec<Memory>, MemoryStoreError> {
        Ok(self.0.values().cloned().collect())
    }
    fn search(&self, _: &str) -> Result<Vec<Memory>, MemoryStoreError> {
        Ok(Vec::new())
    }
    fn delete(&mut self, id: &str) -> Result<bool, MemoryStoreError> {
        Ok(self.0.remove(id).is_some())
    }
}

pub(super) struct Model(pub VecDeque<Result<ModelResponse, ModelError>>);
impl ModelProvider for Model {
    fn complete(&mut self, request: ModelRequest<'_>) -> Result<ModelResponse, ModelError> {
        assert!(!request.messages().is_empty());
        self.0.pop_front().expect("unexpected model request")
    }
}

pub(super) struct Counter(pub Arc<AtomicUsize>);
impl Tool for Counter {
    fn name(&self) -> &str {
        "count"
    }
    fn description(&self) -> &str {
        "record effect"
    }
    fn parameters(&self) -> &[Parameter] {
        &[]
    }
    fn invoke(&self, _: &Arguments) -> Result<ToolOutput, ToolError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ToolOutput::text("effect recorded"))
    }
}

pub(super) fn batch(ids: &[&str]) -> ModelResponse {
    ModelResponse::tool_calls(
        ids.iter()
            .map(|id| ToolCall::new(*id, "count", Arguments::new()))
            .collect(),
    )
}

pub(super) fn runtime<R: RunStore>(
    store: R,
    responses: Vec<Result<ModelResponse, ModelError>>,
) -> (Runtime<Model, R, Memories>, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let mut tools = Registry::new();
    tools.register(Counter(Arc::clone(&count))).unwrap();
    (
        Runtime::new(Model(responses.into()), store, Memories::default(), tools),
        count,
    )
}

fn start<R: RunStore>(runtime: &mut Runtime<Model, R, Memories>, steps: u64) {
    runtime
        .start(
            "run",
            "session",
            "go",
            Context::new(),
            RunLimits::new(steps),
        )
        .unwrap();
}

#[test]
fn pauses_between_tools_and_resumes_without_repeating_input_or_effects() {
    let (mut runtime, count) = runtime(
        MemoryRunStore::new(),
        vec![Ok(batch(&["a", "b"])), Ok(ModelResponse::text("done"))],
    );
    start(&mut runtime, 2);
    runtime.advance("run", &mut |_| {}).unwrap();
    runtime.advance("run", &mut |_| {}).unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    runtime.pause("run").unwrap();
    runtime.advance("run", &mut |_| panic!("paused")).unwrap();
    let mut text = String::new();
    let state = runtime
        .resume("run", &mut |delta| text.push_str(delta))
        .unwrap();
    assert_eq!(state.status(), &RunStatus::Completed);
    assert_eq!(state.budget().model_calls(), 2);
    assert_eq!(state.budget().tool_calls(), 2);
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert_eq!(text, "done");
    assert_eq!(
        state
            .context()
            .history()
            .filter(|m| m.role() == crate::context::Role::User)
            .count(),
        1
    );
}

#[test]
fn failed_model_attempt_and_resume_share_the_same_budget() {
    let (mut runtime, _) = runtime(
        MemoryRunStore::new(),
        vec![
            Err(ModelError::new("network")),
            Ok(ModelResponse::text("recovered")),
        ],
    );
    start(&mut runtime, 1);
    assert!(runtime.resume("run", &mut |_| {}).is_err());
    let state = runtime.resume("run", &mut |_| panic!("budget")).unwrap();
    assert_eq!(state.status(), &RunStatus::Paused(PauseReason::Budget));
    assert_eq!(state.budget().model_calls(), 1);
    runtime.set_max_steps("run", 2).unwrap();
    assert_eq!(
        runtime
            .resume("run", &mut |_| {})
            .unwrap()
            .result()
            .unwrap()
            .text(),
        "recovered"
    );
}

struct FailingStore {
    inner: MemoryRunStore,
    fail_revision: Option<u64>,
}
impl RunStore for FailingStore {
    type Lease = RunLease;
    fn acquire(&self) -> Result<RunLease, RuntimeError> {
        self.inner.acquire()
    }
    fn load(&self, id: &str) -> Result<Option<RunState>, RuntimeError> {
        self.inner.load(id)
    }
    fn create(&mut self, state: &RunState) -> Result<(), RuntimeError> {
        self.inner.create(state)
    }
    fn save(&mut self, state: &RunState, revision: u64) -> Result<(), RuntimeError> {
        if self.fail_revision == Some(state.revision) {
            self.fail_revision = None;
            return Err(RuntimeError::Storage("injected crash".into()));
        }
        self.inner.save(state, revision)
    }
}

#[test]
fn uncertain_tool_is_not_replayed_after_result_checkpoint_failure() {
    let store = FailingStore {
        inner: MemoryRunStore::new(),
        fail_revision: Some(4),
    };
    let (mut runtime, count) = runtime(
        store,
        vec![Ok(batch(&["a"])), Ok(ModelResponse::text("done"))],
    );
    start(&mut runtime, 2);
    runtime.advance("run", &mut |_| {}).unwrap();
    assert!(runtime.advance("run", &mut |_| {}).is_err());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(
        runtime.resume("run", &mut |_| {}).unwrap_err(),
        RuntimeError::NeedsResolution("a".into())
    );
    assert!(runtime.cancel("run").is_err());
    runtime
        .resolve_tool("run", "a", ToolOutput::text("verified effect"))
        .unwrap();
    assert_eq!(
        runtime.resume("run", &mut |_| {}).unwrap().status(),
        &RunStatus::Completed
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn rejected_pre_dispatch_checkpoint_cannot_execute_a_tool() {
    let store = FailingStore {
        inner: MemoryRunStore::new(),
        fail_revision: Some(3),
    };
    let (mut runtime, count) = runtime(
        store,
        vec![Ok(batch(&["a"])), Ok(ModelResponse::text("done"))],
    );
    start(&mut runtime, 2);
    runtime.advance("run", &mut |_| {}).unwrap();
    assert!(runtime.advance("run", &mut |_| {}).is_err());
    assert_eq!(count.load(Ordering::SeqCst), 0);
    runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn duplicate_calls_are_rejected_and_cancelled_runs_cannot_resume() {
    let (mut runtime, count) = runtime(MemoryRunStore::new(), vec![Ok(batch(&["a", "a"]))]);
    start(&mut runtime, 2);
    assert!(runtime.resume("run", &mut |_| {}).is_err());
    assert_eq!(count.load(Ordering::SeqCst), 0);
    runtime.cancel("run").unwrap();
    assert!(runtime.resume("run", &mut |_| {}).is_err());
}

#[test]
fn memory_store_rejects_concurrent_execution_and_stale_checkpoints() {
    let (mut runtime, _) = runtime(MemoryRunStore::new(), vec![]);
    start(&mut runtime, 1);
    let _lease = runtime.store.acquire().unwrap();
    assert!(matches!(runtime.store.acquire(), Err(RuntimeError::Busy)));
    let mut stale = runtime.state("run").unwrap();
    stale.revision += 1;
    runtime.store.save(&stale, 0).unwrap();
    assert_eq!(
        runtime.store.save(&stale, 0).unwrap_err(),
        RuntimeError::Conflict
    );
}

#[test]
fn oversized_tool_result_remains_unresolved_instead_of_being_replayed() {
    let (mut runtime, count) = runtime(
        MemoryRunStore::new(),
        vec![Ok(batch(&["a"])), Ok(ModelResponse::text("done"))],
    );
    let limits = RunLimits {
        max_tool_output_bytes: 3,
        ..RunLimits::new(2)
    };
    runtime
        .start("run", "session", "go", Context::new(), limits)
        .unwrap();
    assert!(runtime.resume("run", &mut |_| {}).is_err());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(matches!(
        runtime.resume("run", &mut |_| {}),
        Err(RuntimeError::NeedsResolution(_))
    ));
    runtime
        .resolve_tool("run", "a", ToolOutput::text("ok"))
        .unwrap();
    runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

struct Finish;
impl Tool for Finish {
    fn name(&self) -> &str {
        "finish"
    }
    fn description(&self) -> &str {
        "finish session"
    }
    fn parameters(&self) -> &[Parameter] {
        &[]
    }
    fn invoke(&self, _: &Arguments) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::finish_session("summary"))
    }
}

#[test]
fn session_finish_settles_the_batch_before_replayable_memory_finalization() {
    let response = ModelResponse::tool_calls(vec![
        ToolCall::new("end", "finish", Arguments::new()),
        ToolCall::new("a", "count", Arguments::new()),
    ]);
    let (mut runtime, count) = runtime(MemoryRunStore::new(), vec![Ok(response)]);
    runtime.tools.register(Finish).unwrap();
    start(&mut runtime, 1);
    runtime.advance("run", &mut |_| {}).unwrap();
    let state = runtime.advance("run", &mut |_| {}).unwrap();
    assert!(matches!(state.phase(), LoopPhase::FinishSession { .. }));
    assert_eq!(state.pending_tool_calls().count(), 0);
    runtime.pause("run").unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert!(state.result().unwrap().session_finished());
    assert_eq!(runtime.memory.list().unwrap().len(), 1);
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn incomplete_custom_model_response_cannot_execute_tools() {
    let (mut runtime, count) = runtime(
        MemoryRunStore::new(),
        vec![
            Ok(batch(&["cut"]).with_stop_reason(crate::model::StopReason::Length)),
            Ok(ModelResponse::text("continued")),
        ],
    );
    start(&mut runtime, 3);
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert!(matches!(
        state.status(),
        RunStatus::Paused(PauseReason::Model(_))
    ));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_eq!(state.pending_tool_calls().count(), 0);
    assert_eq!(
        runtime.resume("run", &mut |_| {}).unwrap().status(),
        &RunStatus::Completed
    );
}

#[test]
fn token_budget_survives_resume_and_counts_cached_input_once() {
    let usage = crate::model::ModelUsage {
        input_tokens: Some(1000),
        output_tokens: Some(100),
        cached_input_tokens: Some(900),
        reasoning_tokens: Some(20),
        ..Default::default()
    };
    let (mut runtime, count) = runtime(
        MemoryRunStore::new(),
        vec![
            Ok(batch(&["a"]).with_usage(usage)),
            Ok(ModelResponse::text("continued").with_usage(usage)),
        ],
    );
    runtime
        .start(
            "run",
            "session",
            "go",
            Context::new(),
            RunLimits {
                max_total_tokens: Some(1500),
                max_output_tokens: Some(8192),
                ..RunLimits::new(3)
            },
        )
        .unwrap();
    let paused = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(
        paused.status(),
        &RunStatus::Paused(PauseReason::TokenBudget)
    );
    assert_eq!(paused.budget().model_calls(), 1);
    assert_eq!(paused.budget().token_usage().total_tokens(), 1100);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    runtime.set_token_budget("run", Some(20_000)).unwrap();
    let done = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(done.status(), &RunStatus::Completed);
    assert_eq!(done.budget().token_usage().total_tokens(), 2200);
    assert_eq!(done.budget().model_calls(), 2);
}

#[test]
fn interrupted_process_keeps_reserved_tokens_and_oversized_calls_have_no_effects() {
    let usage = crate::model::ModelUsage {
        input_tokens: Some(40),
        output_tokens: Some(5),
        ..Default::default()
    };
    let (mut runtime, _) = runtime(
        MemoryRunStore::new(),
        vec![Ok(ModelResponse::text("done").with_usage(usage))],
    );
    runtime
        .start(
            "run",
            "session",
            "go",
            Context::new(),
            RunLimits {
                max_output_tokens: Some(100),
                max_total_tokens: Some(20_000),
                ..RunLimits::new(3)
            },
        )
        .unwrap();
    let mut state = runtime.state("run").unwrap();
    state
        .budget
        .token_usage
        .allocate(200, Some(100), Some(20_000), 0)
        .unwrap();
    state.budget.model_calls = 1;
    state.phase = LoopPhase::ModelInFlight;
    runtime.commit(&mut state).unwrap();
    let done = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(done.budget().model_calls(), 2);
    assert_eq!(done.budget().token_usage().estimated_tokens(), 300);
    assert_eq!(done.budget().token_usage().total_tokens(), 345);
}

#[test]
fn oversized_call_identifiers_are_rejected_before_dispatch() {
    let (mut runtime, count) =
        runtime(MemoryRunStore::new(), vec![Ok(batch(&[&"x".repeat(2048)]))]);
    runtime
        .start(
            "run",
            "session",
            "go",
            Context::new(),
            RunLimits {
                max_context_bytes: 512,
                ..RunLimits::new(2)
            },
        )
        .unwrap();
    assert!(runtime.resume("run", &mut |_| {}).is_err());
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_eq!(runtime.state("run").unwrap().budget().tool_calls(), 0);
}
