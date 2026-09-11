# agent-vendor

厂商适配器可选的公共支持库，不是第二套模型 contract。

- `agent-core::model::ModelProvider` 是运行时消费的唯一接口。
- `http` 复用 JSON 请求字节限制、HTTP 发送、响应读取上限和 URL 主机判断。
- `continuation` 复用不透明续接数据的协议/主机/模型绑定校验。
- 不含厂商名称、服务地址、鉴权规则、缓存字段或应用环境变量。

本 crate 依赖 reqwest，因此本地模型、进程模型等非 HTTP Provider 应直接实现核心接口，无需依赖本 crate。这里的 `pub` API 仅用于跨 crate 共享已有实现，不引入注册中心、通用 Provider 工厂或额外适配 trait。

依赖方向见[项目约定](../AGENTS.md#代码放置与模块边界)，当前验证见[实现状态](../docs/runtime/02-implementation-progress.md#当前验证证据)。
