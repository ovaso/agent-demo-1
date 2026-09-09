use agent_core::tool::Registry;
use serde_json::{Value, json};

use super::*;

fn invoke(cmd: &str, args: Value) -> Result<Value, String> {
    let mut registry = Registry::new();
    registry.register(RunCmd::new()).unwrap();
    let output = registry
        .invoke(
            "run_cmd",
            &Arguments::new()
                .with("cmd", cmd)
                .with("args", args.to_string())
                .with("cwd", env!("CARGO_MANIFEST_DIR")),
        )
        .map_err(|error| error.to_string())?;
    assert!(!output.finishes_session());
    Ok(serde_json::from_str(output.content()).unwrap())
}

#[test]
fn runs_each_allowed_program_in_working_directory() {
    for (cmd, args, expected) in [
        (
            "rg",
            json!(["--no-config", "--color=never", "^name =", "Cargo.toml"]),
            "name = \"agent-app\"\n",
        ),
        (
            "grep",
            json!(["^name =", "Cargo.toml"]),
            "name = \"agent-app\"\n",
        ),
        ("sed", json!(["-n", "1p", "Cargo.toml"]), "[package]\n"),
        (
            "awk",
            json!(["BEGIN { print \"hello world\" }"]),
            "hello world\n",
        ),
    ] {
        let output = invoke(cmd, args).unwrap();
        assert_eq!(output["success"], true, "{cmd}: {output}");
        assert_eq!(output["exit_code"], 0);
        assert_eq!(output["stdout"], expected);
        assert_eq!(output["stderr"], "");
        assert_eq!(output["stdout_truncated"], false);
        assert_eq!(output["stderr_truncated"], false);
    }
}

#[test]
fn rejects_non_allowlisted_names_paths_and_shell_commands() {
    for cmd in [
        "",
        "sh",
        "ls",
        "RG",
        "/usr/bin/grep",
        "./rg",
        " rg",
        "rg ",
        "rg; ls",
        "grep | awk",
        "rg\nls",
    ] {
        assert!(invoke(cmd, json!([])).unwrap_err().contains("只允许"));
    }
}

#[test]
fn rejects_malformed_arguments_and_invalid_directories() {
    let tool = RunCmd::new();
    for args in ["", "null", "{}", "[1]", "[null]", "\"pattern\""] {
        let error = tool
            .invoke(&Arguments::new().with("cmd", "grep").with("args", args))
            .unwrap_err();
        assert!(error.to_string().contains("JSON 字符串数组"));
    }
    assert!(tool.invoke(&Arguments::new().with("cmd", "grep")).is_err());
    for cwd in [
        "".to_owned(),
        format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR")),
    ] {
        assert!(
            tool.invoke(
                &Arguments::new()
                    .with("cmd", "grep")
                    .with("args", "[]")
                    .with("cwd", cwd)
            )
            .is_err()
        );
    }
}

#[test]
fn reports_nonzero_exit_codes_and_stderr_as_results() {
    let no_match = invoke(
        "grep",
        json!(["a-pattern-that-is-not-in-this-manifest", "Cargo.toml"]),
    )
    .unwrap();
    assert_eq!(no_match["success"], false);
    assert_eq!(no_match["exit_code"], 1);
    assert_eq!(no_match["stdout"], "");

    let invalid = invoke("grep", json!(["--rs-agent-invalid-option"])).unwrap();
    assert_eq!(invalid["success"], false);
    assert_ne!(invalid["exit_code"], 0);
    assert!(!invalid["stderr"].as_str().unwrap().is_empty());
}

#[test]
fn passes_shell_metacharacters_and_spaces_literally() {
    let literal = "hello world; $(printf expanded) | grep * > file & `echo x` $HOME";
    let output = invoke("awk", json!(["BEGIN { print ARGV[1]; exit }", literal])).unwrap();
    assert_eq!(output["stdout"], format!("{literal}\n"));
}

#[test]
fn closes_stdin_and_defaults_to_current_directory() {
    let output = RunCmd::new()
        .invoke(
            &Arguments::new()
                .with("cmd", "sed")
                .with("args", r#"["-n","p"]"#),
        )
        .unwrap();
    let output: Value = serde_json::from_str(output.content()).unwrap();
    assert_eq!(output["success"], true);
    assert_eq!(output["stdout"], "");
}

#[test]
fn bounds_output_and_drains_both_pipes_without_deadlock() {
    let output = invoke(
        "awk",
        json!(["BEGIN { for (i=0; i<20000; i++) { print \"abcdefgh\"; print \"ijklmnop\" > \"/dev/stderr\" } }"]),
    ).unwrap();
    assert_eq!(output["exit_code"], 0);
    assert_eq!(output["stdout"].as_str().unwrap().len(), OUTPUT_LIMIT);
    assert_eq!(output["stderr"].as_str().unwrap().len(), OUTPUT_LIMIT);
    assert_eq!(output["stdout_truncated"], true);
    assert_eq!(output["stderr_truncated"], true);
}

#[test]
fn marks_truncation_only_when_bytes_are_dropped() {
    for length in [0, OUTPUT_LIMIT - 1, OUTPUT_LIMIT, OUTPUT_LIMIT + 1] {
        let captured = capture(io::repeat(b'x').take(length as u64)).unwrap();
        assert_eq!(captured.bytes.len(), length.min(OUTPUT_LIMIT));
        assert_eq!(captured.truncated, length > OUTPUT_LIMIT);
    }
}

#[test]
fn replaces_invalid_utf8_in_output() {
    let output = invoke("awk", json!([r#"BEGIN { printf "\377" }"#])).unwrap();
    assert_eq!(output["success"], true);
    assert!(!output["stdout"].as_str().unwrap().is_empty());
}
