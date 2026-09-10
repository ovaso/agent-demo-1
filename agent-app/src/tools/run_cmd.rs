#[cfg(test)]
use super::process_output::{OUTPUT_LIMIT, capture};
use agent_core::tool::{Arguments, Parameter, Tool, ToolError, ToolOutput};
#[cfg(test)]
use std::io::{self, Read};
use std::process::Command;
const ALLOWED_COMMANDS: &[&str] = &["rg", "awk", "sed", "grep"];

pub(crate) struct RunCmd {
    parameters: [Parameter; 3],
}

impl RunCmd {
    pub(crate) fn new() -> Self {
        Self {
            parameters: [
                Parameter::required(
                    "cmd",
                    "程序名称，只允许 rg、awk、sed、grep，不接受路径或整段命令行。",
                ),
                Parameter::required(
                    "args",
                    r#"参数的 JSON 字符串数组，例如 ["-n","pattern","src"]；无参数时传 []。每项原样传递，无需 shell 转义。"#,
                ),
                Parameter::optional(
                    "cwd",
                    "工作目录，默认使用进程当前目录；相对路径基于进程当前目录。",
                ),
            ],
        }
    }
}

impl Tool for RunCmd {
    fn name(&self) -> &str {
        "run_cmd"
    }

    fn description(&self) -> &str {
        "执行 rg、awk、sed 或 grep，通过 PATH 查找程序。不经过 shell，不展开管道、重定向、通配符或环境变量；stdin 关闭。白名单仅限制直接启动的程序，不限制程序内部的脚本能力，不是沙箱。返回 JSON：success、exit_code（信号终止时为 null）、stdout、stderr 及各自 truncated 标记。每个输出流最多保留前 64 KiB，其余丢弃；非 UTF-8 字节替换为替代字符。无匹配等非零退出码也作为正常结果返回。"
    }

    fn parameters(&self) -> &[Parameter] {
        &self.parameters
    }

    fn invoke(&self, arguments: &Arguments) -> Result<ToolOutput, ToolError> {
        let cmd = arguments.get("cmd").unwrap_or_default();
        if !ALLOWED_COMMANDS.contains(&cmd) {
            return Err(ToolError::new("cmd 只允许 rg、awk、sed、grep"));
        }
        let args: Vec<String> = serde_json::from_str(
            arguments
                .get("args")
                .ok_or_else(|| ToolError::new("缺少 args 参数"))?,
        )
        .map_err(|error| ToolError::new(format!("args 必须是 JSON 字符串数组：{error}")))?;

        let mut command = Command::new(cmd);
        command.args(args);
        if let Some(cwd) = arguments.get("cwd") {
            if cwd.is_empty() {
                return Err(ToolError::new("cwd 不能为空"));
            }
            command.current_dir(cwd);
        }

        super::process_output::execute(command)
    }
}

#[cfg(test)]
mod tests;
