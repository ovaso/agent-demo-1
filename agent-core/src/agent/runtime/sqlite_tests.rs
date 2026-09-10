use super::{
    tests::{batch, runtime},
    *,
};
use crate::{context::Context, model::ModelResponse};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agent-runtime-sqlite-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn open(&self) -> SqliteRunStore {
        SqliteRunStore::open(self.0.join("runs.sqlite")).unwrap()
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn reopens_between_tool_calls_with_budget_and_conversation_intact() {
    let db = Database::new();
    let (mut first, count) = runtime(db.open(), vec![Ok(batch(&["a", "b"]))]);
    first
        .start("run", "session", "go", Context::new(), RunLimits::new(2))
        .unwrap();
    first.advance("run", &mut |_| {}).unwrap();
    first.advance("run", &mut |_| {}).unwrap();
    first.pause("run").unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    drop(first);
    let (mut second, second_count) = runtime(db.open(), vec![Ok(ModelResponse::text("done"))]);
    let state = second.resume("run", &mut |_| {}).unwrap();
    assert_eq!(second_count.load(Ordering::SeqCst), 1);
    assert_eq!(state.budget().tool_calls(), 2);
    assert_eq!(state.budget().model_calls(), 2);
    assert_eq!(
        second.store().session_context("session").unwrap().as_ref(),
        Some(state.context())
    );
}

#[test]
fn file_lock_excludes_another_store_and_releases_on_drop() {
    let db = Database::new();
    let first = db.open();
    let lease = first.acquire().unwrap();
    let second = db.open();
    assert!(matches!(second.acquire(), Err(RuntimeError::Busy)));
    drop(lease);
    assert!(second.acquire().is_ok());
}

#[test]
fn stale_save_does_not_change_session_projection_and_active_session_is_unique() {
    let db = Database::new();
    let (mut runtime, _) = runtime(db.open(), vec![]);
    let mut state = runtime
        .start("run", "session", "go", Context::new(), RunLimits::new(1))
        .unwrap();
    assert_eq!(
        runtime
            .start(
                "other",
                "session",
                "other input",
                Context::new(),
                RunLimits::new(1)
            )
            .unwrap_err(),
        RuntimeError::Conflict
    );
    state.context.push_user("must not save");
    state.revision = 2;
    assert_eq!(
        runtime.store.save(&state, 1).unwrap_err(),
        RuntimeError::Conflict
    );
    assert_eq!(
        runtime
            .store
            .session_context("session")
            .unwrap()
            .unwrap()
            .last()
            .unwrap()
            .content(),
        "go"
    );
    assert!(runtime.store.reset_session("session").is_err());
    runtime.cancel("run").unwrap();
    runtime.store.reset_session("session").unwrap();
    assert!(
        runtime
            .store
            .session_context("session")
            .unwrap()
            .unwrap()
            .is_empty()
    );
    assert!(runtime.state("run").unwrap().context().last().is_some());
}

#[test]
fn reopening_inflight_tool_requires_resolution_before_dispatch() {
    let db = Database::new();
    let (mut first, _) = runtime(db.open(), vec![Ok(batch(&["a"]))]);
    first
        .start("run", "session", "go", Context::new(), RunLimits::new(2))
        .unwrap();
    let mut state = first.advance("run", &mut |_| {}).unwrap();
    state.phase = LoopPhase::ToolInFlight {
        call_id: "a".into(),
    };
    state.budget.tool_calls = 1;
    first.commit(&mut state).unwrap();
    drop(first);
    let (mut second, count) = runtime(db.open(), vec![]);
    assert!(matches!(
        second.resume("run", &mut |_| {}),
        Err(RuntimeError::NeedsResolution(_))
    ));
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn oversized_encoding_leaves_checkpoint_and_projection_unchanged() {
    let db = Database::new();
    let (mut runtime, _) = runtime(db.open(), vec![]);
    let original = runtime
        .start("run", "session", "go", Context::new(), RunLimits::new(2))
        .unwrap();
    let mut oversized = original.clone();
    oversized.revision = 1;
    oversized.limits.max_checkpoint_bytes = serde_json::to_vec(&original).unwrap().len() + 128;
    oversized.context.push_user("large\n\"中文".repeat(512));
    assert!(runtime.store.save(&oversized, 0).is_err());
    assert_eq!(runtime.state("run").unwrap(), original);
    assert_eq!(
        runtime.store.session_context("session").unwrap().as_ref(),
        Some(original.context())
    );

    let paused = runtime.pause("run").unwrap();
    assert_eq!(paused.revision(), 1);
    drop(runtime);
    assert_eq!(db.open().load("run").unwrap().unwrap(), paused);
}

#[test]
fn plans_and_blackboard_survive_reopen_and_conflicts_do_not_change_the_checkpoint() {
    use crate::agent::{
        blackboard::{BoardUpdate, EntryKind},
        planning::Plan,
    };
    let db = Database::new();
    let (mut first, _) = runtime(db.open(), vec![]);
    first
        .start("run", "session", "go", Context::new(), RunLimits::new(2))
        .unwrap();
    let plan: Plan = serde_json::from_str(r#"{"goal":"go","requirements":["checked"],"tasks":[{"id":"a","description":"inspect","acceptance":["checked"],"action":{"kind":"agent","prompt":"inspect"}}]}"#).unwrap();
    first.propose_plan("run", 0, plan).unwrap();
    let update = BoardUpdate {
        key: "finding".into(),
        expected_revision: 0,
        kind: EntryKind::Hypothesis,
        content: "candidate".into(),
        sources: vec![],
    };
    let saved = first.write_board("run", "main", update.clone()).unwrap();
    assert_eq!(
        first.write_board("run", "other", update).unwrap_err(),
        RuntimeError::Conflict
    );
    drop(first);
    let store = db.open();
    let restored = store.load("run").unwrap().unwrap();
    assert_eq!(restored, saved);
    assert_eq!(restored.plans().revision(), 1);
    assert_eq!(
        restored.blackboard().latest("finding").unwrap().author,
        "main"
    );
    assert_eq!(restored.budget().model_calls(), 0);
}

#[test]
fn active_graph_context_is_restored_without_polluting_session_projection() {
    use crate::agent::{planning::Plan, routing::ExecutionMode};
    let db = Database::new();
    let (mut first, _) = runtime(db.open(), vec![]);
    first
        .start("run", "session", "go", Context::new(), RunLimits::new(2))
        .unwrap();
    let plan: Plan = serde_json::from_str(r#"{"goal":"go","requirements":["checked"],"tasks":[{"id":"a","description":"node work","acceptance":["checked"],"action":{"kind":"agent","prompt":"inspect"}}]}"#).unwrap();
    first.propose_plan("run", 0, plan).unwrap();
    first.route("run", ExecutionMode::Graph, "run").unwrap();
    first.advance("run", &mut |_| {}).unwrap();
    assert_eq!(
        first
            .store()
            .session_context("session")
            .unwrap()
            .unwrap()
            .last()
            .unwrap()
            .content(),
        "go"
    );
    first.pause("run").unwrap();
    drop(first);
    let (mut second, _) = runtime(
        db.open(),
        vec![
            Ok(ModelResponse::text("node done")),
            Ok(ModelResponse::text("root done")),
        ],
    );
    let state = second.resume("run", &mut |_| {}).unwrap();
    assert_eq!(state.result().unwrap().text(), "root done");
    assert_eq!(state.graph().current().unwrap().nodes["a"].attempts, 1);
    assert!(
        !state
            .context()
            .history()
            .any(|message| message.content() == "node done")
    );
    assert_eq!(
        state
            .node_context(1, "a")
            .unwrap()
            .last()
            .unwrap()
            .content(),
        "node done"
    );
}

#[test]
fn delegated_agent_keeps_its_scope_and_spent_budget_after_reopen() {
    use crate::agent::delegation::AgentSpec;
    let db = Database::new();
    let (mut first, count) = runtime(db.open(), vec![Ok(batch(&["once"]))]);
    first
        .start("run", "session", "go", Context::new(), RunLimits::new(3))
        .unwrap();
    first
        .delegate(
            "run",
            AgentSpec {
                name: "worker".into(),
                instruction: "work".into(),
                acceptance: vec!["checked".into()],
                tools: Some(vec!["count".into()]),
                max_steps: 2,
                depends_on: vec![],
            },
        )
        .unwrap();
    for _ in 0..3 {
        first.advance("run", &mut |_| {}).unwrap();
    }
    first.pause("run").unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    drop(first);
    let (mut second, count) = runtime(
        db.open(),
        vec![
            Ok(ModelResponse::text("worker done")),
            Ok(ModelResponse::text("root done")),
        ],
    );
    let state = second.resume("run", &mut |_| {}).unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 0);
    let policy = state.graph().current().unwrap().nodes["worker"]
        .policy
        .as_ref()
        .unwrap();
    assert_eq!(policy.model_calls, 2);
    assert_eq!(policy.tools, vec!["count"]);
    assert_eq!(state.budget().model_calls(), 3);
}

#[test]
fn resumes_after_reply_commit_without_resending_or_repeating_the_trailing_tool() {
    use super::collaboration_tests;
    let db = Database::new();
    let mut responses = collaboration_tests::responses();
    let remaining = responses.split_off(2);
    let (mut first, count) = runtime(db.open(), responses);
    collaboration_tests::start(&mut first);
    for _ in 0..7 {
        first.advance("run", &mut |_| {}).unwrap();
    }
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(matches!(
        first
            .state("run")
            .unwrap()
            .collaboration()
            .get("run:m1")
            .unwrap()
            .status,
        crate::agent::collaboration::MessageStatus::Answered { .. }
    ));
    drop(first);
    let (mut second, count) = runtime(db.open(), remaining);
    let state = second.resume("run", &mut |_| {}).unwrap();
    assert_eq!(state.status(), &RunStatus::Completed);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(state.budget().model_calls(), 5);
    assert_eq!(state.collaboration.messages().count(), 1);
    assert_eq!(
        state
            .node_context(0, "a")
            .unwrap()
            .history()
            .filter(|message| message.tool_call_id() == Some("ask-a"))
            .count(),
        1
    );
}
