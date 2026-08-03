# CC Switch Fork 差异说明

本 fork 由 **aliveranme** 维护，基于上游 [farion1231/cc-switch](https://github.com/farion1231/cc-switch)。
本文档记录本 fork 与上游的**全部实质性差异**，作为代码审查、上游同步（`sync/upstream`）
与回归验证的对照基线。同步合并上游时请重点核对第 4 节的「行为分歧点」。

## 1. 概览

| 项目 | 值 |
|---|---|
| 上游基线 | `492245dc`（2026-08-03，Codex OAuth 账户用量展示 #4887） |
| 本地领先 | 312 commits（其中非 merge 本地提交约 156 个，其余为上游 merge 同步） |
| 文件差异 | 184 文件，+18 847 / −9 371 |
| 本地版本 | `v3.19.1-b`（fork 发布序列：`v3.19.1-a` → `v3.19.1-b`） |
| 同步方式 | 定期 `Merge remote-tracking branch 'upstream/main'`，最近一次 2026-08-03 |
| 测试规模 | Rust 2434（上游基线约 2000）+ 前端 vitest 592 |

## 2. 修改总览（按主题）

### 2.1 Proxy 协议层（核心，src-tauri/src/proxy/，约 +6 200 行）

| 模块 | 差异 |
|---|---|
| **安全分类器协议**（新增 `classifier.rs` ~1 300 行，上游无此文件） | Claude Code security classifier 完整支持：`<block>`/`</severity>` 双模式、fast 单阶段与 both/thinking 双阶段检测、severity 响应转换、裁决提取与 usage 解析。协议特征逐条对照官方 cli.js（2.1.193/2.1.219）逆向确认 |
| **四向格式转换**（transform.rs / transform_codex_chat.rs / transform_codex_anthropic.rs / transform_gemini.rs / transform_responses.rs） | reasoning 全形状提取（含 DeepSeek 内联 think 块）；Anthropic document → Chat/Responses/Gemini；`json_object` 响应格式保留；`disable_parallel_tool_use` ↔ `parallel_tool_calls` 对称传递；非图片工具媒体降级为文本而非丢弃；usage 三线守恒（fresh-input 语义 + saturating_sub） |
| **prefix-cache 稳定性**（transform 系 + forwarder） | 剥离 `x-anthropic-billing-header` 的 rotating `cch=` nonce（`strip_volatile_cch`，逐行字节级确定）；mid-conversation system 重写为 user（见 4.1）；CacheTrace 调试链路（TRACE 级门控） |
| **SSE 流式协议**（streaming.rs / streaming_codex_chat.rs / streaming_gemini.rs / streaming_responses.rs） | 终态必达（EOF sentinel、[DONE] 去重、截断流补 end_turn）；**伪成功防护**（空 delta chunk 后断流/DONE 发 error 而非伪造成功）；错误状态码与 retry-after 透传；`output_text.done`/`refusal.done` 跳 delta 恢复完整文本；whole-JSON 非流式回退（Responses 方向）；转换器 1MB 缓冲上限防 OOM；[DONE] 后残留数据守卫 |
| **路由/嗅探**（handlers.rs / forwarder.rs / content_encoding.rs） | 响应体嗅探（`<=`→`<` 边界修复保流式、未标记 JSON 识别）；content_encoding 双向全支持（gzip/br/zstd/deflate，堆叠编码，200MB 上限）；`cache_injection` 域收敛（见 4.4）；嗅探超时与故障转移联动 |
| **工具历史恢复**（新增 `codex_chat_history.rs`） | Codex Responses→Chat 桥下按会话恢复 function_call 与 reasoning_content；StoreKey 复合键会话隔离（防串话）+ 512 响应/4096 call 规模上限 |
| **OpenCode Go 网关特化**（claude.rs `is_opencode_go_gateway`） | `opencode.ai/zen/*` 上游**保留** OpenAI 请求体的 cache_control 断点/prompt_cache_key（Go 网关认可），其他 OpenAI 兼容上游维持剥离 |

### 2.2 安全强化

| 项 | 差异 |
|---|---|
| 配置文件权限 | `atomic_write` unix 上**创建时即 0600**（消除"先 0644 后 chmod"窗口期）；既有文件不再沿用旧 0644（统一按凭据文件收紧）；覆盖 codex/claude/grok/opencode/hermes/gemini 全部写路径 |
| 终端启动配置 | `/tmp` 临时配置改用 atomic_write（原 `fs::write` + 事后收紧有窗口期） |
| 错误体 | 上游错误体 JSON 解析失败时截断（500/1024 字节）输出，不泄漏整段 HTML |
| 安全分类器 | 兜底策略与官方不同（见 4.2） |

### 2.3 Codex 配置协议

| 项 | 差异 |
|---|---|
| wire_api 归一化 | `chat`/`chat_completions` → `responses` 迁移（上游 Codex 已移除 Chat wire API，遇 `"chat"` 反序列化报错）；幂等、未知值保留、语法保留式改写 |
| TOML 编辑 | 注释节头 `[x] # comment`、array-of-tables `[[x]]`、空白填充节头 `[ x ]`；前端行扫描与后端 toml_edit 语义对齐 |
| MCP 字段对齐 | `headers`→`http_headers`、`timeout`（秒）→`startup_timeout_ms`（毫秒）、不写 `type` 字段；Hermes SSE 显式 `transport` |
| inline table | `model_providers = { custom = {...} }` 形态全路径支持（`as_table_like_mut`） |
| 空 config 守卫 | 空 live config.toml 不回填擦除已存 TOML；写方向 `(None, None)` 为空操作 |
| per-app 字段 | 代理配置写入不再覆盖各 app（claude/codex/gemini）独立的 max_retries/超时字段 |
| 第三方模型 reasoning 滑块 | 注入 `supported_reasoning_levels`（low…ultra，gpt-5.6-sol 兼容）；对所有走代理/直连网关的 Codex 供应商**强制 `use_responses_lite=false`**（lite 格式会截断工具执行） |
| Gemini `.env` 解析 | 接受 `export ` 前缀（上游解析后 key 带 `export ` 前缀而失效） |

### 2.4 前端 / UI

- JsonEditor / MarkdownEditor 拆分重构（Impl 分离，修复编辑丢数据）
- 供应商表单提交防 stale/lost（codex/gemini/partners）
- 供应商默认模型回退与预设清理（sponsor 预设调整同步上游）
- AuthCenter 账户用量展示（合入上游 #4887）
- DeepLink 导入支持 `claude-desktop` app（见 4.11）
- Claude Desktop 3P 供应商 profile 默认启用 `chatAdvancedFileAnalysisEnabled`（上游无此字段）

### 2.5 发布 / CI

- fork 发布序列 `v3.19.1-a` / `v3.19.1-b`
- `wix.version` 覆盖 MSI ProductVersion 绕过 prerelease 限制
- tag 推送发布正式版而非强制 prerelease
- updater endpoints 指向本 fork 的 GitHub Releases
- 删除 `.github/workflows/claude.yml`；迁移 `tailwind.config.cjs` → postcss
- CI 全绿修复（rustfmt/clippy/前端格式）

### 2.6 服务层（用量统计 / 接管）

| 项 | 差异 |
|---|---|
| Claude 会话用量冻结行上推 | 上游 `INSERT OR IGNORE` 短路 → fork `ON CONFLICT` upsert（`data_source='session_log'` 守卫 + `output_tokens` 单调推进），见 4.9 |
| 小时桶累计 | `get_daily_trends` 小时桶越界从"覆盖"改为"累加"（`9d793c51`），修复最后 1 小时用量少算 |
| 接管判定 | 统一收敛到 `AppType::takeover_active` 策略矩阵，见 4.10 |
| 热切换回滚 | live 写失败时回滚 DB 中 current-provider 指针（`7cebd071`，上游只回滚 backup/live） |
| stale backup | 接管/热切换中 stale live backup 不阻塞 Codex/Gemini 供应商写入（`06b57082`/`7efdc361`） |

### 2.7 个人工具链（非上游内容，同步时忽略）

`.agents/skills/cnb-*`（cnb 平台技能集）、`.cnb.yml`、`skills-lock.json`、`assets/readme/*.svg`。

### 2.8 测试

- 单测从上游基线约 2000 增至 **2434**（proxy 协议层每个改动点都有行为钉桩测试）
- 前端 vitest **592**（新增 codex 预设默认值、universal 预设、TOML 边界等套件）

## 3. 本地新增文件（上游不存在）

```
src-tauri/src/proxy/classifier.rs            # 安全分类器协议（~1300 行）
src-tauri/src/proxy/providers/codex_chat_history.rs  # 工具历史恢复
src-tauri/src/resources/gpt5_6_sol_template.json
src/components/JsonEditorImpl.tsx
src/components/MarkdownEditorImpl.tsx
tests/config/codexProviderPresetDefaults.test.ts
tests/config/universalProviderPresets.test.ts
```

## 4. 与上游的行为分歧点（同步合并时必须核对）

以下为 fork 对上游行为的**有意偏离**。每条给出上游行为、fork 行为、**原因**（
为什么必须偏离）与同步注意。每次 `merge upstream/main` 后逐条复核，防止上游
重构把 fork 的行为覆盖回去（或反之，fork 的改动被误并进上游语义）。

### 4.1 mid-conversation system 重写为 user（prefix-cache 稳定性）

- **上游**：所有 system 消息（含 mid-conversation）hoist 到头部合并。
- **fork**：合并 leading system，mid-conversation system **原地重写为 role=user**。
- **原因**：上游的 hoist 会把**每一次**新增的 mid-conversation reminder 移进
  system 前缀。而 system 前缀正是 prefix-cache 的缓存键核心——前缀里任何一个
  字节变化都会逐出**全部**缓存 token。Claude Code 的 Workflow/Dynamic Workflow
  每轮都可能注入新 reminder，hoist 导致每轮缓存全失效、全价重发。fork 的主要
  用户场景是第三方网关（DeepSeek/OpenRouter/Kimi 等），这些网关的 prefix-cache
  命中价差 10 倍以上，且首 token 延迟随前缀重发线性上升。重写为 user 保持前缀
  字节级稳定。指令强度下降是权衡结果——reminder 的指令文本仍在 user 消息里，
  语义不丢失，只损失"system 特权"。
- ⚠️ 语义损失：操作指令强度下降；且四桥行为不一致（Claude→Chat 重写 /
  Codex→Chat hoist / Claude→Responses 透传 / Claude→Gemini hoist 进 systemInstruction）。
  上游若改为"不 hoist"，本项可整体移除。

### 4.2 安全分类器 fail-open vs 官方 fail-closed

- **官方**（cli.js `uSo`）：无 `<block>` 标签即 BLOCK（fail-closed），不可解析即拦截。
- **fork**：有标签但不可识别 → BLOCK（对齐官方）；**完全无标签 → 启发式解读后默认放行**；
  上游超时/4xx/5xx/JSON 解析失败 → ALLOW。
- **原因**：官方的 fail-closed 假设分类器请求**总能成功**（官方 Anthropic API
  稳定且协议固定）。fork 面向第三方网关：DeepSeek/Kimi/GLM 等对分类器协议
  （`<transcript>` 包裹、`<block>`/`</severity>` 标签、fast 单阶段）的兼容性不可控，
  任一网关异常（超时、4xx、JSON 解析失败、字段被网关改写）都会让**所有工具调用
  被 BLOCK**——agent 完全瘫痪，用户看不到原因也无法继续工作。fork 的取舍：
  上游异常时"记录 + 放行"，宁可少一道安全网，不可让工作流整体不可用。
- ⚠️ 后果：任何上游故障 = 分类器静默关闭。这是 fork 与官方最根本的安全属性分歧，
  已通过 warn 日志留痕；产品文档（README）应明示。

### 4.3 content_filter 语义双向不一致

- **Codex 方向**（chat→responses）：`content_filter` → `incomplete` + `incomplete_details`
  （诚实上报截断，本地提交新增）。
- **Claude Code 方向**：两条路径都掩盖为成功——chat→anthropic 的
  `map_stop_reason`（streaming.rs）与 responses→anthropic 的
  `map_responses_stop_reason`（transform_responses.rs，`"incomplete"` 且
  reason 非 max_output_tokens 时）均映射为 `end_turn`。
- **原因**：两条路径各自忠实于目标客户端的语义模型。Codex（OpenAI Responses
  协议）有 `status=incomplete` 语义，content_filter 必须诚实上报，否则 Codex
  无法区分"回答完成"与"被过滤截断"（截断会静默丢失信息）。Claude Code 的
  stop_reason 枚举（end_turn/max_tokens/tool_use/refusal/ping）**没有**
  content_filter 的等价物；映射 refusal 会把"内容被过滤器截断"错报成"模型拒绝
  回答"（客户端展示错误语义），end_turn 是唯一不引入错误语义的选择。
- ⚠️ 有意设计但有测试钉桩；若 Anthropic 未来新增 content_filter stop_reason，
  应优先映射之。

### 4.4 cache_control 注入域

- **PRE-SEND 优化器**：仅对 Bedrock + DeepSeek 官方 Anthropic 端点
  （`api.deepseek.com/anthropic`）注入。
- **Codex→Anthropic 桥**：对所有 Anthropic 协议上游默认注入，跟随
  `cache_injection` 子开关、**有意绕过优化器总开关**。
- **原因**：PRE-SEND 优化器作用于**任意上游**（body 可能随后被格式转换）——
  cache_control 是 Anthropic 专属字段，注入后若转成 Codex/Gemini 原生格式，
  严格网关会 400 且 NonRetryable 直接失败（本地提交 `bc364191` 移除逃逸分支的
  动机）。桥接路径则相反：`codex_responses_to_anthropic` 仅对 Anthropic 格式
  provider 成立，下游按定义是 Anthropic 协议，cache_control 是标准字段，
  Kimi/GLM/DeepSeek/MiniMax 等官方兼容端点均接受；且 Codex 请求从不携带
  cache_control，不注入则每轮全价重发 system+tools+history（成本与首 token
  延迟双升）。绕过总开关同理：桥接缓存是协议必需而非可选优化，用户要关闭用
  `cache_injection` 子开关（UI 中"缓存断点注入"）。
- ⚠️ 若上游给桥接路径加上自己的注入逻辑，注意双方断点预算（4 BP 上限）叠加。

### 4.5 atomic_write 权限语义

- **上游**：已存在文件沿用其完整权限位（含 group/other），新文件 umask 默认（通常 0644）。
- **fork**：属主位保留，group/other 强制清零；unix 创建时即 0600。
- **原因**：上游的权限语义把**明文 API key** 暴露给同机其他用户——CLI 工具或
  早期版本以 umask 默认 0644 创建的 `~/.claude/settings.json`、
  `~/.codex/config.toml`，在 macOS 默认 home 0755 下可被同机其他用户列目录读取；
  更新时沿用旧 0644 又让收紧永远无法生效。fork 统一按凭据文件对待：属主位保留
  （兼容 0700 等形态），group/other 无任何权限；创建即 0600（消除"先 0644 后
  chmod"窗口期，且 FAT/exFAT 等不支持权限位的挂载上也不存在宽松存在期）。
- ⚠️ 同步时注意：上游若引入新的配置写路径，必须走 `atomic_write`/`harden_secret_file`，
  否则密钥文件会退回 0644。

### 4.6 wire_api 迁移（chat→responses）

- 上游全库只写 `"responses"`，遇存量 `"chat"` 直接报错；fork 在写盘前自动迁移。
- **原因**：上游 Codex 已**移除** Chat wire API——配置里残留 `"chat"` 反序列化
  直接报错、Codex 启动即失败。CC Switch 旧模板、用户手写配置、第三方预设都可能
  残留该值；fork 写盘前自动迁移是唯一出路。迁移保持幂等（无 chat 值逐字节原样
  返回）、只改写 `chat`/`chat_completions`（未知值保留）、语法保留式改写
  （注释/格式不破坏）。
- ⚠️ 对"仅支持 Chat Completions 且不走 fork 代理"的存量端点，迁移后直连会失败
  （可预期的一次性错误；fork 以本地 Responses→Chat 转换兜底）。

### 4.7 severity 模式（2.1.219 新增协议）

- fork 支持 `</severity>` severity 响应转换（剥离多余标签保证恰好一个 `<severity>`，
  上游文本以 `1000` 表达 BLOCK）。
- **原因**：Claude Code 2.1.219 引入 severity 分类模式——Piy 解析器按数值阈值
  比较 `<severity>` 输出（0 ≤ 合法值 ≤ 100）。fork 要支持新版客户端，必须识别
  severity 请求并产出 severity 响应；`1000` 表达 BLOCK（> 任意合法阈值，语义上
  等价于最大值 100 但更醒目）；剥离额外标签是防御性保守行为（解析器取第一个匹配，
  多余标签无害但可能触发校验）。
- ⚠️ 该模式在本仓库的 cli.js 副本（2.1.193）中不存在，取值 `1000` 与"恰好一个"
  假设基于 2.1.219 实测——若客户端对数值做 0-100 范围校验，需改为 `100`（语义等价）。

### 4.8 工具历史恢复的会话键

- fork 的 `enrich_request` 显式接收 `extract_session_id` 的结果（含 `codex_` 前缀），
  与记录侧同源；客户端请求体的裸 `metadata.session_id` **不是**恢复键（裸值不命中）。
- **原因**：`extract_session_id` 给不同 app 的会话加前缀（`codex_`/`grokbuild_`…）
  避免跨 app 串话。fork 修复前 `enrich_request` 从请求体读**裸**值，与记录侧的
  `codex_` 前缀键永不相等——工具历史恢复在生产路径**整体静默失效**（F1，隔离
  确实防了串话，但把功能整个关掉了）。显式传参让记录/恢复两侧永远同源：调用方
  （forwarder）持有与记录侧（ctx.session_id）完全相同的值。
- ⚠️ 上游若新增 record/enrich 调用点，必须复用同一 session 来源，否则恢复静默失效。

### 4.9 Claude 会话用量"冻结行"原地上推（session_usage.rs）

- **上游**：`INSERT OR IGNORE` + request_id 存在即短路。
- **fork**：`ON CONFLICT(request_id) DO UPDATE` 上推为完成态，带两道守卫：
  `data_source='session_log'`（绝不覆盖代理实时记的 `proxy` 行）+ `output_tokens`
  严格单调递增（绝不回退）。
- **原因**：Claude 会话**中途崩溃/强杀**时，会话行已落库但停在中间态（无
  output_tokens）；会话恢复后完成事件到达，被上游的 `INSERT OR IGNORE` 短路丢弃
  ——用量统计**永久偏低**（该会话的 output 永远记为 0）。fork 改为 upsert 上推，
  守卫保证：只推 `session_log` 源（代理实时记的 `proxy` 行是权威值，不得覆盖）、
  只前进不回退（并发乱序时取最大值）。
- ⚠️ merge 上游时若被恢复成 `INSERT OR IGNORE`，冻结行会再次停在中间态（本地提交 `026f6634`）。

### 4.10 接管（takeover）判定策略（app_config.rs / services/proxy.rs / live.rs）

- **上游**：`get_takeover_status` 简单布尔，无 per-app 语义。
- **fork**：统一收敛到 `AppType::takeover_active(has_backup, live_taken_over)`：
  - Claude / ClaudeDesktop：**存在 backup 即视为接管**（switch 语义）；
  - Codex / Gemini：需 **backup 且 live 指向代理** 才算接管（stale backup 不阻塞写入）。
- **原因**：Claude 的切换是"写 live 配置"（switch 语义）——backup 存在即说明
  被 CC Switch 接管过，live 里的内容就是代理写的。Codex/Gemini 是 **additive
  模式**：live 配置里可能只是用户自己写的内容或历史残留，backup 存在**不代表**
  当前被接管——按 Claude 的判据会把 Codex/Gemini 误判为已接管，导致：供应商
  写入被错误阻塞、接管状态 UI 误报、切换回显错误。加上 `live_taken_over`
  （live 指向代理）条件才准确。配套：live 写失败时回滚 DB 中 current-provider
  指针（`7cebd071`，热切换不留下半状态）。
- ⚠️ 这是 merge 上游时最容易被覆盖回去的行为，合入上游新代码前先核对 `takeover_active` 调用点。

### 4.11 DeepLink 支持 `claude-desktop` 导入（deeplink/parser.rs）

- **上游**：parser 白名单不含 `claude-desktop`，解析阶段直接拒绝。
- **fork**：放行 `claude-desktop` 并走 Claude settings 形态合并
  （`claude_desktop_mode=Direct`）；第三方可构造
  `ccswitch://v1/import?resource=provider&app=claude-desktop` 导入。
- **原因**：Claude Desktop 是 CC Switch 的核心管理对象（3P 供应商配置、
  代理切换都支持），`claude-desktop` 是合法 app 标识。上游白名单遗漏导致
  UI/外部生成的 deeplink 导入被解析阶段拒绝——用户从分享链接导入配置直接失败，
  且错误提示不说明原因。放行后走与 Claude 相同的 settings 形态合并（Direct 模式）。
- ⚠️ 新增的公开入口能力，上游白名单若收紧会静默丢失该入口。

### 4.12 Hermes 表单字段删除语义（hermes_config.rs `HERMES_UI_OWNED_KEYS`）

- **上游**：`set_provider` 无条件 carry-over 磁盘旧字段。
- **fork**：表单自有字段（api_key/models 等，`HERMES_UI_OWNED_KEYS` 排除表）
  在 payload 缺席时**不再从磁盘复活**。
- **原因**：Hermes 配置是"磁盘为准 + 增量合并"模型。上游无条件 carry-over 的
  后果：用户在 UI 里**清空 api_key / 删除 model** 后，下次保存/重启时旧值从磁盘
  复活——删除操作静默失效，用户以为删了其实没删（密钥泄露风险与困惑并存）。
  fork 用排除表区分"表单自有字段"（payload 缺席即删除）与"第三方/外部管理字段"
  （继续 carry-over），让 UI 删除真正生效。
- ⚠️ 上游合并时若去掉排除表，UI 删除操作会再次失效（本地提交 `63632ade`）。

## 5. 本地发布序列

| 版本 | 内容 |
|---|---|
| `v3.19.1-a` | CI/发布基础设施修复（tag 推送正式版、wix.version 绕过 prerelease） |
| `v3.19.1-b` | proxy 协议修复收尾（安全分类器、prefix-cache 稳定性、流式终态容错、工具历史恢复会话隔离） |

## 6. 维护约定

- **上游同步**：`git fetch upstream && git merge upstream/main`，merge 后跑
  `cargo test --lib`（2434）+ `pnpm vitest run`（592）+ `cargo fmt/clippy` 全绿再提交。
- **协议修改**：必须先有失败测试（TDD），改动点必须带行为钉桩测试。
- **行为分歧**：凡是有意偏离上游语义的改动，在代码注释中注明理由，并同步本节文档。
- **推送目标**：`origin`（GitHub）+ `cnb`（cnb.cool）双远端。
