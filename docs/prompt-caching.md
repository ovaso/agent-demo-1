# 缓存与输入预算

缓存命中以服务商 usage 为准。OpenAI 兼容适配器同时支持
`prompt_tokens_details.cached_tokens` 和 DeepSeek `prompt_cache_hit_tokens`；
前者存在且为零时不会被后者覆盖。未知报告保留为 unknown。

trace-map 的 `cache_tokens` 是缓存读取 Token / 全部输入 Token。
`hits/reports` 是有命中的请求数 / 上报缓存的请求数，两者口径不同。
总输入已包含缓存输入，输出已包含思考 Token，不能再重复相加。
汇总中有缺失报告时不生成看似完整的 Token 命中率；可用 reports 与 calls 检查覆盖。

模型的长度截断、拒绝和其他未完成结束原因不代表任务完成。
运行时保留已输出的文本并暂停，`/resume` 在原预算内继续；该轮的工具调用不会执行。
OpenAI 与 Anthropic 的完整响应、流式响应均转换为统一的停止原因。

DeepSeek/OpenAI 的 reasoning_content 与 Anthropic 的有序签名思考内容会随
assistant 消息保存，跨进程 resume 后按原协议回传。续接数据绑定协议、主机和
请求模型，不保存 URL 凭据或查询参数；改变绑定时需要新的会话。
trace 的 response_model 记录服务商返回的模型名，原有 model 记录请求配置。
Provider 的流式与完整响应均限制为最多 8 MiB，超限或中断不会执行部分工具。
