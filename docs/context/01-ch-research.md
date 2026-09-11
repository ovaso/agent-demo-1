# 01 · CH、上下文增长与有效性原始调研

阶段：原始调研材料。保留原有讨论内容，外部实现描述尚非本项目独立核验结论。

[阅读索引](README.md) · 下一篇：[02 · 压缩与缓存的平衡策略](02-compaction-and-cache.md)

> 后续整理：[上下文压缩与缓存命中率的平衡策略](02-compaction-and-cache.md)。在此基础上的可配置优化方案见[上下文策略与压缩配置设计](03-context-strategy-design.md)，涵盖动态选取、增长式上下文以及独立的轮次 / 水位 / Token 量压缩触发。以下保留原调研材料；其中外部实现与数值不作为本项目实测结论。

如果你说的是 **badlogic/pi-mono 里的 Pi coding agent**，它能把缓存命中率做到非常高，核心不是“缓存做得复杂”，而是它的 **Context 组织方式天然特别适合 prefix cache**。

可以把 Pi 的做法概括成一句话：

> **稳定的东西永远放前面；会变化的东西只往后追加；尽量不改已经发过的 prefix。**

这比单纯“打开 prompt caching”重要得多。

### 1. Pi 的 Agent Loop 基本是 append-only

假设第一轮请求：

```text
System Prompt
Tools
AGENTS.md
Skills

User 1
Assistant 1
Tool Call 1
Tool Result 1
```

第二轮不是重新拼成一个结构发生变化的新 Prompt，而基本是：

```text
System Prompt                  <- 完全一样
Tools                          <- 完全一样
AGENTS.md                      <- 完全一样
Skills                         <- 完全一样

User 1                         <- 完全一样
Assistant 1                    <- 完全一样
Tool Call 1                    <- 完全一样
Tool Result 1                  <- 完全一样

User 2                         <- 新增
```

第三轮：

```text
[上面所有内容完全不变]

Assistant 2
Tool Call 2
Tool Result 2
User 3
```

所以对于服务端 KV / Prompt Cache 来说：

```text
request N+1
=
request N 的完整 prefix
+
一点新 token
```

这简直就是 prefix cache 最理想的输入形态。

Pi 的 `AgentSession` 代码确实就是把新的 user/tool/assistant message 往现有 messages 后面加，而 system prompt 则保留为 `_baseSystemPrompt`，不是每轮随便重建一个不同版本。([GitHub][1])

---

### 2. 所以它的命中率会随着会话长度自然趋近 100%

比如当前已经有：

```text
system/tools/project context:  12k
历史消息:                      80k
本轮新增:                       2k
---------------------------------
总 prompt:                     94k
```

其中前 92k 和上一次完全一致。

那么：

```text
cache hit ≈ 92 / 94
          ≈ 97.9%
```

再聊几轮：

```text
旧 context = 150k
新增 = 2k
```

就是：

```text
150 / 152 ≈ 98.7%
```

所以你看到 Pi 经常：

```text
CH 95%
CH 97%
CH 99%
```

其实并不神秘。

Pi 自己 footer 里的 CH 也是按最近一次请求的缓存使用情况计算；其 usage 明确区分：

```text
input
cacheRead
cacheWrite
```

项目里甚至专门有 cache miss / cache waste 的统计代码。([GitHub][2])

---

## 3. 真正关键的是：System Prompt 非常稳定

Pi 的 system prompt 主要来自：

```text
Pi base prompt
+
tool snippets
+
guidelines
+
SYSTEM.md / APPEND_SYSTEM.md
+
AGENTS.md / CLAUDE.md
+
skills index
+
cwd
+
date
```

这些东西在一个 session 运行期间基本不会变。Pi 构造 system prompt 的代码就是这么组织的。([GitHub][3])

注意它这里甚至有一个很细的设计：

```text
Current date: 2026-09-11
Current working directory: xxx
```

放在 **system prompt 最尾部**。

它不是：

```text
timestamp
random id
runtime stats
system prompt
AGENTS
tools
```

这种非常毁 cache 的结构。

而且 date 只精确到“天”，不是：

```text
2026-09-11 10:32:41.192
```

否则每次请求 system prompt 都变，prefix cache 基本直接报废。

---

## 4. Tool definitions 也必须稳定

这其实是很多 Agent 框架 cache 命中差的根源。

API 实际上传的东西通常类似：

```text
system
tools [
    read {...schema...},
    bash {...schema...},
    edit {...schema...}
]
messages
```

如果你每轮：

```text
动态注册工具
动态修改 description
动态修改 JSON schema
工具顺序发生变化
```

哪怕 messages 完全一样：

```text
tools A,B,C
```

变成：

```text
tools B,A,C
```

prefix 都可能断。

Pi 的工具集合通常在 session runtime 构建时就固定下来。只有明确调用类似 `setActiveToolsByName()` 时，它才会重建 system prompt。源码里甚至明确：

```ts
this.agent.setTools(tools);

this._baseSystemPrompt =
    this._rebuildSystemPrompt(validToolNames);

this.agent.setSystemPrompt(this._baseSystemPrompt);
```

也就是说：

> 修改工具集就是一个明确的 cache-breaking operation。

而不是每轮偷偷变。([GitHub][1])

---

# 5. Provider 层 Pi 又做了一层 cache affinity

这一点很值得你现在做 `vendor-openai / vendor-deepseek` 时借鉴。

Pi 不只是“保证内容一样”。

针对 OpenAI，它还会利用：

```text
sessionId
```

设置 cache affinity。

目前 OpenAI Responses 路径中可以看到：

```text
prompt_cache_key: clampOpenAIPromptCacheKey(options?.sessionId)
```

同时还会带 session affinity：

```text
session_id
x-client-request-id
```

OpenRouter 则是：

```text
x-session-id
```

目的就是：

> 不仅 prompt 一样，还尽量让同一个 session 的请求被路由到能够复用同一份 prefix cache 的路径。

([GitHub][4])

这其实就是：

```text
Content Stability
        +
Cache Affinity
```

两层一起做。

而不是指望 provider 自己碰运气匹配。

---

# 6. Anthropic 又是另一套适配

Pi 没有硬套 OpenAI 的缓存协议。

Anthropic provider 会生成：

```text
cache_control: {
    type: "ephemeral"
}
```

长缓存则支持类似：

```text
ttl: "1h"
```

Pi 对 `cacheRetention` 做了统一抽象：

```text
none
short
long
```

然后 provider 自己翻译。

例如：

```text
Pi abstraction

cacheRetention = long
        │
        ├── Anthropic
        │      cache_control ttl=1h
        │
        └── OpenAI
               prompt_cache_retention=24h
```

这正是你昨天说的 **vendor capability / vendor-specific adaptation** 的典型例子。([GitHub][5])

---

# 7. Compaction 反而是 cache breaker

这个地方很有意思。

假设原来：

```text
System
A
B
C
D
E
F
G
H
```

100k token。

下一轮正常追加：

```text
System
A
B
C
D
E
F
G
H
I
```

几乎全部 cache hit。

但 compact 后：

```text
System
Summary(A-H)
G
H
I
```

Prompt 已经完全改变。

所以：

> **Compaction 和极致 cache hit 本质上有冲突。**

Pi 的策略不是频繁 compact，而是在 context 接近上限：

```text
contextTokens >
contextWindow - reserveTokens
```

才 compact。

默认还会保留相当一部分 recent context。

甚至 Pi 的 compaction 请求会使用新的 routing session ID，并在支持的 provider 上避免为这种“一次性 summary 请求”写无意义 cache。([GitHub][6])

这个设计其实很漂亮：

```text
正常 Agent Loop
    ↓
疯狂吃 prefix cache
    ↓
接近 context limit
    ↓
一次 cache-breaking compact
    ↓
建立新的稳定 prefix
    ↓
继续疯狂 cache hit
```

而不是：

```text
每轮 summarize
每轮重写 context
每轮 RAG 注入不同顺序
每轮 system prompt 动态变化
```

后者缓存基本废了。

---

# 8. 这也是为什么很多“功能更复杂的 Agent”反而缓存很烂

比如一个典型的 Agent 框架每轮这样做：

```text
System
Current Time: 10:31:27

Dynamic Memory
RAG topK results

Current Git Status

Dynamic Tool List

Previous Summary

Conversation
```

下一轮：

```text
System
Current Time: 10:31:34      <- 这里已经不同

Dynamic Memory              <- 又不同
RAG topK                    <- 顺序不同
Git Status                  <- 不同
Tools                       <- 不同

Conversation
```

从极前面的 token 就产生差异。

于是：

```text
cacheable prefix
      ↓
可能只有几百 token
```

后面即使有 100k 一模一样也没用，因为 **prefix cache 是从头匹配的**。

这也是为什么现在有专门针对 Pi 做的 `cache-guardian` extension。它甚至采用所谓的 **golden freeze**：

> 第一轮完整 system prompt 生成后保存一份；后续 turn 强制恢复同一份，避免 extension 每轮注入动态信息导致 byte-level drift。

它还会把最稳定内容尽量前移。作者报告 extension 动态修改 system prompt 时，cache hit 可以从约 75% 直接掉到 0%。([Pi开发][7])

所以你可以把这件事理解成一个非常简单的原则：

```text
             稳定程度

System      ██████████
Tools       ██████████
AGENTS      ██████████
Skills      █████████
History     ████████
Recent      ███
Dynamic     █
             ↓

排列顺序：
最稳定 ─────────────────→ 最不稳定
```

而绝对不要反过来。

---

# 对你现在这个 LLM Gateway，我反而建议直接把这个原则做到协议层

你昨天不是正在做：

```text
vendor-openai
vendor-deepseek

reasoning
reasoning level
thinking output
```

么。

你的通用 request 最好顺便直接引入：

```kotlin
data class CacheConfig(
    val mode: CacheMode = CacheMode.AUTO,
    val retention: CacheRetention = CacheRetention.SHORT,
    val key: String? = null
)

enum class CacheRetention {
    NONE,
    SHORT,
    LONG
}
```

然后：

```text
                     Unified Request
                           │
                 cacheRetention=LONG
                 cacheKey=session-xxx
                           │
       ┌───────────────────┼──────────────────┐
       ↓                   ↓                  ↓
 vendor-openai      vendor-anthropic   vendor-deepseek
       │                   │                  │
prompt_cache_key      cache_control      provider-specific
retention=24h            ttl=1h
```

但是 **Gateway 层最重要的约束并不是这个字段**。

反而应该明确一个：

```text
Prompt Cache Invariant
```

即：

> 同一个 session 内，system / tool schema / stable context 一旦确定，不得因 request runtime state 无意义变化而改变序列化结果。

甚至我会给你的 Gateway 加一个：

```text
stablePrefixHash
```

调试指标：

```text
llm_prompt_prefix_hash
llm_cache_read_tokens
llm_cache_write_tokens
llm_uncached_input_tokens
llm_cache_hit_ratio
```

如果：

```text
Turn 18
prefixHash = abc123
CH = 98.1%

Turn 19
prefixHash = 7ef891
CH = 2.3%
```

你马上知道：

> **不是模型缓存抽风，是自己把 prompt prefix 改了。**

这实际上比 Pi 当前单纯显示 CH 更适合做成你这种统一 Gateway 的可观测能力。

所以 Pi 真正值得抄的不是某个 `cache_control` 参数，而是它这个架构思想：

**append-only context + immutable stable prefix + provider cache affinity + 少 compact + provider-specific cache adapter。**

这几个东西组合起来，才是它为什么在长 Agent 会话里动不动能跑到 **95%～99% cache hit**。([GitHub][8])

[1]: https://github.com/fivewillow/badlogic-pi-mono/blob/main/packages/coding-agent/src/core/agent-session.ts?utm_source=chatgpt.com "badlogic-pi-mono/packages/coding-agent/src/core/agent-session.ts at main · fivewillow/badlogic-pi-mono · GitHub"
[2]: https://github.com/earendil-works/pi/blob/main/packages/coding-agent/src/core/cache-stats.ts?utm_source=chatgpt.com "pi/packages/coding-agent/src/core/cache-stats.ts at main · earendil-works/pi · GitHub"
[3]: https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/src/core/system-prompt.ts?utm_source=chatgpt.com "pi-mono/packages/coding-agent/src/core/system-prompt.ts at main · badlogic/pi-mono · GitHub"
[4]: https://github.com/earendil-works/pi/blob/main/packages/ai/src/api/openai-responses.ts "pi/packages/ai/src/api/openai-responses.ts at main · earendil-works/pi · GitHub"
[5]: https://github.com/earendil-works/pi/blob/main/packages/ai/src/api/anthropic-messages.ts?utm_source=chatgpt.com "pi/packages/ai/src/api/anthropic-messages.ts at main · earendil-works/pi · GitHub"
[6]: https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/compaction.md?utm_source=chatgpt.com "pi/packages/coding-agent/docs/compaction.md at main · earendil-works/pi · GitHub"
[7]: https://pi.dev/packages/pi-cache-guardian?name=deep+research&utm_source=chatgpt.com "pi-cache-guardian · Packages · Pi"
[8]: https://github.com/earendil-works/pi/blob/main/packages%2Fcoding-agent%2FREADME.md?utm_source=chatgpt.com "pi/packages/coding-agent/README.md at main · earendil-works/pi · GitHub"










对，这正是这种设计最大的代价。

**高缓存命中率 ≠ 上下文使用量变少。**

假设模型上下文窗口是 200k：

```text
System + Tools         10k
AGENTS / Skills         8k
历史对话               80k
Tool Result            60k
代码读取                30k
当前任务                5k
---------------------------
总计                  193k
```

即使其中：

```text
cache hit = 185k
cache miss = 8k
```

模型的上下文仍然是 **193k**。

缓存解决的是：

```text
重复 prefill 计算
API input 成本
TTFT
```

而不是：

```text
context window 占用
```

所以 Pi 这种设计并不是“永远不压缩”，而更准确地说是：

> **能不压缩就不压缩，一旦快撞 context window，再进行一次大的 compaction。**

这实际上是一个典型的锯齿形策略：

```text
Context
200k |                         /
     |                       /
160k |                     /     ← compact threshold
     |                   /
120k |                 /
     |               /
 80k |       /------+
     |     /        ↑ compact
 40k |   /
     | /
     +----------------------------> turns
```

比如：

```text
20k
30k
45k
70k
100k
130k
160k
        ↓ compact
55k
65k
80k
110k
...
```

这样其实是在两个目标之间取平衡：

```text
目标 A：尽量保持 prefix 不变
        → cache hit 高

目标 B：不能让 context 无限膨胀
        → 必须 compact
```

---

### 更麻烦的其实还不只是窗口

长上下文还会带来几个副作用。

第一是 **缓存虽然命中了，但 decode 仍然要面对长上下文**。

KV 已经存在，不需要重新做完整 prefill，但模型每生成一个新 token，本质上仍然需要对很长的 KV context 做 attention。

所以大致可以理解：

```text
100k cached context
≠
10k context 的推理速度
```

通常表现就是：

* TTFT 因 cache 得到明显改善；
* 但 output token 的 generation latency 仍可能随着 context 变长而恶化；
* 服务端 KV cache 占用也越来越大。

因此 99% CH 并不代表“100k context 免费了”。

---

第二个问题是 **历史垃圾越来越多**。

Coding Agent 特别明显。

你可能之前执行过：

```text
cat 3000 行日志
git diff
read 整个文件
npm install output
测试失败 stacktrace
grep 结果
```

这些内容在当时有意义。

但是 20 轮以后：

```text
旧 npm log            8k
旧 test log           12k
旧 grep               5k
已经修改掉的源码      15k
旧 git diff           10k
```

全部还在 context。

虽然：

```text
CH = 98%
```

看起来非常漂亮，但实际上模型可能背着 **50k 垃圾上下文** 工作。

这就是为什么单纯追求 CH 是个错误指标。

甚至可能出现：

```text
Agent A
context = 150k
CH      = 98%

Agent B
context = 50k
CH      = 80%
```

B 反而可能：

* 更快；
* 更便宜；
* 注意力更集中；
* 回答质量更稳定。

---

### 第三个问题更隐蔽：信息会过期

例如 coding agent 早期看到：

```java
class Foo {
    fun oldMethod()
}
```

之后 Agent 自己已经把代码改成：

```java
class Foo {
    fun newMethod()
}
```

但是历史 Tool Result 里面还存在旧版本。

于是上下文同时存在：

```text
Foo v1
Foo v2
Foo v3
```

长上下文不是数据库。

模型实际上需要自己判断：

> 哪一个才是现在的真实状态？

这就产生所谓的 **context pollution**。

所以：

> 「多上下文」并不总等于「更多信息」。

有时候是：

> **更多互相冲突的信息。**

---

## 所以我认为真正比较好的设计不是 Pi 的“无限 append”，而是 Epoch

我会把一次 Agent session 分成多个：

```text
Context Epoch
```

例如：

```text
Epoch 1

Stable Prefix
├── System
├── Tools
├── AGENTS
├── Project Context
└── Task

append
append
append
append

context = 140k
        ↓

COMPACT

        ↓

Epoch 2

Stable Prefix
├── System
├── Tools
├── AGENTS
├── Project Context
├── Task Summary
├── Decisions
├── Modified Files
├── TODO
└── Important State

append
append
append
```

Epoch 内：

> **极致追求 prefix stability。**

Epoch 之间：

> **允许主动破坏缓存，重新建立一个干净的 prefix。**

这样更合理。

---

## 甚至不能只有 Summary

普通的：

```text
150k history
   ↓
LLM summarize
   ↓
20k summary
```

其实也有问题。

因为 summary 是有损压缩。

特别是 coding agent，容易丢：

```text
具体文件名
函数名
参数
用户明确约束
失败过的方案
为什么做某个架构决定
某条测试结果
```

所以我更倾向于：

```text
CompactedContext {
    task
    constraints

    decisions[]
    discoveries[]

    filesTouched[]
    filesRelevant[]

    currentState

    completed[]
    todo[]

    failedApproaches[]

    importantToolResults[]

    conversationSummary
}
```

而不是一句：

```text
"用户正在实现一个 OpenAI provider，目前已经完成了……"
```

换句话说：

> **Compaction 应该更像 checkpoint，而不是聊天摘要。**

---

## Tool Result 更应该单独处理

这里还有一个很大的优化空间。

例如：

```text
read_file foo.kt
→ 5000 tokens
```

Agent 修改完以后，又：

```text
read_file foo.kt
→ 5200 tokens
```

没必要把两个版本永久留下。

可以进行：

```text
Tool output lifecycle
```

例如：

```text
HOT
最近几轮完整保留

    ↓

WARM
只保留关键片段 / diff / reference

    ↓

COLD
只保留：
toolCallId
文件路径
hash
简短摘要
```

必要时重新：

```text
read_file
```

毕竟源码本身才是 source of truth。

这其实是很多 Coding Agent 下一步应该优化的方向。

---

## 所以 Cache Hit Rate 不应该成为最终 KPI

如果让你现在这个 Gateway/Agent Runtime 设计指标，我会同时看：

```text
cache_hit_ratio
context_utilization
effective_context_ratio
compaction_count
context_growth_rate
tool_output_tokens
history_tokens
stable_prefix_tokens
```

尤其我会加一个概念：

### Effective Context Ratio

比如：

```text
总 context = 120k

真正当前任务必要信息 = 45k
历史冗余 = 50k
过期 Tool Result = 25k
```

那么：

```text
Effective Context Ratio
= 45 / 120
= 37.5%
```

即使：

```text
Cache Hit = 98%
```

也不值得高兴。

这是：

> **非常便宜地重复使用了一堆垃圾。**

---

## 最终我觉得最好的策略其实是

不是：

```text
疯狂 append
→ 直到撞墙
```

也不是：

```text
每几轮 summarize
→ cache 天天失效
```

而是：

```text
                 ┌─────────────────────┐
                 │ Stable Context Epoch│
                 └──────────┬──────────┘
                            │
                       append-only
                            │
                 CH 90% → 95% → 98%
                            │
                 context 到 70~80%
                            │
                       checkpoint
                            ↓
                   intelligent compact
                            │
                 丢 Tool 垃圾
                 合并历史状态
                 保留明确决策
                 保留用户约束
                            ↓
                ┌─────────────────────┐
                │    New Epoch        │
                └─────────────────────┘
                            │
                 cache 从低重新爬升
```

我甚至会选择 **70%～80% 就开始考虑 compact，而不是等 95% 才动手**。因为还得给当前这一轮的工具调用、模型输出和突发的大文件读取留余量。

所以你指出的问题是对的：**Pi 的极高 CH 是有代价的。**

更准确地说，Agent Context Management 本质上就是三个指标的三角权衡：

```text
             Cache Hit
                /\
               /  \
              /    \
             /      \
            /        \
Context Quality ---- Context Size
```

任何一个做到极致都会伤另外两个。

我认为目前比较成熟的方向不是继续追求 `CH=99%`，而是追求：

> **在一个稳定 Epoch 内把 CH 做到极致，同时通过低频、高质量 checkpoint 控制上下文熵。**

这个思路其实也很适合你现在设计的模型 Gateway：**cache 是 Provider 能力，而 Context Epoch / Compaction 应该属于 Agent Runtime 能力，二者最好不要混成一层。**
