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

## 解析与生效时间

支持 UTF-8（含 BOM）、`KEY=VALUE`、注释、可选 `export`、单/双引号、多行值与 dotenv 变量引用。单引号用于保留 `$` 等字面内容；变量引用可使用进程变量和文件中此前定义的变量。重复变量、NUL、格式错误、非普通文件会报错；错误仅给出文件和位置附近信息，不回显原始配置行或密钥。

文件输入最多 64 KiB，解析后键值总量最多 64 KiB，最多 256 个变量。默认文件缺失可省略；文件存在但有错误时不会静默忽略。

文件只在启动时解析一次，配置随后传入模型适配器和 CLI 会话。运行中修改文件不会改变当前会话的配置，`/trace` 继续使用本次启动时确定的路径。重启后重新加载；已有任务的预算以检查点为准，需通过 `/budget` 明确调整。更换模型后恢复仍须满足原有模型标识校验。

环境文件用于应用配置读取，不改写进程全局环境；命令工具仍继承启动进程的环境。这样可以在建立 HTTP Client 前完成文件解析，并避免通过全局环境修改传播配置。

## 代码与验证

文件选择、解析和覆盖规则位于 `agent-app/src/config/environment.rs`，类型转换留在 `config.rs`，CLI 只接收已解析配置。使用 `dotenvy 0.15.7` 的[迭代读取接口](https://docs.rs/dotenvy/latest/dotenvy/fn.from_read_iter.html)处理 dotenv 语法；默认 features 关闭，依赖树没有新增传递依赖，未引入配置框架。

本次新增 5 项单元测试和 5 项端到端测试，覆盖引号/注释/变量引用、多行值、优先级与空值、错误脱敏、文件边界、模型/预算/路径读取、离线命令、显式文件选择及重启生效。最终通过：

- 格式检查及 workspace Clippy（`-D warnings`）。
- workspace 全量测试：221 项。
- 核心库无默认 feature：67 项。
- 应用无默认 feature：66 项。
- release CLI 端到端测试：16 项。
- `cargo build --release -p agent-app --locked`。

用实际本地 `.env` 在独立临时目录完成了启动和建立检查点验证，未继承模型环境变量，模型请求数为 0；没有操作现有会话数据库或调用真实模型服务。

同一 `rustc 1.96.1`、`aarch64-apple-darwin`、默认 features 和 release 命令下，产物由 6,174,368 增至 **6,175,424 字节**，增加 **1,056 字节（约 0.02%）**。release 配置未变，未测量或宣称运行耗时收益。
