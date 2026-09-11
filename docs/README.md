# 项目文档

本目录作为项目的技术准则与产品实现方向依据，采用[设计文档驱动开发](common/design-driven-development.md)：先明确设计，再依照当前已确认的规则实现，并按验收标准核对结果。

按长期使用的主题查阅设计与操作说明。现有材料包含设计提案、当前能力和历史验证；归入本目录不代表全部已确认或已实现，适用范围以各文档标注为准。

## 主题导航

| 主题 | 入口 | 内容 |
| --- | --- | --- |
| Context | [上下文调研与设计](context/README.md) | CH、上下文有效性、Epoch、动态选取与压缩配置设计，以及现有缓存和输入预算实现 |
| Runtime | [Loop、Graph 与多 Agent](runtime/README.md) | 执行设计、规划与路由、委托和消息协作、实现状态及 CLI |
| Tracing | [调用树与运行时序](tracing/README.md) | 轨迹查看、调用关系、耗时、Token 与缓存指标 |
| Budget | [有界预算续期](budget/README.md) | 执行额度、有限续期、恢复语义与操作入口 |
| Common | [通用开发](common/README.md) | 配置、环境变量等独立于 Agent 领域的开发主题 |
| Tools | [工具开发](tools/README.md) | 工具宏、自动注册、分组、状态注入与开发边界 |

首次运行可先阅读[启动环境配置](common/environment.md)和[运行时 CLI](runtime/03-runtime-cli.md)；修改代码前查看根 [AGENTS.md](../AGENTS.md) 中的项目约定与验证要求。

## 记录与验证资料

以下链接指向当前保留的状态与历史资料。历史测试命令、数量和体积数据不代表当前可执行入口或当前验证结果。

- [实现状态](runtime/02-implementation-progress.md)：运行时能力、限制与代码归属。
- [历史基准档案](benchmarks/README.md)：旧测量产物、来源及复现限制。

## 阅读约定

编号用于有顺序的设计研讨或专题阅读；领域目录以 `README.md` 为入口。配置、环境变量等脱离 Agent 场景仍适用的通用开发主题直接收录在 `common/` 下，使用描述性文件名，不单独建立领域目录。实现参考不代表此前提案已经实现。历史资料的适用范围以来源版本为准；后续功能变化应更新相应使用说明和能力边界。
