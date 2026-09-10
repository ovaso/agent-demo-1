use agent_core::{
    tool,
    tool::{ToolError, ToolOutput},
};
use std::process::Command;

#[tool(
    created_at = 1789006513,
    version = "v1.0.0-20260910",
    output = "tool",
    read_only,
    description = "只读使用 rg 查找固定文本。禁用 rg 配置，不接受额外程序参数；忽略超过 1 MiB 的文件，每文件至多 20 处匹配。返回 success、exit_code、stdout、stderr 和截断标记。无匹配退出码为 1；截断时缩小范围。"
)]
fn search_files(
    #[arg(description = "按原样匹配的文本，不是正则")] pattern: &str,
    #[arg(description = "搜索文件或目录路径")] path: &str,
) -> Result<ToolOutput, ToolError> {
    let mut command = Command::new("rg");
    command.args([
        "--no-config",
        "--fixed-strings",
        "--line-number",
        "--color=never",
        "--max-count=20",
        "--max-filesize=1M",
        "--",
        pattern,
        path,
    ]);
    super::process_output::execute(command)
}
