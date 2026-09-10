use agent_core::{
    tool,
    tool::{ToolError, ToolOutput},
};
use std::process::Command;
const ALLOWED_COMMANDS: &[&str] = &["rg", "awk", "sed", "grep"];

#[tool(
    created_at = 1788951445,
    version = "v1.0.0-20260910",
    output = "tool",
    description = "执行 rg、awk、sed 或 grep，通过 PATH 查找程序。不经过 shell，不展开管道、重定向、通配符或环境变量；stdin 关闭。白名单仅限制直接启动的程序，不限制程序内部的脚本能力，不是沙箱。返回 JSON：success、exit_code（信号终止时为 null）、stdout、stderr 及各自 truncated 标记。每个输出流最多保留前 64 KiB，其余丢弃；非 UTF-8 字节替换为替代字符。无匹配等非零退出码也作为正常结果返回。"
)]
fn run_cmd(
    #[arg(description = "程序名称，只允许 rg、awk、sed、grep，不接受路径或整段命令行。")] cmd: &str,
    #[arg(
        description = "参数的 JSON 字符串数组，例如 [\"-n\",\"pattern\",\"src\"]；无参数时传 []。每项原样传递，无需 shell 转义。"
    )]
    args: &str,
    #[arg(description = "工作目录，默认使用进程当前目录；相对路径基于进程当前目录。")] cwd: Option<
        &str,
    >,
) -> Result<ToolOutput, ToolError> {
    if !ALLOWED_COMMANDS.contains(&cmd) {
        return Err(ToolError::new("cmd 只允许 rg、awk、sed、grep"));
    }
    let args: Vec<String> = serde_json::from_str(args)
        .map_err(|error| ToolError::new(format!("args 必须是 JSON 字符串数组：{error}")))?;
    let mut command = Command::new(cmd);
    command.args(args);
    if let Some(cwd) = cwd {
        if cwd.is_empty() {
            return Err(ToolError::new("cwd 不能为空"));
        }
        command.current_dir(cwd);
    }
    super::process_output::execute(command)
}
