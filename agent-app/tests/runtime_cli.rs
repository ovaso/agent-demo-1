mod support;

use agent_core::agent::{
    collaboration::MessageStatus,
    graph::{NodeStatus, ValidationKind},
    routing::ExecutionMode,
    runtime::{PauseReason, RunStatus, WorkIntent},
};
use serde_json::json;
use std::fs;
use support::{
    Fixture,
    http::{Provider, Step, call, has_tool},
};

#[test]
fn offline_status_and_invalid_arguments_do_not_require_model_credentials() {
    let fixture = Fixture::new();
    let output = fixture.offline(&["--status"]);
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("没有匹配")
    );
    for args in [vec!["--unknown"], vec!["--status", "id", "extra"]] {
        assert!(!fixture.offline(&args).status.success());
    }
    fixture.run(Provider::OpenAi, "/start offline checkpoint\n/exit\n", &[]);
    let state = fixture.state();
    let output = fixture.offline(&["--status", state.id()]);
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains(state.id())
    );
    assert_eq!(state.budget().model_calls(), 0);
}

#[test]
fn openai_cli_plans_collaborates_and_resumes_across_processes() {
    planned_collaboration(Provider::OpenAi);
}

#[test]
fn anthropic_cli_plans_collaborates_and_resumes_across_processes() {
    planned_collaboration(Provider::Anthropic);
}

fn planned_collaboration(provider: Provider) {
    let fixture = Fixture::new();
    let plan = json!({
        "goal":"collaborate",
        "requirements":["write the agreed contract once and read it back"],
        "tasks":[
            {"id":"a","description":"ask B and write the contract","acceptance":["save contract"],"action":{"kind":"agent","prompt":"ask node/b, then write contract.txt"}},
            {"id":"b","description":"provide contract","acceptance":["reply to A"],"action":{"kind":"agent","prompt":"answer node/a"}},
            {"id":"verify","description":"read saved contract","depends_on":["a","b"],"acceptance":["file readable"],"action":{"kind":"tool","name":"read_file","arguments":{"path":"contract.txt"},"check":"succeeded"}}
        ]
    });
    let result = fixture.run(provider, "/plan collaborate\n/resume\n/exit\n", &[Step::tools("main", vec![
        call("plan", "runtime_plan", json!({"expected_revision":"0","plan":plan.to_string()})),
        call("board", "runtime_board_write", json!({"update":json!({"key":"contract","expected_revision":0,"kind":"decision","content":"peer agreement required"}).to_string()})),
        call("ready", "runtime_plan_ready", json!({})),
    ])]);
    assert!(has_tool(&result.requests[0], "read_file"));
    assert!(!has_tool(&result.requests[0], "write_file"));
    let ready = fixture.state();
    assert_eq!(ready.status(), &RunStatus::Paused(PauseReason::PlanReady));
    assert_eq!(ready.intent(), WorkIntent::PlanOnly);
    assert_eq!(ready.budget().model_calls(), 1);
    assert_eq!(
        ready.blackboard().latest("contract").unwrap().author,
        "main"
    );
    assert!(!fixture.directory.join("contract.txt").exists());

    let request_id = format!("{}:m1", ready.id());
    let result = fixture.run(
        provider,
        "/mode graph\n/execute\n/exit\n",
        &[
            Step::tools(
                "node/a",
                vec![
                    call(
                        "ask",
                        "runtime_ask",
                        json!({"to":"node/b","body":"contract?"}),
                    ),
                    call(
                        "write",
                        "write_file",
                        json!({"path":"contract.txt","content":"契约 v1\n","mode":"append"}),
                    ),
                ],
            ),
            Step::tools(
                "node/b",
                vec![call(
                    "reply",
                    "runtime_reply",
                    json!({"request":request_id,"body":"契约 v1"}),
                )],
            ),
        ],
    );
    assert!(!has_tool(&result.requests[0], "runtime_delegate"));
    assert!(result.requests[1].to_string().contains(&request_id));
    let paused = fixture.state();
    assert_eq!(paused.id(), ready.id());
    assert_eq!(paused.status(), &RunStatus::Paused(PauseReason::Budget));
    assert_eq!(paused.routing().mode(), ExecutionMode::Graph);
    assert_eq!(paused.budget().model_calls(), 3);
    assert!(matches!(
        paused.collaboration().get(&request_id).unwrap().status,
        MessageStatus::Answered { .. }
    ));
    assert!(!fixture.directory.join("contract.txt").exists());
    let status = fixture.offline(&["--status"]);
    assert!(status.status.success());
    assert!(String::from_utf8(status.stdout).unwrap().contains("3/3"));

    let result = fixture.run(
        provider,
        "/budget 6\n/resume\n/exit\n",
        &[
            Step::text("node/b", "B private result"),
            Step::text("node/a", "A private result"),
            Step::text("main", "Contract delivered"),
        ],
    );
    assert!(result.stdout.contains("Contract delivered"));
    assert!(!result.stdout.contains("A private result"));
    assert!(!result.stdout.contains("B private result"));
    assert_eq!(
        fs::read_to_string(fixture.directory.join("contract.txt")).unwrap(),
        "契约 v1\n"
    );
    let done = fixture.state();
    assert_eq!(done.id(), ready.id());
    assert_eq!(done.status(), &RunStatus::Completed);
    assert_eq!(done.budget().model_calls(), 6);
    assert_eq!(done.budget().tool_calls(), 7);
    let graph = done.graph().current().unwrap();
    assert!(
        graph
            .nodes
            .values()
            .all(|node| node.status == NodeStatus::Succeeded)
    );
    assert_eq!(
        graph.nodes["verify"].validation,
        Some(ValidationKind::ToolSucceeded)
    );
    let calls: Vec<_> = done
        .node_context(1, "a")
        .unwrap()
        .history()
        .filter_map(|message| message.tool_call_id())
        .collect();
    assert_eq!(calls, ["ask", "write"]);
    let trace = fixture.offline(&["--trace-map"]);
    assert!(trace.status.success());
    assert!(String::from_utf8(trace.stdout).unwrap().contains("node/a"));
}

#[test]
fn interrupted_openai_stream_never_executes_partial_tool_calls() {
    interrupted_stream(Provider::OpenAi);
}

#[test]
fn interrupted_anthropic_stream_never_executes_partial_tool_calls() {
    interrupted_stream(Provider::Anthropic);
}

fn interrupted_stream(provider: Provider) {
    let fixture = Fixture::new();
    let write = call(
        "write",
        "write_file",
        json!({"path":"once.txt","content":"once\n","mode":"append"}),
    );
    let failed = fixture.run(
        provider,
        "save once\n/exit\n",
        &[Step::tools("main", vec![write.clone()]).interrupted()],
    );
    assert!(failed.stderr.contains("流在结束标记之前中断"));
    let paused = fixture.state();
    assert!(matches!(
        paused.status(),
        RunStatus::Paused(PauseReason::Model(_))
    ));
    assert_eq!(paused.budget().model_calls(), 1);
    assert_eq!(paused.budget().tool_calls(), 0);
    assert_eq!(paused.pending_tool_calls().count(), 0);
    assert!(!fixture.directory.join("once.txt").exists());
    fixture.run(
        provider,
        "/resume\n/exit\n",
        &[
            Step::tools("main", vec![write]),
            Step::text("main", "Saved once"),
        ],
    );
    let done = fixture.state();
    assert_eq!(done.id(), paused.id());
    assert_eq!(done.status(), &RunStatus::Completed);
    assert_eq!(done.budget().model_calls(), 3);
    assert_eq!(done.budget().tool_calls(), 1);
    assert_eq!(
        fs::read_to_string(fixture.directory.join("once.txt")).unwrap(),
        "once\n"
    );
}

const AUTOMATIC_BUDGET: &[(&str, Option<&str>)] = &[
    ("RS_AGENT_AUTO_EXTEND", Some("1")),
    ("RS_AGENT_HARD_MAX_STEPS", Some("5")),
    ("RS_AGENT_STEP_INCREMENT", Some("2")),
    ("RS_AGENT_MAX_STEP_EXTENSIONS", Some("1")),
];

fn reads(id: &str, path: &str) -> Step {
    Step::tools("main", vec![call(id, "read_file", json!({"path":path}))])
}

#[test]
fn new_cli_tasks_default_to_eight_steps_with_bounded_renewal() {
    let fixture = Fixture::new();
    let mut steps = Vec::new();
    for index in 0..8 {
        let name = format!("note-{index}.txt");
        fs::write(fixture.directory.join(&name), &name).unwrap();
        steps.push(reads(&format!("read-{index}"), &name));
    }
    steps.push(Step::text("main", "Finished default task"));
    let result = fixture.run_with_environment(
        Provider::OpenAi,
        "inspect\n/exit\n",
        &steps,
        &[("RS_AGENT_MAX_STEPS", None), ("RS_AGENT_AUTO_EXTEND", None)],
    );
    let state = fixture.state();
    assert_eq!(
        support::http::overview(&result.requests[0])["model_calls_remaining"],
        8
    );
    assert_eq!(state.limits().max_steps, 16);
    assert_eq!(
        state.limits().step_extension.as_ref().unwrap(),
        &Default::default()
    );
    assert_eq!(state.budget().model_calls(), 9);
    assert_eq!(state.status(), &RunStatus::Completed);
    assert_eq!(state.budget().step_extensions().len(), 1);
    assert_eq!(state.budget().step_extensions()[0].previous_limit, 8);
}

#[test]
fn invalid_budget_environment_is_rejected_before_starting_the_cli() {
    for (name, value, expected) in [
        ("RS_AGENT_AUTO_EXTEND", "maybe", "RS_AGENT_AUTO_EXTEND"),
        ("RS_AGENT_HARD_MAX_STEPS", "2", "硬上限"),
        ("RS_AGENT_STEP_INCREMENT", "0", "单次增量"),
        ("RS_AGENT_MAX_STEP_EXTENSIONS", "17", "次数"),
    ] {
        let fixture = Fixture::new();
        let output = fixture
            .command()
            .env("OPENAI_API_KEY", "test-key")
            .env("OPENAI_MODEL", "test-model")
            .env("OPENAI_BASE_URL", "http://127.0.0.1:9/v1")
            .env("RS_AGENT_AUTO_EXTEND", "1")
            .env(name, value)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{name}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains(expected), "{stderr}");
        assert!(!fixture.directory.join("runs.sqlite3").exists());
    }
}

fn automatically_continues(provider: Provider) {
    let fixture = Fixture::new();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(fixture.directory.join(name), name).unwrap();
    }
    let result = fixture.run_with_environment(
        provider,
        "inspect\n/exit\n",
        &[
            reads("a", "a.txt"),
            reads("b", "b.txt"),
            reads("c", "c.txt"),
            Step::text("main", "Inspection complete"),
        ],
        AUTOMATIC_BUDGET,
    );
    let done = fixture.state();
    assert_eq!(done.status(), &RunStatus::Completed);
    assert_eq!(done.budget().model_calls(), 4);
    assert_eq!(done.budget().tool_calls(), 3);
    assert_eq!(done.limits().max_steps, 5);
    assert_eq!(done.budget().step_extensions().len(), 1);
    let view = support::http::overview(&result.requests[3]);
    assert_eq!(view["model_calls_remaining"], 2);
    assert_eq!(view["step_budget"]["hard_model_calls_remaining"], 2);
    assert_eq!(view["step_budget"]["extensions_remaining"], 0);
    assert!(result.stdout.contains("3 → 5"));
    let trace = fs::read_to_string(fixture.directory.join("trace.jsonl")).unwrap();
    assert_eq!(trace.matches("runtime.budget.extended").count(), 1);
}

#[test]
fn openai_cli_extends_from_fresh_results_and_completes_without_manual_resume() {
    automatically_continues(Provider::OpenAi);
}

#[test]
fn anthropic_cli_extends_from_fresh_results_and_completes_without_manual_resume() {
    automatically_continues(Provider::Anthropic);
}

#[test]
fn duplicate_results_pause_with_a_reason_and_resume_cannot_mint_budget() {
    let fixture = Fixture::new();
    fs::write(fixture.directory.join("same.txt"), "same content").unwrap();
    let result = fixture.run_with_environment(
        Provider::OpenAi,
        "inspect\n/resume\n/exit\n",
        &[
            reads("a", "same.txt"),
            reads("b", "same.txt"),
            reads("c", "same.txt"),
        ],
        AUTOMATIC_BUDGET,
    );
    let state = fixture.state();
    assert_eq!(state.status(), &RunStatus::Paused(PauseReason::Budget));
    assert_eq!(state.budget().model_calls(), 3);
    assert!(state.budget().step_extensions().is_empty());
    assert!(result.stdout.contains("最近没有可用于续期的新进展"));
    assert!(result.stdout.contains("进度已保存在检查点中"));
}

#[test]
fn saved_fixed_budget_can_opt_into_renewal_after_restart() {
    let fixture = Fixture::new();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(fixture.directory.join(name), name).unwrap();
    }
    fixture.run(
        Provider::OpenAi,
        "inspect\n/exit\n",
        &[
            reads("a", "a.txt"),
            reads("b", "b.txt"),
            reads("c", "c.txt"),
        ],
    );
    let paused = fixture.state();
    assert!(paused.limits().step_extension.is_none());
    assert_eq!(paused.budget().model_calls(), 3);
    let result = fixture.run(
        Provider::OpenAi,
        "/budget auto 5\n/resume\n/exit\n",
        &[Step::text("main", "Recovered inspection")],
    );
    assert!(result.stdout.contains("已满足续期条件，等待 /resume 恢复"));
    let done = fixture.state();
    assert_eq!(done.id(), paused.id());
    assert_eq!(done.status(), &RunStatus::Completed);
    assert_eq!(done.budget().model_calls(), 4);
    assert_eq!(done.budget().tool_calls(), 3);
    assert_eq!(done.budget().step_extensions().len(), 1);
}
