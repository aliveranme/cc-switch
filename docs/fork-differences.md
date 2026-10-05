# CC Switch Fork 差异说明

本 fork 由 **aliveranme** 维护，基于上游 [farion1231/cc-switch](https://github.com/farion1231/cc-switch)。
本文档记录本 fork 与上游的**全部实质性差异**，作为代码审查、上游同步（`sync/upstream`）
与回归验证的对照基线。同步合并上游时请重点核对第 4 节的「行为分歧点」。

## 1. 概览

| 项目 | 值 |
|---|---|
| 上游基线 | 4804b723（2026-10-05，**38 个提交**；明细见第 5 节；上一基线 372b1698，2026-10-03） |
| 本地领先 | **426** 个提交（`git rev-list --count upstream/main..main`，统计于 merge 提交；本次补记提交再加 1） |
| 本次 merge | 2026-10-05 合入上游 4804b723（38 个提交，**116 文件 +5 222 / −763**，4 处冲突手工解，见第 5 节）；上一次 2026-10-04 372b1698（98 个提交，17 处冲突） |
| 本地版本 | `v4.0.1`（随 merge 对齐上游 v4.0.0/v4.0.1，三处版本号同步 bump；fork 发布序列见第 5 节） |
| 同步方式 | 定期 Merge upstream/main (…, N commits) into fork，最近一次 2026-10-05 |
| 测试规模 | 最近验证于 2026-10-05：Rust **3454** 单元 + **179** 集成（16 个集成测试二进制，其中金标 39）+ main 0；前端 vitest **2161**（186 文件） |

## 2. 修改总览（按主题）

### 2.1 Proxy 协议层（核心，src-tauri/src/proxy/，约 +6 200 行）

| 模块 | 差异 |
|---|---|
| **安全分类器协议**（新增 `classifier.rs` ~1 300 行，上游无此文件） | Claude Code security classifier 完整支持：`<block>`/`</severity>` 双模式、fast 单阶段与 both/thinking 双阶段检测、severity 响应转换、裁决提取与 usage 解析。协议特征逐条对照官方 cli.js（2.1.193/2.1.219）逆向确认 |
| **四向格式转换**（transform.rs / transform_codex_chat.rs / transform_codex_anthropic.rs / transform_gemini.rs / transform_responses.rs） | reasoning 全形状提取（含 DeepSeek 内联 think 块）；Anthropic document → Chat/Responses/Gemini；`json_object` 响应格式保留；`disable_parallel_tool_use` ↔ `parallel_tool_calls` 对称传递；非图片工具媒体降级为文本而非丢弃；usage 三线守恒（fresh-input 语义 + saturating_sub） |
| **prefix-cache 稳定性**（transform 系 + forwarder） | 剥离 `x-anthropic-billing-header` 的 rotating `cch=` nonce（`strip_volatile_cch`，逐行字节级确定）；mid-conversation system 重写为 user（见 4.1）；CacheTrace 调试链路（TRACE 级门控） |
| **缓存链路可配置**（claude.rs / forwarder.rs / classifier.rs / codex_config.rs，见 4.17–4.19） | 会话级 `prompt_cache_key` 路由从 Codex 扩展到 Claude→Chat（复用 `promptCacheRouting` 三态）；`sessionAffinityHeader`（值 = 客户端会话 ID，多实例网关缓存亲和）；`preserveCacheControl`（网关层实现断点的上游）；`midConversationSystemPolicy`（4.1 策略可选）；`classifierSeverityBlockValue`（4.7 数值可配）；`codexNativeResponsesTemplate`（4.13 模板可选） |
| **SSE 流式协议**（streaming.rs / streaming_codex_chat.rs / streaming_gemini.rs / streaming_responses.rs） | 终态必达（EOF sentinel、[DONE] 去重、截断流补 end_turn）；**伪成功防护**（空 delta chunk 后断流/DONE 发 error 而非伪造成功）；错误状态码与 retry-after 透传；`output_text.done`/`refusal.done` 跳 delta 恢复完整文本；whole-JSON 非流式回退（Responses 方向）；转换器 1MB 缓冲上限防 OOM；[DONE] 后残留数据守卫 |
| **路由/嗅探**（handlers.rs / forwarder.rs / content_encoding.rs） | 响应体嗅探（`<=`→`<` 边界修复保流式、未标记 JSON 识别）；content_encoding 双向全支持（gzip/br/zstd/deflate，堆叠编码，200MB 上限）；`cache_injection` 域收敛（见 4.4）；嗅探超时与故障转移联动 |
| **响应体字节上限实现**（v3.19.2 同步） | 方法统一为上游 `bytes_with_limit`（Buffered 变体事后比较 + 流式逐块超限截停 + `ResponseBodyTooLarge` 错误），上限保留 fork 的 `MAX_BUFFERED_PROXY_BODY_BYTES = 200MB`（上游 128MB）；content_encoding 采用上游 `decompress_body_with_limit`（解码器读取侧预算、压缩炸弹在预算处截停、TooLarge 与数据损坏区分） |
| **工具历史恢复**（新增 `codex_chat_history.rs`） | Codex Responses→Chat 桥下按会话恢复 function_call 与 reasoning_content；StoreKey 复合键会话隔离（防串话）+ 512 响应/4096 call 规模上限 |
| **OpenCode Go 网关特化**（claude.rs `is_opencode_go_gateway`） | `opencode.ai/zen/*` 上游**保留** OpenAI 请求体的 cache_control 断点/prompt_cache_key（Go 网关认可），其他 OpenAI 兼容上游维持剥离 |

> 2026-10-03 同步：Claude→Chat 转换路径的流首 inline ` thinking`/`<thinking>` 剥离改用上游
> `InlineThinkSplitter`（#7741）实现；fork 的终态必达（截断流补 `end_turn`）、转换器缓冲上限
> （1MB）、`delta.refusal` 映射、多 `[DONE]` 防护叠加在其上（差异与测试改写在 4.20）。
> `services/proxy.rs` 的 live 写入整体改由上游 `mode`/`live` 引擎接管，fork 在此文件的旧特化
> （接管回滚、stale backup）随之移除（见 4.10 与第 5 节记录）。

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
- **vitest 5 的类型适配**（2026-09-17）：vitest 5 把 `Assertion` 的签名从
  `Assertion<T = any>` 改为 `Assertion<R, T>`（两个必需类型参数），而
  `@testing-library/jest-dom` 7.0.1 的 `declare module 'vitest'` 声明合并因
  类型参数列表不一致**静默失效**——jest-dom 匹配器全部从 `Assertion` 上消失，
  `typecheck` 报 600+ 处 TS2339（运行时不受影响）。新增
  `tests/vitest-jest-dom.d.ts` 按新签名补齐；同时 `vitest.config.ts` 的
  `__dirname` 改为 `import.meta.dirname`（vitest 5 起 Vite config loader 不再
  注入 `__dirname`）。⚠️ jest-dom 上游适配 vitest 5 后应删除该声明文件
- **Tailwind v3(上游) → v4(fork) 的配置桥接**（2026-10-04）：fork 已删
  `tailwind.config.cjs` 改跑 Tailwind v4（`@tailwindcss/postcss`），而上游仍用
  v3 + `tailwind.config.cjs`。**每次同步必须把上游新增的 config 主题项搬进
  `src/index.css` 的 `@theme`**，否则上游新写的 `bg-surface` / `text-fg-2` /
  `rounded-panel` 这类工具类会静默失效（不报错、只是没样式）。本轮搬迁：v7
  设计令牌（色板 / radius `control|panel|dialog` / `shadow-v7-*` / `text` 角色 /
  中日字体栈）、v3 `borderColor.DEFAULT` → base 层 `* { border-color:
  hsl(var(--border)) }`、`tailwindcss-animate` → `tw-animate-css`（官方 v4 版，
  `@utility` 实现才能生成 `data-[state=*]:animate-in` 变体）。详见 4.26
- **React 19 类型适配**（2026-10-04）：上游按 React 18 写，fork 跑 React 19 +
  `@types/react` 19。同步后 typecheck 常报 `RefObject<T>` vs
  `RefObject<T | null>`、`JSX` 全局命名空间消失、lucide 导出改名。本轮修正：
  `SwitchModePanel.scrollRef` / `SessionReaderHeader.{findInputRef,findButtonRef}`
  放宽为 `RefObject<T | null>`、`ProviderCardActions` 的 children 改用
  `DisabledReasonProps["children"]`、`AppsPage.test` 改用 `React.JSX.Element`

### 2.5 发布 / CI

- fork 发布序列 `v3.19.1-a` / `v3.19.1-b`
- `wix.version` 覆盖 MSI ProductVersion 绕过 prerelease 限制（**2026-10-05 移除**：
  该覆盖是 v3.19.2-a 为绕过 prerelease 后缀无法作 MSI ProductVersion 而加的
  `"version": "3.19.2.1"`，此后一直未随版本递增，MSI ProductVersion 被冻结在
  3.19.2.1。v4.0.1 起与上游一致去掉该字段，ProductVersion 回到由 `version`
  推导的 `4.0.1.0`；若将来再发带 `-a` 后缀的 tag，需要临时加回）
- **CI 基建随上游对齐**（2026-10-05）：`backend` 矩阵改用 `Swatinem/rust-cache`
  （只缓存依赖、`shared-key: backend` 与 WSL2 job 共享）+ `cargo-nextest`
  （测试超时见 `src-tauri/.config/nextest.toml`）+ 一律从 `rust-toolchain.toml`
  读编译器版本；push 到 main 也按路径过滤 + 每周日全量跑（缓存被淘汰后重建）。
  fork 保留自己的 `--all-targets` clippy、actions 版本（checkout@v7 / cache@v6 /
  setup-node@v7）与 WSL2 步骤顺序（Setup WSL2 仍在编译前置步骤之后）
- tag 推送发布正式版而非强制 prerelease
- updater endpoints 指向本 fork 的 GitHub Releases
- 删除 `.github/workflows/claude.yml`；迁移 `tailwind.config.cjs` → postcss
- CI 全绿修复（rustfmt/clippy/前端格式）
- **Linux 资产查找路径统一为 `src-tauri/target/release/bundle`**（2026-09-17 修复，
  回归上游写法）：此前 fork 给 arm64 分支加了交叉编译路径
  `target/aarch64-unknown-linux-gnu/release/bundle`，但 `ubuntu-22.04-arm` 是**原生**
  ARM64 runner、构建步骤不带 `--target`，产物与 x86_64 同落 `target/release/bundle`
  ——于是 `Linux-arm64.AppImage` 在**每次发布中静默缺失**（同段的 `.deb`/`.rpm` 走
  写死路径所以正常，v3.20.3 上 arm64 的 deb/rpm 存在而 AppImage 不存在即为此故）。
  同时把 AppImage 缺失的提示从 stderr 提升为 `::error::` 注解，使发布时可见。
- **WSL2 契约测试（`ci.yml` 的 `backend-windows-wsl2`）保持启用，夜间全量（`wsl2-nightly.yml`）仍 `if: false`**
  （2026-08-16 暂禁，2026-10-05 复核仍保留）：当年夜间全量的 link.exe 在
  GitHub windows runner 上写 `lnk{}.tmp` 临时文件到不存在的 `\\wsl.localhost`
  UNC 路径（LNK1327 c1010070）。已排除编译顺序（编译前置）、进程
  TEMP/TMP/GetTempPath（全原生）、manifest 嵌入（`/MANIFEST:NO` 无效）、target
  缓存（全量编译）、runner 版本（windows-2025/latest）等变量；`backend-windows`
  （windows-latest，无 Setup WSL2 步骤）同代码编译通过。属 GitHub runner 环境异常。
  **2026-10-05**：上游 `e4960bba` 已把该 nightly 重构成 lib suite（跳过 `database::`
  与集成测试、`cargo nextest` 逐条跑 + 每测试超时）以绕开 9P 限制，现已随合并入库；
  fork 保留 `if: false`，待删掉该行跑一次 `workflow_dispatch` 复验后再恢复
  （见 wsl2-nightly.yml 的注释）。`ci.yml` 里每个 PR 的 WSL2 契约测试不受影响。

### 2.6 服务层（用量统计 / 接管）

| 项 | 差异 |
|---|---|
| Claude 会话用量冻结行上推 | 上游 `INSERT OR IGNORE` 短路 → fork `ON CONFLICT` upsert（`data_source='session_log'` 守卫 + `output_tokens` 单调推进），见 4.9 |
| 小时桶累计 | `get_daily_trends` 小时桶越界从"覆盖"改为"累加"（`9d793c51`），修复最后 1 小时用量少算 |
| 接管判定 | 统一收敛到 `AppType::takeover_active` 策略矩阵，见 4.10。**2026-10-03 同步后：上游新 `mode` 引擎接管该决策，该矩阵已无生产调用点（仅剩单测）** |
| 热切换回滚 | live 写失败时回滚 DB 中 current-provider 指针（`7cebd071`，上游只回滚 backup/live）。**2026-10-03 同步后：由上游 `mode::controller` 的 settled/staged 写入与指针提交取代** |
| stale backup | 接管/热切换中 stale live backup 不阻塞 Codex/Gemini 供应商写入（`06b57082`/`7efdc361`）。**2026-10-03 同步后：旧接管判定一并移除** |
| v3.20.1 会话扫描重构 | 合入上游增量 byte-cursor 扫描、auto/manual 会话扫描模式、non-append rewrite 检测与 Sync Now 门控；fork 的 Claude upsert 分歧保留（见 4.9），`should_skip_session_insert` 封装继续供 gemini/codex/opencode 使用 |

### 2.7 个人工具链（非上游内容，同步时忽略）

`.agents/skills/cnb-*`（cnb 平台技能集）、`.cnb.yml`、`skills-lock.json`、`assets/readme/*.svg`。

### 2.8 测试

- 单测从上游基线约 2000 增至 **3440** 单元 + **179** 集成（proxy 协议层每个改动点都有行为钉桩测试）
- 前端 vitest **2117**（183 文件；含 codex 预设默认值、universal 预设、TOML 边界等套件）

## 3. 本地新增文件（上游不存在）

```
src-tauri/src/proxy/classifier.rs            # 安全分类器协议（~1300 行）
src-tauri/src/proxy/providers/codex_chat_history.rs  # 工具历史恢复
src-tauri/src/resources/gpt5_6_sol_template.json
src/components/JsonEditorImpl.tsx
src/components/MarkdownEditorImpl.tsx
tests/config/codexProviderPresetDefaults.test.ts
tests/config/universalProviderPresets.test.ts
tests/vitest-jest-dom.d.ts                   # vitest 5 × jest-dom 类型桥接（上游适配后可删）
```

## 4. 与上游的行为分歧点（同步合并时必须核对）

以下为 fork 对上游行为的**有意偏离**。每条给出上游行为、fork 行为、**原因**（
为什么必须偏离）与同步注意。每次 `merge upstream/main` 后逐条复核，防止上游
重构把 fork 的行为覆盖回去（或反之，fork 的改动被误并进上游语义）。

### 4.1 mid-conversation system 重写为 user（prefix-cache 稳定性，已收窄）

- **上游**（`d8065cc6`，#6941 起）：mid-conversation system **原位保留**，不再
  hoist 到头部合并；顶层 system 数组合并为一条 system（跨轮字节稳定）。
- **fork**：上游行为之上**额外把 mid-conversation system 重写为 role=user**；
  该行为**可由 provider meta `midConversationSystemPolicy` 切换**（`"rewrite_user"`
  默认 / `"preserve"` 保留 system 角色，见 4.17）。
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
  上游超时/4xx/5xx/JSON 解析失败 → ALLOW。severity 模式的 BLOCK 数值可由 provider meta `classifierSeverityBlockValue` 覆盖（默认 1000，见 4.18）。
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
- **golden 模式快照必须按 fork 行为维护**（2026-10-05）：`atomic_write` 在 unix 上
  一律 0600（凭据文件语义），因此 `tests/golden/snapshots/modes/*.txt` 中所有条目
  都是 `600`。上游同一位置的快照按 umask 默认写成 `644`（如
  `fresh-after-third-party.txt` 里 `.codex/cc-switch-model-catalog.json` 与
  `.gemini/settings.json`），直接取上游版会让 macOS/Linux 的 CI 在
  `file_modes::switch_creates_expected_files_and_modes` 上红（Windows 无 unix
  权限语义，不会发现）。⚠️ 每次同步后若该快照被上游更新，须按 fork 行为重生成
  （可用 CI 的 `--- actual ---` 输出校对），不能取上游版；与 4.27 的 Gemini 快照同理。

### 4.6 wire_api 迁移（chat→responses）

- 上游全库只写 `"responses"`，遇存量 `"chat"` 直接报错；fork 在写盘前自动迁移。
- **状态（2026-10-03）**：已由上游新引擎取代——`live/project/codex.rs` 在 route 投影时统一写
  `wire_api = "responses"`，上游自有测试 `legacy_shapes_become_the_custom_route` 覆盖该保证；
  fork 的 `migrate_codex_wire_api_in_toml` / `normalize_codex_wire_api` 随旧写入路径删除。
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

> **状态（2026-10-03）**：已由上游新架构取代——`mode::controller` 用显式 mode 状态
> （`mode/state`/`mode/current`）与 `reject_unsupported_official` 决定写入路径，不再依赖
> “backup 存在即接管” 的启发式；`AppType::takeover_active` 已无生产调用点（仅剩
> `app_config.rs` 的单测与 `takeover_active_policy_matrix`），待后续清理。原文保留供参考。

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

> **状态（2026-10-03）**：随 4.19 一并转出——`CodexResponsesTemplate` 与模板选择参数链已随旧
> live 写入路径删除；`gpt5_6_sol_template.json` 仍入库但当前无生产引用
> （上游新引擎默认使用 `codex_native_responses_template.json`）。原文保留供回植参考。

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
  完整能力收益大于 MiMo 等少数网关的兼容风险（此类网关**可经 provider meta
  `codexNativeResponsesTemplate = "neutral"` 切回上游中性模板**，见 4.19；另可走
  ProxyChat 路径规避）。
- ⚠️ 后果：NativeResponses 直连 MiMo/LongCat 等拒绝 freeform apply_patch 的网关
  默认会 400，需显式切中性模板（或走 ProxyChat）。fork 的
  `load_codex_native_responses_template(template)` 按开关二选一，两个模板文件均在库
  （`gpt5_6_sol_template.json` / `codex_native_responses_template.json`）；上游若调整
  模板策略需复核此分歧。

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

> **状态（2026-10-03）**：已由上游新引擎取代——`live/project/codex.rs` 在 route 投影时自行补建
> `[model_providers.<id>]` 表并写表内 `experimental_bearer_token`，fork 的
> `set_codex_experimental_bearer_token` / `plan_codex_live_write` 已删除。原文保留供参考。

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

### 4.17 缓存链路开关（会话级 prompt_cache_key / 会话亲和 / 断点保留）

三项均为「上游不存在的能力」，默认值选取原则是**不改变现有行为**。

**（a）会话级 `prompt_cache_key` 路由扩展到 Claude→Chat**（meta `promptCacheRouting`）

- **上游**：`prompt_cache_key` 只在 Codex Responses→Chat 路径注入（`should_send_codex_chat_prompt_cache_key` 的宿主白名单：`api.openai.com`、`api.kimi.com/coding`）。
  Claude→Chat 路径仅当 meta 显式配 `promptCacheKey` 时才注入——即**默认完全无会话级缓存路由**。
- **fork**：Claude→Chat 复用同一 `promptCacheRouting` 三态与同一宿主白名单，注入值取**客户端提供的会话 ID**（`explicit meta key > session id`）。
- **原因**：上游 issue #3193 的修复方向是“不要让多个会话共享同一个 key”（默认回退到 `provider.id` 会让所有会话互相驱逐），但 Claude 路径的结果是**连会话级 key 也一并关掉**。修正做法是“每会话一个 key”：`promptCacheRouting = enabled` 显式开启，`auto` 仍按宿主白名单保守判定（很多严格网关对未知字段返 400）。
- **实现**：`claude.rs::should_send_claude_chat_prompt_cache_key`（复用 `codex.rs::chat_upstream_accepts_prompt_cache_key`），opencode-go 网关仍无条件注入会话 key（内置特例）。
- **对应测试**：`test_claude_chat_prompt_cache_routing_{auto_injects_on_allowlisted_host,auto_skips_unknown_host,enabled_overrides_host_allowlist,disabled_skips_allowlisted_host}`、`test_claude_chat_explicit_prompt_cache_key_wins_over_session`。

**（b）会话亲和 header**（meta `sessionAffinityHeader`，值 = 客户端会话 ID）

- **上游**：无此概念。
- **fork**：按配置的 header 名注入会话 ID（如 Cloudflare Workers AI 的 `x-session-affinity`）。**仅在客户端提供了会话 ID 时注入**——代理生成的 UUID 每请求不同，注入只会把同一会话打散到不同实例。受保护 header 名（`authorization` 等）复用 `is_protected_local_proxy_override_header` 拒绝。
- **原因**：多实例网关上“前缀相同”不足以保证命中，还需路由到持有该前缀张量的**同一实例**（CF 文档将 `x-session-affinity` 列为提升前缀缓存命中率的主要手段）。
- **对应测试**：`session_affinity_injected_only_for_client_provided_session`、`session_affinity_noop_without_meta_header`、`session_affinity_refuses_protected_header_names`、`session_affinity_skipped_for_copilot`。

**（c）保留 cache_control 断点**（meta `preserveCacheControl`）

- **上游 / fork 现状**：仅 `opencode.ai/zen/*` 网关内置保留断点，其余 OpenAI 兼容上游一律剥离（避免严格后端 400）。
- **fork 新增**：其他**在网关层实现了断点缓存**的上游可由用户显式开启；断点自带 `ttl: 5m` 仍升级为网关上限 `1h`。
- **对应测试**：`test_claude_chat_preserve_cache_control_meta_opt_in`、`test_claude_chat_preserve_cache_control_upgrades_5m_ttl`。
- **UI**：Claude 表单高级配置区（`ClaudeFormFields`），Codex 侧只暴露 `promptCacheRouting` 与会话亲和（Codex→Chat 已有自己的注入逻辑）。

### 4.18 行为开关（mid-conversation system / severity 拦截值）

**（a）`midConversationSystemPolicy`**：4.1 的 user 重写由硬编码改为 provider 级开关。

- `"rewrite_user"`（默认，与 4.1 原行为逐字节一致）/ `"preserve"`（上游 #6941 语义，原位保留 `role=system`；仅前导 system 合并仍执行）。
- **原因**：4.1 的 user 重写对“会自行提升 system 的第三方网关”有缓存收益，但对原生网关 / 严格按序拼接的后端是**纯语义损失**（丢失 system 特权）。此前用户无法关闭。
- **实现**：`transform::MidConversationSystemPolicy` + `normalize_openai_system_messages(messages, policy)`；新增入口 `anthropic_to_openai_with_options(body, OpenAiChatOptions{..})`，旧三参数函数保留为默认值包装（避免动 13 处调用点）。
- **对应测试**：`test_claude_chat_mid_system_default_rewrites_to_user`、`test_claude_chat_mid_system_policy_preserve_keeps_system_role`。

**（b）`classifierSeverityBlockValue`**：4.7 的 severity 拦截值（默认 `1000`）可由 provider 覆盖为 `100`。

- **原因**：`1000` 基于 2.1.219 实测推断（仓库内 cli.js 副本为 2.1.193，无 severity 模式），无法确证客户端是否对数值做 0-100 范围校验；给出开关让用户实测冲突时就地调整，无需等 fork 发版。ALLOW 恒为 `0`，不受该项影响。
- **实现**：`classifier::ClassifierOptions` + `transform_classifier_response_with(..)`；`transform_classifier_response` 保留为默认值入口。
- **对应测试**：`test_severity_block_value_defaults_to_1000`、`test_severity_block_value_honors_override`、`test_severity_allow_stays_zero_under_override`、`test_classifier_options_from_provider_meta`。
- ⚠️ 分类器的 **fail-open 兼容策略（4.2）本次未改动**，仍是“上游异常 → 放行”。

### 4.19 NativeResponses 目录模板可配（meta `codexNativeResponsesTemplate`）

> **状态（2026-10-03）**：**转出**——`CodexResponsesTemplate::{Full,Neutral}`、
> `load_codex_native_responses_template(template)`、`resolve_codex_responses_template` 与
> `prepare_codex_live_config_text_with_optional_catalog` 随旧路径删除；provider meta
> 字段 `codexNativeResponsesTemplate` 保留在 `provider.rs`（未读，兼容旧数据）。
> 如需在 `mode`/`live` 新引擎上恢复该开关，应在目录构建处挂钩（见第 5 节待办）。原文保留供回植参考。

- **默认**：`full`（gpt-5.6-sol 全量模板，即 4.13 的 fork 行为）。
- **`neutral`**：上游中性模板——该文件（`codex_native_responses_template.json`）本次**恢复入库**；无 freeform `apply_patch` / `web_search`，仅 none/high 两档 reasoning。
- **原因**：4.13 把 MiMo/LongCat 等“拒绝 `type=="custom"` 工具”的网关推给 ProxyChat 路径规避；给出开关后这类网关可继续走原生 Responses，代价是能力降级（用户自选）。
- **实现**：`codex_config::CodexResponsesTemplate::{Full,Neutral}` + `load_codex_native_responses_template(template)`；`resolve_codex_responses_template(provider)`（在 `proxy/providers/codex.rs`，与 `resolve_codex_catalog_tool_profile` 并列）经 `prepare_codex_live_config_text_with_optional_catalog` / `write_codex_provider_live_with_catalog` 的**显式参数**下传（4 个 live 写入点传 provider 选定值；无 Provider 在手的逐字恢复路径用默认 `Full`）。ProxyChat profile 不受影响（走独立模板路径）。
- **对应测试**：`codex_responses_template_from_meta_value_maps_neutral_only`、`neutral_template_drops_freeform_tools_and_reasoning_tiers`、`native_catalog_uses_selected_responses_template`。
- ⚠️ 同步注意：上游若给 `prepare_codex_config_text_with_model_catalog` 加新调用点，必须一并传 template 参数，否则静默回退 `Full`（3 参数包装已标 `#[allow(dead_code)]`，不再供生产使用）。

### 4.20 截断流终态：fork 补 `end_turn` vs 上游「不发任何终止事件」

- **上游**：转换流（Claude→Chat）在上游 EOF 且无 `finish_reason`/`[DONE]` 时不发任何终止事件（也不会补 `message_delta`/`message_stop`）。
- **fork**：保留 fork 的「终态必达」——有实质输出（`has_substantive_output`）时补 `end_turn` + `message_stop`；完全无输出时才发 `error`（伪成功防护）。合并后同时保留了上游的 inline-think 剥离（`InlineThinkSplitter`）与 fork 的缓冲上限/`refusal`/多 `[DONE]` 防护。
- **原因**：Claude Code 在只有 `message_start` 时不会自行结束回合，没有终止事件就永久挂起；上游的取舍是“宁可挂起也不伪造成功”。fork 面向不稳定的第三方网关（断流常见），选择诚实补终态（`end_turn` 不是伪造成功，而是“上游未给原因”的最小合法收尾）。
- **对应测试**：上游的 `review_eof_keeps_received_partial_payload` / `review_progress_during_continuous_reasoning` 在本 fork 已改写为断言 fork 语义（载荷必达 + 补 `end_turn` + 有 `message_stop`、无 `error`），并在注释里写明与上游的差异。

### 4.21 请求上下文创建期的错误 → 协议错误响应体（fork）

- **上游**：`RequestContext::new` 失败（如未配置任何供应商 → `NoProvidersConfigured`）时用 `?` 直接返回 `Err(ProxyError)`，由 axum 的 `IntoResponse` 收尾。
- **fork**：Claude / Codex / Gemini 三条入口在 ctx 创建失败时返回该客户端协议的格式化错误体（`build_anthropic_request_error_response` / `build_codex_request_error_response` / `build_gemini_request_error_response`），状态码仍走 `map_proxy_error_to_status`（未配置供应商 = 503）。
- **例外**：`handle_responses_for_app`（Codex / Grok Build 共用的 `/responses`）保持上游的 `?` 传播。
- **原因**：客户端只认真实协议错误体；裸 `Err` 在部分客户端表现为“无结构失败/连接错误”，用户无法得知真实原因（例如“没有配置供应商”）。
- **对应测试**：上游的 `unresolvable_stacked_ids_are_rejected_in_the_clients_protocol` 末段在本 fork 已改写为断言 503 + `{"type":"error","error":{"type":"api_error"}}`。

### 4.22 MCP 扩展字段白名单需含传输专属字段

- **上游**：`json_server_to_toml_table` 用 `skipped_fields`（按传输方式排除另一侧专属字段）+ 无条件写其余键；`extended_fields` 只用于日志措辞。
- **fork**：同一函数只写 `extended_fields` 白名单内的键（防“写了被 Codex serde 静默忽略”的键），但上游新增的传输清单要求把 `env_vars`（stdio）、`env_http_headers` / `http_headers_helper`（url）也列入白名单。
- **原因**：两套名单职责不同——`skipped_fields` 决定“本传输不该写的”，`extended_fields` 决定“Codex 认得、值得写盘的”；差集会导致导入的 MCP 字段在切换时静默丢失（上游 `codex_only_fields_follow_their_transport` 测试会失败）。

### 4.23 DeepSeek V4 Pro 多模态（数据层）

- **上游**：`codex_deepseek_catalog_template.json` 声明 `deepseek-v4-pro` 为 text-only；上游测试 `vendor_catalog_matched_model_keeps_vendor_modalities` 断言 `["text"]`。
- **fork**：该模板声明 `["text","image"]`（2026-09-14 起官方把 V4 Pro 路由到识图的 V4.1 Flash；见 v3.19.2-a 发布说明），因此本 fork 的同一测试断言 `["text","image"]`。

### 4.24 预设数据：不含 `personality`、不含 `disable_response_storage`/`requires_openai_auth`

- **上游**：Codex 预设保留 `disable_response_storage = true`；本轮又给 E-FlowCode 加了 `modelCatalog` 并删掉 `personality`；新增测试 `tests/config/presetPreferenceKeys.test.ts` 规定预设顶层只能是“上游/协议键”。
- **fork**：`generateThirdPartyConfig` 不写 `disable_response_storage`、不写 `requires_openai_auth`（keyless 安全闸）；本 fork 接受上游“预设不携带个人偏好”的规则，删掉 E-FlowCode 的 `personality`，保留 `model_context_window` / `model_auto_compact_token_limit` 与 `modelCatalog`。
- **对应测试**：上游新增的金标快照 `tests/components/__snapshots__/ProviderForm.presetRows.golden.test.tsx.snap` 中 xAI/Nvidia/E-FlowCode 三个 codex 用例已按 fork 预设重新生成（差异仅为上述两项移除 + E-FlowCode 字段），原因即本节。

### 4.25 依赖：`toml` 1.0 / `toml_edit` 0.25（fork 的 dependabot 升级）

- **上游**：`toml = "0.8"`、`toml_edit = "0.22"`（本轮新代码按 0.22 API 写）。
- **fork**：dependabot 已升到 `toml = "1.0"` / `toml_edit = "0.25"`（`494ad6f6`）。合并时需两处适配：
  - `toml_edit::Table::set_position` 在 0.25 接收 `Option<isize>`（0.22 是 `isize`）：`live/patch/toml.rs`、`live/project/codex.rs`、`live/project/grok.rs` 各加一层 `Some(..)`；
  - `toml::Value` 在 1.0 只解析**单个值**（`ValueDeserializer`），解析整份文档必须用 `toml::from_str::<toml::Value>()` / `toml::Table`：`codex_config.rs` 三处 `config_text.parse::<toml::Value>()` 已改为 `toml::from_str::<toml::Value>(config_text)`（否则 `extract_codex_base_url` 等函数静默返回 `None`）。
- ⚠️ 上游新代码里若再出现 `.parse::<toml::Value>()`，同样必须改写。

### 4.26 Tailwind 主题令牌：上游写在 v3 config，fork 写在 v4 `@theme`

- **上游**：`tailwind.config.cjs` 的 `theme.extend`（色板 / `borderRadius` / `boxShadow` /
  `fontSize` / `fontFamily` / `borderColor.DEFAULT`）+ `plugins: [require("tailwindcss-animate")]`。
- **fork**：已删 `tailwind.config.cjs`（见 2.5），改在 `src/index.css` 里用 v4 语法表达：
  - `@theme { --color-<名>: … }`：v7 设计令牌（`app` / `sidebar` / `subtle` / `selected` /
    `surface` / `border-strong` / `fg-1..3` / `action*` / `inverse*` / `overlay` /
    `control-off` / `direct*` / `route*` / `stack*` / `success|warning|danger*` /
    `agent-*` / `diff-*` / `chart-*`）与既有 Apple 色板；
  - `--radius-control|panel|dialog`、`--shadow-v7-sm|md|lg`、`--text-<角色>`（配合
    `--text-<角色>--line-height` / `--text-<角色>--font-weight` 承载 v3 元组第二项）、
    `--font-sans|mono`（含中日字体回退）；
  - v3 的 `borderColor.DEFAULT` → base 层 `*, ::before, ::after, ::backdrop,
    ::file-selector-button { border-color: hsl(var(--border)) }`。v4 默认是 `currentColor`，
    不补这条的话上游 v7 代码里大量只写 `border`（不带颜色）的边框会跟着文字色跑；
  - `tailwindcss-animate` → **`tw-animate-css`**（官方 v4 移植版，devDependency）。
    `animate-in` / `animate-out` / `fade-*-0` / `zoom-*-95` / `slide-in-from-*` /
    `slide-out-to-*` 必须由 `@utility` 定义（普通 CSS class 不会生成
    `data-[state=open]:` / `data-[side=bottom]:` 变体），所以不能只把 CSS 抄进文件；
    手写 `@utility slide-in-from-top-\[48\%\]` 也不行——v4 拒绝带转义字符的 utility 名。
- **合并核对清单**：每次同步后比对上游 `tailwind.config.cjs` 的 `theme.extend` 与
  `plugins`，新增项逐条搬进 `src/index.css`，并跑 `pnpm build:renderer` 后 grep 产物 CSS
  确认新用到的工具类真的生成了（v4 对未定义名字是静默无效）。本轮已验证 53 个 v7 令牌
  工具类全部出现在产物 CSS 中。

### 4.27 Gemini MCP 超时：fork 未配置时不写 `timeout`（金标快照分歧）

- **上游**（`src-tauri/src/gemini_mcp.rs`）：未配置任何超时时仍写默认值
  `DEFAULT_STARTUP_MS = 10_000` / `DEFAULT_TOOL_MS = 60_000`，即 `"timeout": 60000`。
- **fork**（`2196e8c1`）：**两者都未配置时省略 `timeout` 字段**，交给 Gemini CLI 官方默认
  （600 000 ms / 10 分钟）——强制写 60 s 会把首次 `npx` 冷启动、大文件操作这类长耗时工具
  调用提前掉。
- **后果**：上游的 golden 快照
  `src-tauri/tests/golden/snapshots/mcp/gemini-settings.json` 是以上游行为生成的（前一次同步
  随上游 golden 测试一并带入），fork 这边会持续报快照不一致。
- **处理**：用官方开关按 fork 行为重新生成
  （`CC_SWITCH_UPDATE_GOLDEN=1 cargo test --test golden mcp_bytes::gemini_settings_mcp_projection_bytes`）。
  ⚠️ 下次同步若上游改了这个快照，**不能直接取上游版**，必须用 fork 行为重生成。

### 4.28 发布工作流（`release.yml`）保持 fork 的个人模式

- **上游**（`ee66be22` / `f9e2ebbb` / `c62eab2b`，2026-10-05）：把 macOS 拆成
  `macos-binary`（两架构分机并行编译）+ `macos-release`（合并 universal、签名、
  公证），签名密钥准备抽成 composite action
  （`.github/actions/prepare-tauri-signing-key`）；把“缺签名”从警告升级为硬失败
  （macOS `.tar.gz.sig`、Windows MSI 与 MSI 签名、Linux AppImage 与其签名，以及
  latest.json 的六平台签名齐备性）；`publish-release` 加
  `if: github.event_name == 'push' && github.ref_type == 'tag'`，手动触发退化为
  纯干跑（产物只留在 workflow 里，不建 Release）。
- **fork**：保留 `resolve-version`（版本解析 + 三处版本号一致性校验）+ 单
  `release` 矩阵（macOS universal 在矩阵内构建，含 hdiutil 无签名 DMG 兜底）+
  `publish-release`（CHANGELOG + git log 生成正文，手动重跑可用）+ 独立
  `assemble-latest-json` + `sync-to-r2` 的既有结构；签名准备保持内联且 secrets
  缺失时全链路降级（不设 `TAURI_SIGNING_PRIVATE_KEY` 让 Tauri 跳过签名，
  Windows/macOS 构建失败只警告并继续收集已产出安装包）。
- **原因**：fork 是个人使用模式，仓库不配置任何签名 secrets（见 6.1、6.2）。
  上游的新校验在没有签名的仓库上会让**每个平台**都失败（v3.20.3 首次发布即为此
  故障）；上游的 macOS 并行架构依赖 Apple 证书与公证凭据，fork 无法提供。
  latest.json 的齐备性检查同样与 fork 现状冲突——fork 从不产出 `.sig`，
  latest.json 的 `platforms` 本为空（6.2 的既有缺陷），硬失败只会让发布完全停止。
- **代价**：fork 的发布构建仍是单机串行（macOS universal 一次编两个架构），
  不产出 `.sig`（应用内更新仍为空 platforms，见 6.2），也没有上游的干跑模式。
- 上游新增的 `.github/actions/prepare-tauri-signing-key/action.yml` 随合并入库但
  **本 fork 未引用**；`build(release)` 的 `codegen-units = 16`
  （`src-tauri/Cargo.toml`，发布编译快约 64%、体积 +17%）则已随合并且与 fork 不冲突。
- ⚠️ 每次同步都要确认 `release.yml` 仍是 fork 版本：上游若继续在其结构上迭代，
  该文件会持续冲突，按本节策略一律取 fork 侧（仅当 fork 决定配置签名密钥时重估）。

## 5. 本地发布序列

下表按 GitHub Releases 实际发布时间排序补全（2026-09-17 核对：共 11 个 release，与 `aliveranme/cc-switch` 实况一致）。
早于 `v3.19.1-a` 的版本随上游同步对齐版本号发布，变更内容即上游对应版本的变更日志。

| 版本 | 发布时间 | 内容 |
|---|---|---|
| `v3.18.0` | 2026-06-28 | 合入上游 v3.18.0：Grok Build 成为第八个受管 app（独立 `/grokbuild/v1/responses` 路由命名空间与独立预设集）、xAI Grok OAuth 设备流登录、用量统计重建（schema v16）（52 提交 / 217 文件，+21452/-6285）。GitHub 上为 prerelease |
| `v3.19.0` | 2026-07-30 | 合入上游 v3.19.0：安全加固（skill 安装 zip-slip 与路径穿越、Gemini 通用配置凭据泄漏清理、SQL 导入 SQLite authorizer、终端路径转义、deeplink 确认完整展示）、代理工具图片 token 膨胀修复、models.dev 定价自动同步（38 提交 / 132 文件，+14926/-1415）。GitHub 上为 prerelease |
| `v3.19.1` | 2026-07-31 | 合入上游 v3.19.1：DeepSeek/火山 Ark/腾讯混元 Codex 网关改原生 Responses、Claude Desktop 用量双重计数修复（#5938）、Codex 官方 provider 回切残留第三方 key 修复、删除 3,166 行废弃代码（12 提交 / 71 文件，+2324/-3680，首个删除多于新增的版本）。GitHub 上为 prerelease |
| `v3.19.1-a` | 2026-08-02 | CI/发布基础设施修复（tag 推送正式版、wix.version 绕过 prerelease） |
| `v3.19.1-b` | 2026-08-02 | proxy 协议修复收尾（安全分类器、prefix-cache 稳定性、流式终态容错、工具历史恢复会话隔离）。注：GitHub 上该 release 为 prerelease（与 a 同日，应为手动重跑时 `prerelease` 未置 false，见 6.1） |
| `v3.19.2` | 2026-08-09 | 合入上游 v3.19.2（15 提交）+ fork 全特性；字节上限统一为 `bytes_with_limit`（200MB）；content_encoding 解压 bomb 防护；atomic_write Windows 改用 `ReplaceFileW`；版本号与上游对齐（首次无后缀，wix.version 3.19.2.0）；重发补充：接管统一 `ANTHROPIC_AUTH_TOKEN` 占位符避免 Not logged in、官方原生分类器透传 + ALLOW 兜底、分类器检测加固 |
| `v3.19.2-a` | 2026-08-17 | DeepSeek 多模态能力支持（`deepseek-v4-pro` 支持图片输入；`deepseek-v4-flash` 维持纯文本）；同步上游趋势图表点位与 Grok Build 文案修正；wix.version 递增至 3.19.2.1 |
| `v3.20.0` | 2026-08-19 | 合入上游 v3.20.0：Pi 成为第九个受管 app（schema v16→v17）、Codex 多账号 ChatGPT 管理（#3879）、Windows+WSL2 `ReplaceFileW` 修复（#6232）、CLI 检测改用注册表 PATH（#6284）（69 提交 / 284 文件，+53108/-6678） |
| `v3.20.1` | 2026-08-29 | 合入上游 v3.20.1：Codex CLI 0.149 改 config-only 切换（第三方 key 不再写 `auth.json`）、Team workspace 账号互相覆盖修复（#6780）、会话扫描增量 byte-cursor（schema v17→v18）（26 提交 / 66 文件，+7474/-1000） |
| `v3.20.2` | 2026-09-09 | 合入上游 v3.20.2（`2d54e261`，26 提交）；Grok 走 xAI 原生 Responses 路由、一批 catalog/兼容性修复、预设与定价扩充 |
| `v3.20.3` | 2026-09-13 | 合入上游 v3.20.3（`bd247a4a`）；Kimi 等 Codex 预设改原生 Responses 直连、代理正确性修复、预设与定价维护。**首次发布失败**：标签误指上游提交，Release 跑的是上游工作流（硬校验 `TAURI_SIGNING_PRIVATE_KEY`），5 个平台全部在签名步骤失败、附件为空；把标签改指 fork 提交 `b24deaa9` 后重发成功 |
| `v3.20.4` | 2026-09-23 | 合入上游 v3.20.4（`f2537fdf`，25 提交）；Copilot 端点剥离 `stop`、`additional_tools` 抬升为工具、Codex `detail:original` 图片归一化、GPT-6/Grok 家族档位与定价批次、WSL shell 启动输出过滤；fork 侧 rquickjs 0.12 锁文件对齐（14 资产） |
| `v4.0.1` | 2026-10-05 | 合入上游 v4.0.0 **与** v4.0.1（`4804b723`，38 提交）：v4.0 收尾（配额重置倒计时与到期列表、用量表格分页、88API/兔子 API 等预设扩编、官方图标）、代理修复（零用量 `response.incomplete` → `api_error`、Codex 状态库 WSL 路径跳过加锁）、CI 基建（rust-cache + nextest + pinned toolchain）与发布流程改造；fork 侧移除手写的 `wix.version` 覆盖。**发布一次成功**：tag `v4.0.1` 指向 fork 提交 `32e4aa39`，14 资产（Linux x86_64/arm64 各 AppImage+deb+rpm、macOS dmg+zip+tar.gz、Windows x86_64/arm64 各 MSI+Portable.zip、latest.json），正式版（Latest）；随发布一并修掉 macOS/Linux CI 的 golden 模式快照（644→600） |

> 2026-08-16 同步：合入上游 v3.19.2 之后 42 个提交（Pi 原生 coding agent、
> per-model reasoning levels、DeepSeek 官方 catalog mirror、web_search reject
> 黑名单、供应商表单层级重构、IME safe input、路由激活动画等）。fork 版本号保持
> `v3.19.2` 未 bump。fork 全部 4.1–4.14 行为分歧点保留；新增 4.13（NativeResponses
> 模板保留完整能力）与 4.14（passthrough 下 ultra 钳制到 max）。

> 2026-08-28/30 同步：合入上游 v3.20.1（7 提交：会话扫描重构——增量 byte-cursor
> 扫描、auto/manual 模式切换、non-append rewrite 检测、Sync Now 门控——及
> v3.20.1 发版）与 #6941（mid-conversation system 原位保留）。fork 版本号随
> merge 对齐上游 `3.20.1`，并发布了同名 release `v3.20.1`（2026-08-29，13 资产；
> 本注记原记「未单独发版」，2026-09-17 核对 GitHub Releases 后更正）。
> 分歧点变化：4.15 移除（bearer token
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

> 2026-09-15 同日**第二次**合并（本注记补记于 2026-09-17，此前仅记录了上面
> 的 #7331 一次）：`1a2d24c6 Merge remote-tracking branch 'upstream/main'` 合入
> 上游 `15884b20`（#7395 Codex 接管时恢复 stale 账号绑定）与 `06082e18`
> （#7383 MiniMax Code harness 支持）。89 文件 +4090/-275；冲突 5 处
> （`README.md` / `README_DE.md` / `README_JA.md` / `README_ZH.md` /
> `src/types/usage.ts`）已解，随后以 `c27300b1`（prettier 格式化 mcode 与 usage
> 类型）与 `37ef5010`（mcode 预设按钮可访问名兼容 dom-accessibility-api 0.6 的
> SVG title 拼接）收尾。fork 版本号保持 `v3.20.3` 未 bump。

> 2026-09-17 核对（本次无 merge / push / release）：上游自 `06082e18`
> （2026-09-15 04:33 UTC）起**无新提交、无新 release/tag**（最新仍为 `v3.20.3`），
> 上游 126 个分支中无任何 tip 晚于该提交；`git merge-base --is-ancestor
> upstream/main main` 成立、`git rev-list --count upstream/main..main` = 398、
> `git log upstream/main ^main` 为空。fork `main` 与 `origin/main` 一致
> （`aa39d944`），无待推送提交，全部发布标签均指向 fork 提交 → 无需 merge /
> push / release。fork `main` 领先最新 release 标签 `b24deaa9`（`v3.20.3`）
> 17 个提交。
>
> 2026-09-21 同步（无 release）：合入上游 `fdbe3a85` 起的 5 提交——`33c80626` #7489 技能归档
> 条目上限 10_000→30_000，并新增「文件至少计一个磁盘块」的字节预算（`skill.rs` 与
> `webdav_sync/archive.rs` 两处上限对齐，否则技能装得上、同步却恢复不了）；`a659440b`
> #7194 外部改动文件后刷新活跃 prompts；`f2d0b2a6` #7522 README 赞助 CTA 围绕 Kimi Code
> 双区链接重排；`1408f382` #7526 新增 Kimi Global 预设变体（7 个 preset 文件）；
> `fdbe3a85` #7515 OpenCode 从拉取模型列表搜索并批量添加。24 文件 +1153/-45；冲突 4 个
> README 的 Kimi 赞助段落——取上游新文案（双区链接 + 首充福利），保留 fork 的本地横幅
> 资源 `assets/partners/banners/*`（CNB CSP 只放行白名单域，外链图不显示）。
> 分歧点核对：上游改动全部落在归档预算 / prompts 刷新 / 预设数据 / OpenCode 表单 / 测试 /
> README，未触及第 4 节任何分歧实现（proxy 转换、分类器、atomic_write、wire_api、deeplink、
> `hermes_config.rs` 排除表、NativeResponses 模板、lucide 图标）；第 3 节 fork 专属文件未被
> 触碰。验证全绿：`cargo test --lib` 3016、`pnpm vitest run` 1140（140 文件）、
> `cargo fmt --check` / `cargo clippy` / `pnpm typecheck`。上游最新 tag 仍为 `v3.20.3`
> （无新版本），fork 版本号保持未 bump。merge 提交 `e56a239b` 已推送 `origin/main`
> （本机克隆未配置 `cnb` 远端，见 6）。

> 2026-09-22 同步（无 release）：合入上游 `fdbe3a85` 起的 13 提交——`8272707d` #6381 技能
> 在 skillId 与目录名不一致时按「已保存源路径 → 目录名 → metadata name 唯一兜底」定位
> （`skill.rs` 新增 `find_remote_skill_for_install` / `choose_doc_path`）；`37d04760` #7550
> OpenCode 目录在 WSL 时改探 WSL 侧 home 的 OMO 统一配置（`config.rs` 新增
> `derive_wsl_home_dir`，`omo.rs` 的探测改为 home 候选列表，WSL 侧优先）；`2c735bd9` 卡片
> 不再展开陈旧缓存的用量档位、`09498c30` 单色预设图标改取前景色；预设与定价批次
> `48e572cc`/`e06ff90f`/`d8e98be2`/`0859fa6a`/`4d2c6f07`（CN Codex 预设按 Responses API 审计
> 刷新、MiniMax CN → `api.minimax.cn`、BaiLing → `api.ant-ling.com` 且 Ling-2.5-1T →
> Ling-2.6-1T、AICodeWith → `/v1`、新增 FluxA Token Plan 三端预设、DeepSeek V4 Pro 定价修回
> 高峰档 1.32/3.96/0.044 并补 Qwen3.8 2.4T A95B / 27B 与 Hy4 Preview）；`f6c99822` DeepSeek
> 路由暴露 1M 上下文变体。53 文件 +1729/-213，**零冲突**自动合并（首次全自动）；分歧点
> 核对：上游改动未触及第 4 节任何分歧实现（proxy 转换、分类器、atomic_write、wire_api
> 迁移、deeplink、session_usage、接管判定、NativeResponses 模板、`hermes_config.rs` 排除表、
> lucide 图标），第 3 节 fork 专属文件未被触碰；上游在 `codex_config.rs` 的 web_search 拒绝
> 名单与 `services/provider`、`coding_plan` 的 MiniMax host 判定均为新增条目，与 fork 现有
> 逻辑不冲突。验证全绿：`cargo test --lib` 3030、`pnpm vitest run` 1164（141 文件）、
> `cargo fmt --check` / `cargo clippy --all-targets -D warnings` / `pnpm typecheck` /
> `pnpm format:check` / `pnpm build:renderer`。上游最新 tag 仍为 `v3.20.3`（无新版本），
> fork 版本号保持未 bump。merge 提交 `0b4621e8` 已推送 `origin/main`
> （本机克隆未配置 `cnb` 远端，见 6）。
>
> 2026-09-23 同步（无 release）：合入上游 `f2537fdf` 起的 25 提交（自 `37d04760` 之后），含
> v3.20.4 发版（`84efe1fb` 四处版本号 + `43e1d990` 三语 release notes）与其余 23 个修复/特性：
> proxy 协议层 `56df6513` #5404 Copilot Chat 端点剥离 `stop`（auto mode classifier 恢复可用）、
> `a35e5000` #7454 `additional_tools` 载体抬升为工具、`83a24dfb` #7476 Codex `detail:original`
> 图片归一化、`c8e76bbc` #7378 工具描述缺失时省略字段而非写 null、`d6e05152` #7531
> GPT-5.6/GPT-6 Astra 保留 `max` 档、`8e478b2b` #7369 grok-4.x（x≥5）家族进入 reasoning 白名单；
> Codex `5a80e300` #7434 live auth 无凭据时保留 DB 中的登录态；其余为 `f2537fdf` #7595 models
> 响应形状容错、`701c079b` #7348 WSL 探测工具版本时忽略 shell 启动输出、`6f6087cd` #6348 各入口
> `window.show()` 前重置 skip_taskbar、`c715ee2b` #7053 隐藏会话 URL 归因、`4837fe2c` #5211
> 切 app 时重置供应商滚动、`09c5d39d` #7578 mcode 移除后刷新供应商状态、`06c03621` #6826
> Pi 供应商 logo、`4b1ec8b5` 前端测试稳定化 + renderer 构建校验、`f8821c03` About 卡片 GitHub
> star 提示、`bbee784d` Soshow 聚合商预设、`54640b95` SudoCode.chat 备用端点、`bccfaf37`/
> `de970c13` i18n 修正、定价批次 `85caa69e`/`85894582`/`42200b42`。50 文件 +2906/-93。
> 冲突 5 处（上游 4 个提交与 fork 本地改动相交，全部手工解）：`ci.yml` 与 `vitest.config.ts`
> ——#4b1ec8b5 新增的 renderer 构建步骤 / vitest include 与 fork 既有版本重复，取 fork 版，
> 两文件最终与 merge 前**逐字节一致**（净零改动）；`src/proxy/providers/claude.rs`——#5404 的
> Copilot `stop` 剥离与 fork 的 DeepSeek `tool_choice` 降级插在同一位置，**两侧都保留**；
> `AboutSection.tsx`——#f8821c03 重排 GitHub 按钮并新增 star 提示，取上游新布局，并按 4.16
> 把上游的 lucide `Github` 图标换回 fork 的**内联 SVG**；`codexProviderPresets.ts`——SudoCode
> 预设保留 fork 已移除 `requires_openai_auth` 的清理，同时采纳上游新增的 `api.sudorelay.com`
> 备用端点。
> 分歧点核对：4.1–4.16 逐条复核全部保留——4.16 因上游新增 lucide `Github` 使用而再次套用内联
> SVG（见上）；4.9（Claude upsert）、4.10（`takeover_active` 策略矩阵）、4.13/4.14（NativeResponses
> 模板、`ultra` 钳制）的对应实现与钉桩测试均未被上游触及；上游对 `transform.rs` 的推理档位改动
> 落在 `resolve_reasoning_effort`（Claude→OpenAI 方向），与 fork 的 `map_reasoning_effort`（Codex
> 方向）钳制不冲突。第 3 节 fork 专属文件（`classifier.rs`、`codex_chat_history.rs`、两个
> `*Impl.tsx`、`gpt5_6_sol_template.json`、`tests/vitest-jest-dom.d.ts` 等）未被触碰。
> 附带修正：`src-tauri/Cargo.lock` 与 `Cargo.toml` 长期不一致——`cfg(windows-aarch64)` 的
> `rquickjs 0.12`（`bindgen`）分支没落到锁文件里，此前每次本地 `cargo` 调用都会重写它（工作区
> 从 2026-09-18 起一直挂着这份未提交改动）。本次 `cargo test` 重新解析后已并入 merge 提交
> `476d3723`（convert_case 0.12→0.11；新增 rquickjs/-core/-sys/-macro 0.12.2）。
> 验证：`cargo test --lib` 3050、`cargo fmt --check`、`cargo clippy --all-targets -D warnings`
> 全绿；前端 `pnpm vitest run --maxWorkers=8` 141 文件 1173 用例全绿（32 线程默认并发下两个长
> 用例贴在超时边界，见 6.4）、`pnpm typecheck` / `pnpm format:check` / `pnpm build:renderer` 全绿。
> 上游最新 tag 已为 `v3.20.4`，fork 版本号随 merge 对齐 `3.20.4`（未发同名 release）。merge 提交
> `476d3723` 已推送 `origin/main`（本机克隆未配置 `cnb` 远端，见 6）。

> 2026-09-25 同步（无 release）：合入上游 `f2537fdf` 起的 4 提交（自 2026-09-23 同步之后）——
> `da193d4f` #7621 预设与定价批次：Pi 目录/预设新增 Step 5 Preview、Step 3.7 Flash、Claude Opus 5.5、
> Claude Fable 5.1、GPT-6（Sol/Luna/Astra）、Gemini 3.8 Flash、MiMo V2.6（Pro/Flash/Pro UltraSpeed）、
> DeepSeek V4.1 Flash，`piThinkingProfiles` 新增 `openaiResponsesGpt6Astra` 档位映射（off/minimal → null）
> 与 Opus 5.5 / Fable 5.1 的 `forceAdaptiveThinking` 绑定；小米 MiMo 的 Codex 目录改官方四档
> （none/low/medium/high，默认 low，此前 none/high 两档）并补回官方系统提示词
> `MIMO_CODEX_BASE_INSTRUCTIONS`；Codex / Hermes / OpenClaw / OpenCode / Claude Desktop 预设同步刷新；
> `schema.rs` 定价新增 `step-5-preview`，修正 `o3-mini`（0.55/2.20 → 1.10/4.40）与 `mimo-v2.5`
> （输出 0.29 → 0.28），新增 MiMo 2.6 三档，并加只在「仍匹配旧内置值」时才覆盖的 `UPDATE` 白名单
> （自定义价不动；`database/tests.rs` 补幂等 + 保自定义价用例）。其余三个：`f8788719` #6632 Sub2API
> 品牌图标入库（大 SVG 走 `?url` 以 `<img>` 渲染）、`3ed58925` 应用切换器非激活态彩色品牌图标改
> `grayscale + opacity-60`、悬停/选中恢复（单色 Codex / Grok Build / Pi 已随 `currentColor` 变暗，
> 列入 `CURRENT_COLOR_APPS` 白名单不重复处理）、`a06a41ec` FluxA 付费 API 计数改 13,000+
> （四语 README + 四语 i18n）。28 文件 +1735/-199，**零冲突**全自动合并（与 2026-09-22 同为全自动，
> 上一轮 2026-09-23 为 5 处冲突）。
> 分歧点核对：本次上游改动全部落在预设数据 / 图标资产 / 应用切换器样式 / 定价种子 / i18n，**未触及**
> 第 4 节任何分歧实现（proxy 转换、分类器、`atomic_write`、wire_api 迁移、deeplink、`session_usage`、
> 接管判定、NativeResponses 模板、`hermes_config.rs` 排除表、lucide 内联 SVG），第 3 节 fork 专属文件
> 未被触碰。逐文件复核 fork 侧不变式：`codexProviderPresets.ts` 的 `generateThirdPartyConfig`
> （含 fork 的 `requiresOpenAiAuth` 选项与 keyless 安全闸注释）未被上游 MiMo 目录/模型名改动覆盖；
> `piThinkingProfiles.ts` / `piModelCatalog.ts` 本就源自上游 #6064，fork 无自有改动，不存在覆盖风险。
> 验证全绿：`cargo test --lib` 3051（`HOME` 隔离，见 6.3）、`cargo fmt --check`、
> `cargo clippy --all-targets -D warnings`；前端 `pnpm vitest run --maxWorkers=8` 141 文件 1173 用例、
> `pnpm typecheck` / `pnpm format:check` / `pnpm build:renderer`。上游最新 tag 仍为 `v3.20.4`
> （无新版本），fork 版本号保持 `3.20.4` 未 bump；本地 `v3.20.4` 标签经复核仍指向 fork 提交
> `004e2dc1`（非上游提交，符合 6.1）。merge 提交 `447750a7` 已推送 `origin/main`
> （本机克隆未配置 `cnb` 远端，见 6）。

> 2026-09-28 同步（无 release）：合入上游 a06a41ec 之后的 13 个提交至 1ee2fdc3——
> GPT-6 Sol/Luna 的 Codex OAuth 身份要求升至 0.155.0，并在代理中保留 max 推理档；
> Homebrew Cask 改用 brew upgrade --cask，Codex 独立安装版原地重跑官方安装器升级；
> About 增加 MiniMax Code 安装/升级检测，MiniMax Code 预设透传 MCode 可表达的 Pi compat
> 并跳过不支持的 compat，另修正空状态文案；Zhipu 增加支持图片输入的 GLM-5.3-Flash
> Codex 预设。四语 README 重组和事实刷新、开发文档迁入 CONTRIBUTING、赞助联系邮箱更新；
> Codex 路由指南及 v3.20.4 中/英/日用户手册同步刷新。116 文件 +5848/-4055。
> 冲突 5 处：四语 README 与本地改版相交，保留英文 hero 与四语架构说明，正文采用上游刷新内容；
> mcodeProviderPresets.ts 采用上游可表达 compat 的筛选与映射实现。
> 分歧点核对：第 4 节逐条复核，分歧实现均保留。transform.rs 只扩展允许 max 的 GPT-6
> 模型列表；4.14 的 Codex ultra→max 钳制实现仍在 transform_codex_chat.rs，
> 不受本次 Claude→OpenAI 档位变更影响。claude.rs 的改动仅更新 OAuth 身份版本断言；
> 4.5 atomic_write、4.9 会话用量 upsert、4.10 takeover 策略、4.13 NativeResponses 模板、
> 4.16 内联品牌 SVG 以及其余 fork 专属实现未被本次上游改动覆盖。Codex/MiniMax 安装升级
> 逻辑并入现有 lifecycle 管理，UpdateCommand::Unmanaged 防止未知原生安装被 npm 旁路安装。
> 冲突检查与 git diff --check 通过；本轮未运行测试。上游没有新版本 tag，fork 仍为 3.20.4，
> 未 bump 版本。merge 提交 bb7b5f11 尚未推送 origin/main。

> 2026-09-28 **功能增量（无同步 / 无 release）**：落地缓存链路与行为开关（第 4.17–4.19 节）。
>
> **缺口补足**（上一轮协议审查发现的 4 项）：
> - Claude→Chat 会话级 `prompt_cache_key` 路由（此前只有 Codex 路径有；上游 #3193 的修复把 Claude 路径的会话 key 一并关掉了）
> - `sessionAffinityHeader`（值 = 客户端会话 ID；CF 等多实例网关的前缀缓存亲和）
> - `preserveCacheControl`（网关层实现断点缓存的上游，从 opencode-go 特例推广为可配）
> - `CodexResponsesTemplate::Neutral`（恢复上游中性模板文件；拒绝自定义工具的网关不再被迫走 ProxyChat）
>
> **开关化**（此前硬编码的 fork 行为）：`midConversationSystemPolicy`（4.1）、
> `classifierSeverityBlockValue`（4.7）、`codexNativeResponsesTemplate`（4.13）。
> 三者默认值均与改动前一致（`rewrite_user` / `1000` / `full`）。
>
> 改动文件：`provider.rs`（+5 meta 字段）、`transform.rs`（选项结构 + 策略枚举）、
> `claude.rs`（路由 / 断点 / 策略接线）、`forwarder.rs`（会话亲和注入）、
> `classifier.rs`（选项结构）、`codex_config.rs`（模板枚举 + 参数下传）、
> `proxy/providers/codex.rs`（resolve）、`proxy/providers/mod.rs`（导出）、
> `services/{config,proxy}.rs` 与 `services/provider/live.rs`（调用点传参）。
> 前端：`types.ts`、`ProviderForm.tsx`、`ClaudeFormFields.tsx`、`CodexFormFields.tsx`、
> `GrokBuildProviderForm.tsx`、四语 i18n。
>
> 验证：`cargo test --lib` **3094 passed / 0 failed**（`HOME` 隔离，见 6.3）、
> `cargo fmt --check` / `cargo clippy --all-targets -D warnings` 全绿；
> 前端 `pnpm vitest run --maxWorkers=8` **143 文件 / 1183 用例**、
> `pnpm typecheck` 全绿。新增钉桩测试：Rust 16 个、前端 4 个。
>
> ⚠️ 本地验证过程中一次未隔离 `HOME` 的全量 `cargo test --lib` 污染了真实
> `~/.cc-switch/model-pricing.json`（6.3 记录的已知问题），已按 6.3 的清理说明恢复为
> 默认值（`includeCommonModels: true`、空 `models` / `deletedModelIds`）。

> 2026-10-03 同步（无 release）：合入上游 `1ee2fdc3` 之后的 **64 个提交**至 `bfaaba16`
> （306 文件 +49 253 / −30 217）。本轮是**上游的架构级重写**：新增 `src-tauri/src/live/`
> （`engine.rs` 320 / `patch/{json,toml,dotenv}.rs` / `floor.rs` / `project/{claude,codex,gemini,grok}.rs`）
> 与 `src-tauri/src/mode/`（`controller.rs` 6 242 行：`lock_settled`、`enter/exit`、`switch_route`、
> `stack` 成员集、`reject_unsupported_official`……）取代旧 live 写入路径；
> 同时新增 `services/provider/{claude,codex,gemini,grok}_direct.rs` / `*_editor.rs`、
> `live/project/codex.rs` 的 route 投影与 `codex_stack_*` 目录、`toml` 补丁引擎、
> 「Stack 模式」（原 attached models 改名）。净删除：`services/proxy.rs −10 497`、
> `provider/mod.rs −2 051`、`provider/live.rs −2 112`、`codex_config.rs −4 361`、`services/config.rs −190`。
>
> **冲突 28 处，全部手工解**：README.md（保留 fork 本地 banner 资源 + 取上游 track_id 链接）、
> `assets/partners/logos/etok.png`（随上游赞助位下线删除）、`claude_desktop_config.rs`（改用上游
> patch 引擎，回植 fork 的 3P profile 字段/`supports_1m=false`/测试）、`config.rs`（fork 的
> `retry_transient_io` fallback 与上游 `write_text_file_private` 并存）、`database/{tests.rs,dao/proxy.rs}`、
> `mcp/codex.rs`（采用上游 0.160 传输字段名单 + 保留 fork 白名单，见 4.22）、`services/config.rs`、
> `import_export_sync.rs`、`provider_service.rs`（采用上游；fork 的旧 sync-path 测试随 `ConfigService::
> sync_current_providers_to_live` 下线一并移除）、`streaming.rs`（上游 inline-think 剥离 + fork 的
> 伪成功防护/截断流防护/缓冲上限/`refusal` 全部并存，两处上游测试按 4.20 改写）、
> `transform_codex_chat.rs`（fork 的 `response_format` 注入 + 上游 compaction 语义并存）、
> `response_processor.rs`、`handler_context.rs`（fork 的 `is_classifier_request`/`classifier_mode`
> 与上游 `is_stack` 并存）、`proxy/providers/mod.rs`、`handlers.rs`（fork 的错误响应/分类器分支 +
> 上游 Stack：`resolve_stack_target` 与 `stack` 参数）、`forwarder.rs`（fork 媒体整流注释 + 上游
> `mut e`）、McodeProviderForm.tsx（采用上游 providerKey 校验）、codexProviderPresets.ts（见 4.24）、
> ClaudeFormFields/CodexFormFields/ProviderForm（fork 缓存开关 UI 与上游 Stack 布局/`reasoningFields`
> 并存）、`providerConfigUtils{,.test}.ts`（保留 fork 的 `isMatchingDomain`，随上游删除
> common-config snippet 机制）。
>
> **行为分歧变更**（逐条核对结果）：
> - **由上游新引擎取代、fork 实现随之移除**（转出 4.x，待后续清理残余物）：
>   4.6（`wire_api = "chat"` 存量迁移）——新引擎在 route 投影时统一写 `wire_api = "responses"`
>   （上游 `live/project/codex.rs::legacy_shapes_become_the_custom_route` 已覆盖该保证）；
>   4.10（接管判定策略矩阵）——`AppType::takeover_active` 已无生产调用点，仅剩 `app_config.rs` 单测；
>   4.13/4.19（NativeResponses 模板可配）——`CodexResponsesTemplate`、`resolve_codex_responses_template`、
>   `prepare_codex_live_config_text_with_optional_catalog` 随旧路径删除；provider meta
>   `codexNativeResponsesTemplate` 字段保留（未读）；
>   4.15a（custom 表缺失时补建 bearer token）——上游 `live/project/codex.rs` 自行建表并写表内 token。
> - **保留**：4.1（mid-conversation system → user，开关 `midConversationSystemPolicy`）、4.2（分类器
>   fail-open）、4.3、4.4、4.5（`atomic_write` unix 权限语义 + create-new 即 0600）、4.7（severity）、
>   4.8、4.9（Claude 会话用量 upsert）、4.11（deeplink `claude-desktop`）、4.12（`HERMES_UI_OWNED_KEYS`）、
>   4.14（`ultra` → `max` 钳制）、4.16（品牌图标内联 SVG）、4.17（a/b/c 缓存链路开关）、4.18（a/b 行为开关）。
> - **本轮新增分歧**：4.20（截断流终态）、4.21（ctx 创建期错误 → 协议错误体）、4.22（MCP 白名单）、
>   4.23（DeepSeek V4 Pro 多模态）、4.24（预设数据取舍）、4.25（`toml` 1.0 / `toml_edit` 0.25 适配）。
>
> 验证（2026-10-03）：Rust `cargo test --lib` **3276 passed / 0 failed**（`HOME` 隔离，见 6.3）、
> `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings` 全绿；
> 前端 `pnpm vitest run --maxWorkers=8` **158 文件 / 1795 用例**全绿、`pnpm typecheck` /
> `pnpm format:check` / `pnpm build:renderer` 全绿。金标快照按 4.24 重新生成（3 个 codex 用例）。
> ⚠️ 本机 `cargo test --tests`（集成测试二进制）因 rlib/staticlib 解析问题无法链接（既有环境问题，
> 非本次回归；`cargo check --all-targets` 与 `cargo test --lib` 均通过），详见 6.5。
> merge 提交尚未推送 `origin/main`（本机克隆未配置 `cnb` 远端，见 6）。

> 2026-10-04 同步（无 release）：合入上游 `bfaaba16` 之后的 **98 个提交**至 `372b1698`
> （483 文件 +86 924 / −28 753；144 新增 / 34 删除 / 304 修改 / 1 重命名）。本轮主线是
> **上游 v7/v8 界面重构 + 会话阅读器重写**：新增 sidebar shell（侧栏外壳 + 全局页面 +
> 分组设置）、v7 设计令牌与共享原语、结构化会话阅读器
> （`src-tauri/src/session_manager/{cache,content,model}.rs` + `providers/{blocks,codex_items,
> opencode_blocks,pi_blocks}.rs`）、usage 全时段热力图与日瓦片、MCP `AppMatrix` 矩阵、
> 设置页 accounts center（新 `src/components/apps/` 页 + 工具链管理）。同批修复：Claude/Codex
> 路径的 inline `<think>` 剥离（#7741）、Codex late-argument function_call 补全、
> `late-arguments` 整流、Codex 历史重试、Grok token 刷新状态、MCP 字段拆分（#7735/#7845）。
>
> **冲突 17 处，全部手工解**（总冲突量比上轮小，但多为“上游重写 vs fork 拆包”型）：
> `package.json` / `pnpm-lock.yaml`（保留 fork 的新工具链版本，**不引入** `tailwindcss-animate`，
> 改用 `tw-animate-css` 并同步锁文件）、`tailwind.config.cjs`（保持删除）、
> `session_usage.rs`（采用上游 `SessionRowOutcome` 重构）、
> `JsonEditor.tsx` / `MarkdownEditor.tsx`（取 fork 的 lazy 拆分，上游改动移植进 Impl）、
> `UsageDashboard.tsx`（取上游版 + 重放 fork 的 chart lazy 拆分）、
> `AboutSection.tsx` / `CopilotAuthSection.tsx`（取上游 v7 版 + 重放 lucide `Github` 内联 SVG）、
> `AuthCenterPanel.tsx`、`src/types/subscription.ts`（取上游，新增 `refresh_pending`）、
> `zh-TW.json`（合并工具链键与上游 `sections/appConfig/routing/data`）、
> `McodeProviderForm.test.tsx`（取上游 `name: "MiniMax"`，`PresetRow` 已带 `aria-label`）；
> 另 4 处 modify/delete（`ProviderActions` / `ProviderStatusBadge` /
> `ClaudeDesktopRouteToggle` / `ProviderActions.test`）经引用核查后**接受上游删除**
> （`ProviderActions` 的功能由上游新的 `ProviderCardActions` 接管）。
>
> **本轮最大的结构性差异是 Tailwind**：上游仍在 v3（`tailwind.config.cjs` +
> `tailwindcss-animate`），fork 已迁 v4。上游 v7 引入的全部设计令牌都写在 v3 config 里，
> 不搬进 `src/index.css` 的 `@theme` 就会**静默失效**。已逐条搬迁并验证（53 个 v7 令牌
> 工具类均出现在 `pnpm build:renderer` 产物 CSS 中），另补上 v3 的 `borderColor.DEFAULT`
> 语义与 `tw-animate-css`。详见 4.26。
>
> **测试隔离根因修复**：`get_app_config_dir()` 的 Windows v3.10.3 遗留回退忽略了
> `CC_SWITCH_TEST_HOME`，导致整套测试读写真实 `~/.cc-switch`。现在检测到该变量已设置
> 即跳过回退（6.3 记录的根治方向）。此一条解掉 `services::model_pricing`（3 个）与
> `services::skill`（1 个）的确定性失败。历史残留已征得同意后备份并清理（见 6.3）。
>
> **过期断言/快照修正**（合并前就是红的，非本轮回归）：
> `tests/golden/snapshots/mcp/gemini-settings.json` 按 fork 分歧重生成（4.27）；
> `tests/provider_commands.rs` 改为断言第三方 Key 写成路由表 `experimental_bearer_token`
> （4.15a 已交由上游 live 引擎实现）；`AppMatrix` 键盘焦点判定改用 `isKeyboardModality()`；
> `usage_stats` 的 12 元组表用例加 `#[allow(clippy::type_complexity)]`（Rust 1.95 阈值收紧）；
> `src/**` 下 15 个上游新文件按 fork 的 prettier 3.9 重排版。
>
> 验证（2026-10-04）：Rust `cargo test` 全套（lib **3440** + 集成 **179** + 金标 39）
> 0 failed、`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings` 全绿；
> 前端 `pnpm vitest run` **183 文件 / 2117 用例**全绿（连跑 2 次）、`pnpm typecheck` /
> `pnpm format:check` / `pnpm build:renderer` 全绿。集成测试二进制本机可跑通（见 6.5 新写的绕法）。
>
> 待办（未触碰）：6.1 的发布 tag 未打（上游未发新 tag，fork 版本号仍 `v3.20.4`）；
> 6.2 的更新链缺陷仍在。

> 2026-10-05 同步（**v4.0.0/v4.0.1，发布 v4.0.1**）：合入上游 `372b1698` 之后的
> **38 个提交**至 `4804b723`（116 文件 +5 222 / −763；29 新增 / 4 删除 / 83 修改）。
> 本轮是 v4.0.0 与 v4.0.1 的发布周期：托管预设扩编（88API 与兔子 API 赞助预设跨
> 九个 app、DMXAPI/BaiLing/TheRouter/Together AI/Astron 官方图标、Gemini 冗余自定义
> 预设下线）、订阅与配额增强（ChatGPT 限额重置倒计时与到期列表、卡片配额点击刷新
> 反馈、余额用尽前统一单色）、用量表格全面分页（请求日志/供应商/模型/定价）、
> Stack 聚合模型按上游 id 与窗口描述、Gemini CLI JSONL 会话读取、OpenCode 推理模型
> 标记、Grok Build 预设 API Key 链接修正、Claude Code onboarding 跳过提示。
>
> 代理侧：Codex Responses 零用量 `response.incomplete` 不再被报成 `max_tokens`
> （改为 `api_error`，`streaming_responses.rs` 新增 `is_unprocessed_max_output_rejection`，
> #7868）、Codex 状态库在 WSL 路径上跳过加锁（Windows 无法锁 9P）、父级 Codex 同步
> 不再被毒化缓存死锁，`config.rs` 新增 `is_wsl_path` 判定。
>
> **冲突 4 处，全部手工解**：
> - `usage_stats.rs`——采用上游 `type LogRow` 类型别名（取代 fork 的
>   `#[allow(clippy::type_complexity)]`，两者同为解决 Rust 1.95 阈值收紧，上游写法更根治）；
> - `ci.yml` / `wsl2-nightly.yml`——采纳上游 rust-cache + nextest + 从
>   `rust-toolchain.toml` 读编译器（新增 `src-tauri/.config/nextest.toml`），保留 fork 的
>   actions 版本（checkout@v7 / cache@v6 / setup-node@v7）、`clippy --all-targets`、
>   WSL2 步骤顺序（Setup WSL2 在编译前置步骤之后）与 nightly 的 `if: false`；
> - `release.yml`——**整文件取 fork 侧**（见新增的 4.28）：上游的 macOS 双 job 并行 +
>   fail-fast 签名校验与 fork 不配 secrets 的个人模式直接冲突。
>
> 版本号随 merge 对齐上游 `4.0.1`（`package.json` / `Cargo.toml` / `tauri.conf.json`
> 三处），并移除 fork 自 v3.19.2-a 起手写的 `wix.version = "3.19.2.1"`（见 2.5）。
>
> 分歧点核对：4.1–4.27 逐条复核全部保留——本轮上游改动集中在预设数据/配额与用量 UI/
> 会话读取/CI/发布，未触及 proxy 转换、分类器 fail-open、`atomic_write` 权限语义、
> deeplink `claude-desktop`、`session_usage.rs`（**本轮上游未动**，fork 的 upsert
> 守卫与单调推进逐行未变）、`hermes_config.rs` 排除表、NativeResponses 模板、
> lucide 内联 SVG、`transform_codex_chat.rs` 的 ultra→max 钐制（上游本轮对该文件只有
> 一处 clippy 修复，与 fork 版本一致）。上游新增的 4.28 已写入。
>
> Tailwind 核对（4.26）：上游本轮**未改** `tailwind.config.cjs`，`src/index.css`
> 也未被上游触碰，@theme 无需搬迁；已抽查 v4.0.1 新增组件
> （`QuotaBreakdown` / `TablePagination`）用到的令牌类（`bg-surface` / `text-fg-2` /
> `rounded-panel` / `shadow-v7-md` / `border-border-strong` / `text-caption` 等）均出现在
> `pnpm build:renderer` 产物 CSS 中。
>
> 验证（2026-10-05）：Rust `cargo test -j 4` 全套绿（lib **3454** + 集成 **179**，
> 含金标 39）、`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings` 全绿；
> 前端 `pnpm vitest run --maxWorkers=8` **186 文件 / 2161 用例**全绿、`pnpm typecheck` /
> `pnpm format:check` / `pnpm build:renderer` 全绿。合并提交 `e2b24ae5`。
> ⚠️ 本机 `node_modules` 在本次开始时有两处包目录为空（typescript@7.0.2、
> @typescript/typescript-win32-x64），已 `rm -rf node_modules` 后按锁文件重装。
>
> **发布（2026-10-05）**：tag `v4.0.1` → fork 提交 `32e4aa39`（推 `origin/main`
> 后 `git tag -f` 显式重建，符合 6.1），Release run 37268765647 全绿——
> Resolve Version 6s、五个平台构建 11–22 分钟（macOS universal 22m6s）、
> Publish 33s、Assemble latest.json 5s、Sync to R2 2s（无 secrets 自动跳过），
> **14 个资产**、正式版（Latest）。六路（`Ubuntu-22.04` / `ubuntu-22.04-arm` /
> `windows-2022` / `windows-11-arm` / `macos-14`）构建全部产出，Linux arm64
> AppImage 未再静默缺失。latest.json 的 `platforms` 仍为空（无签名，见 6.2）。
>
> **附带的 CI 修复**：push 后的 CI run 在 macOS-latest 与 ubuntu-22.04 的
> `backend` 矩阵红于 golden `file_modes::switch_creates_expected_files_and_modes`
> ——上游快照 `snapshots/modes/fresh-after-third-party.txt` 期望
> `.codex/cc-switch-model-catalog.json` 与 `.gemini/settings.json` 为 644
> （上游 umask 默认），而 fork 的 `atomic_write` 在 unix 上一律创建即 0600
> （4.5）。这是 **2026-10-04 那轮 CI 就已存在的红**（非本轮回归）。已按 CI 的
> actual 输出（6 行全 600）重生成快照（`013f4f2f`），与 4.27 的 Gemini 快照同理
> ——上游若再改它，仍须按 fork 行为重生成。修复后手动跑了一次完整 CI
> （`workflow_dispatch`，run 37270697230）：**五个 job 全部绿**——Frontend 4m29s、
> WSL2 4m15s、ubuntu 8m35s、windows 12m24s、macos 10m5s（macOS/Linux 的红首次消除）。
> 注：docs-only 的 push 会被 ci.yml 的路径过滤跳过两端 job，想用一次 push 同时
> 验证源码与文档不可行；且同 concurrency 组的连续 push 会 `cancel-in-progress`
> 取消上一次（快照修复的首次验证即因此被 docs push 取消），验证需用
> `gh workflow run ci.yml` 手动触发。

## 6. 维护约定

- **上游同步**：`git fetch upstream --no-tags && git merge upstream/main`，merge 后跑
  `cargo test --lib` + `pnpm vitest run` + `cargo fmt/clippy` 全绿再提交
  （用例规模以第 1 节为准）。必须显式带 `--no-tags`：命令行 `--tags` 会**覆盖** 6.1
  配置的 `remote.upstream.tagOpt=--no-tags`，把上游标签拉进本地。
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

  - 2026-09-23 同步时一次 `git fetch upstream --tags` 绕过了该配置（命令行 `--tags` 会
    **覆盖** `remote.upstream.tagOpt`），把上游新标签 `v3.20.4`（指向上游提交）拉进了本地；
    已用 `git tag -d v3.20.4` 删除且未推送。日常同步 fetch 务必显式带 `--no-tags`。

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

### 6.3 Windows 本地跑 Rust 测试必须隔离 `HOME`（2026-09-15 发现，2026-10-04 已根治）

> **状态：根因已修复（2026-10-04）**。`get_app_config_dir()` 现在检测到
> `CC_SWITCH_TEST_HOME` 已设置（非空）即跳过那层遗留回退，隔离真正生效——不再需要
> 手工把 `HOME` 指向临时目录，也不再会把夹具写进真实 `~/.cc-switch`。
> 修复后 `cargo test` 全套（含集成测试）绿；已知历史残留的清理见本节末。
> 下面保留原始记录以说明根因与判断依据。

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

- **根治方向**（2026-10-04 已实施）：检测到 `CC_SWITCH_TEST_HOME` 已设置即跳过该回退
  （`src-tauri/src/config.rs`，`test_home_isolated` 守卫）。未采用「收紧为 `$HOME` 与真实
  用户目录不同」那条——那会改变生产环境下的遗留库兼容语义，风险更大。
- **历史残留清理**（2026-10-04，已征得同意后执行）：受污染内容整体移动到
  `~/.cc-switch/test-pollution-backup-20261004/`，真实目录只留用户自己的内容：
  - `model-pricing.json`（夹具：假模型 `custom-model`、`deletedModelIds:
    ["claude-sonnet-5"]`、`includeCommonModels: false`、`lastSyncError: "offline"`）→
    已备份后重置为干净值（空 `models` / 空 `deletedModelIds` / `includeCommonModels: true`），
    并**显式保留 `autoSyncEnabled: false`**——上游同步（`d76a1cda`）把该默认值改成了 `true`，
    写 false 是为了不静默改变你当前的生效行为；想要新的上游默认就在设置页打开。
  - `skills/{conflict,fresh,good-skill,native-skill,shared-name,valid}`（全是
    "Test skill" 夹具）与 `skill-backups/` 下 16 个 `*test-skill*`、
    1 个 `20260727_120000_evil`（路径穿越安全用例）一并移出；`skills/pebrel-runtime`
    与 3 个真实 `nature-*`/`pebrel-runtime` 备份保留。
  - `settings.json` 已被覆盖且**无备份可还原**（见上），需在应用设置页人工核对托盘／代理／
    会话自动同步／skill 存储位置等开关。

### 6.4 前端全量测试在 32 线程机器上的超时边界（2026-09-23 发现，2026-10-04 已修掉本轮发现的三处）

本机（i9-13900HX，32 逻辑核）跑 `pnpm vitest run` 默认并发（`maxWorkers = cpus-1 = 31`）时，
全档会稳定产出 1 个失败，且每次失败的长用例不同：

- `tests/components/ProviderForm.codexManagedAccount.test.tsx`「requires confirmation before
  falling back when a selected account disappears」——默认预算 5s，并发下实测 **5.14s**
  （单文件跑 1.3s）；
- `tests/integration/App.test.tsx`「covers basic provider flows via real hooks」——自带 10s 预算，
  单独跑已需 8.3s，并发下被顶穿。

根因是环境开销量级：141 个测试文件各建一次 jsdom（累计 ≈512s，占 whole-run tracked time 47%），
31 个 worker 争抢 CPU 时长用例被拖慢约 4 倍。**不是代码回归**——`--testTimeout=30000` 的全量跑
141 文件全绿，单文件跑也全绿。

规避（已验证，且总耗时基本不变，≈73s）：

```bash
pnpm vitest run --maxWorkers=8
```

建议（未擅自实施，会连带改变 CI 行为，需人工定夺）：在 `vitest.config.ts` 固定 `maxWorkers`，
或给这两个长用例留出显式预算，否则每次全档跑都可能随机红一个。v3.20.3 之前本地也出现过同类
边界失败。

**2026-10-04 补充：本轮又修掉三处并发下的偶发红**（都不改 `maxWorkers`）：

1. `JsonEditor.test.tsx`、`UsageDashboard.smoke.test.tsx`——两处都在等**懒加载 chunk**
   （CodeMirror / recharts）到货，并行时超过 `waitFor` 默认的 1s。已显式给 5s 超时。
   ⚠️ 后续再写「等 Impl / 图表出现」的用例，一律带 `{ timeout: 5000 }`。
2. `PromptPanel.test.tsx` 的 `waitForPanelReady()`——只等行按钮 `toBeEnabled()`，而按钮灰显
   走 `useDelayedFlag`（延迟 300 ms 才变灰），点开的守卫却是立即生效的 `interactionBlocked`，
   于是挂载时那次重读没结束时点下去会被吞掉。已在 helper 里拍一拍事件循环（`setTimeout(0)`）。
3. `AddProviderDialog.test.tsx` 的两个 claude 用例——v8 改成「先选预设 → 再填表」两步后，
   mock 的 ProviderForm 不注册预设选择器，需要等自动进第二步（改 `findByRole`）。

验证方式：连跑 2 次全量（183 文件 / 2117 用例）均全绿。

### 6.5 本机 `cargo test --tests` 无法链接（环境问题，已有可用绕法）

2026-10-03 同步时发现：本机跑 `cargo test --tests`（或裸 `cargo test`）会在**集成测试二进制**的
链接阶段失败，报错形如：

```
error[E0462]: found staticlib `displaydoc` instead of rlib or dylib which `cc_switch_lib` depends on
error: crate `walkdir` required to be available in rlib format, but was not found in this form
error: only metadata stub found for `rlib` dependency `alloc` ... / cannot resolve a prelude import
```

与代码无关：`cargo check --all-targets`（全部 test target 类型检查）、`cargo clippy --all-targets`、
`cargo test --lib`（3276 用例）都正常。推测本机 target 目录中依赖的 rlib/staticlib 形态被
`[lib] crate-type = ["staticlib","cdylib","rlib"]` + 集成测试的链接需求混用所致；一次
`cargo clean -p cc-switch` 也不能修复。

规避：日常按本文档的口径验证 —— `cargo test --lib` + `cargo clippy --all-targets -- -D warnings`
+ `cargo fmt --check`；如需完整集成测试（`src-tauri/tests/*`）建议在 CI 或 WSL 中跑。
根治方向（未擅自实施）：给集成测试目标单独跑 `CARGO_TARGET_DIR`，或把 `crate-type` 中的
`staticlib`/`cdylib` 移到仅在打包时启用。

**2026-10-04 补充：实际根因是页面文件耗尽，不是 rlib 形态。** 当天的报错首行是
`memory allocation of 146170702 bytes failed` + `failed to mmap file
'target/debug/deps/libcc_switch_lib.rlib'（页面文件太小，无法完成操作。os error 1455）`
——即链接巨型的 lib 测试 rlib（~1.3 GB）时虚拟内存不够，**mmap 失败后留下一个损坏的 rlib**，
后续所有 target 才报 E0463/E0786/“cannot find crate”这类迷惑错误（看起来像形态问题）。
可用绕法（已实测跑通，含 17 个集成测试二进制）：

```bash
# 1) 删掉损坏的 rlib/rmeta（cargo 不会自己发现它坏了）
rm -f src-tauri/target/debug/deps/libcc_switch_lib*.rlib \
      src-tauri/target/debug/deps/libcc_switch_lib*.rmeta
# 2) 降并发重编（默认全程并发会在链接阶段再次把页面文件顶穿）
cd src-tauri && cargo test -j 4
```

前置条件：跑之前确认物理内存/页面文件有余量（同时跑 vitest 全量会吃掉十几 GB）。
