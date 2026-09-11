# agent-vendor-deepseek

DeepSeek Chat Completions 适配器，直接实现 `agent_core::model::ModelProvider`。依赖核心 contract 与 `agent-vendor` 的有界 HTTP/续接校验；不读取环境变量，不依赖其他厂商 crate。

## 适配范围（核对日期：2026-09-11）

以下清单记录本项目的实现状态，不代表 DeepSeek 的全部能力。“未做”表示本项目尚未接入，不表示厂商不支持，也不代表已承诺实现。当前覆盖文本 Agent 的主要链路，尚未完整覆盖 DeepSeek API；已实现项的验证程度见下方验证记录。

### 已做清单

- [x] Chat Completions 文本请求：system、user、assistant、tool 消息转换，支持非流式与 SSE 流式响应。
- [x] 鉴权与连接配置：API key、模型名、base URL 显式传入，可注入和复用 HTTP Client。
- [x] 工具调用：转换核心工具定义、解析普通及流式工具调用、回传工具结果；参数 schema 沿用核心当前的字符串参数结构。
- [x] 推理档位：`default / none / low / high / max`，显式转换 thinking 开关与 reasoning_effort，拒绝未支持的配置值；策略不依赖域名。
- [x] 思考输出：将 reasoning_content 转换为通用思考增量，与答案增量分开；非流式及流式返回值均保留可展示的思考内容。
- [x] 思考续接：保留并回传历史 assistant 消息的 reasoning_content，覆盖工具调用和普通回答轮次；校验续接协议、主机与模型绑定。
- [x] 输出预算与用量：传入 max_tokens，读取输入、输出、思考 token 和缓存命中量；缺失用量保持未知。
- [x] 响应边界：复用有界请求编码和响应读取，检测 SSE 缺少结束标记；非正常结束时不执行该响应中的工具调用。
- [x] 应用链路接入：`RS_AGENT_PROVIDER=deepseek` 与 `DEEPSEEK_*` 启动配置、调试配置快照、CLI 思考/回答分段展示，以及有部分回答时的截断续接恢复。配置和终端行为由应用与核心实现，不放入本 crate。

### 未做清单

- [ ] 图片输入：尚无图片内容块转换，也未接通核心图片附件、CLI 附图或工具图片结果链路。
- [ ] Files API：上传文件、管理文件生命周期及在消息中引用 file_id。
- [ ] JSON 输出模式与更完整的请求参数配置，例如 response_format、stop、temperature、top_p。
- [ ] 更丰富的工具协议配置：任意 JSON Schema、strict 模式及可配置的 tool_choice 策略。
- [ ] 模型能力机制：按模型和端点描述图片、推理等能力，支持显式覆盖，并校验具体模型支持的参数；当前只校验本地已实现的推理档位取值。
- [ ] 其他 API：Responses、FIM、模型列表和余额查询等；当前 vendor 只调用 Chat Completions。
- [ ] 会话内切换推理档位：当前仅在应用启动时读取配置。
- [ ] 真实 DeepSeek API 联调：文本、工具调用、思考流、各档位和用量尚未经过真实服务验证。

## 使用示例

```rust
use agent_core::model::{ModelError, ModelProvider, ModelRequest, ModelResponse};
use agent_vendor_deepseek::{DeepSeekProvider, ReasoningEffort};

fn complete(api_key: String, request: ModelRequest<'_>) -> Result<ModelResponse, ModelError> {
    let mut provider = DeepSeekProvider::new(api_key, "deepseek-flash")
        .with_reasoning_effort(ReasoningEffort::High);
    provider.complete(request)
}
```

`ReasoningEffort` 支持 Default、None、Low、High、Max；字符串解析支持对应小写值。Default 沿用模型默认值，None 关闭思考，其余显式开启并发送强度。设置同样适用于代理地址。

`stream_events` 增量回调 `ModelStreamEvent::ReasoningDelta` 和 `TextDelta`；`stream` 保留仅答案文本的原接口。`complete` 和流式返回值均可通过 `ModelResponse::reasoning_content()` 读取思考。展示字段只保留在本轮响应中，`into_reply_parts` 不将它重复写入历史。它不代替 `ModelContinuation`：后续请求仍回传原始 reasoning_content；独立协议标识防止误用其他适配器的续接数据。

`request.rs`、`response.rs`、`stream.rs`、`reasoning.rs` 分别管理请求、响应、SSE 与档位策略。虽然线格式与 OpenAI 兼容，两个适配器分别拥有协议转换，避免互相依赖或把厂商协议字段放入公共 `agent-vendor`；连接复用、有界请求/响应和续接绑定直接复用公共支持。未引入新的第三方依赖或 features。

依据：[思考模式](https://api-docs.deepseek.com/guides/thinking_mode/)、[Chat Completions](https://api-docs.deepseek.com/api/create-chat-completion/)。应用配置见[启动环境配置](../docs/common/environment.md)。

## 本次验证（2026-09-11）

通过 `cargo fmt --all -- --check`、`cargo check --workspace`、`cargo check -p agent-core --no-default-features`、`cargo build --release -p agent-app --locked` 和 `git diff --check`。没有新增或保留测试/夹具/基准程序，没有运行 cargo test。

使用临时目录和本地 HTTP 端点人工核验了五个档位、非法值零请求、思考先于答案增量到达、纯文本输出、工具调用与下一轮普通回答的完整思考回传、长度截断后的恢复、SSE 中断报错，以及 Anthropic thinking/signature/redacted 块的展示与续接分离。未调用真实模型 API。

同一 rustc 1.96.1、aarch64-apple-darwin、默认 features 和上述 release 命令，修改前已有 vendor 拆分工作区的主可执行文件为 6,363,424 字节，本次为 6,417,520 字节，增加 54,096 字节（约 0.85%）。仅增加本地 vendor crate 的依赖边，第三方依赖、features 和 release 配置不变。未测量运行耗时或峰值内存；展示思考与 opaque 续接各有自己的所有权，响应解析期间可能持有两份思考正文，受现有响应读取上限约束，不重复持久化展示字段。
