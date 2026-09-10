use agent_core::tool;

use super::DebugContext;

/// 显示本次启动实际生效的配置项（KEY=VALUE），包含默认值，不含 API 密钥。
/// 配置为启动快照，不反映运行中修改的环境文件或任务预算。
#[tool(
    created_at = 1789028730,
    version = "v1.0.0-20260910",
    group = "debug",
    read_only,
    output = "text"
)]
fn debug_show_config(#[context] context: &DebugContext) -> &str {
    &context.config_snapshot
}
