use super::support::{
    Fixture,
    http::{Provider, Step, call},
};
use agent_core::agent::runtime::{PauseReason, RunStatus};
use serde_json::json;

#[test]
fn length_stop_preserves_partial_text_without_completing() {
    for (provider, reason) in [
        (Provider::OpenAi, "length"),
        (Provider::Anthropic, "max_tokens"),
    ] {
        let fixture = Fixture::new();
        fixture.run(
            provider,
            "inspect\n/exit\n",
            &[Step::text("main", "partial answer").stopped(reason)],
        );
        let state = fixture.state();
        assert!(matches!(
            state.status(),
            RunStatus::Paused(PauseReason::Model(_))
        ));
        assert!(state.result().is_none());
        assert!(
            state
                .context()
                .history()
                .any(|m| m.content() == "partial answer")
        );
        fixture.run(
            provider,
            "/resume\n/exit\n",
            &[Step::text("main", "complete answer")],
        );
        assert_eq!(fixture.state().status(), &RunStatus::Completed);
    }
}

#[test]
fn length_stop_never_executes_tools_even_when_arguments_are_valid_json() {
    for (provider, reason) in [
        (Provider::OpenAi, "length"),
        (Provider::Anthropic, "max_tokens"),
    ] {
        let fixture = Fixture::new();
        let write = call(
            "w",
            "write_file",
            json!({"path":"once.txt","content":"once","mode":"append"}),
        );
        fixture.run(
            provider,
            "save\n/exit\n",
            &[Step::tools("main", vec![write.clone()]).stopped(reason)],
        );
        assert!(!fixture.directory.join("once.txt").exists());
        assert_eq!(fixture.state().budget().tool_calls(), 0);
        fixture.run(
            provider,
            "/resume\n/exit\n",
            &[
                Step::tools("main", vec![write]),
                Step::text("main", "saved"),
            ],
        );
        assert_eq!(
            std::fs::read_to_string(fixture.directory.join("once.txt")).unwrap(),
            "once"
        );
    }
}
