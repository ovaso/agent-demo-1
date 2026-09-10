//! 工具调用链路的回显探针。保留迁移前的名称、参数、权限和 JSON 输出格式。

use agent_core::tool;

#[tool(created_at = 1788784117, version = "v1.0.0-20260910", group = "debug")]
fn echo(text: String) -> String {
    text
}
