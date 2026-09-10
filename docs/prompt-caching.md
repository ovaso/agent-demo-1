# 缓存与输入预算

缓存命中以服务商 usage 为准。OpenAI 兼容适配器同时支持
`prompt_tokens_details.cached_tokens` 和 DeepSeek `prompt_cache_hit_tokens`；
前者存在且为零时不会被后者覆盖。未知报告保留为 unknown。

trace-map 的 `cache_tokens` 是缓存读取 Token / 全部输入 Token。
`hits/reports` 是有命中的请求数 / 上报缓存的请求数，两者口径不同。
总输入已包含缓存输入，输出已包含思考 Token，不能再重复相加。
汇总中有缺失报告时不生成看似完整的 Token 命中率；可用 reports 与 calls 检查覆盖。
