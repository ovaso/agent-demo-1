use agent_core::{
    tool,
    tool::{ToolError, ToolOutput},
};
use std::process::Command;

#[tool(
    created_at = 1789008818,
    version = "v1.0.0-20260910",
    output = "tool",
    description = "执行固定 Rust 验证命令：cargo fmt --all -- --check、cargo clippy --workspace --all-targets -- -D warnings 或 cargo test --workspace。返回带 success、exit_code 和有界输出的 JSON。构建及测试可能有副作用，只能用于执行阶段。"
)]
fn run_check(
    #[arg(description = "fmt、clippy 或 test")] check: &str,
    #[arg(description = "Rust 项目目录")] cwd: &str,
) -> Result<ToolOutput, ToolError> {
    let args: &[&str] = match check {
        "fmt" => &["fmt", "--all", "--", "--check"],
        "clippy" => &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
        "test" => &["test", "--workspace"],
        _ => return Err(ToolError::new("check 只允许 fmt、clippy、test")),
    };
    if cwd.is_empty() {
        return Err(ToolError::new("cwd 不能为空"));
    }
    let mut command = Command::new("cargo");
    command
        .args(args)
        .current_dir(cwd)
        .env("CARGO_TERM_COLOR", "never");
    super::process_output::execute(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::tool::{Arguments, Tool};
    #[test]
    fn formatting_check_returns_real_exit_status_without_modifying_source() {
        let path = std::env::temp_dir().join(format!("agent-check-tool-{}", std::process::id()));
        std::fs::create_dir_all(path.join("src")).unwrap();
        std::fs::write(
            path.join("Cargo.toml"),
            "[package]\nname = \"check-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        let source = "pub fn value()->u32{1}";
        std::fs::write(path.join("src/lib.rs"), source).unwrap();
        let output = run_check_tool()
            .invoke(
                &Arguments::new()
                    .with("check", "fmt")
                    .with("cwd", path.to_string_lossy()),
            )
            .unwrap();
        let result: serde_json::Value = serde_json::from_str(output.content()).unwrap();
        assert!(!output.succeeded());
        assert_eq!(result["success"], false);
        assert_ne!(result["exit_code"], 0);
        assert_eq!(
            std::fs::read_to_string(path.join("src/lib.rs")).unwrap(),
            source
        );
        std::fs::remove_dir_all(path).unwrap();
    }
}
