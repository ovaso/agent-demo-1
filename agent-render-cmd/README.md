# agent-render-cmd

面向命令行的 Rust Markdown 渲染库。使用持久块状态栈推进文档，使用 **pulldown-cmark** 处理行内内容、局部候选判定和表格单元格，选择性移植 **Glow / Glamour** 的宽度、块栈和主题规则，流式处理由本项目实现。没有嵌入 Go 运行时，也不启动外部 Glow 进程。

## 职责与接口

`agent-app` 负责输入框、快捷键、命令、环境变量和终端尺寸探测；本库只接收显式 `Options`、文本片段与 `std::io::Write`。`agent-core` 不依赖本库。

```rust
use agent_render_cmd::{Options, Renderer};

fn main() -> std::io::Result<()> {
    let options = Options { color: true, columns: 80, rows: 24, ..Options::default() };
    let mut output = Renderer::new(std::io::stdout().lock(), options);
    output.push("## 标题\n\n")?;
    output.push("**正在")?;
    output.push("生成**\n")?;
    output.finish()?;
    Ok(())
}
```

- `push`：立即输出当前可显示内容并 flush；不等待整个模型响应结束。
- `resize`：由调用方传入新的物理尺寸，避免依赖全局终端状态。
- `finish`：提交当前叶子块、关闭容器、恢复终端样式并 flush；可重复调用，完成后不再接受片段。彩色模式提交末行时会换行；原文模式仍按字节保持输入。
- `color: false`：按字节保留原始 Markdown 和换行，不产生 ANSI 控制码。应用在重定向、`NO_COLOR` 或不支持的终端使用此模式。
- `width_mode`：与终端的 Unicode、wcwidth 或无 ZWJ 合并模式匹配。终端本身的字体/emoji 宽度仍可能有差异。

## 持久状态栈与流式语义

生产入口已改为 `stream/state`，移除了原先全局 `pending → 整块重解析 → 过高则原文回退` 的路径。输入片段只装配当前逻辑行；完整行驱动进入/退出，当前栈顶决定处理方式。列表、列表项和引用是外层容器，段落、标题、分隔线、代码、表格、HTML、Math 是叶子帧。

例如列表引用里的代码，其状态为：

```text
List（列表种类、下一个序号）
  Item（输入缩进、显示标记、是否已输出）
    Quote
      Code（围栏字符/长度、缩进、语言、Syntect 状态）
```

先验证既有容器的续行前缀，再由栈顶分发。代码中的 `#`、`>`、列表符号等不会重新进入 Markdown 块识别；只有匹配的围栏、缩进退出、容器退出或 EOF 才结束代码块。测试验证 1,000 行已进入的代码不会再调用 Markdown 解析器，2,000 行代码跨越滚屏后仍保持面板和高亮状态。

| 状态 | 增量策略 |
|---|---|
| 段落 | 保留局部行内候选，处理跨行强调、链接等；明确结束后提交 |
| 标题、分隔线 | 完整控制行确认类型后提交；Setext 只在下划线候选出现时做局部判定 |
| 列表、引用 | 栈中保存容器、缩进和编号，提交子块后只保留上下文，不累计历史条目 |
| 围栏/缩进代码 | 头部只写一次；完整代码行直接追加，未完成行仅预览当前行；空行和结束条件按该状态处理 |
| 表格 | 表头和分隔行确认后进入表格状态；最多缓冲 8 行或预览/字节上限内的候选，随后固定列宽并逐行输出 |
| HTML | 保留独立的字面文本状态，按空行或相应终止标记退出，不把内部 Markdown 符号识别为新块 |
| Math | 保留当前公式和定界符；未闭合时预览源码，闭合后提交二维排版，超限仍保持公式状态并输出源码 |

`pulldown-cmark` 仍用于行内内容、候选语法和单元格解析；这不等于重新解析整个文档。已经写入终端历史的内容不再用于推断当前代码、列表、引用或 HTML 的类型。Glamour 移植的 `BlockStack` 仅负责布局几何；持久语法栈是本项目自行实现，两者职责不同。

### 有界预览与窗口变化

- `max_pending_bytes` 默认 32 KiB，限制局部候选文本；行装配缓冲上限取它与 8 KiB 的较小值，至少能容纳一个 UTF-8 字符。容器、表头、列宽和高亮状态单独保存，这不是整个库的总内存预算。
- `max_preview_rows` 默认 32，同时不超过终端高度减 3，只约束可回画的当前候选区。达到上限会固定当前部分，**保留块类型**继续输出，不再把整个代码块变为原始 Markdown。
- 超长单行会分段显示，可能增加视觉换行；代码高亮可降为普通代码样式，但背景、语言和正常结束围栏状态仍保留。超出行装配上限的未决控制行按字面内容分段处理。
- 很长的段落达到候选边界后，其后未能闭合的行内格式可能按字面文本显示；有限缓冲无法保证任意远的闭合符或后置引用能重排终端历史。
- 表格固定列宽后，后来的长值按列内换行；过窄时按单元格展开。表格仍留在栈中，后续行使用同一表头/对齐信息。
- 缩放保留整个状态栈和代码高亮状态。已经输出的历史不回画，当前未完成行必要时从新行续写；之后使用新宽度。表格在新宽度下输出新的表头并继续。暂时探测到 0 尺寸时保留最后已知尺寸。

链接标签本身就是目标网址或邮箱时，不重复追加相同地址；有名称的链接继续显示目标地址。

### 引用定义、脚注与单元格样式

- 普通链接的已确认引用定义在当前响应内保存，文本预算为 `max_pending_bytes`。标签空白归一化、Unicode 大小写匹配、转义和首个定义优先均交给 pulldown-cmark；保存的是定义源码，不累计已输出正文。
- 段落、标题和列表项中尚未解析的引用会立即预览，并在有界区域内保留该片段及容器前缀。后置定义到达后只重算这些片段，中间的代码等内容仅暂存已渲染行。该区域同时受 `max_preview_rows`、终端高度与 `max_pending_bytes` 约束。
- 超限或缩放会固定未决片段，继续推进原状态栈；不追溯终端历史。此后到达的定义显示为 `[label]: destination`，避免旧引用失去可查的目标。定义存储超限时也显示定义源码。这几个预算独立，不是整个渲染器的总内存限制。
- 启用 pulldown-cmark 的脚注识别，基础引用显示为 `[^label]`，定义显示为 `[^label]: 内容`。不生成跳转、不自动重新编号、不把脚注移到文末；多段及复杂嵌套脚注不保证完整排版。
- 表格单元格保留粗体、斜体、删除线、行内代码和链接样式；列内折行仍按完整字素处理，窄屏展开也保留样式。HTML `<br>` 仍作为标签文字显示。
- 已闭合的行内 `<sub>…</sub>` / `<sup>…</sup>` 在正文、标题、列表、引用及表格中生效，保留内部粗体/斜体等样式。可映射的数字、运算符和部分字母采用 [Unicode 上下标字符](https://www.unicode.org/charts/PDF/U2070.pdf)，例如 `H₂O`、`x²`、`xⁿ⁺¹`；中文等无法完整映射的内容显示为 `_{下标}` / `^{上标}`，不丢失原文字义。这是终端文本表示，不是网页的任意字号和基线排版。
- 上下标处理仅作用于解析器识别的行内 HTML 标签，标签名不区分大小写，属性不参与样式。代码、转义后的标签及其他 HTML 保留原有行为；未闭合标签按原文显示，不跨段落/单元格传播。整个 HTML 块仍按字面内容输出。
- 字母表示选用有精确兼容分解的 Unicode 修饰字母，支持 `Bʲ`、`Kᵢⱼᵀ`、`1ˢᵗ`、`2ⁿᵈ`、`3ʳᵈ`、`4ᵗʰ` 等常见记号。字符依据见 [Spacing Modifier Letters](https://www.unicode.org/charts/PDF/U02B0.pdf)、[Phonetic Extensions](https://www.unicode.org/charts/PDF/U1D00.pdf)、[其补充区](https://www.unicode.org/charts/PDF/U1D80.pdf)和 [Latin Extended-C](https://www.unicode.org/charts/PDF/U2C60.pdf)。映射仍是选定字符集：范围外的字母/符号保留回退表示，不更改字母大小写或采用形似字符。

当前边界：没有 Mermaid 图表排版、图片显示、HTML 布局、除上述上下标之外的 HTML 内联标签样式、目录锚点跳转或链接标题悬停提示；其他 HTML 标签和锚点目标仅作为文字显示。GitHub Alert、定义列表、`==高亮==`、Emoji 短码和裸网址自动识别扩展尚未启用。表格固定后的行不参与后置引用补全。字符换行遵循显示宽度，尚未实现所有语言的词边界排版。

## LaTeX 数学公式

渲染库提供可选 `math` feature，`agent-app` 默认启用。`Options { render_math: false, .. }` 可关闭某个渲染器的数学排版；`color: false` 始终按字节保留全部输入。

- 行内使用 `$E=mc^2$`，显示为 `E = mc²`。正文、标题、列表、引用及表格中的行内公式均使用紧凑文本；分数可表示为 `(a)/(b)`。
- 块级使用 `$$ … $$`，支持分多行输入；也支持以 `\[` 开始、`\]` 结束的独立公式块。建议块级定界符独占一行。行内 `\(...\)` 暂未接入。
- 支持常用符号、希腊字母、上下标、分数、平方根和带次数的根、求和/积分、伸缩括号、矩阵、分段条件及部分数学字体。此功能是终端数学排版，不是完整 TeX 文档/宏包引擎。
- 普通 `$5 and $10`、转义美元和代码内的公式保持文字/代码语义；未闭合公式、未知命令、缺少参数及未实现的解析事件保留源码，不静默删去参数。
- 公式状态只保留当前公式。未闭合时及时显示源码；收到结束标记后替换当前区域并提交，不等整个 Agent 回复结束。窗口缩放或超出预览区后固定源码显示，仍保留公式状态直到结束。
- 当前公式缓冲上限为 `min(max_pending_bytes, 4096)`；解析最多 1,024 个事件、32 层嵌套，并在分配二维字符网格前检查保守布局预算（512 列、64 行、8,192 个单元格）。自定义宏定义/展开不在支持范围内。超限保留原文。
- 二维结果宽于可用终端宽度时，优先使用紧凑表示；无法可靠转换时保留源码。终端字体仍会影响数学符号的字形。

本库使用 **pulldown-latex** 严格解析，再由自己的事件适配器构造 **term-maths** 所需的 AST 并排版。`rust-latex-parser` 仅提供该公开 AST 类型；没有调用它的宽松解析入口。这样 `pi` 不会被当成 `\pi`，`a/b` 不会被擅自改成二维分数，`\frac{a}` 等缺参输入会保留原式。

```sh
# 默认 CLI：代码高亮 + 数学
cargo run -p agent-app
# 保留代码高亮，关闭数学依赖
cargo run -p agent-app --no-default-features --features syntax-highlighting
# 查看数学样例，模拟逐字符输入
cargo run -p agent-render-cmd --features math,syntax-highlighting --example preview -- 100 24 1 < input.md
```

## 代码面板与语法高亮

代码块采用完整矩形背景：语言标题栏、两侧内边距、底部留白；空行与视觉换行填满背景。面板样式由 `code/panel.rs` 自主实现。嵌套列表内的代码面板另起一行，并保留所在层级的缩进。没有语言标签时显示 `text`；未知语言也保留面板和原始代码，不猜测语言或执行代码。

- 渲染库的 `syntax-highlighting` feature 默认关闭；`agent-app` 默认启用同名 feature。
- 已编译高亮能力时，`Options { highlight_code: false, .. }` 可为该渲染器关闭高亮。`color: false` 仍按字节透传原始 Markdown。
- 语法定义和 `base16-ocean.dark` 主题通过 `LazyLock` 按进程惰性加载一次。只输出普通 Markdown 或未标记语言的代码时，不加载高亮资源。
- 生产流式代码帧只保存语言及最后一个已提交行的解析/高亮状态，不保存历史代码文本或每行 token 缓存。未完成行从状态副本预览，完整行提交时推进状态一次。
- 代码状态在匹配结束围栏、容器/缩进退出或 `finish` 时释放；窗口变化不会丢掉状态。主题和语法资源继续按进程复用。
- 超过 8 KiB 的代码单行不送入语法解析器；解析失败保留原文和面板，完整行解析失败后该块使用普通代码样式。过长代码块本身不会触发原文回退。
- 已验证 Python、JavaScript、JSON、Rust、C、shell、YAML 和 diff；其他语言由 syntect 内置语法集决定。`bash`/`shell`/`console`、`c++`、`c#`、`yml` 等常用标签做了别名映射。

```sh
# 默认 CLI 启用高亮和数学
cargo run -p agent-app
# 关闭高亮依赖，保留数学；代码仍使用矩形面板
cargo run -p agent-app --no-default-features --features math
# 独立库的 feature 验证
cargo check -p agent-render-cmd --features syntax-highlighting
cargo check -p agent-render-cmd --no-default-features
```

## 文件划分与来源

```text
src/
  config.rs                  显式配置与字符宽度模式（自主实现）
  ansi.rs                    ANSI/真彩色样式和安全文本输出（自主实现）
  code/                      矩形代码面板、syntect 适配与行状态缓存（自主实现）
  math/                      可选数学解析/排版适配（自主实现）
    parse.rs                 pulldown-latex 事件转换为布局 AST
    budget.rs                输入、嵌套和二维布局预算
    compact.rs               行内及窄屏紧凑表示
  markdown/                  pulldown-cmark 适配、布局、定义保存（自主实现）
    supsub.rs                行内上下标标签栈与终端字符表示（自主实现）
    table/cell.rs            带样式单元格、字素折行（自主实现）
  stream/
    frame.rs                 当前预览区的终端行定位（自主实现）
    state/
      mod.rs                 输入行装配和公开 Renderer 接口
      stack.rs               持久容器/叶子帧
      lex.rs                 新行控制语法的局部判定
      accept.rs              依据栈顶推进状态
      leaf.rs                各块的状态数据及结束规则
      emit.rs                各类型的提交与输出
      preview.rs             有界候选预览
      output.rs              ANSI 输出与当前区域重绘
      references.rs          未决引用的有界片段保留与补全
      math.rs                持久公式帧、源码预览与闭合提交
  upstream/
    glow/mod.rs              Glow 自动宽度策略的 Rust 移植
    glamour/block_stack.rs   Glamour 块栈几何计算的 Rust 移植
    glamour/theme.rs         Glamour dark 主题选用字段的 Rust 移植
```

上游模块为私有实现细节，不向使用方暴露 Go 风格 API。后续新增移植代码仍按来源项目划分，不把其他上游或自主实现混入这些目录。

### 直接依赖

- [pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark)：直接使用 Rust 解析器，不是移植。关闭默认 features，仅启用实际使用的解析选项：表格、删除线、任务列表、脚注、数学；通过未解析链接回调识别待补全引用。
- [term-maths](https://docs.rs/term-maths/1.0.0/term_maths/)：可选的 Rust 二维数学布局依赖，关闭默认 features，不启用 crossterm、ratatui、Python 绑定或其未使用的 pulldown-latex feature。直接调用布局 API，不是移植。随附 [MIT 许可](LICENSES/term-maths-MIT.txt)。
- [pulldown-latex](https://docs.rs/pulldown-latex/0.8.0/pulldown_latex/)：可选的严格 LaTeX 事件解析器，默认 features 关闭，不使用 MathML 渲染器。随附 [MIT 许可](LICENSES/pulldown-latex-MIT.txt)。
- [rust-latex-parser](https://docs.rs/rust-latex-parser/0.1.0/rust_latex_parser/)：term-maths 已依赖的 AST 类型，本库显式声明以使用公开类型；不调用其解析函数。随附 [MIT 许可](LICENSES/rust-latex-parser-MIT.txt)，来自其上游仓库（0.1.0 发布包未包含独立许可证文件）。
- [syntect](https://github.com/trishume/syntect)：可选的 Rust 语法高亮依赖，不是 Go 移植。只启用 `default-syntaxes`、`default-themes`、`regex-fancy`，使用内置资源与 Rust 正则后端，关闭 HTML 输出、YAML/plist 文件加载和 Oniguruma 后端。许可见 [Syntect MIT](LICENSES/syntect-MIT.txt)。
- `unicode-segmentation` / `unicode-width`：字素边界和终端显示宽度，均沿用 workspace 已有版本，关闭默认 features。

测试依赖 [vt100](https://docs.rs/vt100/) 只用于验证终端滚屏、颜色和缩放结果，不进入发布程序。

### Glow：实际移植

固定版本：`7b2431d4a82428fb477eb4361e11583e1644e9ba`。

[main.go / validateOptions](https://github.com/charmbracelet/glow/blob/7b2431d4a82428fb477eb4361e11583e1644e9ba/main.go) 中自动宽度的默认 80 列、自动上限 120 列逻辑，移植到 `src/upstream/glow/mod.rs`。适配为接收调用方提供的尺寸；CLI 参数、环境变量、配置、文件读取和 TUI 均未移植。本库还会按物理终端宽度限制布局，以保证重绘范围可计算。

许可证：[Glow MIT](LICENSES/glow-MIT.txt)，保留 Copyright (c) 2019-2024 Charmbracelet, Inc。

### Glamour：实际移植

固定版本：`cf874d7039af3485a38afa7d2e8e87ee42a7bbaa`。

| 原文件 | 本地文件 | 移植与适配 |
|---|---|---|
| [ansi/blockstack.go](https://github.com/charmbracelet/glamour/blob/cf874d7039af3485a38afa7d2e8e87ee42a7bbaa/ansi/blockstack.go) | `src/upstream/glamour/block_stack.rs` | 栈操作、缩进/边距累计和可用宽度；使用显式几何值和饱和算术，未移植 Go buffer 与样式继承 |
| [styles/dark.json](https://github.com/charmbracelet/glamour/blob/cf874d7039af3485a38afa7d2e8e87ee42a7bbaa/styles/dark.json) | `src/upstream/glamour/theme.rs` | 文档边距、列表缩进、引用符号、标题/链接/代码配色；转为 Rust 常量，正文使用终端默认颜色，未移植 Chroma 语法主题 |

许可证：[Glamour MIT](LICENSES/glamour-MIT.txt)，保留 Copyright (c) 2019-2023 Charmbracelet, Inc。

### 设计参考与未移植部分

Glamour 的 [ansi/table.go](https://github.com/charmbracelet/glamour/blob/cf874d7039af3485a38afa7d2e8e87ee42a7bbaa/ansi/table.go) 通过 Go Lip Gloss 排版表格。这里只参考“收集表格单元格，再按可用宽度统一布局”的方法；本库的列宽分配、折行与 pulldown-cmark 事件适配是自主实现，不能标作 Lip Gloss 或 Glamour 表格代码移植。

Glow 的整篇读取流程、Glamour 的 Go Markdown 解析器和整篇缓冲转换、Chroma 高亮器、Lip Gloss、Bubble Tea 均未引入。流式缓存/重绘不来自这两个项目。上游 Go 代码仅在所列范围内翻译为 Rust，没有整库照搬。

发布包含移植代码的产物时应保留 Glow/Glamour 的许可证及来源声明；启用 syntect 时还应保留其对应许可证。

## 验证和本地预览

```sh
cargo check -p agent-render-cmd
cargo run --release -p agent-render-cmd --features syntax-highlighting --example preview -- 80 < input.md
# 120 列、12 行可视区，逐字符喂入
cargo run --release -p agent-render-cmd --features syntax-highlighting --example preview -- 120 12 1 < input.md
```

`preview` 是供人工检查 ANSI 输出的例子，参数为列数、行数、每片段字符数（默认 `80 40 0`，0 表示按完整行）；实际应用同时支持不完整行片段。上面的 `input.md` 为自行提供的 Markdown 文件。

自动化测试、测试夹具和 `measure*` 基准程序已按用户要求删除。此前的构建与测量记录保留在 [PLAN.md](PLAN.md)，不代表当前仍有对应测试入口。
