use super::support::{
    Fixture,
    http::{Provider, Step, call, has_tool},
};
use serde_json::{Value, json};
use std::fs;

fn tool_result<'a>(request: &'a Value, call_id: &str) -> &'a str {
    request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|message| {
            if message["role"] == "tool" && message["tool_call_id"] == call_id {
                message["content"].as_str()
            } else {
                message["content"].as_array().and_then(|blocks| {
                    blocks.iter().find_map(|block| {
                        (block["type"] == "tool_result" && block["tool_use_id"] == call_id)
                            .then(|| block["content"].as_str())
                            .flatten()
                    })
                })
            }
        })
        .expect("tool result")
}

#[test]
fn debug_tools_are_absent_without_explicit_mode() {
    for provider in [Provider::OpenAi, Provider::Anthropic] {
        let fixture = Fixture::new();
        let result = fixture.run_with_environment(
            provider,
            "inspect\n/exit\n",
            &[Step::text("main", "done")],
            &[("RS_AGENT_MODE", Some("debug"))],
        );
        assert!(!has_tool(&result.requests[0], "debug_show_config"));
        assert!(!has_tool(&result.requests[0], "echo"));
        for name in [
            "session_finish",
            "run_check",
            "runtime_agents",
            "runtime_result",
            "runtime_board_read",
            "runtime_inbox",
        ] {
            assert!(
                has_tool(&result.requests[0], name),
                "normal task tool missing: {name}"
            );
        }
    }
}

#[test]
fn migrated_echo_is_callable_only_in_debug_mode() {
    for provider in [Provider::OpenAi, Provider::Anthropic] {
        let fixture = Fixture::new();
        let result = fixture.run_with_options(
            provider,
            "inspect tool invocation\n/exit\n",
            &[
                Step::tools(
                    "main",
                    vec![call("echo", "echo", json!({"text": "hello\nworld"}))],
                ),
                Step::text("main", "done"),
            ],
            &[],
            &["--mode=debug"],
        );
        assert!(has_tool(&result.requests[0], "echo"));
        assert!(has_tool(&result.requests[0], "debug_show_config"));
        assert_eq!(
            tool_result(&result.requests[1], "echo"),
            "\"hello\\nworld\""
        );
    }
}

#[test]
fn model_tool_schemas_are_stable_across_fresh_processes() {
    for provider in [Provider::OpenAi, Provider::Anthropic] {
        for args in [&[][..], &["--mode=debug"][..]] {
            let run = || {
                let fixture = Fixture::new();
                let result = fixture.run_with_options(
                    provider,
                    "inspect\n/exit\n",
                    &[Step::text("main", "done")],
                    &[],
                    args,
                );
                serde_json::to_string(&result.requests[0]["tools"]).unwrap()
            };
            assert_eq!(run(), run());
        }
    }
}

#[test]
fn debug_configuration_uses_resolved_values_and_excludes_secrets() {
    let fixture = Fixture::new();
    fs::write(fixture.directory.join(".env"), "OPENAI_MODEL=ignored-model\nOPENAI_STREAM_USAGE=0\nRS_AGENT_MAX_STEPS=5\nRS_AGENT_AUTO_EXTEND=true\nRS_AGENT_SESSION=ignored-session\nUNRELATED_SECRET=hidden-value\n").unwrap();
    let result = fixture.run_with_options(
        Provider::OpenAi,
        "inspect\n/exit\n",
        &[
            Step::tools("main", vec![call("config", "debug_show_config", json!({}))]),
            Step::text("main", "done"),
        ],
        &[
            ("RS_AGENT_PROVIDER", Some("openai-compatible")),
            ("RS_AGENT_MAX_STEPS", None),
            ("RS_AGENT_AUTO_EXTEND", None),
        ],
        &["--mode=debug"],
    );
    assert!(has_tool(&result.requests[0], "debug_show_config"));
    let output = tool_result(&result.requests[1], "config");
    for line in [
        "RS_AGENT_PROVIDER=openai-compatible",
        "OPENAI_MODEL=test-model",
        "OPENAI_STREAM_USAGE=false",
        "RS_AGENT_MAX_STEPS=5",
        "RS_AGENT_AUTO_EXTEND=true",
        "RS_AGENT_HARD_MAX_STEPS=32",
        "RS_AGENT_STEP_INCREMENT=8",
        "RS_AGENT_MAX_STEP_EXTENSIONS=3",
        "RS_AGENT_MAX_DELEGATIONS=8",
        "RS_AGENT_SESSION=e2e",
    ] {
        assert!(output.lines().any(|actual| actual == line), "{output}");
    }
    for (key, file) in [
        ("RS_AGENT_DB", "runs.sqlite3"),
        ("RS_AGENT_MEMORY_DIR", "memories"),
        ("RS_AGENT_TRACE_FILE", "trace.jsonl"),
    ] {
        assert!(output.contains(&format!("{key}={}", fixture.directory.join(file).display())));
    }
    assert!(output.contains("OPENAI_BASE_URL=http://127.0.0.1:"));
    for excluded in [
        "test-key",
        "API_KEY",
        "UNRELATED_SECRET",
        "hidden-value",
        "ignored-model",
        "ANTHROPIC_",
    ] {
        assert!(!output.contains(excluded));
    }
}

#[test]
fn anthropic_debug_configuration_reports_defaults_and_disabled_policy() {
    let fixture = Fixture::new();
    let result = fixture.run_with_options(
        Provider::Anthropic,
        "/plan inspect\n/exit\n",
        &[
            Step::tools("main", vec![call("config", "debug_show_config", json!({}))]),
            Step::text("main", "done"),
        ],
        &[
            ("ANTHROPIC_MAX_TOKENS", Some("invalid")),
            ("RS_AGENT_HARD_MAX_STEPS", Some("ignored")),
        ],
        &["--mode=debug"],
    );
    assert!(has_tool(&result.requests[0], "debug_show_config"));
    let output = tool_result(&result.requests[1], "config");
    for line in [
        "RS_AGENT_PROVIDER=anthropic",
        "ANTHROPIC_MODEL=test-model",
        "ANTHROPIC_MAX_TOKENS=1024",
        "RS_AGENT_AUTO_EXTEND=false",
        "RS_AGENT_HARD_MAX_STEPS=<disabled>",
        "RS_AGENT_STEP_INCREMENT=<disabled>",
        "RS_AGENT_MAX_STEP_EXTENSIONS=<disabled>",
    ] {
        assert!(output.lines().any(|actual| actual == line), "{output}");
    }
    assert!(output.contains("ANTHROPIC_BASE_URL=http://127.0.0.1:"));
    assert!(!output.contains("API_KEY"));
    assert!(!output.contains("test-key"));
    assert!(!output.contains("OPENAI_"));
}

#[test]
fn debug_mode_rejects_extra_arguments_and_unknown_modes() {
    let fixture = Fixture::new();
    for args in [
        &["--mode=debug", "extra"][..],
        &["--mode=other"],
        &["--mode=debug", "--status"],
    ] {
        let output = fixture.offline(args);
        assert!(!output.status.success());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("支持的参数")
        );
    }
}

#[test]
fn debug_configuration_remains_the_startup_snapshot_after_dotenv_changes() {
    let fixture = Fixture::new();
    fs::write(fixture.directory.join(".env"), "RS_AGENT_MAX_STEPS=4\n").unwrap();
    let result = fixture.run_with_options(
        Provider::OpenAi,
        "update then inspect\n/exit\n",
        &[
            Step::tools(
                "main",
                vec![call(
                    "write",
                    "write_file",
                    json!({
                        "path": ".env", "content": "RS_AGENT_MAX_STEPS=12\n", "mode": "overwrite"
                    }),
                )],
            ),
            Step::tools("main", vec![call("config", "debug_show_config", json!({}))]),
            Step::text("main", "done"),
        ],
        &[("RS_AGENT_MAX_STEPS", None)],
        &["--mode=debug"],
    );
    assert_eq!(
        fs::read_to_string(fixture.directory.join(".env")).unwrap(),
        "RS_AGENT_MAX_STEPS=12\n"
    );
    assert!(
        tool_result(&result.requests[2], "config")
            .lines()
            .any(|line| line == "RS_AGENT_MAX_STEPS=4")
    );
}
