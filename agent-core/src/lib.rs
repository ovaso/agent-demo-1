/// Agent 执行循环。
pub mod agent;
/// 上下文模型与存储抽象。
pub mod context;
/// 长期记忆模型与存储抽象。
pub mod memory;
/// 与供应商无关的模型请求、响应和抽象接口。
pub mod model;
/// 工具定义、注册与调用。
pub mod tool;
/// 执行链路追踪。
pub mod trace;

pub use agent_tool_macro::tool;
#[doc(hidden)]
pub use inventory;
pub use serde;
pub use serde_json;
