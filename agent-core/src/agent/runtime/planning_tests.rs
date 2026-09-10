use super::{
    tests::{batch, runtime},
    *,
};
use crate::{
    context::Context,
    model::ModelResponse,
    tool::{Arguments, ToolCall},
};
use std::sync::atomic::Ordering;

pub(super) fn plan_call(id: &str) -> ToolCall {
    ToolCall::new(id, "runtime_plan", Arguments::new().with("expected_revision", "0").with("plan", r#"{"goal":"ignored paraphrase","requirements":["checked"],"tasks":[{"id":"a","description":"inspect","acceptance":["checked"],"action":{"kind":"agent","prompt":"inspect and check"}}]}"#))
}

#[test]
fn plan_only_blocks_actual_tools_and_execute_preserves_budget_and_plan() {
    let response = ModelResponse::tool_calls(vec![
        ToolCall::new("bad", "count", Arguments::new()),
        plan_call("plan"),
        ToolCall::new("ready", "runtime_plan_ready", Arguments::new()),
        ToolCall::new("skip", "count", Arguments::new()),
    ]);
    let (mut runtime, count) = runtime(
        MemoryRunStore::new(),
        vec![
            Ok(response),
            Ok(batch(&["work"])),
            Ok(ModelResponse::text("done")),
        ],
    );
    runtime
        .start_with_options(
            "run",
            "session",
            "go",
            Context::new(),
            RunOptions::plan_only(RunLimits::new(3)),
        )
        .unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_eq!(state.status(), &RunStatus::Paused(PauseReason::PlanReady));
    assert_eq!(state.plans().current().unwrap().goal, "go");
    assert_eq!(state.pending_tool_calls().count(), 0);
    assert_eq!(
        runtime
            .resume("run", &mut |_| panic!("must remain paused"))
            .unwrap()
            .budget()
            .model_calls(),
        1
    );
    runtime.execute_plan("run").unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(state.intent(), WorkIntent::Execute);
    assert_eq!(state.budget().model_calls(), 3);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(state.plans().revision(), 1);
    assert_eq!(state.result().unwrap().text(), "done");
}

#[test]
fn model_board_writes_have_runtime_authorship_and_cannot_claim_verification() {
    let entry = |kind| {
        serde_json::json!({"key":"finding","expected_revision":0,"kind":kind,"content":"candidate"})
            .to_string()
    };
    let response = ModelResponse::tool_calls(vec![
        ToolCall::new(
            "fake",
            "runtime_board_write",
            Arguments::new().with("update", entry("verification")),
        ),
        ToolCall::new(
            "real",
            "runtime_board_write",
            Arguments::new().with("update", entry("hypothesis")),
        ),
    ]);
    let (mut runtime, _) = runtime(
        MemoryRunStore::new(),
        vec![Ok(response), Ok(ModelResponse::text("need a plan"))],
    );
    runtime
        .start_with_options(
            "run",
            "session",
            "go",
            Context::new(),
            RunOptions::plan_only(RunLimits::new(2)),
        )
        .unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(state.blackboard().sequence(), 1);
    assert_eq!(state.blackboard().latest("finding").unwrap().author, "main");
    assert!(state.result().is_none());
    assert!(runtime.execute_plan("run").is_err());
}

#[test]
fn planning_context_exposes_only_read_capabilities_and_keeps_saved_history_unchanged() {
    let (mut runtime, _) = runtime(MemoryRunStore::new(), vec![]);
    let state = runtime
        .start_with_options(
            "run",
            "session",
            "go",
            Context::new(),
            RunOptions::plan_only(RunLimits::new(1)),
        )
        .unwrap();
    let (messages, tools) = super::planning_prompt::request_context(&state).unwrap();
    assert!(!tools.iter().any(|tool| tool.name() == "count"));
    assert!(tools.iter().any(|tool| tool.name() == "runtime_plan"));
    assert_eq!(messages.len(), state.context().len() + 1);
    assert_eq!(
        state.context().snapshot(),
        vec![crate::context::Message::user("go")]
    );
}
