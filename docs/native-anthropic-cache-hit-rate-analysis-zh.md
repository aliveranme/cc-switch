# Native Anthropic 路径缓存命中率下降分析

本文记录「同一 Anthropic 端点，直连命中率约 99%，经 CC Switch 本地路由后降到约 85%」的
排查过程、机制推演与已实施的修复。目标是把散落的怀疑点收敛成**可判定**的清单，而不是继续
罗列可能性。

## 1. 现象与关键特征

| 项 | 观察 |
|---|---|
| 上游 | **同一个** Anthropic 协议端点（`api_format = "anthropic"`，`needs_transform = false`） |
| 直连 | 缓存命中率 ~99% |
| 经代理 | 缓存命中率 ~85% |
| 形态 | **稳定损失约 15 个百分点**，不是归零，也不是偶发抖动 |

「稳定损失一截」这个形态是全文最重要的判据：它排除「前缀被打断一次」（那是一次性冷启动）
与「完全不命中」（那是 0%），指向**只有部分请求不命中**或**请求被切到另一个缓存桶**。

## 2. 判定基准：Anthropic 的缓存规则

缓存键是**完整前缀的哈希**，层级为 `tools → system → messages`，改动某一层即失效该层及
其后所有层。官方文档 *What invalidates the cache* 给出的失效表（与本文相关的部分）：

| 变更 | tools 缓存 | system 缓存 | messages 缓存 |
|---|---|---|---|
| 工具定义（名 / 描述 / 参数） | 失效 | 失效 | 失效 |
| `tool_choice` | 保留 | 保留 | 失效 |
| 图片增删 | 保留 | 保留 | 失效 |
| thinking 参数（mode / `budget_tokens`） | 视模型 | 视模型 | **恒失效** |
| `output_config.effort` | 视模型 | 视模型 | **恒失效** |
| web search / citations 开关、`speed` | 保留 | 失效 | 失效 |
| 丢弃 thinking 块 | 保留 | 保留 | 从该块起失效 |
| 断点块自身变化 / 超出 5 分钟 TTL | 无命中 | 无命中 | 无命中 |

推论：**缓存按「服务变体」分桶**（模型、1M 上下文、speed 等都是变体维度）。同一前缀在两个
变体下互不命中，且各桶的驱逐策略不同。这正好能产生「稳定部分损失」。

## 3. 结论摘要（按解释力排序）

1. **`context-1m-2025-08-07` 被无条件注入**（A1）——唯一同时满足「代码级确证」与
   「形态吻合」的一条。**已修复**，见第 6 节。
2. **整流器改写历史**（C1/C2）——仅对触发整流的那几轮生效；看日志 `[RECT-*]` 密度。
3. **结构性稀释**（E3）——子代理 / 预热请求低于缓存最小长度，永不命中，纯分母效应。
4. **前缀字节差异**（B1/B2/B3）——只造成一次性冷启动，**不解释**持续的 15pp 缺口。
5. **测量口径**（D）——本场景基本可排除（原生 Anthropic 语义下分母正确）。

## 4. native Claude 路径的实际改写点

代理在 `api_format = "anthropic"` 时**并非逐字节透传**。出站前会经过以下改写；注意哪些是
「跨轮稳定」（只冷启动一次）与哪些是「可能逐轮变化」（持续压制命中率）。

| 位置 | 改写 | 跨轮是否稳定 | 是否可能持续压制 |
|---|---|---|---|
| `forwarder.rs:2007` | `should_send_anthropic_headers` 判定 | — | — |
| `forwarder.rs:2030` | **`anthropic-beta` 头重建**（A1，已修） | 否（旧行为每条加 1M beta） | **是** |
| `forwarder.rs:1296` | `strip_one_m_suffix_for_upstream_from_body` 剥 `[1M]` 后缀（A2） | 稳定 | 一次性 |
| `forwarder.rs:3887` | `prepare_upstream_request_body`（B1/B2） | 稳定 | 一次性 |
| `json_canonical.rs:6` | 递归**按 key 字典序重排**整个 body（B1） | 稳定 | 一次性 |
| `body_filter.rs:68` | 递归删除 `_` 前缀字段（B2） | 稳定 | 一次性 |
| `forwarder.rs:1441` | `normalize_anthropic_messages_for_provider`（no-op） | 稳定 | 无（空操作） |
| `forwarder.rs:3891` | `log_prompt_cache_trace`（仅 DEBUG 日志） | — | 无（只读） |

**关键**：上表里唯一「可能逐轮/逐条变化」的是 `anthropic-beta` 重建。其余改写一旦发生就固定
下来，只让**代理缓存与直连缓存互不命中一次**，不会把命中率长期压在 85%。

## 5. 可能导致命中率下降的原因（按机制分类）

### A 类 · 服务变体切换 —— 预测形态：稳定部分损失 ✅ 最贴合

| # | 机制 | 代码位置 | 解释力 |
|---|---|---|---|
| **A1** | `context-1m-2025-08-07` 无条件注入，把每条 native 请求切到 1M 服务变体的独立缓存池；直连只在客户端声明 1M 时携带该 beta | `forwarder.rs:2030`（旧 `BASE_BETAS`）/ 修复后 `forwarder.rs:3059` | **高** |
| A2 | 模型名 `[1M]` 后缀被剥离，若剥离结果与直连模型串不一致 → 冷启动 + 变体差 | `forwarder.rs:1296`、`model_mapper.rs:149` | 中 |
| A3 | speed / effort 被整流器改写（见 C 类） | 见 C | 中 |

### B 类 · 前缀字节差异 —— 预测形态：一次性冷启动

| # | 机制 | 代码位置 | 说明 |
|---|---|---|---|
| B1 | body 递归按 key 排序（`serde_json` 开启 `preserve_order`） | `forwarder.rs:3887`、`json_canonical.rs:6` | 首轮写入字节 ≠ 直连，此后代理自身稳定 |
| B2 | 递归删 `_` 前缀字段 | `body_filter.rs:68` | 原生 Claude 形状通常空操作 |
| B3 | beta **追加顺序**与客户端不同（旧代码还会前置整串） | 修复后 `forwarder.rs:3059` | 若上游按集合比对则无害；若按字符串则一次性。**未确证** |

B 类只解释「与直连的绝对值差一次」，**不解释稳定 15pp 缺口**。

### C 类 · 整流 / 重试改写历史 —— 预测形态：特定轮次永久重写

| # | 机制 | 代码位置 | 影响 |
|---|---|---|---|
| C1 | thinking 签名整流：删除 `thinking` / `redacted_thinking` 块、去 `signature` | `thinking_rectifier.rs:130` | 触发即从该块起 messages 全部失效，不可逆 |
| C2 | budget 整流：改 `thinking.budget_tokens`、抬 `max_tokens` | `thinking_budget_rectifier.rs:81` | 依官方规则，改 thinking 参数 → messages 恒失效 |
| C3 | 失败重试 / 故障转移 | `forwarder.rs:464` | 重复请求多付 cache-write（分母变大） |

C 类只对「触发了整流的那几轮」生效，但若整流频繁触发，可贡献可观的稳定损失。

### D 类 · 测量口径 —— 预测形态：整体误判

| # | 机制 | 代码位置 | 结论 |
|---|---|---|---|
| D1 | `cacheHitRate = cache_read / (input + cache_creation + cache_read)` | `usage.ts:95`、`UsageHero.tsx:119` | 公式本身正确 |
| D2 | native Anthropic `input_tokens` 已排除缓存 → 分母正确 | `parser.rs:104` | 不会像 OpenAI 格式上游那样分母含 total 而失真 |
| D3 | `message_start` 缓存字段先写、`message_delta` 未覆盖 | `parser.rs:172`–`228` | 纯统计口径，与真实计费无关 |

**D 类在本场景基本可排除**。这是本 fork 历史上（DeepSeek / GLM / Kimi 等 **OpenAI 格式**
端点）几次「看起来命中率暴跌」实为统计口径，但**原生 Anthropic 语义下 85% 大概率是真实数字**。

### E 类 · 结构性（非代理缺陷）—— 预测形态：部分损失

| # | 机制 | 说明 |
|---|---|---|
| E1 | 多个 Claude Code 会话共享同一上游缓存池，互相驱逐（历史 issue #3193） | 与代理无关，但走代理时常被一并观察到 |
| E2 | 5 分钟 TTL 过期；生成耗时也计入寿命 | 由使用模式决定 |
| E3 | 子代理 / 预热请求低于缓存最小长度 → 永不命中 | **纯分母稀释**：子代理占输入约 15% 时，天花板就是 ~85% |

E3 尤其值得先排除：它能在**不改任何前缀**的情况下把命中率封顶在 85% 附近。

### F 类 · 第二序效应（不体现在命中率）

整流 / 重试的**重复 cache-write**、故障转移换池，让实际成本高于命中率所示。排查费用时单列。

## 6. 已实施的修复

**改动**：把 native Claude 路径的 `anthropic-beta` 注入从**无条件**改为**条件式**
（`forwarder.rs:2010`–`2045`）。

- 改动前：只要 `should_send_anthropic_headers`，就给每条请求补上
  `claude-code-20250219` + `context-1m-2025-08-07`，与客户端是否声明 1M 无关。
- 改动后：
  - `context-1m-2025-08-07`：仅在客户端**确有 1M 意图**时保留 / 补齐。意图有两种声明形式，
    取并集——模型名带 `[1M]` 标记（接管写入的 `claude-*-5[1M]` 别名），或客户端自带该 beta。
    用**原始** body 判定，因为随后的模型映射与 `[1M]` 剥离会改写模型名。
  - `claude-code-20250219`：保持注入（原生 Claude Code 每条请求本就携带，去重后等同透传）。
  - 客户端自带的其它 beta：按原顺序逐 token 保留，不增删。

**抽出的纯函数**：

- `client_declared_one_m_context`（`forwarder.rs:3029`）—— 判定 1M 意图；
- `native_anthropic_beta_value`（`forwarder.rs:3059`）—— 构建头值。

**未改动**：Codex→Anthropic 桥路径（`forwarder.rs:2032`–`2045`）本就只在「模拟 Claude Code」
或 `[1m]` 模型时注入，语义已正确。

**验证**：新增 3 个行为钉桩测试并全部通过；`proxy::` 全量 **1615 passed / 0 failed**；
`cargo fmt --check` 干净、`cargo clippy --lib` 无 warning。

## 7. 复测与判定步骤

修复的正确性已由单测锁定，但其**对真实命中率的收益**需要上游 A/B 复测：

1. 开 DEBUG 日志，抓同一会话连续 3–5 轮的 `[CacheTrace]`（`forwarder.rs:3891`）：
   - 若无 1M 声明的会话里出站 `anthropic-beta` **不再含** `context-1m-2025-08-07` → 修复生效；
   - 若 `body_hash` / `system_hash` / `tools_hash` / `messages_hash` 跨轮只按预期增长，而命中率
     仍为 85% → 变量在**请求头之外**，转查 A2 / C / E3；
   - 该行同时打印 `cache_controls=count=N,ttls=...`，可确认客户端 4 个断点是否完整到达上游。
2. 抓一次直连与一次经代理的出站请求做 diff，重点比对 `anthropic-beta` 与 `model` 串。
3. 统计子代理 / 预热请求占比（E3），估算理论的命中率天花板。
4. 观察 `[RECT-001]` / `[RECT-010]` 出现频率（C1/C2）。

### 判定矩阵

| 观察 | 指向 |
|---|---|
| 出站 beta 含 `context-1m` 但直连不含 | **A1**（已修，复测即可） |
| 跨轮 hash 只按预期增长、命中率仍 85% | **A** 或 **E3**，排除 B |
| 日志频繁 `[RECT-001]` / `[RECT-010]` | **C1 / C2** |
| 命中率在**特定轮次**骤降后不恢复 | **C 类** |
| 新会话首轮全 miss、之后接近 100%，但整段平均低 | **E3 / B1**（冷启动摊销） |
| `[CacheTrace] cache_controls=count=4` 稳定 | 断点未被代理破坏 |

## 8. 确认排除项（不必再查）

- **PRE-SEND `cache_control` 注入**：`cache_injector::inject` 只对 Bedrock / DeepSeek 官方端点
  生效（`forwarder.rs:506`–`520`）；桥接注入只走 Codex→Anthropic（`forwarder.rs:1629`）。
  **native 路径不注入**，客户端自带断点原样保留。
- **`cch=` nonce**：`strip_volatile_cch` 只在转换桥运行（`transform.rs:239`–`269`）；native 路径
  不剥离，且官方服务端识别自己的 billing header。
- **`normalize_anthropic_messages_for_provider`**：对 `anthropic` 格式是空操作
  （`claude.rs:314`）。

## 9. 待确认项

- **B3**：上游对 `anthropic-beta` 是「集合比对」还是「字符串比对」？前者则顺序无关；后者则
  任何顺序差异都造成一次性失效。目前无直接证据。
- **A1 的最终确认**：需要一次直连 / 代理出站 diff，证明修复后 beta 已条件化，且命中率回升。
- **E3 的量化**：子代理请求占比需要从会话日志统计，才能判断 85% 是否本就是天花板。

> 立场说明：A1 的修复基于「Anthropic 对 1M 上下文使用独立缓存池」这一机制推演，与
> 「稳定部分损失」的形态高度吻合，且改动本身在语义上是正确的（直连与代理的 `anthropic-beta`
> 应当一致）。但**根因尚未经上游 A/B 复测确证**——若修复后命中率未回升，应回到第 7 节的
> 判定矩阵，优先转向 C 类与 E3。