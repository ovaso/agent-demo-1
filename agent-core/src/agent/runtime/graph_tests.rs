use super::{tests::runtime, *};
use crate::agent::{
    graph::NodeStatus,
    planning::{Plan, PlanTask, TaskAction, ToolCheck},
    routing::ExecutionMode,
};
use crate::{
    context::Context,
    model::ModelResponse,
    tool::{Arguments, ToolCall},
};
use std::{collections::BTreeMap, sync::atomic::Ordering};

fn plan(actions: Vec<TaskAction>) -> Plan {
    Plan {
        goal: "go".into(),
        requirements: vec!["checked".into()],
        tasks: actions
            .into_iter()
            .enumerate()
            .map(|(index, action)| PlanTask {
                id: format!("n{index}"),
                description: "work".into(),
                acceptance: vec!["checked".into()],
                depends_on: if index == 0 {
                    vec![]
                } else {
                    vec![format!("n{}", index - 1)]
                },
                action,
            })
            .collect(),
    }
}
fn tool(check: ToolCheck) -> TaskAction {
    TaskAction::Tool {
        name: "count".into(),
        arguments: BTreeMap::new(),
        check,
    }
}
fn route_call(mode: &str) -> ToolCall {
    ToolCall::new(
        format!("route-{mode}"),
        "runtime_route",
        Arguments::new()
            .with("mode", mode)
            .with("reason", "task structure"),
    )
}

#[test]
fn graph_runs_tools_in_dependency_order_and_survives_mode_switch_without_repeating() {
    let (mut runtime, count) =
        runtime(MemoryRunStore::new(), vec![Ok(ModelResponse::text("done"))]);
    runtime
        .start("run", "session", "go", Context::new(), RunLimits::new(1))
        .unwrap();
    runtime
        .propose_plan(
            "run",
            0,
            plan(vec![tool(ToolCheck::Succeeded), tool(ToolCheck::Succeeded)]),
        )
        .unwrap();
    runtime
        .route("run", ExecutionMode::Graph, "dependencies")
        .unwrap();
    // start, execute, and accept the first node
    for _ in 0..3 {
        runtime.advance("run", &mut |_| {}).unwrap();
    }
    assert_eq!(count.load(Ordering::SeqCst), 1);
    runtime
        .route("run", ExecutionMode::Loop, "inspect")
        .unwrap();
    runtime
        .route("run", ExecutionMode::Graph, "continue")
        .unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert_eq!(state.status(), &RunStatus::Completed);
    assert_eq!(state.budget().model_calls(), 1);
    assert!(
        state
            .graph()
            .current()
            .unwrap()
            .nodes
            .values()
            .all(|node| node.status == NodeStatus::Succeeded)
    );
    assert_eq!(state.routing().history().len(), 3);
}

#[test]
fn failed_verifier_cannot_be_reported_as_root_success() {
    let (mut runtime, _) = runtime(
        MemoryRunStore::new(),
        vec![Ok(ModelResponse::text("claim success"))],
    );
    runtime
        .start("run", "session", "go", Context::new(), RunLimits::new(1))
        .unwrap();
    runtime
        .propose_plan("run", 0, plan(vec![tool(ToolCheck::ExitCodeZero)]))
        .unwrap();
    runtime.route("run", ExecutionMode::Graph, "check").unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(
        state.graph().current().unwrap().nodes["n0"].status,
        NodeStatus::Failed
    );
    assert!(matches!(
        state.status(),
        RunStatus::Paused(PauseReason::GraphBlocked(_))
    ));
    assert!(state.result().is_none());
}

#[test]
fn model_can_suspend_node_to_loop_then_resume_graph_with_the_same_context() {
    let (mut runtime, _) = runtime(
        MemoryRunStore::new(),
        vec![
            Ok(ModelResponse::tool_calls(vec![route_call("loop")])),
            Ok(ModelResponse::tool_calls(vec![route_call("graph")])),
            Ok(ModelResponse::text("node result")),
            Ok(ModelResponse::text("root result")),
        ],
    );
    runtime
        .start_with_options(
            "run",
            "session",
            "go",
            Context::new(),
            RunOptions {
                planning: true,
                limits: RunLimits::new(4),
                ..Default::default()
            },
        )
        .unwrap();
    runtime
        .propose_plan(
            "run",
            0,
            plan(vec![TaskAction::Agent {
                prompt: "work".into(),
            }]),
        )
        .unwrap();
    runtime.route("run", ExecutionMode::Graph, "start").unwrap();
    let mut output = String::new();
    let state = runtime
        .resume("run", &mut |text| output.push_str(text))
        .unwrap();
    assert_eq!(state.result().unwrap().text(), "root result");
    assert_eq!(output, "root result");
    assert_eq!(state.budget().model_calls(), 4);
    assert_eq!(state.graph().current().unwrap().nodes["n0"].attempts, 1);
    assert_eq!(
        state.graph().current().unwrap().nodes["n0"].output,
        "node result"
    );
}

#[test]
fn switching_after_effect_before_validation_preserves_the_receipt() {
    let (mut runtime, count) =
        runtime(MemoryRunStore::new(), vec![Ok(ModelResponse::text("done"))]);
    runtime
        .start("run", "session", "go", Context::new(), RunLimits::new(1))
        .unwrap();
    runtime
        .propose_plan("run", 0, plan(vec![tool(ToolCheck::Succeeded)]))
        .unwrap();
    runtime.route("run", ExecutionMode::Graph, "run").unwrap();
    for _ in 0..2 {
        runtime.advance("run", &mut |_| {}).unwrap();
    }
    runtime
        .route("run", ExecutionMode::Loop, "inspect receipt")
        .unwrap();
    runtime.pause("run").unwrap();
    runtime
        .route("run", ExecutionMode::Graph, "resume")
        .unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(state.status(), &RunStatus::Completed);
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn replanning_reuses_completed_work_without_new_model_calls() {
    use crate::tool::{Parameter, Tool, ToolError, ToolOutput};
    struct LargeResult;
    impl Tool for LargeResult {
        fn name(&self) -> &str {
            "large"
        }
        fn description(&self) -> &str {
            "large result fixture"
        }
        fn parameters(&self) -> &[Parameter] {
            &[]
        }
        fn invoke(&self, _: &Arguments) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput::text("x".repeat(16384)))
        }
    }
    let (mut runtime, _) = runtime(MemoryRunStore::new(), vec![]);
    runtime.tools.register(LargeResult).unwrap();
    runtime
        .start("run", "session", "go", Context::new(), RunLimits::new(1))
        .unwrap();
    let plan = plan(vec![TaskAction::Tool {
        name: "large".into(),
        arguments: BTreeMap::new(),
        check: ToolCheck::Succeeded,
    }]);
    runtime.propose_plan("run", 0, plan.clone()).unwrap();
    runtime.route("run", ExecutionMode::Graph, "run").unwrap();
    for _ in 0..3 {
        runtime.advance("run", &mut |_| {}).unwrap();
    }
    for revision in 1..=8 {
        runtime.propose_plan("run", revision, plan.clone()).unwrap();
    }
    let state = runtime.state("run").unwrap();
    assert_eq!(state.budget().tool_calls(), 1);
    assert_eq!(state.budget().model_calls(), 0);
    assert_eq!(
        state.graph().current().unwrap().nodes["n0"].status,
        NodeStatus::Succeeded
    );
    println!(
        "replanning_checkpoint_bytes={}",
        serde_json::to_vec(&state).unwrap().len()
    );
    assert_eq!(
        state
            .node_context(9, "n0")
            .unwrap()
            .last()
            .unwrap()
            .content()
            .len(),
        16384
    );
}

#[test]
fn failed_node_retries_are_bounded_and_keep_previous_attempts() {
    let (mut runtime, _) = runtime(
        MemoryRunStore::new(),
        vec![
            Ok(ModelResponse::text("blocked")),
            Ok(ModelResponse::text("blocked")),
            Ok(ModelResponse::text("blocked")),
        ],
    );
    runtime
        .start("run", "session", "go", Context::new(), RunLimits::new(3))
        .unwrap();
    runtime
        .propose_plan("run", 0, plan(vec![tool(ToolCheck::ExitCodeZero)]))
        .unwrap();
    runtime.route("run", ExecutionMode::Graph, "check").unwrap();
    for attempt in 1..=3 {
        runtime.resume("run", &mut |_| {}).unwrap();
        if attempt < 3 {
            runtime.retry_node("run", "n0").unwrap();
        }
    }
    assert!(runtime.retry_node("run", "n0").is_err());
    let state = runtime.state("run").unwrap();
    let node = &state.graph().current().unwrap().nodes["n0"];
    assert_eq!(node.attempts, 3);
    assert_eq!(node.history.len(), 2);
    assert!(
        node.history
            .iter()
            .all(|attempt| attempt.status == NodeStatus::Failed)
    );
}

#[test]
fn business_failure_in_a_returned_tool_output_is_not_success() {
    use crate::tool::{Parameter, Tool, ToolError, ToolOutput};
    struct Failure;
    impl Tool for Failure {
        fn name(&self) -> &str {
            "failure"
        }
        fn description(&self) -> &str {
            "reported failure"
        }
        fn parameters(&self) -> &[Parameter] {
            &[]
        }
        fn invoke(&self, _: &Arguments) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput::text("check failed").with_success(false))
        }
    }
    let (mut runtime, _) = runtime(
        MemoryRunStore::new(),
        vec![Ok(ModelResponse::text("cannot finish"))],
    );
    runtime.tools.register(Failure).unwrap();
    runtime
        .start("run", "session", "go", Context::new(), RunLimits::new(1))
        .unwrap();
    runtime
        .propose_plan(
            "run",
            0,
            plan(vec![TaskAction::Tool {
                name: "failure".into(),
                arguments: BTreeMap::new(),
                check: ToolCheck::Succeeded,
            }]),
        )
        .unwrap();
    runtime.route("run", ExecutionMode::Graph, "check").unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert!(state.result().is_none());
    assert_eq!(
        state.graph().current().unwrap().nodes["n0"].status,
        NodeStatus::Failed
    );
}
