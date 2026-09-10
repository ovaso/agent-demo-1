use super::support;

use agent_core::agent::runtime::{RunStatus, SqliteRunStore};
use serde_json::json;
use std::fs;
use support::{
    Fixture,
    http::{Provider, Step, call},
};

#[test]
fn dotenv_supplies_model_budget_paths_and_offline_command_settings() {
    let fixture = Fixture::new();
    fs::write(fixture.directory.join(".env"), "\u{feff}# startup settings\nexport OPENAI_API_KEY='test-key'\nOPENAI_MODEL=test-model\nOPENAI_BASE_URL=http://127.0.0.1:9/v1\nRS_AGENT_PROVIDER=openai\nRS_AGENT_DB=runs.sqlite3\nRS_AGENT_SESSION=e2e\nRS_AGENT_MEMORY_DIR='file memories'\nRS_AGENT_TRACE_FILE='file trace.jsonl'\nRS_AGENT_MAX_STEPS=2\nRS_AGENT_AUTO_EXTEND=true\nRS_AGENT_HARD_MAX_STEPS=6\nRS_AGENT_STEP_INCREMENT=2\nRS_AGENT_MAX_STEP_EXTENSIONS=2\nRS_AGENT_MAX_DELEGATIONS=3\n").unwrap();
    let result = fixture.run_with_environment(
        Provider::OpenAi,
        "inspect\n/trace\n/exit\n",
        &[Step::text("main", "dotenv works")],
        &[
            ("OPENAI_API_KEY", None),
            ("OPENAI_MODEL", None),
            ("RS_AGENT_PROVIDER", None),
            ("RS_AGENT_DB", None),
            ("RS_AGENT_SESSION", None),
            ("RS_AGENT_MEMORY_DIR", None),
            ("RS_AGENT_TRACE_FILE", None),
            ("RS_AGENT_MAX_STEPS", None),
            ("RS_AGENT_AUTO_EXTEND", None),
        ],
    );
    let state = fixture.state();
    assert_eq!(state.status(), &RunStatus::Completed);
    assert_eq!(state.limits().max_steps, 2);
    assert_eq!(state.limits().max_delegations, 3);
    let policy = state.limits().step_extension.as_ref().unwrap();
    assert_eq!(
        (
            policy.hard_max_steps,
            policy.step_increment,
            policy.max_extensions
        ),
        (6, 2, 2)
    );
    assert!(fixture.directory.join("file memories").is_dir());
    assert!(fixture.directory.join("file trace.jsonl").is_file());
    assert!(result.stdout.contains("dotenv works"));
    assert_eq!(result.requests.len(), 1);
    assert!(result.stderr.is_empty());

    // Offline commands only need their paths/session, not model credentials or budgets.
    fs::write(fixture.directory.join(".env"), "RS_AGENT_DB=runs.sqlite3\nRS_AGENT_SESSION=e2e\nRS_AGENT_TRACE_FILE='file trace.jsonl'\nRS_AGENT_MAX_STEPS=not-a-number\n").unwrap();
    let status = fixture
        .command()
        .env_remove("RS_AGENT_DB")
        .env_remove("RS_AGENT_SESSION")
        .arg("--status")
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(
        String::from_utf8(status.stdout)
            .unwrap()
            .contains(state.id())
    );
    let trace = fixture
        .command()
        .env_remove("RS_AGENT_TRACE_FILE")
        .arg("--trace-map")
        .output()
        .unwrap();
    assert!(trace.status.success());
    assert!(
        String::from_utf8(trace.stdout)
            .unwrap()
            .contains("agent.run")
    );
}

#[test]
fn process_values_override_file_settings_including_empty_required_values() {
    let fixture = Fixture::new();
    fs::write(fixture.directory.join(".env"), "OPENAI_API_KEY=test-key\nOPENAI_MODEL=test-model\nRS_AGENT_MAX_STEPS=not-a-number\nRS_AGENT_AUTO_EXTEND=invalid\n").unwrap();
    fixture.run(Provider::OpenAi, "/start inspect\n/exit\n", &[]);
    assert_eq!(fixture.state().limits().max_steps, 3);
    assert!(fixture.state().limits().step_extension.is_none());
    let output = fixture
        .command()
        .env("OPENAI_API_KEY", "")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("OPENAI_API_KEY"));
    assert!(!error.contains("test-key"));
}

#[test]
fn explicit_env_file_is_selected_and_missing_explicit_file_is_an_error() {
    let fixture = Fixture::new();
    fs::write(fixture.directory.join(".env"), "invalid default file").unwrap();
    fs::write(
        fixture.directory.join("settings.env"),
        "RS_AGENT_MAX_STEPS=5\nRS_AGENT_AUTO_EXTEND=false\n",
    )
    .unwrap();
    fixture.run_with_environment(
        Provider::OpenAi,
        "/start inspect\n/exit\n",
        &[],
        &[
            ("RS_AGENT_ENV_FILE", Some("settings.env")),
            ("RS_AGENT_MAX_STEPS", None),
            ("RS_AGENT_AUTO_EXTEND", None),
        ],
    );
    assert_eq!(fixture.state().limits().max_steps, 5);
    let missing = fixture
        .command()
        .env("RS_AGENT_ENV_FILE", "missing.env")
        .arg("--status")
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(
        String::from_utf8(missing.stderr)
            .unwrap()
            .contains("missing.env")
    );
}

#[test]
fn malformed_oversized_and_non_file_dotenv_fail_without_exposing_values() {
    let fixture = Fixture::new();
    let path = fixture.directory.join(".env");
    for source in [
        "OPENAI_API_KEY=\"sensitive-fixture-value\n".to_owned(),
        "RS_AGENT_MAX_STEPS=3\n".repeat(4096),
    ] {
        fs::write(&path, source).unwrap();
        let output = fixture.offline(&["--status"]);
        assert!(!output.status.success());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("环境文件"));
        assert!(!error.contains("sensitive-fixture-value"));
        assert!(!fixture.directory.join("runs.sqlite3").exists());
    }
    fs::remove_file(&path).unwrap();
    fs::create_dir(path).unwrap();
    assert!(!fixture.offline(&["--status"]).status.success());
}

#[test]
fn startup_snapshot_keeps_trace_path_and_new_task_settings_until_restart() {
    let fixture = Fixture::new();
    fs::write(
        fixture.directory.join(".env"),
        "RS_AGENT_MAX_STEPS=4\nRS_AGENT_TRACE_FILE=boot.jsonl\n",
    )
    .unwrap();
    let updated = "RS_AGENT_MAX_STEPS=6\nRS_AGENT_TRACE_FILE=next.jsonl\n";
    let result = fixture.run_with_environment(
        Provider::OpenAi,
        "update settings\n/trace\n/start next task\n/exit\n",
        &[
            Step::tools(
                "main",
                vec![call(
                    "write",
                    "write_file",
                    json!({"path":".env", "content":updated, "mode":"overwrite"}),
                )],
            ),
            Step::text("main", "updated"),
        ],
        &[("RS_AGENT_MAX_STEPS", None), ("RS_AGENT_TRACE_FILE", None)],
    );
    assert!(result.stdout.contains("agent.run"));
    assert_eq!(fixture.state().limits().max_steps, 4);
    assert!(fixture.directory.join("boot.jsonl").is_file());
    assert!(!fixture.directory.join("next.jsonl").exists());
    fixture.run_with_environment(
        Provider::OpenAi,
        "/cancel\n/start after restart\n/exit\n",
        &[],
        &[("RS_AGENT_MAX_STEPS", None), ("RS_AGENT_TRACE_FILE", None)],
    );
    let state = SqliteRunStore::open(fixture.directory.join("runs.sqlite3"))
        .unwrap()
        .latest("e2e")
        .unwrap()
        .unwrap();
    assert_eq!(state.limits().max_steps, 6);
    assert!(fixture.directory.join("next.jsonl").is_file());
}
