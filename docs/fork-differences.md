# CC Switch Fork 差异说明

本 fork 由 **aliveranme** 维护，基于上游 [farion1231/cc-switch](https://github.com/farion1231/cc-switch)。
本文档记录本 fork 与上游的**全部实质性差异**，作为代码审查、上游同步（`sync/upstream`）
与回归验证的对照基线。同步合并上游时请重点核对第 4 节的「行为分歧点」。

## 1. 概览

| 项目 | 值 |
|---|---|
| 上游基线 | `42ac174d`（2026-09-14，v3.20.3 之后 1 个提交：#7331 Claude Desktop 3P 配置支持 Linux） |
| 本地领先 | 领先上游的本地提交（fork 全特性 + 历次上游 merge 同步） |
| 本次 merge | 2026-09-15：`42ac174d` 合入 #7331（1 提交，4 文件 +136/-12，全部自动合并、无冲突） |
| 本地版本 | `v3.20.3`（随 merge 对齐上游版本号，无后缀；fork 发布序列见第 5 节） |
| 同步方式 | 定期 `Merge remote-tracking branch 'upstream/main'`，最近一次 2026-09-15（此前已吸收 v3.20.2 `2d54e261` 与 v3.20.3 `bd247a4a`） |
| 测试规模 | Rust 2988（`--lib` 全绿；Windows 本地需隔离 `HOME`，见 6.3）+ 前端 vitest 1121（139 文件全绿） |

## 2. 修改总览（按主题）

### 2.1 Proxy 协议层（核心，src-tauri/src/proxy/，约 +6 200 行）

| 模块 | 差异 |
|---|---|
| **安全分类器协议**（新增 `classifier.rs` ~1 300 行，上游无此文件） | Claude Code security classifier 完整支持：`<block>`/`</severity>` 双模式、fast 单阶段与 both/thinking 双阶段检测、severity 响应转换、裁决提取与 usage 解析。协议特征逐条对照官方 cli.js（2.1.193/2.1.219）逆向确认 |
| **四向格式转换**（transform.rs / transform_codex_chat.rs / transform_codex_anthropic.rs / transform_gemini.rs / transform_responses.rs） | reasoning 全形状提取（含 DeepSeek 内联 think 块）；Anthropic document → Chat/Responses/Gemini；`json_object` 响应格式保留；`disable_parallel_tool_use` ↔ `parallel_tool_calls` 对称传递；非图片工具媒体降级为文本而非丢弃；usage 三线守恒（fresh-input 语义 + saturating_sub） |
| **prefix-cache 稳定性**（transform 系 + forwarder） | 剥离 `x-anthropic-billing-header` 的 rotating `cch=` nonce（`strip_volatile_cch`，逐行字节级确定）；mid-conversation system 重写为 user（见 4.1）；CacheTrace 调试链路（TRACE 级门控） |
| **SSE 流式协议**（streaming.rs / streaming_codex_chat.rs / streaming_gemini.rs / streaming_responses.rs） | 终态必达（EOF sentinel、[DONE] 去重、截断流补 end_turn）；**伪成功防护**（空 delta chunk 后断流/DONE 发 error 而非伪造成功）；错误状态码与 retry-after 透传；`output_text.done`/`refusal.done` 跳 delta 恢复完整文本；whole-JSON 非流式回退（Responses 方向）；转换器 1MB 缓冲上限防 OOM；[DONE] 后残留数据守卫 |
| **路由/嗅探**（handlers.rs / forwarder.rs / content_encoding.rs） | 响应体嗅探（`<=`→`<` 边界修复保流式、未标记 JSON 识别）；content_encoding 双向全支持（gzip/br/zstd/deflate，堆叠编码，200MB 上限）；`cache_injection` 域收敛（见 4.4）；嗅探超时与故障转移联动 |
| **响应体字节上限实现**（v3.19.2 同步） | 方法统一为上游 `bytes_with_limit`（Buffered 变体事后比较 + 流式逐块超限截停 + `ResponseBodyTooLarge` 错误），上限保留 fork 的 `MAX_BUFFERED_PROXY_BODY_BYTES = 200MB`（上游 128MB）；content_encoding 采用上游 `decompress_body_with_limit`（解码器读取侧预算、压缩炸弹在预算处截停、TooLarge 与数据损坏区分） |
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

- JsonEditor / MarkdownEditor 拆分重构（Impl 分离，修复编辑丢数据）；2026-08-16 同步吸收上游 readOnly / ariaLabel / 光标位置保持（minimalTextChange / mapPositionByContext），lazy 加载拆分保留
- 供应商表单提交防 stale/lost（codex/gemini/partners）
- 供应商默认模型回退与预设清理（sponsor 预设调整同步上游）
- AuthCenter 账户用量展示（合入上游 #4887）
- DeepLink 导入支持 `claude-desktop` app（见 4.11）
- Claude Desktop 3P 供应商 profile 默认启用 `chatAdvancedFileAnalysisEnabled`、
  `modelPrefer1mContext`、`skipWebFetchPreflight`、`coworkVmIpv6Enabled`
  （上游无这四个字段；direct 与 proxy 两种模式共用 `build_gateway_profile`，
  两处钉桩测试各断言一次）

### 2.5 发布 / CI

- fork 发布序列 `v3.19.1-a` / `v3.19.1-b`
- `wix.version` 覆盖 MSI ProductVersion 绕过 prerelease 限制
- tag 推送发布正式版而非强制 prerelease
- updater endpoints 指向本 fork 的 GitHub Releases
- 删除 `.github/workflows/claude.yml`；迁移 `tailwind.config.cjs` → postcss
- CI 全绿修复（rustfmt/clippy/前端格式）
- **WSL2 CI job 暂禁用**（2026-08-16）：`backend-windows-wsl2`（Windows+WSL2
  文件系统契约测试）的 link.exe 在 GitHub windows runner 上写 `lnk{}.tmp` 临时
  文件到不存在的 `\\wsl.localhost` UNC 路径（LNK1327 c1010070）。已排除编译顺序
  （编译前置）、进程 TEMP/TMP/GetTempPath（全原生）、manifest 嵌入（`/MANIFEST:NO`
  无效）、target 缓存（全量编译）、runner 版本（windows-2025/latest）等变量；
  `backend-windows`（windows-latest，无 Setup WSL2 步骤）同代码编译通过。属
  GitHub runner 环境异常，`if: false` 暂禁，待修复后恢复（见 ci.yml 注释）。

### 2.6 服务层（用量统计 / 接管）

| 项 | 差异 |
|---|---|
| Claude 会话用量冻结行上推 | 上游 `INSERT OR IGNORE` 短路 → fork `ON CONFLICT` upsert（`data_source='session_log'` 守卫 + `output_tokens` 单调推进），见 4.9 |
| 小时桶累计 | `get_daily_trends` 小时桶越界从"覆盖"改为"累加"（`9d793c51`），修复最后 1 小时用量少算 |
| 接管判定 | 统一收敛到 `AppType::takeover_active` 策略矩阵，见 4.10 |
| 热切换回滚 | live 写失败时回滚 DB 中 current-provider 指针（`7cebd071`，上游只回滚 backup/live） |
| stale backup | 接管/热切换中 stale live backup 不阻塞 Codex/Gemini 供应商写入（`06b57082`/`7efdc361`） |
| v3.20.1 会话扫描重构 | 合入上游增量 byte-cursor 扫描、auto/manual 会话扫描模式、non-append rewrite 检测与 Sync Now 门控；fork 的 Claude upsert 分歧保留（见 4.9），`should_skip_session_insert` 封装继续供 gemini/codex/opencode 使用 |

### 2.7 个人工具链（非上游内容，同步时忽略）

`.agents/skills/cnb-*`（cnb 平台技能集）、`.cnb.yml`、`skills-lock.json`、`assets/readme/*.svg`。

### 2.8 测试

- 单测从上游基线约 2000 增至 **2867**（proxy 协议层每个改动点都有行为钉桩测试）
- 前端 vitest **1022**（新增 codex 预设默认值、universal 预设、TOML 边界等套件）

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

### 4.1 mid-conversation system 重写为 user（prefix-cache 稳定性，已收窄）

- **上游**（`d8065cc6`，#6941 起）：mid-conversation system **原位保留**，不再
  hoist 到头部合并；顶层 system 数组合并为一条 system（跨轮字节稳定）。
- **fork**：上游行为之上**额外把 mid-conversation system 重写为 role=user**。
- **原因**：上游修复只保证 Anthropic→OpenAI 转换自身不再上提；但 fork 的主要
  用户场景是第三方网关（DeepSeek/OpenRouter/Kimi/OpenCode Go 网关等），这些
  OpenAI 兼容上游会**自行把所有 system 消息提升回前缀**——保持 system 角色
  仍会每轮重写前缀、逐出全部缓存 token。重写为 user 使前缀字节级稳定，内容
  留在对话尾部。背景（历史 hoist 问题）：system 前缀是 prefix-cache 的缓存键
  核心，Claude Code 的 Workflow 每轮注入新 reminder 时，任何上提/合并都会
  全价重发；第三方网关 prefix-cache 命中价差 10 倍以上。
- **对应测试**：`test_anthropic_to_openai_preserves_mid_conversation_system_in_place`
  （断言原位 + role=user，注释说明与上游的差异原因）。
- ⚠️ 原定的退出条件"上游改为不 hoist"已部分满足（#6941），但 user 重写在
  网关自行提升 system 的场景下仍有收益，故保留。语义损失仅剩"system 特权"
  差异（指令文本仍在消息里）；四桥行为不一致依旧存在（Claude→Chat 重写 /
  Codex→Chat hoist / Claude→Responses 透传 / Claude→Gemini hoist 进
  systemInstruction）。

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
- **v3.19.2 同步**：`atomic_write` 的 Windows 替换方式合入上游 `ReplaceFileW`
  （原 remove+rename 有"目标文件短暂不存在"窗口，`ReplaceFileW` 原子替换保留 ACL 并处理
  共享冲突）；fork 的 unix 权限语义完整保留——create_new 循环在 unix 上 open 时即 `mode(0o600)`
  （创建即收紧，无 0644 窗口期）+ 后续属主位收紧块原样保留。fork 的
  `retry_transient_io`/`is_transient_reparse_error`（跨卷符号链接 448/183/32 退避重试）因
  生产调用点被 ReplaceFileW 取代，标记 `#[allow(dead_code)]` 保留作 fallback（测试仍在）。

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
- **v3.20.1 同步**（`8927aba5`）：上游会话扫描重构（增量 byte-cursor、auto/manual
  模式）引入 `should_skip_session_insert` 封装；fork 的 Claude upsert 分歧保留，
  仅 gemini/codex/opencode 走 skip 封装；两个 upsert 单测适配
  `insert_session_log_entry_on_conn` 新签名。

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

### 4.13 NativeResponses 模板保留完整能力（gpt-5.6-sol vs 上游中性模板）

- **上游**（40cac1a6）：NativeResponses 用中性模板 `codex_native_responses_template.json`
  （无 apply_patch、web_search，仅 none/high 两档 reasoning），因 MiMo 等网关拒绝
  freeform apply_patch（400）。
- **fork**：NativeResponses 用 `gpt5_6_sol_template.json`（gpt-5.6-sol 全量模板：
  `apply_patch_tool_type: freeform`、`web_search_tool_type: text_and_image`、
  `model_messages`/`tool_mode`/`use_responses_lite` 等全字段、6 档 reasoning
  low…ultra）。
- **原因**：fork 的核心特性是第三方模型 reasoning 滑块（low…ultra，gpt-5.6-sol
  兼容）与 `use_responses_lite=false` 强制；上游中性模板只声明两档会削弱该功能。
  fork 主要面向 DeepSeek/OpenRouter 等支持 freeform apply_patch 的网关，保留
  完整能力收益大于 MiMo 等少数网关的兼容风险（此类网关走 ProxyChat 路径规避）。
- ⚠️ 后果：NativeResponses 直连 MiMo/LongCat 等拒绝 freeform apply_patch 的网关
  可能 400（fork 用户经 ProxyChat 规避）。fork 的 `load_codex_native_responses_template`
  引用 `gpt5_6_sol_template.json`，上游的中性模板文件被 fork 删除；上游若调整模板
  策略需复核此分歧。

### 4.14 map_reasoning_effort passthrough 下 ultra 钳制到 max

- **上游**（40cac1a6）：passthrough 下 `ultra` 原值透传（`Some("ultra")`），依赖
  per-model reasoningLevels 背书。
- **fork**：passthrough 下 `ultra` 钳到 `max`。
- **原因**：严格 OpenAI 兼容上游不认 `ultra`（`400 reasoning_effort: Invalid
  option`，M1 回归）；钳制不损失"最深思考"语义且避免被拒收。deepseek/openrouter/
  low_high 专用模式同样钳到自身最高合法档。
- ⚠️ 上游合并时若恢复透传，M1 回归测试（`responses_request_to_chat_fallback_
  filters_invalid_reasoning_effort`）会失败，需复核。

### 4.15 保留供应商（openai 等）bearer token 注入策略（已移除偏离）

- **状态**：**已对齐上游**（2026-08-30，v3.20.1 合并后按本条预设的退出条件移除）。
- **上游**（cbb79127 config-only 重构后）：`set_codex_experimental_bearer_token` 对
  保留 provider ID **写顶层 `experimental_bearer_token`**（uses-top-level）；
  安全性由 `plan_codex_live_write` 的前置安全门统一把关（第三方密钥 + 无 token 槽位 /
  无密钥回退官方登录均被拒绝）。
- **fork 历史**：曾**显式报错**（`bearer_token_needs_provider` / `bearer_token_reserved_provider`），
  不写无效键。原因：观察到的 Codex 行为是 `experimental_bearer_token` 只存在于
  `[model_providers.<id>]` 表内，顶层写盘被 serde 静默忽略——鉴权注入落空后请求以
  登录态打到第三方 base_url（认证失败且无报错）。
- **移除原因**：上游 v3.20.1 的 `bedrock_runtime_is_a_reserved_provider_id` 测试期望
  保留 ID（amazon-bedrock-runtime）的 prepare 成功且不合成表，与 fork 的 reject 行为
  互斥，合并后 CI 红。上游集成测试 `switch_codex_projects_mcp_despite_broken_claude_json`
  同样依赖无路由场景的顶层 fallback。上游重构已将防御前移到 plan 层安全门，符合本条
  预设的"上游确认后移除"条件。无路由与保留 ID 两个分支均已改为顶层 fallback。
- **对应测试**：`prepare_provider_live_config_uses_top_level_token_for_reserved_provider`
  （正向断言顶层写入）。

### 4.15a custom 表缺失时自动补建并写入 bearer token（fork 保留）

- **上游**：custom `model_provider` 引用存在但 `[model_providers.<id>]` 表缺失时，
  `set_codex_experimental_bearer_token` 写顶层 `experimental_bearer_token`。
- **fork**：自动补建缺失的表并写入表内 token（顶层写入会被当前 Codex 静默忽略，
  不能作为兜底；`model_providers` 不是表时仍报错）。
- **对应测试**：`prepare_provider_live_config_creates_missing_provider_table_for_bearer_token`。

### 4.16 lucide-react 品牌图标（Github 等）用内联 SVG

- **上游**：`lucide-react ^0.542` 导出 `Github` 品牌图标（AuthCenterPanel、
  CopilotAuthSection 使用）。
- **fork**：`lucide-react ^1.31`（#54 升级）移除了 `Github` 品牌导出，改回**内联 SVG**。
- **原因**：fork 依赖版本较新（lucide 1.x 移除品牌图标）。
- ⚠️ 上游若新增 lucide 品牌图标（Github/GitLab 等），fork 需同步改内联 SVG。

## 5. 本地发布序列

| 版本 | 内容 |
|---|---|
| `v3.19.1-a` | CI/发布基础设施修复（tag 推送正式版、wix.version 绕过 prerelease） |
| `v3.19.1-b` | proxy 协议修复收尾（安全分类器、prefix-cache 稳定性、流式终态容错、工具历史恢复会话隔离） |
| `v3.19.2` | 合入上游 v3.19.2（15 提交）+ fork 全特性；字节上限统一为 `bytes_with_limit`（200MB）；content_encoding 解压 bomb 防护；atomic_write Windows 改用 `ReplaceFileW`；版本号与上游对齐（首次无后缀，wix.version 3.19.2.0）；重发补充：接管统一 `ANTHROPIC_AUTH_TOKEN` 占位符避免 Not logged in、官方原生分类器透传 + ALLOW 兜底、分类器检测加固 |
| `v3.19.2-a` | DeepSeek 多模态能力支持（`deepseek-v4-pro` 支持图片输入；`deepseek-v4-flash` 维持纯文本）；同步上游趋势图表点位与 Grok Build 文案修正；wix.version 递增至 3.19.2.1 |
| `v3.20.2` | 合入上游 v3.20.2（`2d54e261`，26 提交）；Grok 走 xAI 原生 Responses 路由、一批 catalog/兼容性修复、预设与定价扩充 |
| `v3.20.3` | 合入上游 v3.20.3（`bd247a4a`）；Kimi 等 Codex 预设改原生 Responses 直连、代理正确性修复、预设与定价维护。**首次发布失败**：标签误指上游提交，Release 跑的是上游工作流（硬校验 `TAURI_SIGNING_PRIVATE_KEY`），5 个平台全部在签名步骤失败、附件为空；把标签改指 fork 提交 `b24deaa9` 后重发成功 |

> 2026-08-16 同步：合入上游 v3.19.2 之后 42 个提交（Pi 原生 coding agent、
> per-model reasoning levels、DeepSeek 官方 catalog mirror、web_search reject
> 黑名单、供应商表单层级重构、IME safe input、路由激活动画等）。fork 版本号保持
> `v3.19.2` 未 bump。fork 全部 4.1–4.14 行为分歧点保留；新增 4.13（NativeResponses
> 模板保留完整能力）与 4.14（passthrough 下 ultra 钳制到 max）。

> 2026-08-28/30 同步：合入上游 v3.20.1（7 提交：会话扫描重构——增量 byte-cursor
> 扫描、auto/manual 模式切换、non-append rewrite 检测、Sync Now 门控——及
> v3.20.1 发版）与 #6941（mid-conversation system 原位保留）。fork 版本号随
> merge 对齐上游 `3.20.1`（未单独发版）。分歧点变化：4.15 移除（bearer token
> 对齐上游顶层 fallback，保留 4.15a custom 表自动补建）；4.1 收窄（上游不再
> hoist，fork 保留 user 重写）；4.9 保留（upsert 适配新签名）；其余 4.x 分歧点
> 经逐条核对全部保留。

> 2026-09-15 同步：合入上游 #7331（Claude Desktop 3P 配置支持 Linux——从**绝对**
> `XDG_CONFIG_HOME` 解析配置根、未设置或相对路径时回落 `~/.config`；CC Switch 自身
> 以 Flatpak 运行时刻意改用宿主机 `~/.config` 而非沙箱私有的 `XDG_CONFIG_HOME`；
> en/zh/ja 用户手册补 Linux 路径与 Flatpak 边界说明）。1 提交 / 4 文件（+136/-12），
> 与 fork 无冲突自动合并；分歧点 4.1–4.16 逐条核对无变化（改动全部落在新的
> `#[cfg(target_os = "linux")]` 分支，未与 fork 的 claude-desktop 3P 逻辑相交）。
> fork 版本号保持 `v3.20.3` 未 bump。

## 6. 维护约定

- **上游同步**：`git fetch upstream && git merge upstream/main`，merge 后跑
  `cargo test --lib`（2867）+ `pnpm vitest run`（1022）+ `cargo fmt/clippy` 全绿再提交。
- **协议修改**：必须先有失败测试（TDD），改动点必须带行为钉桩测试。
- **行为分歧**：凡是有意偏离上游语义的改动，在代码注释中注明理由，并同步本节文档。
- **推送目标**：`origin`（GitHub）+ `cnb`（cnb.cool）双远端。

### 6.1 发布纪律（tag 必须指向 fork 提交）

fork 与上游**共用 tag 名**（`v3.20.x`），两边指向不同提交。因此：

- **绝不推送上游标签**。上游标签一旦进入本仓库并被 `git push --tags` 推上去，
  该 tag 上跑的 Release 会用**上游的 `release.yml`**——它硬校验
  `TAURI_SIGNING_PRIVATE_KEY`，而 fork 仓库不配置任何 Secrets（签名/公证/R2
  全部按"个人使用模式"降级跳过）。结果就是 5 个平台全部在签名步骤失败、
  Release 附件为空。`v3.20.3` 首次发布即为此故障（见第 5 节）。
- upstream 远端已配置 `tagOpt = --no-tags`，`git fetch upstream` 不再拉取上游
  标签；如换机器克隆需重新执行：

  ```bash
  git config remote.upstream.tagOpt --no-tags
  ```

- 发布前必须核对 tag 指向并显式重建（轻量标签，与 `v3.20.0`–`v3.20.2` 一致）：

  ```bash
  git tag -f v3.20.x <fork-main-commit>     # 必须是 fork 提交，不是上游提交
  git push -f origin v3.20.x                # 推送 tag 触发 Release
  git log -1 --oneline v3.20.x              # 复核：提交信息应为 fork 的提交
  ```

- 发布前确认 `package.json` / `src-tauri/Cargo.toml` / `src-tauri/tauri.conf.json`
  三处版本号与 tag 一致（`resolve-version` 作业会硬校验并中止）。
- **手动重跑**：`Actions → Release → Run workflow`（`workflow_dispatch` 从
  `Cargo.toml` 推导 tag，需把 `prerelease` 置为 false 才会发正式版）。

### 6.2 已知缺陷：应用内更新实际不生效（待定夺）

`tauri.conf.json` 的 updater 是启用的（`bundle.createUpdaterArtifacts = true`，
endpoints 指向本 fork 的 `releases/latest/download/latest.json`），但 fork 未配置
`TAURI_SIGNING_PRIVATE_KEY`，构建不产出 `.sig`；而 `assemble-latest-json` 作业只在
签名非空时才写入对应平台，于是每个 fork 版本的 `latest.json` 都是：

```json
{ "version": "3.20.3", "notes": "...", "pub_date": "...", "platforms": {} }
```

`platforms` 为空 → 更新器找不到任何平台的更新，**自动更新静默失效**。
已确认 v3.20.2 与 v3.20.3 均为空，属既有问题（非本次回归）。

两条修复路径（需人工决策，涉及密钥，未擅自执行）：

1. 把与该 `pubkey`（minisign key id `RWTjKDlXmowCyC9Q`）配对的**私钥**配成仓库
   Secret `TAURI_SIGNING_PRIVATE_KEY`（+ 可选 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`），
   构建即产出 `.sig`，清单自动填满——变更面最小，老用户可无缝升级。
2. 重新生成密钥对，并把新公钥写回 `tauri.conf.json` 的 `plugins.updater.pubkey`
   ——会让**已在旧版本的用户无法验证新包**（签名公钥不匹配），仅在私钥确实丢失时使用。

⚠️ 注意 `release.yml` 的构建步骤对签名失败是"吞掉继续"（`|| echo "⚠️ 已忽略"`），
所以配好密钥后仍需复核产物里确实带 `.sig`，否则会再次静默产出空 `platforms`。

### 6.3 Windows 本地跑 Rust 测试必须隔离 `HOME`（2026-09-15 同步时发现）

**直接跑 `cargo test --lib` 会改写真实的 `~/.cc-switch`，并留下夹具状态导致 5 个用例
确定性失败**（现象是 `assertion left: 0, right: 1` + `assert!(state.config.include_common_models)`，
集中出现在 `services::model_pricing::tests` 的 4 个用例与
`services::skill::tests::migrate_storage_safely_leaves_an_existing_pi_ssot_alias`），
**看起来像回归，实为环境污染**。

- **根因**：`get_app_config_dir()`（`src-tauri/src/config.rs` 的 `#[cfg(windows)]` 分支）
  保留了 v3.10.3 兼容回退——若默认目录 `<home>/.cc-switch` 下**没有** `cc-switch.db`，
  而 `$HOME/.cc-switch/cc-switch.db` 存在，则返回后者。测试统一用 `Database::memory()`，
  从不落盘 db 文件，于是该回退恒被触发，`CC_SWITCH_TEST_HOME` 的隔离被绕过。
- **后果**：`model-pricing.json`、`settings.json`、`skills/`、`skill-backups/` 会被测试写入
  真实配置目录，测试执行删除时还会把文件送进用户回收站；其中 `settings.json` 会被覆盖成
  "默认开关 + `skillSyncMethod: auto` / `skillStorageLocation: cc_switch`"（skill 迁移测试
  写入的形态），原始内容在 `~/.cc-switch/backups/*.db` 的 `settings` 表里**不存**
  （该表只有 `*_migrated_v1` / `default_skill_repos_initialized` 等 10 个 key），
  **DB 备份无法还原**。一旦 `model-pricing.json` 带上夹具状态（`includeCommonModels: false`、
  `deletedModelIds: ["claude-sonnet-5"]`），上述 5 个用例就会稳定失败（`claude-sonnet-5`
  被 tombstone 后 seeding 缺失，`UPDATE ... WHERE model_id='claude-sonnet-5'` 影响 0 行）。
- **规避**（已验证 2988 用例全绿）：跑测试时把 `HOME` 指向临时目录，
  使 `$HOME/.cc-switch/cc-switch.db` 不存在，回退不再命中真实目录。

  ```bash
  mkdir -p "$TEMP/ccswitch-iso-probe"
  cd src-tauri && HOME="$TEMP/ccswitch-iso-probe" cargo test --lib
  ```

- **根治方向**（未擅自实施，涉及 Windows 兼容语义与用户数据路径，需人工定夺）：
  测试构建下检测到 `CC_SWITCH_TEST_HOME` 已设置即跳过该回退；或把回退条件从
  `$HOME/.cc-switch` 收紧为"仅在默认目录确实无 db 且 `$HOME` 与真实用户目录不同"。
- **残留清理**：受污染的真实 `~/.cc-switch/model-pricing.json` 内容是纯测试夹具数据
  （`custom-model` 两条 + `deletedModelIds: ["claude-sonnet-5"]`），删除后应用会按默认值
  （`includeCommonModels: true`）重建。`cc-switch.db` 未被测试改写，但 `settings.json`
  已被覆盖且**无备份可还原**（见上），需在应用设置页人工核对托盘／代理／会话自动同步／
  skill 存储位置等开关。
