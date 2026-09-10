use super::{
    tests::{Memories, Model, batch, runtime},
    *,
};
use crate::{
    agent::{delegation::AgentSpec, graph::NodeStatus, routing::ExecutionMode},
    context::Context,
    model::{ModelError, ModelResponse},
    tool::{Arguments, Parameter, Registry, Tool, ToolCall, ToolError, ToolOutput},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Probe(Arc<AtomicUsize>);
impl Tool for Probe {
    fn name(&self) -> &str {
        "probe"
    }
    fn description(&self) -> &str {
        "distinct observations"
    }
    fn parameters(&self) -> &[Parameter] {
        &[]
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn invoke(&self, _: &Arguments) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::text(format!(
            "observation {}",
            self.0.fetch_add(1, Ordering::SeqCst)
        )))
    }
}

fn probe(id: usize) -> Result<ModelResponse, ModelError> {
    Ok(ModelResponse::tool_calls(vec![ToolCall::new(
        format!("p{id}"),
        "probe",
        Arguments::new(),
    )]))
}

fn observed<R: RunStore>(
    store: R,
    responses: Vec<Result<ModelResponse, ModelError>>,
) -> (Runtime<Model, R, Memories>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = Registry::new();
    registry.register(Probe(Arc::clone(&calls))).unwrap();
    (
        Runtime::new(
            Model(responses.into()),
            store,
            Memories::default(),
            registry,
        ),
        calls,
    )
}

fn limits(initial: u64, hard: u64, increment: u64, count: usize) -> RunLimits {
    RunLimits {
        step_extension: Some(StepExtensionPolicy {
            hard_max_steps: hard,
            step_increment: increment,
            max_extensions: count,
        }),
        ..RunLimits::new(initial)
    }
}

#[test]
fn fresh_progress_extends_until_hard_cap_and_resume_cannot_refill_it() {
    let (mut runtime, calls) = observed(MemoryRunStore::new(), (0..5).map(probe).collect());
    runtime
        .start("run", "session", "read", Context::new(), limits(2, 5, 2, 3))
        .unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(state.status(), &RunStatus::Paused(PauseReason::Budget));
    assert_eq!(state.budget().model_calls(), 5);
    assert_eq!(calls.load(Ordering::SeqCst), 5);
    assert_eq!(
        state
            .budget()
            .step_extensions()
            .iter()
            .map(|grant| (grant.previous_limit, grant.granted_limit))
            .collect::<Vec<_>>(),
        [(2, 4), (4, 5)]
    );
    assert_eq!(
        state.step_extension_block(),
        Some(StepExtensionBlock::HardLimit)
    );
    let resumed = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(resumed.budget().tool_calls(), state.budget().tool_calls());
    assert_eq!(
        resumed.budget().step_extensions(),
        state.budget().step_extensions()
    );
    assert_eq!(resumed.budget().model_calls(), 5);
}

#[test]
fn extension_count_can_stop_before_the_hard_cap() {
    let (mut runtime, _) = observed(MemoryRunStore::new(), (0..2).map(probe).collect());
    runtime
        .start(
            "run",
            "session",
            "read",
            Context::new(),
            limits(1, 10, 1, 1),
        )
        .unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(state.limits().max_steps, 2);
    assert_eq!(
        state.step_extension_block(),
        Some(StepExtensionBlock::ExtensionLimit)
    );
    assert_eq!(state.budget().step_extensions().len(), 1);
}

#[test]
fn duplicate_results_and_stale_progress_do_not_earn_another_grant() {
    for initial in [1, 3] {
        let responses = (0..3).map(|id| Ok(batch(&[&format!("c{id}")]))).collect();
        let (mut runtime, _) = runtime(MemoryRunStore::new(), responses);
        runtime
            .start(
                "run",
                "session",
                "repeat",
                Context::new(),
                limits(initial, 10, 1, 3),
            )
            .unwrap();
        let state = runtime.resume("run", &mut |_| {}).unwrap();
        assert_eq!(
            state.step_extension_block(),
            Some(StepExtensionBlock::NoRecentProgress)
        );
        assert_eq!(
            state.budget().model_calls(),
            if initial == 1 { 2 } else { 3 }
        );
    }
}

#[test]
fn failures_denied_tools_and_runtime_polling_do_not_count_as_progress() {
    let responses = [
        Err(ModelError::new("offline")),
        Ok(ModelResponse::tool_calls(vec![ToolCall::new(
            "x",
            "missing",
            Arguments::new(),
        )])),
        Ok(ModelResponse::tool_calls(vec![ToolCall::new(
            "poll",
            "runtime_agents",
            Arguments::new(),
        )])),
    ];
    for response in responses {
        let (mut runtime, _) = observed(MemoryRunStore::new(), vec![response]);
        runtime
            .start_with_options(
                "run",
                "session",
                "read",
                Context::new(),
                RunOptions {
                    limits: limits(1, 5, 1, 3),
                    planning: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let _ = runtime.resume("run", &mut |_| {});
        let state = runtime.resume("run", &mut |_| {}).unwrap();
        assert_eq!(
            state.step_extension_block(),
            Some(StepExtensionBlock::NoRecentProgress)
        );
        assert_eq!(state.budget().model_calls(), 1);
        assert!(state.budget().step_extensions().is_empty());
    }
}

#[test]
fn tool_and_transition_limits_are_not_extended() {
    let (mut runtime, _) = observed(
        MemoryRunStore::new(),
        vec![Ok(ModelResponse::tool_calls(vec![
            ToolCall::new("a", "probe", Arguments::new()),
            ToolCall::new("b", "probe", Arguments::new()),
        ]))],
    );
    let mut config = limits(1, 5, 1, 3);
    config.max_tool_calls = 1;
    runtime
        .start("run", "session", "read", Context::new(), config)
        .unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(state.budget().tool_calls(), 1);
    assert_eq!(state.pending_tool_calls().count(), 1);
    assert!(state.budget().step_extensions().is_empty());
    let (mut runtime, calls) = observed(MemoryRunStore::new(), vec![probe(0)]);
    let mut config = limits(1, 5, 1, 3);
    config.max_transitions = 1;
    runtime
        .start("run", "session", "read", Context::new(), config)
        .unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(state.budget().step_extensions().is_empty());
}

#[test]
fn grants_are_shared_with_delegates_without_raising_their_local_limit() {
    let (mut runtime, _) = observed(
        MemoryRunStore::new(),
        vec![probe(0), Ok(ModelResponse::text("coordinator summary"))],
    );
    runtime
        .start_with_options(
            "run",
            "session",
            "read",
            Context::new(),
            RunOptions {
                limits: limits(1, 3, 1, 2),
                planning: true,
                ..Default::default()
            },
        )
        .unwrap();
    runtime
        .delegate(
            "run",
            AgentSpec {
                name: "worker".into(),
                instruction: "read".into(),
                acceptance: vec!["report".into()],
                tools: Some(vec!["probe".into()]),
                max_steps: 1,
                depends_on: vec![],
            },
        )
        .unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(state.budget().model_calls(), 2);
    assert_eq!(state.budget().step_extensions().len(), 1);
    assert_eq!(
        state.graph().current().unwrap().nodes["worker"].status,
        NodeStatus::BudgetExceeded
    );
    assert_eq!(
        state.graph().current().unwrap().nodes["worker"]
            .policy
            .as_ref()
            .unwrap()
            .max_steps,
        1
    );
}

#[test]
fn mode_switch_and_manual_allocation_preserve_the_grant_history() {
    let (mut runtime, _) = observed(MemoryRunStore::new(), vec![probe(0), probe(1)]);
    runtime
        .start("run", "session", "read", Context::new(), limits(1, 2, 1, 1))
        .unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    runtime
        .route("run", ExecutionMode::Loop, "retain progress")
        .unwrap();
    let fixed = runtime.set_max_steps("run", 3).unwrap();
    assert!(fixed.limits().step_extension.is_none());
    assert_eq!(
        fixed.budget().step_extensions(),
        state.budget().step_extensions()
    );
    let reenabled = runtime
        .set_step_extension_policy(
            "run",
            StepExtensionPolicy {
                hard_max_steps: 5,
                step_increment: 1,
                max_extensions: 1,
            },
        )
        .unwrap();
    assert_eq!(
        reenabled.step_extension_block(),
        Some(StepExtensionBlock::ExtensionLimit)
    );
    assert_eq!(reenabled.budget().model_calls(), 2);
}

#[test]
fn old_checkpoints_stay_fixed_until_operator_adopts_a_successful_receipt() {
    let (mut runtime, _) = observed(
        MemoryRunStore::new(),
        vec![probe(0), Ok(ModelResponse::text("done"))],
    );
    let original = runtime
        .start("run", "session", "read", Context::new(), RunLimits::new(1))
        .unwrap();
    let json = serde_json::to_string(&original).unwrap();
    assert!(!json.contains("step_extension"));
    assert!(!json.contains("step_progress"));
    let old: RunState = serde_json::from_str(&json).unwrap();
    assert_eq!(
        old.step_extension_block(),
        Some(StepExtensionBlock::Disabled)
    );
    let paused = runtime.resume("run", &mut |_| {}).unwrap();
    let configured = runtime
        .set_step_extension_policy(
            "run",
            StepExtensionPolicy {
                hard_max_steps: 3,
                step_increment: 1,
                max_extensions: 2,
            },
        )
        .unwrap();
    assert_eq!(configured.status(), paused.status());
    assert_eq!(configured.budget().model_calls(), 1);
    let done = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(done.status(), &RunStatus::Completed);
    assert_eq!(done.budget().model_calls(), 2);
    assert_eq!(done.budget().step_extensions().len(), 1);
}

#[test]
fn completed_graph_nodes_earn_shared_grants_across_mode_changes() {
    let (mut runtime, _) = observed(
        MemoryRunStore::new(),
        vec![
            Ok(ModelResponse::text("A complete")),
            Ok(ModelResponse::text("B complete")),
            Ok(ModelResponse::text("Root complete")),
        ],
    );
    runtime
        .start_with_options(
            "run",
            "session",
            "go",
            Context::new(),
            RunOptions {
                limits: limits(1, 3, 1, 2),
                planning: true,
                ..Default::default()
            },
        )
        .unwrap();
    let plan = serde_json::from_str(r#"{"goal":"go","requirements":["reported"],"tasks":[{"id":"a","description":"a","acceptance":["reported"],"action":{"kind":"agent","prompt":"a"}},{"id":"b","description":"b","acceptance":["reported"],"action":{"kind":"agent","prompt":"b"}}]}"#).unwrap();
    runtime.propose_plan("run", 0, plan).unwrap();
    runtime.route("run", ExecutionMode::Graph, "run").unwrap();
    runtime.advance("run", &mut |_| {}).unwrap();
    runtime.advance("run", &mut |_| {}).unwrap();
    runtime
        .route("run", ExecutionMode::Loop, "inspect")
        .unwrap();
    runtime
        .route("run", ExecutionMode::Graph, "continue")
        .unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(state.status(), &RunStatus::Completed);
    assert_eq!(state.budget().model_calls(), 3);
    assert_eq!(state.budget().step_extensions().len(), 2);
}

#[test]
fn unknown_side_effect_is_resolved_before_any_budget_extension() {
    let (mut runtime, _) = observed(MemoryRunStore::new(), vec![probe(0), probe(1)]);
    runtime
        .start("run", "session", "go", Context::new(), limits(2, 4, 1, 2))
        .unwrap();
    for _ in 0..3 {
        runtime.advance("run", &mut |_| {}).unwrap();
    }
    let mut state = runtime.state("run").unwrap();
    state.phase = LoopPhase::ToolInFlight {
        call_id: "p1".into(),
    };
    state.budget.tool_calls = 2;
    runtime.commit(&mut state).unwrap();
    assert!(matches!(
        runtime.resume("run", &mut |_| {}),
        Err(RuntimeError::NeedsResolution(_))
    ));
    let state = runtime.state("run").unwrap();
    assert_eq!(state.budget().model_calls(), 2);
    assert!(state.budget().step_extensions().is_empty());
}

#[test]
fn invalid_policies_are_rejected_before_creating_a_run() {
    for config in [
        limits(2, 1, 1, 1),
        limits(1, 2, 0, 1),
        limits(1, 2, 1, 0),
        limits(1, 20, 1, 17),
    ] {
        let (mut runtime, _) = observed(MemoryRunStore::new(), vec![]);
        assert!(
            runtime
                .start("run", "session", "go", Context::new(), config)
                .is_err()
        );
        assert!(runtime.store().load("run").unwrap().is_none());
    }
}

#[cfg(feature = "sqlite")]
#[test]
fn sqlite_reopen_after_grant_reservation_preserves_usage_and_deduplication() {
    let db = super::sqlite_tests::Database::new();
    let (mut first, _) = observed(
        db.open(),
        vec![probe(0), Err(ModelError::new("interrupted"))],
    );
    first
        .start("run", "session", "read", Context::new(), limits(1, 3, 2, 1))
        .unwrap();
    assert!(first.resume("run", &mut |_| {}).is_err());
    let mut saved = first.state("run").unwrap();
    assert_eq!(saved.budget().model_calls(), 2);
    assert_eq!(saved.budget().step_extensions().len(), 1);
    // The same durable state exists if the process exits after reserving the
    // next attempt but before its response/error is committed.
    saved.phase = LoopPhase::ModelInFlight;
    saved.status = RunStatus::Running;
    first.commit(&mut saved).unwrap();
    drop(first);
    let (mut second, _) = observed(db.open(), vec![Ok(ModelResponse::text("done"))]);
    assert_eq!(second.state("run").unwrap(), saved);
    let done = second.resume("run", &mut |_| {}).unwrap();
    assert_eq!(done.budget().model_calls(), 3);
    assert_eq!(
        done.budget().step_extensions(),
        saved.budget().step_extensions()
    );
    drop(second);
}
