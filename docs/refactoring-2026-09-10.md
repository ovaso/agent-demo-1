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
