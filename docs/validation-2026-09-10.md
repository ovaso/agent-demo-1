# 当前验证入口与历史范围

测试、夹具和基准源码已在 `f6e832c` 删除。本页不再将删除前的测试数量作为当前交付证据；不执行 `cargo test`，不恢复旧测试基础设施。

## 当前检查

Rust 修改后执行格式检查和受影响范围的编译检查；跨 crate 修改使用：

```sh
cargo fmt --all -- --check
cargo check --workspace
```

核心库可选能力变化时补充 `cargo check -p agent-core --no-default-features`；涉及发布体积时执行 `cargo build --release -p agent-app --locked`，用相同工具链、target、features 比较最终可执行文件。编译通过不等价于行为回归通过。

最近结构重构的实际结果见 [结构审查](structure-audit-2026-09-10.md)；review 修复验证见 [修复记录](review-fixes-2026-09-10.md)。有限人工检查只能证明其覆盖的输入与路径，不代表完整端到端或真实模型服务验证。

## 删除前的历史验证

旧报告针对 `1482042` 阶段的产品代码，由 `f57f08d` 加入 CLI 协议与跨进程场景，随后又有功能迭代。旧版本的报告可使用 `git show f57f08d:docs/validation-2026-09-10.md` 查看；其中的测试命令、数量、体积以及源码路径仅对应当时版本。

当前不存在 `agent-app/tests/runtime_cli.rs` 或 `agent-app/tests/support/`。不把这些路径渲染为可访问文件链接，也不把历史“全部通过”解释为当前代码状态。

性能归档的来源与复现限制见 [基准档案说明](benchmarks/README.md)。
