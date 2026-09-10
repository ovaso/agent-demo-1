use agent_core::tool::{ToolError, ToolOutput};
use std::{
    io::{self, Read},
    process::{Command, Stdio},
    thread,
};
pub(super) const OUTPUT_LIMIT: usize = 64 * 1024;

pub(super) fn execute(mut command: Command) -> Result<ToolOutput, ToolError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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
        execute().map_err(|error| ToolError::new(format!("执行程序失败：{error}")))?;

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
    )
    .with_success(status.success()))
}

pub(super) struct CapturedOutput {
    pub(super) bytes: Vec<u8>,
    pub(super) truncated: bool,
}

pub(super) fn capture(mut reader: impl Read) -> io::Result<CapturedOutput> {
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
