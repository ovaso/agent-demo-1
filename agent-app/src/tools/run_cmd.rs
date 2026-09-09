use std::{
    io::{self, Read},
    process::{Command, Stdio},
    thread,
};

use agent_core::tool::{Arguments, Parameter, Tool, ToolError, ToolOutput};

const ALLOWED_COMMANDS: &[&str] = &["rg", "awk", "sed", "grep"];
const OUTPUT_LIMIT: usize = 64 * 1024;

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
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = arguments.get("cwd") {
            if cwd.is_empty() {
                return Err(ToolError::new("cwd 不能为空"));
            }
            command.current_dir(cwd);
        }

        let mut execute = || -> io::Result<_> {
            let mut child = command.spawn()?;
            let stdout = child.stdout.take().expect("stdout configured as piped");
            let stderr = child.stderr.take().expect("stderr configured as piped");
            // 同时消费两个管道，避免任一管道写满导致子进程死锁。
            thread::scope(|scope| {
                let stderr_reader = scope.spawn(|| capture(stderr));
                let stdout = capture(stdout);
                let stderr = stderr_reader
                    .join()
                    .map_err(|_| io::Error::other("stderr 读取线程异常"));
                // 即使读取失败也回收子进程。
                if stdout.is_err() || !matches!(&stderr, Ok(Ok(_))) {
                    let _ = child.kill();
                }
                let status = child.wait()?;
                Ok((status, stdout?, stderr??))
            })
        };
        let (status, stdout, stderr) =
            execute().map_err(|error| ToolError::new(format!("执行 {cmd} 失败：{error}")))?;

        Ok(ToolOutput::text(
            serde_json::json!({
                "success": status.success(),
                "exit_code": status.code(),
                "stdout": String::from_utf8_lossy(&stdout.bytes),
                "stderr": String::from_utf8_lossy(&stderr.bytes),
                "stdout_truncated": stdout.truncated,
                "stderr_truncated": stderr.truncated,
            })
            .to_string(),
        ))
    }
}

struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

fn capture(mut reader: impl Read) -> io::Result<CapturedOutput> {
    let mut output = CapturedOutput {
        bytes: Vec::new(),
        truncated: false,
    };
    let mut buffer = [0; 8192];
    loop {
        let count = match reader.read(&mut buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            return Ok(output);
        }
        let retained = count.min(OUTPUT_LIMIT - output.bytes.len());
        output.bytes.extend_from_slice(&buffer[..retained]);
        output.truncated |= retained < count;
        // 超限后继续排空管道，保留程序正常退出与退出码语义。
    }
}

#[cfg(test)]
mod tests;
