use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use agent_core::tool::{Arguments, Parameter, Tool, ToolError, ToolOutput};

pub(crate) struct WriteFile {
    parameters: [Parameter; 3],
}

impl WriteFile {
    pub(crate) fn new() -> Self {
        Self {
            parameters: [
                Parameter::required(
                    "path",
                    "目标文件路径；相对路径基于进程当前工作目录，也支持绝对路径，不展开 ~ 或环境变量。",
                ),
                Parameter::required(
                    "content",
                    "要保存的完整 UTF-8 文本，原样写入，不自动添加换行。",
                ),
                Parameter::optional(
                    "mode",
                    "create（默认）：仅新建，已存在则报错；overwrite：覆盖；append：追加。后两者也可新建文件。",
                ),
            ],
        }
    }
}

impl Tool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "当用户要求把对话内容、总结或其他文本保存到文件时调用。自动创建缺失的父目录。默认仅新建；用户要求覆盖或追加时设置对应 mode。成功返回路径及本次写入字节数，失败返回错误。"
    }

    fn parameters(&self) -> &[Parameter] {
        &self.parameters
    }

    fn invoke(&self, arguments: &Arguments) -> Result<ToolOutput, ToolError> {
        let path = arguments
            .get("path")
            .filter(|path| !path.is_empty())
            .ok_or_else(|| ToolError::new("path 不能为空"))?;
        // Arguments 中的字符串已由 provider 解码，不再次解析 JSON，以保留文本原貌。
        let content = arguments
            .get("content")
            .ok_or_else(|| ToolError::new("缺少 content 参数"))?;
        let mode = arguments.get("mode").unwrap_or("create");
        let mut options = OpenOptions::new();
        options.write(true);
        match mode {
            "create" => options.create_new(true),
            "overwrite" => options.create(true).truncate(true),
            "append" => options.create(true).append(true),
            _ => return Err(ToolError::new("mode 必须是 create、overwrite 或 append")),
        };

        let write = || -> std::io::Result<()> {
            if let Some(parent) = Path::new(path)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
            {
                fs::create_dir_all(parent)?;
            }
            let mut file = options.open(path)?;
            file.write_all(content.as_bytes())
        };
        write().map_err(|error| ToolError::new(format!("写入文件 {path:?} 失败：{error}")))?;

        Ok(ToolOutput::text(
            serde_json::json!({ "path": path, "bytes_written": content.len(), "mode": mode })
                .to_string(),
        ))
    }
}

#[cfg(test)]
mod tests;
