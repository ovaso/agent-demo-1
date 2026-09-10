use super::run_cmd::RunCmd;
use agent_core::tool::{Arguments, Parameter, Tool, ToolError, ToolOutput};

pub(crate) struct SearchFiles {
    parameters: [Parameter; 2],
    runner: RunCmd,
}
impl SearchFiles {
    pub(crate) fn new() -> Self {
        Self {
            parameters: [
                Parameter::required("pattern", "按原样匹配的文本，不是正则"),
                Parameter::required("path", "搜索文件或目录路径"),
            ],
            runner: RunCmd::new(),
        }
    }
}
impl Tool for SearchFiles {
    fn name(&self) -> &str {
        "search_files"
    }
    fn description(&self) -> &str {
        "只读使用 rg 查找固定文本。禁用 rg 配置，不接受额外程序参数；忽略超过 1 MiB 的文件，每文件至多 20 处匹配。返回 success、exit_code、stdout、stderr 和截断标记。无匹配退出码为 1；截断时缩小范围。"
    }
    fn parameters(&self) -> &[Parameter] {
        &self.parameters
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn invoke(&self, arguments: &Arguments) -> Result<ToolOutput, ToolError> {
        let pattern = arguments
            .get("pattern")
            .ok_or_else(|| ToolError::new("缺少 pattern"))?;
        let path = arguments
            .get("path")
            .ok_or_else(|| ToolError::new("缺少 path"))?;
        let args = serde_json::to_string(&[
            "--no-config",
            "--fixed-strings",
            "--line-number",
            "--color=never",
            "--max-count=20",
            "--max-filesize=1M",
            "--",
            pattern,
            path,
        ])
        .map_err(|error| ToolError::new(error.to_string()))?;
        self.runner
            .invoke(&Arguments::new().with("cmd", "rg").with("args", args))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn searches_literals_without_interpreting_regex_or_options() {
        let path = std::env::temp_dir().join(format!("agent-search-tool-{}", std::process::id()));
        std::fs::write(&path, "a.*b\naxxb\n--pre=anything\n").unwrap();
        let search = SearchFiles::new();
        let output = search
            .invoke(
                &Arguments::new()
                    .with("pattern", "a.*b")
                    .with("path", path.to_string_lossy()),
            )
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(output.content()).unwrap();
        assert!(value["stdout"].as_str().unwrap().contains("a.*b"));
        assert!(!value["stdout"].as_str().unwrap().contains("axxb"));
        let output = search
            .invoke(
                &Arguments::new()
                    .with("pattern", "--pre=anything")
                    .with("path", path.to_string_lossy()),
            )
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(output.content()).unwrap();
        assert_eq!(value["success"], true);
        std::fs::remove_file(path).unwrap();
    }
}
