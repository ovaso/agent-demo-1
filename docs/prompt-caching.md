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

执行状态不再重复传递已在 tools 中声明的 schema。PlanOnly 协调者仅保留
尚未开放执行的工具目录；子 Agent 仍由其工具权限限制能力。
节点结果默认只携带至多 512 字节的 UTF-8 预览，output_truncated 标记截断，
需要完整证据时使用 runtime_result 分页读取。

运行状态以完整快照开始，后续按字段追加状态增量并保存到检查点，既有历史不重写。
协作输入与投递标记在发请求前一起持久化，模型失败或进程恢复不会丢失准备好的输入。
每个 actor 独立保留最近状态；历史裁剪/清空后补充完整快照。旧检查点缺少此元数据时
自动建立第一个快照，已用预算不重置。runtime.prompt.updated 记录角色、更新字段、
快照类型和历史代数，不记录消息正文。
