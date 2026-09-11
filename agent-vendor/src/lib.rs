//! 厂商适配器共享的 HTTP 与续接校验支持。
//!
//! 模型 contract 由 `agent_core::model` 定义。本 crate 不定义第二套模型接口，
//! 不包含具体厂商的协议、凭据或应用配置，也不是所有模型实现的必需依赖。
pub mod continuation;
pub mod http;
