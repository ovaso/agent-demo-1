use super::{
    tests::{batch, runtime},
    *,
};
use crate::agent::{delegation::AgentSpec, graph::NodeStatus};
use crate::{
    context::Context,
    model::ModelResponse,
    tool::{Arguments, ToolCall},
};
use std::sync::atomic::Ordering;

fn spec(name: &str, max_steps: u64, tools: &[&str]) -> AgentSpec {
    AgentSpec {
        name: name.into(),
        instruction: format!("work on {name}"),
        acceptance: vec!["report evidence".into()],
        tools: Some(tools.iter().map(|name| (*name).into()).collect()),
        max_steps,
        depends_on: vec![],
    }
}

#[test]
fn loop_delegation_has_isolated_contexts_and_scoped_tools() {
    let (mut runtime, count) = runtime(
        MemoryRunStore::new(),
        vec![
            Ok(batch(&["denied"])),
            Ok(ModelResponse::text("worker result")),
            Ok(ModelResponse::text("root result")),
        ],
    );
    runtime
        .start("run", "session", "go", Context::new(), RunLimits::new(3))
        .unwrap();
    runtime.delegate("run", spec("reader", 2, &[])).unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_eq!(state.result().unwrap().text(), "root result");
    assert_eq!(
        state.routing().mode(),
        crate::agent::routing::ExecutionMode::Loop
    );
    let worker = &state.graph().current().unwrap().nodes["reader"];
    assert_eq!(worker.policy.as_ref().unwrap().model_calls, 2);
    assert_eq!(state.budget().model_calls(), 3);
    assert!(
        !state
            .context()
            .history()
            .any(|message| message.content() == "worker result")
    );
    assert_eq!(
        state
            .node_context(0, "reader")
            .unwrap()
            .last()
            .unwrap()
            .content(),
        "worker result"
    );
}

#[test]
fn local_budget_can_be_extended_without_resetting_local_or_root_usage() {
    let (mut runtime, count) = runtime(
        MemoryRunStore::new(),
        vec![
            Ok(batch(&["effect"])),
            Ok(ModelResponse::text("needs quota")),
            Ok(ModelResponse::text("worker done")),
            Ok(ModelResponse::text("root done")),
        ],
    );
    runtime
        .start("run", "session", "go", Context::new(), RunLimits::new(4))
        .unwrap();
    runtime
        .delegate("run", spec("worker", 1, &["count"]))
        .unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(
        state.graph().current().unwrap().nodes["worker"].status,
        NodeStatus::BudgetExceeded
    );
    assert_eq!(state.budget().model_calls(), 2);
    runtime.set_agent_budget("run", "worker", 2).unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(state.status(), &RunStatus::Completed);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(state.budget().model_calls(), 4);
    assert_eq!(
        state.graph().current().unwrap().nodes["worker"]
            .policy
            .as_ref()
            .unwrap()
            .model_calls,
        2
    );
}

#[test]
fn plan_only_delegates_cannot_gain_write_tools_or_complete_the_root_plan() {
    let (mut runtime, count) = runtime(
        MemoryRunStore::new(),
        vec![
            Ok(batch(&["blocked"])),
            Ok(ModelResponse::text("investigation")),
            Ok(ModelResponse::tool_calls(vec![
                super::planning_tests::plan_call("p"),
                ToolCall::new("ready", "runtime_plan_ready", Arguments::new()),
            ])),
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
    assert!(
        runtime
            .delegate("run", spec("writer", 1, &["count"]))
            .is_err()
    );
    runtime.delegate("run", spec("reader", 2, &[])).unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(state.status(), &RunStatus::Paused(PauseReason::PlanReady));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_eq!(state.budget().model_calls(), 3);
}

#[test]
fn cancelling_a_delegate_does_not_reset_creation_limit_or_block_root_completion() {
    let (mut runtime, _) = runtime(MemoryRunStore::new(), vec![Ok(ModelResponse::text("done"))]);
    let limits = RunLimits {
        max_delegations: 1,
        ..RunLimits::new(2)
    };
    runtime
        .start("run", "session", "go", Context::new(), limits)
        .unwrap();
    runtime.delegate("run", spec("unused", 1, &[])).unwrap();
    runtime.cancel_agent("run", "unused").unwrap();
    assert!(runtime.delegate("run", spec("another", 1, &[])).is_err());
    assert_eq!(
        runtime.resume("run", &mut |_| {}).unwrap().status(),
        &RunStatus::Completed
    );
}
