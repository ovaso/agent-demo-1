use agent_core::{
    tool,
    tool::{ToolError, ToolOutput},
};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
};
const READ_LIMIT: usize = 64 * 1024;

#[tool(
    created_at = 1789006513,
    version = "v1.0.0-20260910",
    output = "tool",
    read_only,
    description = "只读普通文件的有界片段，返回 JSON：text、offset、next_offset、truncated、lossy。偏移按字节计，非 UTF-8 或拆开的字符以替代字符显示。"
)]
fn read_file(
    #[arg(description = "普通文件路径")] path: &str,
    #[arg(description = "读取起始字节，默认 0")] offset: Option<&str>,
    #[arg(description = "最多读取字节，默认 16384，上限 65536")] limit: Option<&str>,
) -> Result<ToolOutput, ToolError> {
    if !fs::metadata(path)
        .map_err(|error| ToolError::new(error.to_string()))?
        .is_file()
    {
        return Err(ToolError::new("只允许读取普通文件"));
    }
    let offset = number(offset, 0)?;
    let limit = number(limit, 16384)?;
    if limit == 0 || limit > READ_LIMIT as u64 {
        return Err(ToolError::new("limit 必须为 1..=65536"));
    }
    let mut file = File::open(path).map_err(|error| ToolError::new(error.to_string()))?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| ToolError::new(error.to_string()))?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| ToolError::new(error.to_string()))?;
    let truncated = bytes.len() > limit as usize;
    bytes.truncate(limit as usize);
    let lossy = std::str::from_utf8(&bytes).is_err();
    Ok(ToolOutput::text(serde_json::json!({"text":String::from_utf8_lossy(&bytes),"offset":offset,"next_offset":offset.saturating_add(bytes.len() as u64),"truncated":truncated,"lossy":lossy}).to_string()))
}

#[tool(
    created_at = 1789006513,
    version = "v1.0.0-20260910",
    output = "tool",
    read_only,
    description = "只读列出目录的至多 200 个条目，返回 names 和 truncated；不递归、不执行程序。截断时应指定更小的目录。"
)]
fn list_directory(
    #[arg(description = "目录路径")] path: &str
) -> Result<ToolOutput, ToolError> {
    let mut names = fs::read_dir(path)
        .map_err(|error| ToolError::new(error.to_string()))?
        .take(201)
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| ToolError::new(error.to_string()))?;
    let truncated = names.len() > 200;
    names.truncate(200);
    names.sort();
    Ok(ToolOutput::text(
        serde_json::json!({"names":names,"truncated":truncated}).to_string(),
    ))
}

fn number(value: Option<&str>, default: u64) -> Result<u64, ToolError> {
    value
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|error| ToolError::new(error.to_string()))
        })
        .unwrap_or(Ok(default))
}
