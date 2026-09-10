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

#[test]
fn cli_token_budget_is_durable_and_output_limit_is_sent_to_provider() {
    let fixture = Fixture::new();
    std::fs::write(fixture.directory.join("a.txt"), "evidence").unwrap();
    let result = fixture.run_with_environment(
        Provider::OpenAi,
        "inspect\n/exit\n",
        &[Step::tools(
            "main",
            vec![call("r", "read_file", json!({"path":"a.txt"}))],
        )],
        &[("RS_AGENT_MAX_TOTAL_TOKENS", Some("25000"))],
    );
    assert_eq!(result.requests[0]["max_tokens"], 8192);
    let paused = fixture.state();
    assert_eq!(
        paused.status(),
        &RunStatus::Paused(PauseReason::TokenBudget)
    );
    let used = paused.budget().token_usage().total_tokens();
    assert!(used > 0);
    let result = fixture.run(
        Provider::OpenAi,
        "/tokens 100000\n/output-budget 4096\n/resume\n/exit\n",
        &[Step::text("main", "done")],
    );
    assert_eq!(result.requests[0]["max_tokens"], 4096);
    let done = fixture.state();
    assert_eq!(done.status(), &RunStatus::Completed);
    assert!(done.budget().token_usage().total_tokens() > used);
    assert_eq!(done.budget().model_calls(), 2);
}
