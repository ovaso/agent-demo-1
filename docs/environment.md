# 启动环境配置

应用启动时自动读取工作目录中的 `.env`，按 **进程环境变量 → 环境文件 → 内置默认值** 取值。普通启动、`--status`、`--trace-map` 使用相同的配置来源。默认 `.env` 不存在时继续使用环境变量和默认值。

## 使用方式

本机已经有 `.env` 时直接编辑它。首次配置可以从仓库示例创建，填写自己的模型密钥：

```sh
cp -n .env.example .env
chmod 600 .env
cargo run -p agent-app
```

`.env` 和 `.env.*` 已被 Git 忽略，只有不含真实密钥的 `.env.example` 纳入版本管理。模型配置沿用已有的 `RS_AGENT_PROVIDER`、`OPENAI_*` 或 `ANTHROPIC_*` 名称，完整示例见 [.env.example](../.env.example)。必需的模型密钥或模型名称为空时，会在启动阶段报错。

预算策略可以直接写入 `.env`：

```dotenv
RS_AGENT_MAX_STEPS=8
RS_AGENT_AUTO_EXTEND=true
RS_AGENT_HARD_MAX_STEPS=32
RS_AGENT_STEP_INCREMENT=8
RS_AGENT_MAX_STEP_EXTENSIONS=3
RS_AGENT_MAX_DELEGATIONS=8
```

进程环境按变量名覆盖文件，即使值为空也视为已设置。例如临时使用 12 步初始额度：

```sh
RS_AGENT_MAX_STEPS=12 cargo run -p agent-app
```

`mise`、IDE 或 shell 导出的变量也属于进程环境，会覆盖 `.env` 中的同名设置。初始额度不能大于硬上限；只覆盖初始额度时，仍需满足文件中设置的硬上限。

如需指定另一个文件，在进程环境中设置：

```sh
RS_AGENT_ENV_FILE=.env.production cargo run -p agent-app
```

显式指定的文件必须存在。只读取这一份文件，不与默认 `.env` 叠加，也不向父目录搜索。文件选择变量只从进程环境读取，不通过文件内容递归加载其他文件。所有相对路径仍以启动时的工作目录为准，选择其他目录的环境文件不会改变工作目录。

## 调试工具

使用 `cargo run -p agent-app -- --mode=debug`（或 `./target/release/agent-app --mode=debug`）启动后，可让模型调用无参数只读工具 `debug_show_config` 查看配置。普通启动不注册该工具；环境变量不能开启调试工具。该启动参数单独使用，不与 `--status`、`--trace-map` 组合，也不同于会话内切换执行方式的 `/mode`。

工具按 `KEY=VALUE` 返回本次启动解析后的配置，包含当前 provider 的模型、实际 base URL、OpenAI stream usage 或 Anthropic max tokens，以及步数、委派上限、数据库、记忆目录、会话和追踪路径。默认值和布尔值按实际生效结果展示；自动扩展关闭时，三个扩展参数显示 `<disabled>`。只输出明确列出的配置字段，不枚举环境变量，不输出 API key。

这是启动配置快照：运行中修改 `.env`、通过 `/budget` 调整任务或恢复旧任务，不会改变工具结果；当前任务预算以 `/status` 为准。工具实现与统一注册入口位于 `agent-tool-debug` crate，配置快照在 `agent-app/src/config/debug.rs` 组装，后续调试工具放入同一 crate，标注 `#[tool(group = "debug")]` 后由其入口按组自动注册。该 crate 直接依赖关闭默认 features 的 `agent-core`；宏选项、状态注入和自动发现说明见[工具开发](tool-development.md)。

调试工具首次实现验证：新增 1 项工具单元测试和 5 项本地 mock CLI 测试，覆盖注册开关、非法参数、配置覆盖、默认值、关闭扩展、敏感字段排除、Anthropic、Plan Mode 只读调用及启动快照。通过 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`cargo test -p agent-core --no-default-features`（67 项）和 `cargo build --release -p agent-app --locked`。同一 `rustc 1.96.1`、`aarch64-apple-darwin`、默认 features 和 release 命令下，主可执行文件由 6,175,424 增至 6,193,168 字节，增加 17,744 字节（约 0.29%）。普通模式不创建配置快照；未测量运行耗时变化。

## 解析与生效时间

支持 UTF-8（含 BOM）、`KEY=VALUE`、注释、可选 `export`、单/双引号、多行值与 dotenv 变量引用。单引号用于保留 `$` 等字面内容；变量引用可使用进程变量和文件中此前定义的变量。重复变量、NUL、格式错误、非普通文件会报错；错误仅给出文件和位置附近信息，不回显原始配置行或密钥。

文件输入最多 64 KiB，解析后键值总量最多 64 KiB，最多 256 个变量。默认文件缺失可省略；文件存在但有错误时不会静默忽略。

文件只在启动时解析一次，配置随后传入模型适配器和 CLI 会话。运行中修改文件不会改变当前会话的配置，`/trace` 继续使用本次启动时确定的路径。重启后重新加载；已有任务的预算以检查点为准，需通过 `/budget` 明确调整。更换模型后恢复仍须满足原有模型标识校验。

环境文件用于应用配置读取，不改写进程全局环境；命令工具仍继承启动进程的环境。这样可以在建立 HTTP Client 前完成文件解析，并避免通过全局环境修改传播配置。

## 代码与验证

下列测试数量和体积数据属于原实现阶段的历史记录；当前项目已删除测试与基准程序，本轮验证见 [结构审查与重构](structure-audit-2026-09-10.md)。

文件选择、解析和覆盖规则位于 `agent-app/src/config/environment.rs`，运行配置转换位于 `config/mod.rs` 和 `config/prompt.rs`，模型服务配置位于 `config/provider.rs`，CLI 只接收已解析配置。使用 `dotenvy 0.15.7` 的[迭代读取接口](https://docs.rs/dotenvy/latest/dotenvy/fn.from_read_iter.html)处理 dotenv 语法；默认 features 关闭，依赖树没有新增传递依赖，未引入配置框架。

本次新增 5 项单元测试和 5 项端到端测试，覆盖引号/注释/变量引用、多行值、优先级与空值、错误脱敏、文件边界、模型/预算/路径读取、离线命令、显式文件选择及重启生效。最终通过：

- 格式检查及 workspace Clippy（`-D warnings`）。
- workspace 全量测试：221 项。
- 核心库无默认 feature：67 项。
- 应用无默认 feature：66 项。
- release CLI 端到端测试：16 项。
- `cargo build --release -p agent-app --locked`。

用实际本地 `.env` 在独立临时目录完成了启动和建立检查点验证，未继承模型环境变量，模型请求数为 0；没有操作现有会话数据库或调用真实模型服务。

同一 `rustc 1.96.1`、`aarch64-apple-darwin`、默认 features 和 release 命令下，产物由 6,174,368 增至 **6,175,424 字节**，增加 **1,056 字节（约 0.02%）**。release 配置未变，未测量或宣称运行耗时收益。

## 输入与记忆边界

| 变量 | 默认值 | 用途 |
|---|---:|---|
| `RS_AGENT_MAX_CONTEXT_BYTES` | `1048576` | 会话及完整 Provider 请求 JSON 的字节上限 |
| `RS_AGENT_MEMORY_MAX_RESULTS` | `8` | 最多加载的记忆记录数；0 禁用检索 |
| `RS_AGENT_MEMORY_MAX_BYTES` | `65536` | 选中记录字段的 UTF-8 总量上限 |
| `RS_AGENT_MEMORY_ENTRY_BYTES` | `16384` | 单文件读取及单条记录上限 |

运行时将记忆总量进一步限制在上下文字节上限的一半内，保留任务与工具空间。
旧任务恢复时也会限制旧记忆；配置值随任务检查点保存，不由后续进程环境悄悄覆盖。

## 上下文批量压缩

| 变量 | 默认值 | 用途 |
|---|---:|---|
| `RS_AGENT_CONTEXT_COMPACTION` | `1` | 启用模型调用边界的批量压缩；0 使用原数量窗口 |
| `RS_AGENT_CONTEXT_HIGH_BYTES` | `262144` | 逻辑会话 JSON 高水位，默认不超过硬上限的 3/4 |
| `RS_AGENT_CONTEXT_LOW_BYTES` | 高水位的 1/2 | 保留近期内容的字节目标 |
| `RS_AGENT_HISTORY_MAX_MESSAGES` | `512` | 数量高水位，至少 4；一次压缩保留近期约一半消息 |
| `RS_AGENT_SUMMARY_MAX_BYTES` | `8192` | 原文摘录字节上限，默认不超过低水位的 1/4 |

有效范围：256 <= 摘录上限 < 低水位 < 高水位 <= RS_AGENT_MAX_CONTEXT_BYTES。
会话水位与实际传输 JSON 大小口径不同；发送前仍校验完整 Provider 请求硬上限。

## Token 预算

| 变量 | 默认值 | 用途 |
|---|---:|---|
| `RS_AGENT_MAX_TOTAL_TOKENS` | `2000000` | 单个根任务的累计输入加输出预算，0 关闭累计限制 |
| `RS_AGENT_MAX_OUTPUT_TOKENS` | `8192` | 每次输出上限，必须大于零；Anthropic 可沿用显式 ANTHROPIC_MAX_TOKENS |
| `OPENAI_MAX_TOKENS_FIELD` | 按主机选择 | 官方 OpenAI 使用 max_completion_tokens，其他兼容服务使用 max_tokens；可显式指定其一 |

`/tokens` 查看；`/tokens <新总额度>` 调整当前任务；`/tokens 0` 关闭累计限制。
`/output-budget <数量>` 调整当前任务单次输出上限，随后 `/resume` 继续。
这些操作不重置已用步数、续期次数或已记录用量，模型不能自行提高根额度。

## 服务商策略

- `OPENAI_REASONING_EFFORT`：缺省或 `default` 沿用服务商设置；可选 none、minimal、low、medium、high、xhigh、max。实际支持取决于服务商与模型。DeepSeek 官方端点会同时设置其 thinking 开关；none 关闭思考。
- `ANTHROPIC_CACHE_TTL`：off、5m、1h。官方 Anthropic 主机默认 5m；兼容主机默认 off，可在确认支持后显式开启。
- DeepSeek OpenAI 兼容端点使用服务端自动缓存，不添加 Anthropic cache_control。

根目录 `.env.example` 提供无密钥示例。进程环境仍优先于 .env。

`ANTHROPIC_MAX_TOKENS` 未设置时使用 1024；设置后必须是非零 `u32` 整数，非法值直接报错，不再静默回退。
