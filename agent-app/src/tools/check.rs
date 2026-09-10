use agent_core::tool::{Arguments, Parameter, Tool, ToolError, ToolOutput};
use std::process::Command;

pub(crate) struct RunCheck {
    parameters: [Parameter; 2],
}
impl RunCheck {
    pub(crate) fn new() -> Self {
        Self {
            parameters: [
                Parameter::required("check", "fmt、clippy 或 test"),
                Parameter::required("cwd", "Rust 项目目录"),
            ],
        }
    }
}
impl Tool for RunCheck {
    fn name(&self) -> &str {
        "run_check"
    }
    fn description(&self) -> &str {
        "执行固定 Rust 验证命令：cargo fmt --all -- --check、cargo clippy --workspace --all-targets -- -D warnings 或 cargo test --workspace。返回带 success、exit_code 和有界输出的 JSON。构建及测试可能有副作用，只能用于执行阶段。"
    }
    fn parameters(&self) -> &[Parameter] {
        &self.parameters
    }
    fn invoke(&self, arguments: &Arguments) -> Result<ToolOutput, ToolError> {
        let args: &[&str] = match arguments.get("check") {
            Some("fmt") => &["fmt", "--all", "--", "--check"],
            Some("clippy") => &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
            Some("test") => &["test", "--workspace"],
            _ => return Err(ToolError::new("check 只允许 fmt、clippy、test")),
        };
        let cwd = arguments
            .get("cwd")
            .filter(|path| !path.is_empty())
            .ok_or_else(|| ToolError::new("cwd 不能为空"))?;
        let mut command = Command::new("cargo");
        command
            .args(args)
            .current_dir(cwd)
            .env("CARGO_TERM_COLOR", "never");
        super::process_output::execute(command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let output = RunCheck::new()
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
