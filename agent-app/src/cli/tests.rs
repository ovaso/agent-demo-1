use super::{commands::parse, session::Session};
use agent_core::{
    agent::runtime::{PauseReason, RunLimits, RunStatus, Runtime, SqliteRunStore},
    memory::MarkdownMemoryStore,
    model::{ModelError, ModelProvider, ModelRequest, ModelResponse},
    tool::{Arguments, Registry, ToolCall},
    trace::FileTraceSink,
};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Model(VecDeque<ModelResponse>);
impl ModelProvider for Model {
    fn complete(&mut self, _: ModelRequest<'_>) -> Result<ModelResponse, ModelError> {
        self.0
            .pop_front()
            .ok_or_else(|| ModelError::new("unexpected model request"))
    }
}

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "agent-cli-resume-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn session(&self, responses: Vec<ModelResponse>) -> Session<Model> {
        let runtime = Runtime::new(
            Model(responses.into()),
            SqliteRunStore::open(self.0.join("runs.db")).unwrap(),
            MarkdownMemoryStore::open(self.0.join("memories")).unwrap(),
            Registry::new(),
        )
        .with_trace_sink(FileTraceSink::open(self.0.join("trace.jsonl")).unwrap());
        Session {
            runtime,
            session_id: "session".into(),
            limits: RunLimits::new(1),
        }
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn request_tool() -> ModelResponse {
    ModelResponse::tool_calls(vec![ToolCall::new("a", "missing", Arguments::new())])
}

#[test]
fn cli_reopens_and_resumes_with_explicit_budget_extension() {
    let directory = Directory::new();
    let mut first = directory.session(vec![request_tool()]);
    first.handle(parse("/start work").unwrap()).unwrap();
    first.handle(parse("/step").unwrap()).unwrap();
    first.handle(parse("/pause").unwrap()).unwrap();
    drop(first);
    let mut second = directory.session(vec![ModelResponse::text("done")]);
    second.handle(parse("/resume").unwrap()).unwrap();
    let state = second.runtime.store().latest("session").unwrap().unwrap();
    assert_eq!(state.status(), &RunStatus::Paused(PauseReason::Budget));
    assert_eq!(state.budget().model_calls(), 1);
    second.handle(parse("/budget 2").unwrap()).unwrap();
    second.handle(parse("/resume").unwrap()).unwrap();
    let state = second.runtime.store().latest("session").unwrap().unwrap();
    assert_eq!(state.status(), &RunStatus::Completed);
    assert_eq!(state.result().unwrap().text(), "done");
    assert_eq!(state.budget().model_calls(), 2);
}

#[test]
fn cli_requires_settling_active_task_and_cancel_preserves_tool_protocol() {
    let directory = Directory::new();
    let mut session = directory.session(vec![request_tool(), ModelResponse::text("next done")]);
    session.handle(parse("/start first").unwrap()).unwrap();
    assert!(session.handle(parse("second").unwrap()).is_err());
    session.handle(parse("/step").unwrap()).unwrap();
    session.handle(parse("/cancel").unwrap()).unwrap();
    session.handle(parse("second").unwrap()).unwrap();
    assert_eq!(
        session
            .runtime
            .store()
            .latest("session")
            .unwrap()
            .unwrap()
            .result()
            .unwrap()
            .text(),
        "next done"
    );
}

#[test]
fn cli_plan_survives_restart_and_requires_execute_without_resetting_budget() {
    let directory = Directory::new();
    let plan = r#"{"goal":"inspect","requirements":["checked"],"tasks":[{"id":"a","description":"inspect","acceptance":["checked"],"action":{"kind":"agent","prompt":"inspect"}}]}"#;
    let response = ModelResponse::tool_calls(vec![
        ToolCall::new(
            "p",
            "runtime_plan",
            Arguments::new()
                .with("expected_revision", "0")
                .with("plan", plan),
        ),
        ToolCall::new("ready", "runtime_plan_ready", Arguments::new()),
    ]);
    let mut first = directory.session(vec![response]);
    first.handle(parse("/plan inspect").unwrap()).unwrap();
    drop(first);
    let mut second = directory.session(vec![ModelResponse::text("executed")]);
    second.handle(parse("/resume").unwrap()).unwrap();
    let state = second.runtime.store().latest("session").unwrap().unwrap();
    assert_eq!(state.status(), &RunStatus::Paused(PauseReason::PlanReady));
    assert_eq!(state.budget().model_calls(), 1);
    second.handle(parse("/budget 2").unwrap()).unwrap();
    second.handle(parse("/execute").unwrap()).unwrap();
    let state = second.runtime.store().latest("session").unwrap().unwrap();
    assert_eq!(state.result().unwrap().text(), "executed");
    assert_eq!(state.budget().model_calls(), 2);
    assert_eq!(state.plans().revision(), 1);
}

#[test]
fn cli_can_select_graph_before_executing_a_saved_plan() {
    use agent_core::agent::{graph::NodeStatus, routing::ExecutionMode};
    let directory = Directory::new();
    let plan = r#"{"goal":"inspect","requirements":["checked"],"tasks":[{"id":"a","description":"inspect","acceptance":["checked"],"action":{"kind":"agent","prompt":"inspect"}}]}"#;
    let response = ModelResponse::tool_calls(vec![
        ToolCall::new(
            "p",
            "runtime_plan",
            Arguments::new()
                .with("expected_revision", "0")
                .with("plan", plan),
        ),
        ToolCall::new("ready", "runtime_plan_ready", Arguments::new()),
    ]);
    let mut session = directory.session(vec![
        response,
        ModelResponse::text("node complete"),
        ModelResponse::text("root complete"),
    ]);
    session.limits = RunLimits::new(3);
    session.handle(parse("/plan inspect").unwrap()).unwrap();
    session.handle(parse("/mode graph").unwrap()).unwrap();
    let state = session.runtime.store().latest("session").unwrap().unwrap();
    assert_eq!(state.status(), &RunStatus::Paused(PauseReason::PlanReady));
    assert_eq!(state.routing().pending_mode(), Some(ExecutionMode::Graph));
    session.handle(parse("/execute").unwrap()).unwrap();
    let state = session.runtime.store().latest("session").unwrap().unwrap();
    assert_eq!(state.result().unwrap().text(), "root complete");
    assert_eq!(
        state.graph().current().unwrap().nodes["a"].status,
        NodeStatus::Succeeded
    );
    assert_eq!(state.budget().model_calls(), 3);
}

#[test]
fn model_can_delegate_from_loop_and_cli_reports_local_usage() {
    let directory = Directory::new();
    let spec = serde_json::json!({"name":"inspector","instruction":"inspect independently","acceptance":["report findings"],"tools":[],"max_steps":1}).to_string();
    let response = ModelResponse::tool_calls(vec![ToolCall::new(
        "d",
        "runtime_delegate",
        Arguments::new().with("spec", spec),
    )]);
    let mut session = directory.session(vec![
        response,
        ModelResponse::text("worker report"),
        ModelResponse::text("root report"),
    ]);
    session.limits = RunLimits::new(3);
    session.handle(parse("inspect this").unwrap()).unwrap();
    session.handle(parse("/agents").unwrap()).unwrap();
    let state = session.runtime.store().latest("session").unwrap().unwrap();
    assert_eq!(state.result().unwrap().text(), "root report");
    assert_eq!(state.budget().model_calls(), 3);
    assert_eq!(
        state.graph().current().unwrap().nodes["inspector"]
            .policy
            .as_ref()
            .unwrap()
            .model_calls,
        1
    );
}
