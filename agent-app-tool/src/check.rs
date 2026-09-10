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
