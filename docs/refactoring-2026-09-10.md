# 结构与资源开销重构

## 基线与方法

生产代码基线为 `f57f08d`。使用 `rustc 1.96.1`、`aarch64-apple-darwin`、现有 release 配置和默认 features。应用产物为 6,173,808 字节；不更改功能、SQLite bundled 链接方式或发布参数来缩小体积。

固定本地基准位于 `agent-core/examples/runtime_bench/`，只使用标准库和现有依赖，不调用网络模型。编译与运行：

```sh
cargo build --release -p agent-core --example runtime_bench --locked
target/release/examples/runtime_bench loop 5
target/release/examples/runtime_bench planning 5
target/release/examples/runtime_bench sqlite 5
target/release/examples/runtime_bench memory-search 5
```

每个进程先预热一次，测量 5 次；重复 5 个进程样本，报告中位数。前三个场景每次完成 17 次模型调用、16 次工具调用，工具输出固定 16 KiB，历史上限 16 条，注册 16 个工具。`planning` 同时保留 32 个计划任务；`sqlite` 使用 FULL 同步的实际 SQLite 文件。`memory-search` 扫描 512 条各约 30 KiB 的记忆，8 条匹配。

分配次数和累计分配字节包括 realloc 请求；峰值是测量期间相对起点增加的 Rust 活跃堆字节，不是 RSS，不含 SQLite 的 C 分配。计数器本身会引入开销，所以只比较同一个基准和工具链下的数据；不推断线上模型吞吐或 CPU 占用率。原始样本见 [refactor-baseline.json](benchmarks/refactor-baseline.json)。

| 场景 | 每次耗时 µs | 分配次数 | 累计分配字节 | 增量峰值 Rust 堆字节 |
| --- | ---: | ---: | ---: | ---: |
| Loop | 19,581 | 8,879 | 9,134,521 | 820,725 |
| 带规划的 Loop | 26,140 | 33,094 | 13,189,034 | 922,798 |
| SQLite Loop | 66,960 | 4,773 | 31,570,537 | 827,043 |
| Markdown 记忆搜索 | 18,014 | 3,620 | 47,461,124 | 15,807,798 |

## 分步范围

1. 建立独立、可重复的本地资源基准并提交。
2. 拆出检查点编码职责，减少 SQLite 保存时重复序列化，保留字节上限、事务投影、版本比较与逐工具持久化边界。
3. 用借用数据的规划视图直接序列化，减少临时 JSON 树及重复复制，保留模型可见字段和节点权限。
4. 让 Markdown 搜索逐条读取筛选，拆分文件格式与目录查询职责，保留排序、大小写匹配和损坏文件报错行为。
5. 分离 CLI 配置、状态展示与交互循环，完成全局验证和相同输入下的前后比较。

每项独立提交，实际结果在完成对应步骤后记录。

## 检查点编码

将有界 JSON 遍历放入 `runtime/serialization.rs`，存储模块继续负责状态有效性与版本比较，SQLite 只在编码成功后开始事务。编码过程中逐段检查剩余字节，消除保存前的完整计数遍历；未增加常驻缓存，也未调整事务、刷盘或工具边界。

新增 UTF-8/转义字节边界、单次遍历、序列化错误以及超限后检查点和会话投影保持不变的测试。格式、Clippy、workspace 186 项测试、无默认 feature 核心库 50 项测试通过。release 为 6,171,344 字节，比基线减少 2,464 字节。

3 组交替运行的初测中，SQLite 样例中位耗时为 68,402 → 56,425 µs，降低约 17.5%；分配次数、分配字节与峰值 Rust 堆不变。Loop 和 planning 样例有约 1–2% 耗时波动，暂不认定收益；最终统一比较见后续记录。

## 规划状态投影

`planning_prompt.rs` 保留固定规则、能力过滤和消息装配；`planning_view.rs` 单独负责协调者/节点可见数据投影，用借用的类型化视图直接编码。目标、计划、工具定义与节点结果不再经过完整临时 JSON 树，活动节点查找在一次投影中复用。带前缀的 JSON 直接写入同一个有界缓冲区。

新增完整数据字段与子节点可见范围回归，保留原有 JSON 数据含义；对象键顺序不作为接口契约。格式、Clippy、workspace 187 项测试、无默认 feature 核心库 51 项测试通过。release 为 6,154,192 字节，本步减少 17,152 字节。

planning 初测每次分配从 33,094 降至 22,537 次（约 -31.9%），累计分配字节从 13,189,034 降至 12,078,628（约 -8.4%）。耗时 26,196 → 26,815 µs，增量峰值 Rust 堆 922,798 → 927,918 字节；未测得耗时或峰值堆改善，本步收益是减少分配和产物体积。

## Markdown 记忆查询与文件格式

目录扫描/筛选留在 `memory/markdown.rs`，元数据头与正文读写放入 `memory/markdown/format.rs`。查询逐条读取并筛选，只排序匹配结果；读取正文复用原文件缓冲区，元数据明显大于正文时收缩容量。保存时使用借用的元数据头和缓冲写入，避免构造含完整正文的第二份文档；临时文件重命名与原有持久化语义保持一致。文件名十六进制编码改为写入单个预分配字符串。

新增 Unicode 大小写、ID/正文/标签匹配、排序、空查询、损坏文件、旧格式与特殊字符原样往返测试。格式、Clippy、workspace 190 项测试、无默认 feature 核心库 54 项测试通过。release 保持 6,154,192 字节。

memory-search 初测中位耗时为 18,596 → 17,234 µs，分配次数 3,620 → 3,101，累计分配字节 47,461,124 → 31,623,044，增量峰值 Rust 堆 15,807,798 → 309,637 字节（约 -98.0%）。此结果针对少量匹配；空查询仍按接口约定返回全部记忆，不能宣称所有查询都保持固定内存。

## 应用装配与 CLI

应用环境解析移至 `agent-app/src/config.rs`；CLI 状态、计划、Agent 和消息展示移至 `cli/view.rs`；`main.rs` 负责启动参数分发，`cli/mod.rs` 负责交互循环，`cli/session.rs` 负责命令执行和运行时装配。

`ConfiguredProvider` 在应用边界统一协议分派，使 CLI 使用同一种运行时实例类型。核心库的通用 ModelProvider 接口保持不变，两种协议仍分别维护 HTTP 转换和增量流解析；模型名称透传以保持已保存任务的恢复检查。该分派增加一次枚举匹配，没有新增堆分配、依赖或异步运行时。应用产物本步减少 400 字节，未对这一次分派单独宣称耗时收益。

## 最终比较与验证

同一份基准源码分别与重构前后的核心实现编译，交替运行 5 对进程；每个进程预热一次、测量 5 次。下面使用这一轮的中位数，避免把不同阶段的时间混合比较。原始样本、范围和其他指标见 [refactor-comparison.json](benchmarks/refactor-comparison.json)。

| 场景 | 耗时 µs：前 → 后 | 分配次数：前 → 后 | 累计分配字节：前 → 后 | 增量峰值 Rust 堆字节：前 → 后 |
| --- | ---: | ---: | ---: | ---: |
| Loop | 19,544 → 19,765（+1.13%） | 8,879 → 8,879 | 9,134,521 → 9,134,521 | 820,725 → 820,725 |
| 带规划的 Loop | 26,228 → 26,306（+0.30%） | 33,094 → 22,537 | 13,189,034 → 12,078,628 | 922,798 → 927,918 |
| SQLite Loop | 66,562 → 54,881（-17.55%） | 4,773 → 4,773 | 31,570,537 → 31,570,537 | 827,043 → 827,043 |
| Markdown 记忆搜索 | 18,143 → 17,227（-5.05%） | 3,620 → 3,101 | 47,461,124 → 31,623,044 | 15,807,798 → 309,637 |

本轮保留的取舍是：应用实际使用的 SQLite 路径减少序列化耗时，规划和记忆路径减少分配，内存后端 Loop 的固定样例约慢 1.1%。带规划的 Loop 增量峰值 Rust 堆增加 5,120 字节，不能表述为所有路径都更快或峰值都更低。计数器开销、文件系统与主机负载会影响耗时；这些结果不等同于真实模型延迟、进程 RSS 或 CPU 占用率。

最终 release 为 **6,153,792 字节**，相对基线减少 **20,016 字节（约 0.32%）**。根 release 配置、依赖和 features 均未更改；SQLite bundled、数学渲染和语法高亮仍保留。

最终验证：

- `cargo fmt --all -- --check` 通过。
- `cargo clippy --workspace --all-targets -- -D warnings` 通过。
- `cargo test --workspace --quiet`：190 项通过，0 失败、0 忽略。
- `cargo test -p agent-core --no-default-features --quiet`：54 项通过。
- `cargo clippy -p agent-core --all-targets --no-default-features -- -D warnings` 通过。
- `cargo test -p agent-app --no-default-features --quiet`：49 项通过。
- `cargo test --release -p agent-app --test runtime_cli --locked --quiet`：5 项通过。
- `cargo build --release -p agent-app --locked` 通过。

端到端验证使用本机 HTTP 桩，覆盖 OpenAI / Anthropic、只规划边界、Blackboard、子 Agent 等待/答复、预算暂停、跨进程恢复、工具只执行一次、流中断和轨迹读取。没有连接真实模型服务。

## 提交顺序

| 提交 | 内容 |
| --- | --- |
| `f6df2a0` | 固定运行时和记忆资源基准 |
| `fe2e2b3` | 有界检查点单次编码 |
| `a6effad` | 借用的规划状态投影 |
| `cd4cec9` | Markdown 逐条筛选与正文缓冲复用 |
| 本报告所在提交 | 应用配置/CLI 展示拆分、统一 Provider 分派及最终验证记录 |
