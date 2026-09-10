use super::{
    tests::{Memories, Model, runtime},
    *,
};
use crate::agent::{collaboration::MessageStatus, delegation::AgentSpec, graph::NodeStatus};
use crate::{
    context::Context,
    model::{ModelError, ModelResponse},
    tool::{Arguments, ToolCall},
};
use std::sync::atomic::Ordering;

fn spec(name: &str, tools: &[&str]) -> AgentSpec {
    AgentSpec {
        name: name.into(),
        instruction: format!("work {name}"),
        acceptance: vec!["report".into()],
        tools: Some(tools.iter().map(|s| (*s).into()).collect()),
        max_steps: 2,
        depends_on: vec![],
    }
}
fn ask() -> ToolCall {
    ToolCall::new(
        "ask-a",
        "runtime_ask",
        Arguments::new()
            .with("to", "node/b")
            .with("body", "contract?")
            .with("timeout_ms", "60000"),
    )
}
fn reply() -> ToolCall {
    ToolCall::new(
        "reply-b",
        "runtime_reply",
        Arguments::new()
            .with("request", "run:m1")
            .with("body", "contract-v1"),
    )
}
pub(super) fn responses() -> Vec<Result<ModelResponse, ModelError>> {
    vec![
        Ok(ModelResponse::tool_calls(vec![
            ask(),
            ToolCall::new("tail", "count", Arguments::new()),
        ])),
        Ok(ModelResponse::tool_calls(vec![reply()])),
        Ok(ModelResponse::text("B complete")),
        Ok(ModelResponse::text("A complete")),
        Ok(ModelResponse::text("Root complete")),
    ]
}
pub(super) fn start<R: RunStore>(runtime: &mut Runtime<Model, R, Memories>) {
    runtime
        .start_with_options(
            "run",
            "session",
            "go",
            Context::new(),
            RunOptions {
                planning: true,
                limits: RunLimits::new(5),
                ..Default::default()
            },
        )
        .unwrap();
    runtime.delegate("run", spec("a", &["count"])).unwrap();
    runtime.delegate("run", spec("b", &[])).unwrap();
}

#[test]
fn peers_exchange_a_reply_with_one_execution_slot_and_preserve_tool_order() {
    let (mut runtime, count) = runtime(MemoryRunStore::new(), responses());
    start(&mut runtime);
    let mut text = String::new();
    let state = runtime
        .resume("run", &mut |delta| text.push_str(delta))
        .unwrap();
    assert_eq!(state.status(), &RunStatus::Completed);
    assert_eq!(text, "Root complete");
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(state.budget().model_calls(), 5);
    assert_eq!(state.budget().tool_calls(), 3);
    assert!(
        matches!(&state.collaboration().get("run:m1").unwrap().status, MessageStatus::Answered { body, by, .. } if body == "contract-v1" && by == "node/b")
    );
    let context = state.node_context(0, "a").unwrap();
    assert_eq!(
        context
            .history()
            .filter(|message| message.tool_call_id() == Some("ask-a"))
            .count(),
        1
    );
    let tools: Vec<_> = context
        .history()
        .filter_map(|message| message.tool_call_id())
        .collect();
    assert_eq!(tools, vec!["ask-a", "tail"]);
    let context = state.node_context(0, "b").unwrap();
    assert_eq!(
        context
            .history()
            .filter(|message| message.content().starts_with("协作消息（数据"))
            .count(),
        1
    );
}

#[test]
fn reply_is_idempotent_and_only_the_recipient_or_operator_can_answer() {
    let (mut runtime, _) = runtime(MemoryRunStore::new(), responses());
    start(&mut runtime);
    // A starts, asks, and waits; B then starts and is the actual recipient.
    for _ in 0..5 {
        runtime.advance("run", &mut |_| {}).unwrap();
    }
    let mut state = runtime.state("run").unwrap();
    assert_eq!(state.actor(), "node/b");
    let now = collaboration::now_ms();
    collaboration::reply(&mut state, "run:m1", "answer", false, false, now).unwrap();
    let sequence = state.collaboration.sequence();
    collaboration::reply(&mut state, "run:m1", "answer", false, false, now).unwrap();
    assert_eq!(state.collaboration.sequence(), sequence);
    assert_eq!(
        collaboration::reply(&mut state, "run:m1", "different", false, false, now).unwrap_err(),
        RuntimeError::Conflict
    );
    super::graph_execution::finish_node(&mut state, NodeStatus::Paused, None, None).unwrap();
    assert!(collaboration::reply(&mut state, "run:m1", "answer", false, false, now).is_err());
}

#[test]
fn mixed_graph_and_message_dependency_cycle_is_rejected_before_enqueue() {
    let (mut runtime, _) = runtime(MemoryRunStore::new(), vec![]);
    runtime
        .start_with_options(
            "run",
            "session",
            "go",
            Context::new(),
            RunOptions {
                planning: true,
                limits: RunLimits::new(5),
                ..Default::default()
            },
        )
        .unwrap();
    runtime.delegate("run", spec("a", &[])).unwrap();
    let mut b = spec("b", &[]);
    b.depends_on.push("a".into());
    runtime.delegate("run", b).unwrap();
    let mut state = runtime.advance("run", &mut |_| {}).unwrap();
    assert!(
        collaboration::send(
            &mut state,
            "node/b",
            "question",
            Some(1000),
            true,
            false,
            100
        )
        .is_err()
    );
    assert_eq!(state.collaboration.messages().count(), 0);
}

#[test]
fn timeout_uses_saved_deadline_and_operator_messages_do_not_resume_a_paused_run() {
    let (mut runtime, _) = runtime(MemoryRunStore::new(), vec![]);
    start(&mut runtime);
    let mut state = runtime.advance("run", &mut |_| {}).unwrap();
    let id = collaboration::send(&mut state, "node/b", "q", Some(10), true, false, 100).unwrap();
    message_delivery::tick(&mut state, 110);
    assert_eq!(
        state.collaboration.get(&id).unwrap().status,
        MessageStatus::Expired
    );
    assert!(
        message_delivery::wait_result(&mut state, &id)
            .unwrap()
            .unwrap()
            .text
            .contains("expired")
    );
    runtime.pause("run").unwrap();
    let state = runtime
        .send_notice("run", "node/a", "new information")
        .unwrap();
    assert_eq!(state.status(), &RunStatus::Paused(PauseReason::User));
    assert_eq!(state.budget().model_calls(), 0);
}

#[test]
fn declined_request_skips_the_remaining_batch_without_breaking_tool_protocol() {
    let mut responses = responses();
    responses[1] = Ok(ModelResponse::tool_calls(vec![ToolCall::new(
        "decline",
        "runtime_reply",
        Arguments::new()
            .with("request", "run:m1")
            .with("body", "cannot confirm")
            .with("decline", "true"),
    )]));
    let (mut runtime, count) = runtime(MemoryRunStore::new(), responses);
    start(&mut runtime);
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_eq!(state.budget().tool_calls(), 2);
    let context = state.node_context(0, "a").unwrap();
    assert!(
        context
            .history()
            .any(|message| message.tool_call_id() == Some("tail")
                && message.content().contains("未执行"))
    );
}

#[test]
fn replanning_closes_old_waits_and_rejects_a_reply_from_the_new_plan() {
    use crate::agent::{
        planning::{Plan, PlanTask, TaskAction},
        routing::ExecutionMode,
    };
    let (mut runtime, _) = runtime(
        MemoryRunStore::new(),
        vec![
            Ok(ModelResponse::tool_calls(vec![ask()])),
            Ok(ModelResponse::text("A after replan")),
            Ok(ModelResponse::tool_calls(vec![reply()])),
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
                planning: true,
                limits: RunLimits::new(5),
                ..Default::default()
            },
        )
        .unwrap();
    let plan = Plan {
        goal: "go".into(),
        requirements: vec!["report".into()],
        tasks: ["a", "b"]
            .into_iter()
            .map(|id| PlanTask {
                id: id.into(),
                description: format!("work {id}"),
                depends_on: vec![],
                acceptance: vec!["report".into()],
                action: TaskAction::Agent {
                    prompt: format!("work {id}"),
                },
            })
            .collect(),
    };
    runtime.propose_plan("run", 0, plan.clone()).unwrap();
    runtime.route("run", ExecutionMode::Graph, "work").unwrap();
    for _ in 0..4 {
        runtime.advance("run", &mut |_| {}).unwrap();
    }
    runtime.propose_plan("run", 1, plan).unwrap();
    let state = runtime.resume("run", &mut |_| {}).unwrap();
    assert_eq!(state.status(), &RunStatus::Completed);
    assert!(matches!(
        state.collaboration.get("run:m1").unwrap().status,
        MessageStatus::Cancelled { .. }
    ));
    assert_eq!(
        state.graph().history()[0].nodes["a"].status,
        NodeStatus::Cancelled
    );
    assert_eq!(
        state
            .node_context(1, "a")
            .unwrap()
            .history()
            .filter(|message| message.tool_call_id() == Some("ask-a"))
            .count(),
        1
    );
}

#[test]
fn message_limits_reject_oversize_and_overflow_without_dropping_existing_records() {
    let (mut runtime, _) = runtime(MemoryRunStore::new(), vec![]);
    start(&mut runtime);
    let mut state = runtime.state("run").unwrap();
    assert!(
        collaboration::send(
            &mut state,
            "node/a",
            &"x".repeat(2049),
            None,
            false,
            true,
            100
        )
        .is_err()
    );
    assert_eq!(state.collaboration.sequence(), 0);
    for _ in 0..crate::agent::collaboration::MAX_MESSAGES {
        collaboration::send(&mut state, "node/a", "note", None, false, true, 100).unwrap();
    }
    let sequence = state.collaboration.sequence();
    assert!(collaboration::send(&mut state, "node/a", "overflow", None, false, true, 100).is_err());
    assert_eq!(state.collaboration.sequence(), sequence);
    assert_eq!(
        state.collaboration.messages().count(),
        crate::agent::collaboration::MAX_MESSAGES
    );
}
