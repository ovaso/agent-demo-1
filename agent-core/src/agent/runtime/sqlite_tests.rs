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
    let second = db.open();
    let lease = first.acquire().unwrap();
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
