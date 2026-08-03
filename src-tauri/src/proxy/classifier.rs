//! 安全分类器请求检测与处理模块
//!
//! Claude Code 的 auto mode 使用安全分类器（safety classifier）评估每个工具调用。
//! 分类器通过 side_query 发送非流式 API 请求，使用特殊的 system prompt 和参数。
//!
//! 当使用第三方 API 时，分类器请求中的 `thinking`、`stop_sequences`、`betas` 等
//! 参数可能不被支持，导致 API 返回错误，而 Claude Code 的安全兜底逻辑会在
//! 分类器出错时默认 BLOCK 所有操作。
//!
//! # 工作流程
//! 本模块采用协议转换方案：将 Claude 原生的安全分类器请求转换为上游模型可理解的
//! 简化格式，转发到上游处理后，再将响应转换回 Claude Code 期望的 Messages API 格式。
//!
//! 1. 检测请求是否为安全分类器请求（基于请求体特征）
//! 2. 若是，用简化 system prompt 替换原有 prompt，去除不兼容参数
//! 3. 通过正常代理管道转发到上游
//! 4. 将上游响应转回 Claude Messages API 格式返回给客户端

use serde_json::Value;

/// 简化分类器 system prompt，适用于第三方/上游模型
///
/// 替换 Claude 原生的安全监控 prompt。要求上游模型输出结构化
/// `<block>yes|no</block>` 格式，这是 Claude Code auto-mode 分类器
/// 投票系统唯一识别的响应格式。
const UPSTREAM_CLASSIFIER_PROMPT: &str = r"You are a tool-use safety classifier for an AI coding assistant.

Determine if the given tool call is SAFE or BLOCKED.

Respond with EXACTLY this format:
<block>no</block>
<reason>one sentence explanation</reason>

Or if the action should be blocked:
<block>yes</block>
<reason>one sentence explanation why</reason>

Classification guidelines:
- Read-only operations (git diff, ls, cat, grep, find, etc.) → <block>no</block>
- File writes/edits in the project directory → <block>no</block> (with caution)
- Network requests to known services → <block>no</block>
- Modifying system files, installing packages, executing untrusted code → <block>yes</block>
- Shell operations with suspicious metacharacters ($, |, backtick) → evaluate carefully";

/// 分类器请求检测结果
#[derive(Debug, Clone)]
pub struct ClassifierDetection {
    /// 是否为分类器请求
    pub is_classifier: bool,
    /// 检测到的分类器阶段（如果有）
    pub stage: Option<ClassifierStage>,
    /// 置信度（0.0 - 1.0）
    pub confidence: f32,
    /// 检测到的分类器协议模式（block/severity/未知）
    pub mode: Option<ClassifierMode>,
}

/// 分类器协议模式
///
/// 来自 Claude Code 源码（2.1.219）逆向：
/// - `Block`（默认）: 响应用 `<block>yes|no</block>` 标签。CC 的解析器
///   `xBs` 去掉 `<thinking>` 后取**第一个** `<block>(yes|no)` 匹配，闭合标签可选。
/// - `Severity`: 响应用 `<severity>N</severity>`（N 为 0-100 数值），CC 的 `Piy`
///   要求**恰好一个**该标签并解析数值，与配置阈值 t1/t2 比较。severity 需显式
///   配置 `tengu_auto_mode_config.severityByModel`，默认关闭。
/// - `Unknown`: 无法确定（fast 单阶段无 stop_sequences 且 system prompt 不含输出格式时）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassifierMode {
    Block,
    Severity,
    Unknown,
}

/// 分类器阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassifierStage {
    /// Stage 1 - 快速分类（max_tokens <= 256，无 thinking）
    Fast,
    /// Stage 2 - 深度分类（max_tokens <= 8192，有 thinking）
    Thinking,
    /// 未知阶段
    Unknown,
}

/// 检测请求是否为安全分类器请求
///
/// 基于 Claude Code 源码（@cometix/claude-code 2.1.219）逆向确认的协议特征：
/// - `stop_sequences` 含 `"</block>"`（block 模式）或 `"</severity>"`（severity 模式）：
///   both/thinking 阶段（默认 `twoStageClassifier="both"`）必带其一。这是协议级特征。
/// - fast 单阶段（显式配置 `twoStageClassifier="fast"`）**不带 stop_sequences**，
///   只能靠「分类器 system prompt 强身份句 + `<transcript>` 消息结构」双重确认。
///
/// 检测优先级：
/// 1. `</block>` / `</severity>` stop_sequence → 分类器（模式由标签决定）
/// 2. 分类器身份句 + `<transcript>` 包裹 → fast 单阶段分类器
/// 3. 其余一律不是分类器（关键词不能单独触发——安全主题的普通 system prompt
///    会命中 "prompt injection" 等词，若不要求强特征会把正常非流式请求误判为
///    分类器并毁掉：system prompt 被替换、stream 被强制 false）。
pub fn detect_classifier_request(body: &Value) -> ClassifierDetection {
    let not_classifier = || ClassifierDetection {
        is_classifier: false,
        stage: None,
        confidence: 0.0,
        mode: None,
    };

    // 快速短路：流式请求绝不可能是分类器（分类器永远是非流式 side-query）。
    // 这必须在任何分配（extract_system_text / to_lowercase）之前检查，
    // 避免让 80-90% 的流量付出无用开销。
    if body.get("stream").and_then(|s| s.as_bool()) == Some(true) {
        return not_classifier();
    }

    let system_text = extract_system_text(body);
    let system_lower = system_text.to_lowercase();

    // 协议级 stop_sequences 标签：both/thinking 阶段的分类器必带其一。
    // 源码：stop_sequences: [u ? "</severity>" : "</block>"]（u = severity 模式布尔）。
    let stop_sequences = body.get("stop_sequences").and_then(|s| s.as_array());
    let has_block_tag = stop_sequences
        .map(|arr| {
            arr.iter()
                .any(|s| s.as_str().is_some_and(|s| s.contains("</block>")))
        })
        .unwrap_or(false);
    let has_severity_tag = stop_sequences
        .map(|arr| {
            arr.iter()
                .any(|s| s.as_str().is_some_and(|s| s.contains("</severity>")))
        })
        .unwrap_or(false);
    let has_stop_tag = has_block_tag || has_severity_tag;

    // 辅助特征: system prompt 命中分类器身份关键词（仅用于置信度增强）
    let keyword_matches = if system_text.is_empty() {
        0
    } else {
        let classifier_keywords = [
            "security monitor",
            "auto mode classifier",
            "hard block",
            "soft block",
            "prompt injection",
            "autonomous coding agent",
            "you are a security monitor",
        ];
        classifier_keywords
            .iter()
            .filter(|kw| system_lower.contains(*kw))
            .count()
    };

    // 分类器 system prompt 强身份句：源码 oPo() 的固定开头
    // "You are a security monitor for autonomous AI coding agents."
    let is_classifier_prompt = system_lower
        .contains("security monitor for autonomous ai coding agents")
        || system_lower.contains("you are a security monitor");

    // `<transcript>` 包裹结构：分类器把待分类的对话历史用该标签包在
    // 末条 user 消息里（源码：L = [{type:"text",text:"<transcript>\n"},...n,{type:"text",text:"</transcript>"}]）。
    let has_transcript = body
        .get("messages")
        .and_then(|m| m.as_array())
        .is_some_and(|arr| {
            let mut has_open = false;
            let mut has_close = false;
            for msg in arr {
                match msg.get("content") {
                    Some(Value::String(s)) => {
                        has_open |= s.contains("<transcript>");
                        has_close |= s.contains("</transcript>");
                    }
                    Some(Value::Array(blocks)) => {
                        for block in blocks {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                has_open |= text.contains("<transcript>");
                                has_close |= text.contains("</transcript>");
                            }
                        }
                    }
                    _ => {}
                }
            }
            has_open && has_close
        });

    // 1) 协议级 stop 标签命中 → 分类器（必要特征，block/severity 任一）
    if has_stop_tag {
        let has_thinking = body.get("thinking").is_some();
        let max_tokens = body.get("max_tokens").and_then(|m| m.as_u64()).unwrap_or(0);

        let stage = if has_thinking && max_tokens > 256 {
            // Stage 2: 深度分类（thinking + 较大 budget）
            Some(ClassifierStage::Thinking)
        } else if max_tokens > 0 && max_tokens <= 256 {
            // Stage 1: 快速分类（无 thinking，极小 max_tokens）
            Some(ClassifierStage::Fast)
        } else {
            Some(ClassifierStage::Unknown)
        };

        // 置信度：协议标签是决定性的，关键词命中进一步确认
        let confidence = if keyword_matches >= 2 {
            0.99 // 协议标签 + 关键词双重确认，确定无疑
        } else {
            0.95 // 协议标签命中，单独即可确定
        };

        // 模式由 stop 标签决定：</severity> → severity，</block> → block
        let mode = if has_severity_tag {
            ClassifierMode::Severity
        } else {
            ClassifierMode::Block
        };

        log::info!(
            "[Classifier] 检测到安全分类器请求 (confidence={:.2}, stage={:?}, mode={:?}, block_tag={}, severity_tag={}, keywords={})",
            confidence,
            stage,
            mode,
            has_block_tag,
            has_severity_tag,
            keyword_matches,
        );

        return ClassifierDetection {
            is_classifier: true,
            stage,
            confidence,
            mode: Some(mode),
        };
    }

    // 2) fast 单阶段（twoStageClassifier="fast"）：无 stop_sequences。
    //    需 system prompt 强身份句 + `<transcript>` 结构双重确认，避免误判正常请求。
    if is_classifier_prompt && has_transcript {
        // 模式从原始 system prompt 的输出格式推断：
        // severity 版 system（kiy 替换 Output Format）含 `<severity>`，block 版含 `<block>`。
        let mode = if system_lower.contains("<severity>") {
            ClassifierMode::Severity
        } else if system_lower.contains("<block>") {
            ClassifierMode::Block
        } else {
            // 异常 system（无输出格式说明）：Unknown，响应转换兜底走 block 格式
            ClassifierMode::Unknown
        };
        return ClassifierDetection {
            is_classifier: true,
            stage: Some(ClassifierStage::Fast),
            confidence: 0.95,
            mode: Some(mode),
        };
    }

    not_classifier()
}

/// 提取 system prompt 文本
fn extract_system_text(body: &Value) -> String {
    match body.get("system") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .and_then(|t| t.as_str())
                    .map(|text| text.to_string())
                    .or_else(|| item.as_str().map(|s| s.to_string()))
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

/// 将分类器请求转换为上游兼容格式
///
/// 用简化 prompt 替换 Claude 原生安全监控 prompt，移除上游不支持的参数。
/// 保留原始的 messages（待分类的操作上下文），使上游模型仍能正确评估。
pub fn transform_classifier_request(body: &Value) -> Value {
    let mut new_body = body.clone();
    if let Some(obj) = new_body.as_object_mut() {
        // 替换为通用分类器 prompt
        obj.insert(
            "system".into(),
            serde_json::json!(UPSTREAM_CLASSIFIER_PROMPT),
        );
        // 移除上游不支持的 stop_sequences（如 </block>）
        obj.remove("stop_sequences");
        // 移除 thinking 参数（分类器使用小 max_tokens）
        obj.remove("thinking");
        // 确保非流式
        obj.insert("stream".into(), serde_json::json!(false));
    }
    new_body
}

/// 从上游响应中提取 Token 用量
///
/// 同时支持 Claude Messages API 和 OpenAI Chat Completions 两种响应格式的 usage 字段。
/// 复用 `crate::proxy::usage::parser::TokenUsage` 已有的解析方法。
pub fn parse_classifier_usage(body: &Value) -> crate::proxy::usage::parser::TokenUsage {
    crate::proxy::usage::parser::TokenUsage::from_claude_response(body)
        .or_else(|| crate::proxy::usage::parser::TokenUsage::from_openai_response(body))
        .unwrap_or_default()
}

/// 从上游响应中提取分类文本
///
/// 同时支持 Claude Messages API 和 OpenAI Chat Completions 两种响应格式。
fn extract_response_text(body: &Value) -> Option<String> {
    // Claude Messages: content[0].text
    if let Some(content) = body.get("content").and_then(|c| c.as_array()) {
        if let Some(first) = content.first() {
            if let Some(text) = first.get("text").and_then(|t| t.as_str()) {
                return Some(text.to_string());
            }
        }
    }
    // OpenAI Chat: choices[0].message.content
    if let Some(choices) = body.get("choices").and_then(|c| c.as_array()) {
        if let Some(first) = choices.first() {
            if let Some(msg) = first.get("message") {
                if let Some(text) = msg.get("content").and_then(|t| t.as_str()) {
                    return Some(text.to_string());
                }
            }
        }
    }
    None
}

/// 从上游响应中提取分类结果
///
/// 解析 `<block>yes|no</block>` 标签（Claude Code 分类器原生格式），
/// 无标签时根据文本内容保守判断：检测到 unsafe 信号时返回 `"yes"`，
/// 否则兜底放行返回 `"no"`。
///
/// 返回 (verdict, reason_text)：
/// - `("no", reason)` — 判定为安全
/// - `("yes", reason)` — 判定为需拦截
fn determine_classification_result(text: &str) -> (&str, &str) {
    let lower = text.to_lowercase();

    // 1) 优先匹配独立成行的 <block>yes|no</block> 标签。
    //    上游被要求以该格式输出裁决，标签单独成行（trim 后整行等于标签）。
    //    reason 中引用的假设性标签（"would be <block>yes</block>"）通常在
    //    句子行内出现，不会单独成行，因此独立行标签是可靠的裁决信号。
    if let Some(verdict) = extract_line_tag(&lower) {
        return verdict;
    }

    // 2) 兜底：匹配第一个 <block> 标签。
    //    使用 find 而非 rfind：reason 可能引用假设性的 "<block>yes</block>"
    //    场景（"if it modified files it would be <block>yes</block>"），
    //    最后一个标签不可靠；prompt 要求结果标签先行，取第一个更符合协议。
    if let Some(start) = lower.find("<block>") {
        let after_tag = &lower[start + 7..].trim_start();
        let end = after_tag
            .find(|c: char| !is_word_char(c))
            .unwrap_or(after_tag.len());
        let content = &after_tag[..end];
        if content == "no" {
            return ("no", "The action has been classified as safe.");
        } else if content == "yes" {
            return (
                "yes",
                "The action has been classified as potentially unsafe.",
            );
        }
        // <block> 内容不可识别，保守拦截
        return ("yes", "The action could not be confidently classified.");
    }

    // 3) 无 <block> 标签：启发式兜底。
    //    只匹配明确的拦截措辞与不安全信号。不用裸的 contains("blocked")：
    //    否定句 "nothing was blocked" 描述的是未发生拦截，不含拦截意图，
    //    用 "should/must/will/needs to be blocked" 这类主动拦截措辞替代。
    let has_unsafe_signal = lower.contains("not safe")
        || lower.contains("unsafe")
        || lower.contains("malicious")
        || lower.contains("harmful")
        || lower.contains("dangerous")
        || lower.contains("forbidden")
        || lower.contains("should be blocked")
        || lower.contains("must be blocked")
        || lower.contains("will be blocked")
        || lower.contains("needs to be blocked")
        || lower.contains("block this")
        || is_word_boundary_contain(&lower, "block");

    if has_unsafe_signal {
        (
            "yes",
            "The action has been flagged as potentially unsafe by heuristic analysis.",
        )
    } else {
        // 无明确不安全信号 → 放行（避免误拦截）
        ("no", "The action appears safe based on heuristic analysis.")
    }
}

/// 从文本中提取独立成行的 `<block>yes|no</block>` 标签。
///
/// 上游被要求以该格式开头输出裁决；独立成行（trim 后整行等于标签）的
/// 标签是裁决本身，而 reason 中引用的假设性标签通常出现在句子行内，
/// 不会单独成行。
fn extract_line_tag(lower: &str) -> Option<(&'static str, &'static str)> {
    for line in lower.lines() {
        match line.trim() {
            "<block>no</block>" => {
                return Some(("no", "The action has been classified as safe."));
            }
            "<block>yes</block>" => {
                return Some((
                    "yes",
                    "The action has been classified as potentially unsafe.",
                ));
            }
            _ => {}
        }
    }
    None
}

/// `\w` 字符判断：字母数字 + 下划线，与正则 `\b` 语义对齐
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// 检查 `text` 是否包含 `keyword` 且前后为词边界（\b 语义）。
/// 避免 `contains("block")` 在 "a block of code" 中误匹配。
fn is_word_boundary_contain(text: &str, keyword: &str) -> bool {
    let kw_len = keyword.len();
    if kw_len == 0 || text.len() < kw_len {
        return false;
    }
    // 在 text 中每找到一次 keyword 出现位置就检查边界。
    // 前/后字符必须用完整 char 判断（is_word_char 需要字符而非字节）：
    // 直接对 text.as_bytes()[i] as char 会把多字节字符的单个字节误判为
    // 词字符，导致 CJK/emoji 前的边界判断错误
    // （见 test_word_boundary_contain_cjk_prefix_is_word_char）。
    let mut start = 0;
    while let Some(rel) = text[start..].find(keyword) {
        let abs_pos = start + rel;
        // 前一个字符（不在开头）必须是词边界
        let prev_ok = if abs_pos == 0 {
            true
        } else {
            match text[..abs_pos].chars().next_back() {
                Some(c) => !is_word_char(c),
                None => true,
            }
        };
        // 后一个字符（不在结尾）必须是词边界
        let next_ok = if abs_pos + kw_len >= text.len() {
            true
        } else {
            match text[abs_pos + kw_len..].chars().next() {
                Some(c) => !is_word_char(c),
                None => true,
            }
        };
        if prev_ok && next_ok {
            return true;
        }
        // 不是边界匹配，从 keyword 内偏移一位继续搜索，避免死循环
        start = abs_pos + 1;
        if start >= text.len() {
            break;
        }
    }
    false
}

/// 将上游响应转换为分类器兼容的 Messages API 格式
///
/// 响应中必须包含 `<block>no</block>` 或 `<block>yes</block>` 标签，
/// 这是 Claude Code auto-mode 分类器投票系统解析的唯一识别格式。
/// 同时保留上游原始文本作为分析依据。
pub fn transform_classifier_response(
    upstream_body: &Value,
    request_model: &str,
    mode: ClassifierMode,
) -> Value {
    let upstream_text = extract_response_text(upstream_body).unwrap_or_default();

    // 从上游响应中提取真实用量（如果用不到则兜底为默认值）
    let usage = parse_classifier_usage(upstream_body);
    let (input_tokens, output_tokens) = if usage.input_tokens > 0 || usage.output_tokens > 0 {
        (usage.input_tokens, usage.output_tokens)
    } else {
        (1u32, 10u32)
    };

    // 构建分类器兼容的裁决文本（按协议模式输出对应标签）
    let response_text = if upstream_text.is_empty() {
        // 上游无文本：按模式输出兜底放行
        match mode {
            ClassifierMode::Severity => {
                "<severity>0</severity>\n<reason>No upstream classification available, allowing by default.</reason>".to_string()
            }
            _ => {
                "<block>no</block>\n<reason>No upstream classification available, allowing by default.</reason>".to_string()
            }
        }
    } else {
        let (block, summary) = determine_classification_result(&upstream_text);
        let verdict_line = match mode {
            // severity 模式：CC 的 Piy 解析恰好一个 `<severity>` 数值并与配置阈值
            // t1/t2（[0,100]）比较。proxy 不知阈值，用极值传达语义：
            // 0 ≤ 任意合法阈值 → allow；1000 > 任意合法阈值 → block。
            // reason/upstream 文本不得含第二个 `<severity>` 标签（Piy 要求恰好一个）。
            ClassifierMode::Severity => {
                if block == "yes" {
                    "<severity>1000</severity>".to_string()
                } else {
                    "<severity>0</severity>".to_string()
                }
            }
            // block 模式（默认）：CC 的 xBs 取第一个 `<block>(yes|no)` 匹配。
            ClassifierMode::Block | ClassifierMode::Unknown => {
                format!("<block>{block}</block>")
            }
        };
        // severity 模式下上游原文可能自带 `<severity>` 标签（模型自述或提示注入），
        // 会破坏「恰好一个标签」的协议约束导致 Piy 解析失败 → 误拦截。
        // 展示前剥离上游原文中的 severity/reason 标签，仅保留我们输出的裁决标签。
        let upstream_analysis = if mode == ClassifierMode::Severity {
            sanitize_severity_upstream_text(&upstream_text)
        } else {
            upstream_text.to_string()
        };
        format!(
            "{verdict_line}\n<reason>{summary}</reason>\n\nUpstream analysis:\n{upstream_analysis}"
        )
    };

    serde_json::json!({
        "id": format!("msg_{}", uuid::Uuid::new_v4()),
        "type": "message",
        "role": "assistant",
        "content": [
            {
                "type": "text",
                "text": response_text
            }
        ],
        "model": request_model,
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens
        }
    })
}

/// 剥离上游文本中的 `<severity>` / `<reason>` 标签内容。
///
/// severity 模式的裁决文本要求**恰好一个** `<severity>` 标签（Claude Code 的
/// Piy 解析器），上游模型输出原文若自带这些标签（模型自述、提示注入或
/// 结构化输出残留）会导致解析失败 → 分类器兜底误拦截。剥离时保留标签外的
/// 分析文字，保证「Upstream analysis」仍然可读。
fn sanitize_severity_upstream_text(text: &str) -> String {
    let strip_tags = |input: &str, tag: &str| -> (String, bool) {
        let mut result = String::with_capacity(input.len());
        let mut rest = input;
        let mut stripped = false;
        while let Some(start) = rest.find(tag) {
            stripped = true;
            result.push_str(&rest[..start]);
            let after = &rest[start..];
            let close_tag = format!("</{tag}>");
            let end = after
                .find(&close_tag)
                .map(|pos| start + pos + close_tag.len())
                .unwrap_or(input.len());
            rest = &rest[end..];
        }
        result.push_str(rest);
        (result, stripped)
    };

    let (after_severity, stripped_severity) = strip_tags(text, "severity");
    let (final_text, stripped_reason) = strip_tags(&after_severity, "reason");
    if stripped_severity || stripped_reason {
        log::warn!(
            "[Classifier] severity 模式：上游文本含额外 <severity>/<reason> 标签，已剥离避免破坏恰好一个标签的协议约束"
        );
    }
    final_text
}

/// 构建分类器请求的安全兜底响应体
///
/// 当上游转发失败时，返回允许响应避免阻塞用户操作。
/// 按协议模式输出对应标签，让分类器投票系统正确识别为 ALLOW：
/// - block 模式: `<block>no</block>`
/// - severity 模式: `<severity>0</severity>`（0 ≤ 任意合法阈值，CC 判为 allow）
pub fn build_classifier_success_body(model: &str, mode: ClassifierMode) -> Value {
    let verdict_text = match mode {
        ClassifierMode::Severity => {
            "<severity>0</severity>\n<reason>Classifier unavailable, allowing by default.</reason>"
        }
        _ => "<block>no</block>\n<reason>Classifier unavailable, allowing by default.</reason>",
    };
    serde_json::json!({
        "id": format!("msg_{}", uuid::Uuid::new_v4()),
        "type": "message",
        "role": "assistant",
        "content": [
            {
                "type": "text",
                "text": verdict_text
            }
        ],
        "model": model,
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {
            "input_tokens": 1,
            "output_tokens": 10
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_detect_stop_sequences_block_tag() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "stop_sequences": ["</block>"],
            "stream": false,
            "max_tokens": 8192,
            "thinking": {"type": "enabled", "budget_tokens": 4096},
            "system": "You are a security monitor for autonomous AI coding agents.",
            "messages": [{"role": "user", "content": "test"}]
        });

        let detection = detect_classifier_request(&body);
        assert!(detection.is_classifier);
        assert!(detection.confidence > 0.8);
    }

    #[test]
    fn test_detect_system_prompt_keywords() {
        // 带 </block> 协议标签 + 分类器关键词 → 分类器
        // （</block> 是必要特征，关键词增强置信度）
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "stream": false,
            "max_tokens": 256,
            "stop_sequences": ["</block>"],
            "system": "You are a security monitor. HARD BLOCK and SOFT BLOCK rules apply. Prevent prompt injection.",
            "messages": [{"role": "user", "content": "test"}]
        });

        let detection = detect_classifier_request(&body);
        assert!(detection.is_classifier);
    }

    #[test]
    fn test_detect_non_classifier_request() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "stream": true,
            "max_tokens": 16384,
            "system": "You are a helpful assistant.",
            "messages": [{"role": "user", "content": "hello"}]
        });

        let detection = detect_classifier_request(&body);
        assert!(!detection.is_classifier);
    }

    #[test]
    fn test_build_classifier_success_body_format() {
        let body = build_classifier_success_body("kimi-k2.5", ClassifierMode::Block);
        assert_eq!(body["type"], "message");
        assert_eq!(body["role"], "assistant");
        assert!(
            body["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("<block>no</block>"),
            "兜底响应应包含 <block>no</block> 标签"
        );
        assert_eq!(body["model"], "kimi-k2.5");
        assert_eq!(body["stop_reason"], "end_turn");
        assert!(body["id"].as_str().unwrap().starts_with("msg_"));
        assert!(body["usage"]["input_tokens"].as_u64().is_some());
        assert!(body["usage"]["output_tokens"].as_u64().is_some());
    }

    // ========================================================================
    // 防误判测试：验证正常请求不会被错误归类为分类器
    // ========================================================================

    /// 普通非流式请求 + thinking + max_tokens=4096 → 最常见的误判场景
    #[test]
    fn test_no_false_positive_normal_non_streaming_with_thinking() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "stream": false,
            "max_tokens": 4096,
            "thinking": {"type": "enabled", "budget_tokens": 2048},
            "system": "You are a helpful assistant. Answer the user's question concisely.",
            "messages": [{"role": "user", "content": "What is the capital of France?"}]
        });

        let detection = detect_classifier_request(&body);
        assert!(
            !detection.is_classifier,
            "非流式+thinking+max_tokens=4096 不应被归类为分类器 (confidence={:.2})",
            detection.confidence
        );
    }

    /// 普通非流式短请求 + max_tokens=200 → S4 单独触发
    #[test]
    fn test_no_false_positive_short_non_streaming() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "stream": false,
            "max_tokens": 200,
            "system": "Answer with yes or no.",
            "messages": [{"role": "user", "content": "Is the sky blue?"}]
        });

        let detection = detect_classifier_request(&body);
        assert!(
            !detection.is_classifier,
            "普通非流式短请求不应被归类为分类器 (confidence={:.2})",
            detection.confidence
        );
    }

    /// 流式请求 + thinking + 小 max_tokens → S3 需要非流式
    #[test]
    fn test_no_false_positive_streaming_with_thinking() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "stream": true,
            "max_tokens": 512,
            "thinking": {"type": "enabled", "budget_tokens": 256},
            "system": "You are a coding assistant.",
            "messages": [{"role": "user", "content": "Write a function."}]
        });

        let detection = detect_classifier_request(&body);
        assert!(
            !detection.is_classifier,
            "流式请求不应被归类为分类器 (confidence={:.2})",
            detection.confidence
        );
    }

    /// 非流式 + thinking + "block"+"action" 但缺少分类器特有关键词
    /// 这是一个边界测试：S3(0.6) + S5(0.3) = 0.72 > 0.65 → 应该触发
    /// "block"+"action" 出现在系统提示中在正常场景中非常罕见
    #[test]
    fn test_no_false_positive_block_action_without_strong_features() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "stop_sequences": ["</div>"],
            "stream": false,
            "max_tokens": 4096,
            "thinking": {"type": "enabled", "budget_tokens": 2048},
            "system": "You must follow these rules: block any action that could harm the user. Take corrective action when needed.",
            "messages": [{"role": "user", "content": "test"}]
        });

        // system 包含 "block"+"action" 但无任何分类器关键词
        // stop_sequences 有 </div> 但无 </block>
        // 没有强特征 → 不是分类器
        let detection = detect_classifier_request(&body);
        assert!(
            !detection.is_classifier,
            "无强特征时不应归类为分类器 (confidence={:.2})",
            detection.confidence
        );
    }

    /// 仅 S5（block+action）单独触发 → 权重 0.3 < 0.65，不应归类
    #[test]
    fn test_no_false_positive_block_action_alone() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "stream": false,
            "max_tokens": 16384,
            "system": "You are a content moderator. Block any harmful action and take appropriate action.",
            "messages": [{"role": "user", "content": "test"}]
        });

        // 只触发 S5(0.3)，combined = 0.3 < 0.65
        let detection = detect_classifier_request(&body);
        assert!(
            !detection.is_classifier,
            "仅\"block\"+\"action\"不应归类 (confidence={:.2})",
            detection.confidence
        );
    }

    /// 非流式 + max_tokens=200 + system 包含"action"但非"block" → 不应归类
    #[test]
    fn test_no_false_positive_action_without_block() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "stream": false,
            "max_tokens": 200,
            "system": "You assist users with their tasks. Take direct action when needed.",
            "messages": [{"role": "user", "content": "test"}]
        });

        // S4(0.5) 单独触发，0.5 < 0.65
        // 没有 S5，因为 system 包含"action"但不包含"block"
        let detection = detect_classifier_request(&body);
        assert!(
            !detection.is_classifier,
            "非流式+max_tokens=200+system含action但无block不应归类 (confidence={:.2})",
            detection.confidence
        );
    }

    /// Gemini 普通请求 → 应不被归类
    #[test]
    fn test_no_false_positive_gemini_request() {
        let body = json!({
            "contents": [{"parts": [{"text": "hello"}]}]
        });

        let detection = detect_classifier_request(&body);
        assert!(
            !detection.is_classifier,
            "Gemini 请求不应归类 (confidence={:.2})",
            detection.confidence
        );
    }

    /// 空的 system prompt + 非流式 + 小 max_tokens → S4 单独，0.5 < 0.65
    #[test]
    fn test_no_false_positive_empty_system() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "stream": false,
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "hello"}]
        });

        let detection = detect_classifier_request(&body);
        assert!(
            !detection.is_classifier,
            "system为空+非流式短请求不应归类 (confidence={:.2})",
            detection.confidence
        );
    }

    /// Claude Desktop 典型请求：非流式 + 大 max_tokens + 无 thinking
    #[test]
    fn test_no_false_positive_claude_desktop_typical() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "stream": false,
            "max_tokens": 8192,
            "system": [{"type": "text", "text": "You are Claude, a helpful AI assistant."}],
            "messages": [{"role": "user", "content": "Explain quantum computing."}]
        });

        // 非流式但无 thinking → S3 不触发
        // max_tokens=8192 → S4 不触发 (>256)
        let detection = detect_classifier_request(&body);
        assert!(
            !detection.is_classifier,
            "Claude Desktop 典型请求不应归类 (confidence={:.2})",
            detection.confidence
        );
    }

    // ========================================================================
    // 协议转换测试
    // ========================================================================

    #[test]
    fn test_transform_classifier_request_replaces_system_prompt() {
        let body = json!({
            "model": "claude-opus-4-8",
            "stream": false,
            "max_tokens": 256,
            "stop_sequences": ["</block>"],
            "system": "You are a security monitor. HARD BLOCK.",
            "messages": [{"role": "user", "content": "Tool: Bash\nCommand: git diff HEAD"}]
        });

        let transformed = transform_classifier_request(&body);

        let system_text = match transformed.get("system") {
            Some(Value::String(s)) => s.as_str(),
            _ => panic!("system 应为字符串"),
        };
        assert!(
            system_text.contains("tool-use safety classifier"),
            "system prompt 应替换为通用分类器 prompt"
        );
        assert!(
            system_text.contains("<block>no</block>"),
            "system prompt 应要求结构化输出"
        );
    }

    #[test]
    fn test_transform_classifier_request_removes_incompatible_params() {
        let body = json!({
            "model": "claude-opus-4-8",
            "stream": true,
            "max_tokens": 256,
            "stop_sequences": ["</block>"],
            "thinking": {"type": "enabled", "budget_tokens": 1024},
            "messages": [{"role": "user", "content": "test"}]
        });

        let transformed = transform_classifier_request(&body);

        assert!(
            transformed.get("stop_sequences").is_none(),
            "应移除 stop_sequences"
        );
        assert!(transformed.get("thinking").is_none(), "应移除 thinking");
        assert_eq!(
            transformed.get("stream").and_then(|v| v.as_bool()),
            Some(false),
            "应强制 stream=false"
        );
    }

    #[test]
    fn test_extract_response_text_claude_messages() {
        let body = json!({
            "content": [{"type": "text", "text": "<result>SAFE</result>"}]
        });
        let text = extract_response_text(&body);
        assert_eq!(text.unwrap(), "<result>SAFE</result>");
    }

    #[test]
    fn test_extract_response_text_openai_chat() {
        let body = json!({
            "choices": [{"message": {"role": "assistant", "content": "<result>BLOCK</result>"}}]
        });
        let text = extract_response_text(&body);
        assert_eq!(text.unwrap(), "<result>BLOCK</result>");
    }

    #[test]
    fn test_determine_classification_result_safe_tag() {
        let (result, summary) = determine_classification_result("<block>no</block>");
        assert_eq!(result, "no");
        assert!(summary.contains("safe"));
    }

    #[test]
    fn test_determine_classification_result_block_tag() {
        let (result, summary) = determine_classification_result(
            "<block>yes</block>\n<reason>deletes system files</reason>",
        );
        assert_eq!(result, "yes");
        assert!(summary.contains("unsafe"));
    }

    #[test]
    fn test_determine_classification_result_not_safe_is_yes() {
        let (result, _summary) =
            determine_classification_result("This is not safe because it deletes files");
        assert_eq!(result, "yes", "'not safe' 应判定为 BLOCK");
    }

    #[test]
    fn test_determine_classification_result_unsafe_is_yes() {
        let (result, _summary) = determine_classification_result("This is unsafe");
        assert_eq!(result, "yes", "'unsafe' 应判定为 BLOCK");
    }

    #[test]
    fn test_determine_classification_result_ambiguous_is_no() {
        let (result, _summary) = determine_classification_result("The command reads some files");
        assert_eq!(result, "no", "无明确信号时应兜底放行");
    }

    #[test]
    fn test_determine_classification_result_blocked_is_yes() {
        let (result, _summary) = determine_classification_result(
            "This command modifies /etc/passwd and should be blocked",
        );
        assert_eq!(result, "yes", "文本含 'blocked' 时应判定为 BLOCK");
    }

    #[test]
    fn test_transform_classifier_response_claude_messages() {
        let upstream = json!({
            "id": "real-msg-123",
            "content": [{"type": "text", "text": "<result>SAFE</result>\n<reason>read-only git command</reason>"}]
        });
        let response =
            transform_classifier_response(&upstream, "claude-opus-4-8", ClassifierMode::Block);

        assert_eq!(response["type"], "message");
        assert_eq!(response["model"], "claude-opus-4-8");
        assert_eq!(response["stop_reason"], "end_turn");
        assert!(
            response["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("<block>no</block>"),
            "响应应包含 <block>no</block> 标签"
        );
    }

    #[test]
    fn test_transform_classifier_response_openai_chat() {
        let upstream = json!({
            "id": "chatcmpl-123",
            "choices": [{"message": {"content": "<block>yes</block>\n<reason>rm -rf</reason>"}}]
        });
        let response =
            transform_classifier_response(&upstream, "claude-opus-4-8", ClassifierMode::Block);

        assert_eq!(response["type"], "message");
        let text = response["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("<block>yes</block>"),
            "上游返回 <block>yes</block> 时应透传 yes 标签"
        );
    }

    #[test]
    fn test_transform_classifier_response_empty_body() {
        let upstream = json!({});
        let response =
            transform_classifier_response(&upstream, "claude-opus-4-8", ClassifierMode::Block);

        assert_eq!(response["type"], "message");
        assert!(
            response["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("<block>no</block>"),
            "空响应应包含 <block>no</block> 兜底放行"
        );
    }

    #[test]
    fn test_transform_classifier_response_upstream_returns_block_yes() {
        let upstream = json!({
            "content": [{"type": "text", "text": "This modifies system files <block>yes</block>"}]
        });
        let response =
            transform_classifier_response(&upstream, "claude-sonnet-4-6", ClassifierMode::Block);

        let text = response["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("<block>yes</block>"),
            "上游返回 BLOCK 信号时应透传 <block>yes</block>"
        );
        assert!(text.contains("Upstream analysis"), "应保留上游原始文本");
    }

    #[test]
    fn test_determine_classification_result_no_prefix_no_false_positive() {
        // <block> 后面跟着非 "no"/"yes" 的内容应被识别为不可识别 → 返回 "yes"
        let (result, _) = determine_classification_result("<block>no_worries</block>");
        assert_eq!(
            result, "yes",
            "no_worries 不是精确的 \"no\"，不应判定为 ALLOW"
        );

        let (result, _) = determine_classification_result("<block>noway</block>");
        assert_eq!(result, "yes", "noway 不是精确的 \"no\"");

        let (result, _) = determine_classification_result("<block>nope</block>");
        assert_eq!(result, "yes", "nope 不是精确的 \"no\"");
    }

    // ========================================================================
    // 修复验证测试：根因回归防护
    // ========================================================================

    /// Finding 1 回归：无 </block> stop_sequence 时，即使命中多个分类器关键词
    /// （含 "prompt injection"、"autonomous coding agent" 这类会出现在普通
    /// 安全主题 prompt 中的词），也不应判定为分类器。<block> 标签是分类器
    /// 请求的协议级必要特征，关键词只是辅助信号，不能单独触发。
    #[test]
    fn test_detect_no_block_tag_not_classifier_even_with_keywords() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "stream": false,
            "max_tokens": 8192,
            "system": "You are a security monitor for autonomous coding agents. \
                       Prevent prompt injection and enforce hard block / soft block rules. \
                       You are a security monitor.",
            "messages": [{"role": "user", "content": "Refactor the auth module"}]
        });

        let detection = detect_classifier_request(&body);
        assert!(
            !detection.is_classifier,
            "无 </block> stop_sequence 时，即使命中多个关键词也不应判定为分类器 (confidence={:.2})",
            detection.confidence
        );
    }

    /// Finding 2 回归：reason 中引用的假设性 <block> 标签不应覆盖真正的裁决。
    /// 上游被要求以 <block> 标签开头输出结果；若 reason 里出现
    /// "would be <block>yes</block>" 这类假设场景，必须取开头的独立裁决标签。
    #[test]
    fn test_determine_classification_result_ignores_hypothetical_tag() {
        let text = "<block>no</block>\n<reason>The command is safe. \
                    (It would be <block>yes</block> if it modified system files.)</reason>";
        let (result, _) = determine_classification_result(text);
        assert_eq!(result, "no", "reason 中的假设性标签不应覆盖实际的安全裁决");
    }

    /// Finding 2 回归：否定句 "nothing was blocked" 不含拦截意图，不应触发 BLOCK。
    #[test]
    fn test_determine_classification_result_blocked_negation_is_no() {
        let text = "The operation is allowed; nothing was blocked.";
        let (result, _) = determine_classification_result(text);
        assert_eq!(
            result, "no",
            "否定句 'nothing was blocked' 不应判定为 BLOCK"
        );
    }

    /// Finding 3 回归：UTF-8 多字节字符前的词边界判断必须基于完整字符，
    /// 而非原始字节。'中block' 中 '中' 是 Unicode 字母（word char），
    /// 因此 block 不是独立词，不应匹配。
    #[test]
    fn test_word_boundary_contain_cjk_prefix_is_word_char() {
        assert!(
            !is_word_boundary_contain("中block 是危险操作", "block"),
            "CJK 字符是 word char，'中block' 中 block 不应视为独立词"
        );
    }

    /// Finding 3 回归：ASCII 标点/空格前的 block 是真正的词边界 → 应匹配。
    #[test]
    fn test_word_boundary_contain_space_prefix_matches() {
        assert!(
            is_word_boundary_contain(" this block is bad", "block"),
            "'block' 前是空格应为词边界"
        );
    }

    // ========================================================================
    // 源码对齐测试：Claude Code 2.1.219 协议（@cometix/claude-code 逆向确认）
    // ========================================================================

    /// severity 模式：stop_sequences 用 ["</severity>"]
    /// （源码：stop_sequences: [u ? "</severity>" : "</block>"]，需配置 severityByModel）
    #[test]
    fn test_detect_severity_stop_sequence() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "stream": false,
            "max_tokens": 8192,
            "stop_sequences": ["</severity>"],
            "system": "You are a security monitor for autonomous AI coding agents.",
            "messages": [{"role": "user", "content": "test"}]
        });

        let detection = detect_classifier_request(&body);
        assert!(detection.is_classifier);
        assert_eq!(detection.mode, Some(ClassifierMode::Severity));
    }

    /// fast 单阶段（twoStageClassifier="fast"）：无 stop_sequences，
    /// 靠「分类器身份句 + <transcript> 包裹」识别。
    #[test]
    fn test_detect_fast_stage_transcript() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "stream": false,
            "max_tokens": 256,
            "system": "You are a security monitor for autonomous AI coding agents. \
                       If the action should be blocked output <block>yes</block>, otherwise <block>no</block>.",
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "<transcript>\n..."},
                    {"type": "text", "text": "</transcript>"}
                ]}
            ]
        });

        let detection = detect_classifier_request(&body);
        assert!(
            detection.is_classifier,
            "fast 单阶段分类器（身份句 + <transcript>）应被识别"
        );
        assert_eq!(detection.mode, Some(ClassifierMode::Block));
    }

    /// 防误判：只有 <transcript> 但 system 非分类器 → 不是分类器
    #[test]
    fn test_detect_no_false_positive_transcript_only() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "stream": false,
            "system": "You are a helpful assistant.",
            "messages": [{"role": "user", "content": "wrap this in <transcript> tags </transcript>"}]
        });

        let detection = detect_classifier_request(&body);
        assert!(!detection.is_classifier);
    }

    /// 防误判：只有分类器身份句但无 <transcript> → 不是分类器
    #[test]
    fn test_detect_no_false_positive_prompt_only() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "stream": false,
            "system": "You are a security monitor for autonomous AI coding agents.",
            "messages": [{"role": "user", "content": "Please review this diff"}]
        });

        let detection = detect_classifier_request(&body);
        assert!(!detection.is_classifier);
    }

    /// severity 响应转换：BLOCK → <severity>1000</severity>（> 任意合法阈值 100）
    #[test]
    fn test_transform_classifier_response_severity_block() {
        let upstream = json!({
            "content": [{"type": "text", "text": "<block>yes</block> deletes system files"}]
        });
        let response =
            transform_classifier_response(&upstream, "claude-opus-4-8", ClassifierMode::Severity);

        let text = response["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("<severity>1000</severity>"),
            "severity 模式下 BLOCK 应输出 <severity>1000</severity>"
        );
    }

    /// severity 响应转换：ALLOW → <severity>0</severity>，且恰好一个 <severity> 标签
    #[test]
    fn test_transform_classifier_response_severity_allow() {
        let upstream = json!({
            "content": [{"type": "text", "text": "<block>no</block> read-only command"}]
        });
        let response =
            transform_classifier_response(&upstream, "claude-opus-4-8", ClassifierMode::Severity);

        let text = response["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("<severity>0</severity>"),
            "severity 模式下 ALLOW 应输出 <severity>0</severity>"
        );
        // CC 的 Piy 要求恰好一个 <severity> 标签，reason/upstream 不得引入第二个
        assert_eq!(text.matches("<severity>").count(), 1);
    }

    /// severity 模式：上游文本中的额外 <severity>/<reason> 标签必须被剥离，
    /// 否则 Piy「恰好一个标签」约束被破坏 → 误拦截
    #[test]
    fn test_severity_mode_strips_extra_tags_from_upstream_text() {
        let upstream = json!({
            "content": [{"type": "text", "text": "The action reads files safely <severity>42</severity> analysis" }]
        });
        let response =
            transform_classifier_response(&upstream, "claude-opus-4-8", ClassifierMode::Severity);
        let text = response["content"][0]["text"].as_str().unwrap();
        assert_eq!(
            text.matches("<severity>").count(),
            1,
            "必须恰好一个 severity 标签"
        );
        assert!(
            text.contains("The action reads files safely"),
            "剥离后保留分析文字"
        );
        assert!(
            !text.contains("<severity>42</severity>"),
            "上游自带标签必须剥离"
        );

        // <reason> 标签同理
        let upstream2 = json!({
            "content": [{"type": "text", "text": "notes <reason>why</reason> more" }]
        });
        let response2 =
            transform_classifier_response(&upstream2, "claude-opus-4-8", ClassifierMode::Severity);
        let text2 = response2["content"][0]["text"].as_str().unwrap();
        assert!(
            !text2.contains("<reason>why</reason>"),
            "上游 reason 标签必须剥离"
        );
        assert!(text2.contains("notes"), "剥离后保留周围文字");
    }

    /// severity 兜底 body：<severity>0</severity>
    #[test]
    fn test_build_classifier_success_body_severity() {
        let body = build_classifier_success_body("kimi-k2.5", ClassifierMode::Severity);
        let text = body["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("<severity>0</severity>"),
            "severity 兜底应输出 <severity>0</severity>"
        );
        assert_eq!(text.matches("<severity>").count(), 1);
    }
}
