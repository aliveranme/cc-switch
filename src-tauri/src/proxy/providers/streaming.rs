//! 流式响应转换模块
//!
//! 实现 OpenAI SSE → Anthropic SSE 格式转换

use crate::proxy::sse::{strip_sse_field, take_sse_block};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// OpenAI 流式响应数据结构
#[derive(Debug, Deserialize)]
struct OpenAIStreamChunk {
    #[serde(default)]
    id: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: Delta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    // OpenAI 结构化输出的流式拒绝走 delta.refusal。非流式 openai_to_anthropic
    // 会把它转成文本块，流式此前完全没读，导致拒绝时客户端收到空的成功消息。
    #[serde(default)]
    refusal: Option<String>,
    // OpenRouter/Kimi/其它 使用 reasoning，DeepSeek 使用 reasoning_content
    #[serde(default, alias = "reasoning_content")]
    reasoning: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<DeltaToolCall>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct DeltaToolCall {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "type", default)]
    call_type: Option<String>,
    #[serde(default)]
    function: Option<DeltaFunction>,
}

#[derive(Debug, Deserialize, Serialize)]
struct DeltaFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// OpenAI 流式响应的 usage 信息（完整版）
#[derive(Debug, Deserialize)]
struct Usage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    completion_tokens_details: Option<CompletionTokensDetails>,
    #[serde(default)]
    prompt_tokens_details: Option<PromptTokensDetails>,
    /// Some compatible servers return Anthropic-style cache fields directly
    #[serde(default)]
    cache_read_input_tokens: Option<u32>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u32>,
}

/// Nested completion details from OpenAI format.
#[derive(Debug, Deserialize)]
struct CompletionTokensDetails {
    #[serde(default)]
    reasoning_tokens: u32,
}

/// Nested token details from OpenAI format
#[derive(Debug, Deserialize)]
struct PromptTokensDetails {
    #[serde(default)]
    cached_tokens: u32,
    #[serde(default)]
    cache_write_tokens: u32,
}

#[derive(Debug, Clone)]
struct ToolBlockState {
    anthropic_index: u32,
    id: String,
    name: String,
    started: bool,
    pending_args: String,
    /// 连续空白字符计数 — 用于检测 Copilot 无限换行 bug
    /// 当 function call 参数中的连续空白字符达到阈值时，强制终止流
    consecutive_whitespace: usize,
    /// 是否已因无限空白 bug 被中止
    aborted: bool,
}

/// 无限空白 bug 的连续空白字符阈值
const INFINITE_WHITESPACE_THRESHOLD: usize = 500;

fn build_anthropic_usage_json(usage: &Usage) -> Value {
    // OpenAI prompt_tokens 含缓存，Anthropic input_tokens 不含，需减去 cache_read 与 cache_creation
    // （三桶互斥，恒等 input + cache_read + cache_creation == prompt_tokens）。
    let cached = extract_cache_read_tokens(usage).unwrap_or(0);
    let cache_creation = extract_cache_write_tokens(usage).unwrap_or(0);
    let input_tokens = usage
        .prompt_tokens
        .saturating_sub(cached)
        .saturating_sub(cache_creation);
    let mut usage_json = json!({
        "input_tokens": input_tokens,
        "output_tokens": usage.completion_tokens
    });
    if cached > 0 {
        usage_json["cache_read_input_tokens"] = json!(cached);
    }
    if cache_creation > 0 {
        usage_json["cache_creation_input_tokens"] = json!(cache_creation);
    }
    if let Some(reasoning_tokens) = usage
        .completion_tokens_details
        .as_ref()
        .map(|details| details.reasoning_tokens)
        .filter(|tokens| *tokens > 0)
    {
        usage_json["output_tokens_details"] = json!({"thinking_tokens": reasoning_tokens});
    }
    usage_json
}

fn default_anthropic_usage_json() -> Value {
    json!({
        "input_tokens": 0,
        "output_tokens": 0
    })
}

fn build_message_delta_event(stop_reason: Option<String>, usage_json: Option<Value>) -> Value {
    let usage = usage_json
        .filter(|usage| usage.is_object())
        .unwrap_or_else(default_anthropic_usage_json);

    json!({
        "type": "message_delta",
        "delta": {
            "stop_reason": stop_reason,
            "stop_sequence": null
        },
        "usage": usage
    })
}

/// 创建 Anthropic SSE 流
pub fn create_anthropic_sse_stream<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let mut message_id = None;
        let mut current_model = None;
        let mut next_content_index: u32 = 0;
        let mut has_sent_message_start = false;
        // 某些上游 provider（如 OpenRouter 的 kimi-k2.6）会在 tool_use 后发送多个
        // 带 finish_reason 的 SSE chunk。Anthropic 协议要求每个消息流只能有一个
        // message_delta，重复会导致 Claude Code abort 连接。因此需要：
        // 1) has_emitted_message_delta: 去重，只处理第一个 finish_reason
        // 2) pending_message_delta: 缓存延迟到 [DONE] 发送，确保 usage 完整
        let mut has_emitted_message_delta = false;
        let mut pending_message_delta: Option<(Option<String>, Option<Value>)> = None;
        let mut has_sent_message_stop = false;
        let mut stream_ended_with_error = false;
        let mut latest_usage: Option<Value> = None;
        let mut current_non_tool_block_type: Option<&'static str> = None;
        let mut current_non_tool_block_index: Option<u32> = None;
        let mut tool_blocks_by_index: HashMap<usize, ToolBlockState> = HashMap::new();
        let mut open_tool_block_indices: HashSet<u32> = HashSet::new();
        // Chat 流式工具调用保序指针：只按连续 index 释放 content_block_start
        let mut next_tool_start_index: usize = 0;

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    crate::proxy::sse::append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);

                    while let Some(line) = take_sse_block(&mut buffer) {
                        if line.trim().is_empty() {
                            continue;
                        }

                        // SSE 规范允许一个事件携带多条 data: 行，必须以 \n 连接后
                        // 整体解析（多行 JSON 事件拆行解析会全部失败并静默丢块）。
                        // 与 streaming_codex_chat.rs 的 join 行为保持一致。
                        let data_payloads: Vec<&str> = line
                            .lines()
                            .filter_map(|l| strip_sse_field(l, "data"))
                            .collect();
                        if data_payloads.is_empty() {
                            continue;
                        }
                        let data = data_payloads.join("\n");

                        if data.trim() == "[DONE]" {
                                    log::debug!("[Claude/OpenRouter] <<< OpenAI SSE: [DONE]");

                                    // 防御：异常上游可能发多个 [DONE]——终态已发出
                                    // 时直接跳过，避免重复 message_delta / message_stop
                                    // （Anthropic 协议要求每个消息流恰好一个终止序列）。
                                    if has_sent_message_stop {
                                        continue;
                                    }

                                    // 截断流防护：上游漏发 finish_reason 时（[DONE]
                                    // 前无 choices 终止事件），已打开的内容块必须闭合，
                                    // 否则 Claude Code 收到「message_stop 无块 stop」
                                    // 的残缺序列。若从未有 finish_reason，补一个
                                    // end_turn 终止事件（有实质输出时）。
                                    if let Some(index) = current_non_tool_block_index.take() {
                                        let event = json!({
                                            "type": "content_block_stop",
                                            "index": index
                                        });
                                        let sse_data = format!("event: content_block_stop\ndata: {}\n\n",
                                            serde_json::to_string(&event).unwrap_or_default());
                                        yield Ok(Bytes::from(sse_data));
                                    }
                                    current_non_tool_block_type = None;
                                    if !open_tool_block_indices.is_empty() {
                                        let mut tool_indices: Vec<u32> =
                                            open_tool_block_indices.iter().copied().collect();
                                        tool_indices.sort_unstable();
                                        for index in tool_indices {
                                            let event = json!({
                                                "type": "content_block_stop",
                                                "index": index
                                            });
                                            let sse_data = format!("event: content_block_stop\ndata: {}\n\n",
                                                serde_json::to_string(&event).unwrap_or_default());
                                            yield Ok(Bytes::from(sse_data));
                                        }
                                        open_tool_block_indices.clear();
                                    }

                                    // 流正常结束，发出缓存的 message_delta（含完整 usage）。
                                    // 无 finish_reason 但有实质输出时补 end_turn，避免客户端挂起。
                                    if let Some((stop_reason, usage_json)) = pending_message_delta.take() {
                                        let event = build_message_delta_event(stop_reason, usage_json);
                                        let sse_data = format!("event: message_delta\ndata: {}\n\n",
                                            serde_json::to_string(&event).unwrap_or_default());
                                        log::debug!("[Claude/OpenRouter] >>> Anthropic SSE: message_delta (from pending)");
                                        yield Ok(Bytes::from(sse_data));
                                    } else if has_sent_message_start {
                                        log::warn!(
                                            "[Claude/OpenRouter] 上游在 [DONE] 前未发送 finish_reason，补发 end_turn 终止事件"
                                        );
                                        let event = build_message_delta_event(
                                            Some("end_turn".to_string()),
                                            latest_usage.clone(),
                                        );
                                        let sse_data = format!("event: message_delta\ndata: {}\n\n",
                                            serde_json::to_string(&event).unwrap_or_default());
                                        yield Ok(Bytes::from(sse_data));
                                    }

                                    let event = json!({"type": "message_stop"});
                                    let sse_data = format!("event: message_stop\ndata: {}\n\n",
                                        serde_json::to_string(&event).unwrap_or_default());
                                    log::debug!("[Claude/OpenRouter] >>> Anthropic SSE: message_stop");
                                    yield Ok(Bytes::from(sse_data));
                                    has_sent_message_stop = true;
                                    continue;
                                }

                                if let Ok(chunk) = serde_json::from_str::<OpenAIStreamChunk>(&data) {
                                    log::debug!("[Claude/OpenRouter] <<< SSE chunk received");

                                    if message_id.is_none() && !chunk.id.is_empty() {
                                        message_id = Some(chunk.id.clone());
                                    }
                                    if current_model.is_none() && !chunk.model.is_empty() {
                                        current_model = Some(chunk.model.clone());
                                    }

                                    let chunk_usage_json =
                                        chunk.usage.as_ref().map(build_anthropic_usage_json);
                                    if let Some(usage_json) = &chunk_usage_json {
                                        latest_usage = Some(usage_json.clone());
                                        if let Some((_, pending_usage)) = pending_message_delta.as_mut() {
                                            *pending_usage = Some(usage_json.clone());
                                        }
                                    }

                                    if let Some(choice) = chunk.choices.first() {
                                        if !has_sent_message_start {
                                            // Build usage with cache tokens if available from first chunk
                                            let mut start_usage = json!({
                                                "input_tokens": 0,
                                                "output_tokens": 0
                                            });
                                            if let Some(u) = &chunk.usage {
                                                let cached = extract_cache_read_tokens(u).unwrap_or(0);
                                                let cache_creation =
                                                    extract_cache_write_tokens(u).unwrap_or(0);
                                                let input = u
                                                    .prompt_tokens
                                                    .saturating_sub(cached)
                                                    .saturating_sub(cache_creation);
                                                start_usage["input_tokens"] = json!(input);
                                                if cached > 0 {
                                                    start_usage["cache_read_input_tokens"] = json!(cached);
                                                }
                                                if cache_creation > 0 {
                                                    start_usage["cache_creation_input_tokens"] =
                                                        json!(cache_creation);
                                                }
                                            }

                                            let event = json!({
                                                "type": "message_start",
                                                "message": {
                                                    "id": message_id.clone().unwrap_or_default(),
                                                    "type": "message",
                                                    "role": "assistant",
                                                    "model": current_model.clone().unwrap_or_default(),
                                                    "usage": start_usage
                                                }
                                            });
                                            let sse_data = format!("event: message_start\ndata: {}\n\n",
                                                serde_json::to_string(&event).unwrap_or_default());
                                            yield Ok(Bytes::from(sse_data));
                                            has_sent_message_start = true;
                                        }

                                        // 处理 reasoning（thinking）
                                        if let Some(reasoning) = &choice.delta.reasoning {
                                            if current_non_tool_block_type != Some("thinking") {
                                                if let Some(index) = current_non_tool_block_index.take() {
                                                    let event = json!({
                                                        "type": "content_block_stop",
                                                        "index": index
                                                    });
                                                    let sse_data = format!("event: content_block_stop\ndata: {}\n\n",
                                                        serde_json::to_string(&event).unwrap_or_default());
                                                    yield Ok(Bytes::from(sse_data));
                                                }
                                                let index = next_content_index;
                                                next_content_index += 1;
                                                let event = json!({
                                                    "type": "content_block_start",
                                                    "index": index,
                                                    "content_block": {
                                                        "type": "thinking",
                                                        "thinking": ""
                                                    }
                                                });
                                                let sse_data = format!("event: content_block_start\ndata: {}\n\n",
                                                    serde_json::to_string(&event).unwrap_or_default());
                                                yield Ok(Bytes::from(sse_data));
                                                current_non_tool_block_type = Some("thinking");
                                                current_non_tool_block_index = Some(index);
                                            }

                                            if let Some(index) = current_non_tool_block_index {
                                                let event = json!({
                                                    "type": "content_block_delta",
                                                    "index": index,
                                                    "delta": {
                                                        "type": "thinking_delta",
                                                        "thinking": reasoning
                                                    }
                                                });
                                                let sse_data = format!("event: content_block_delta\ndata: {}\n\n",
                                                    serde_json::to_string(&event).unwrap_or_default());
                                                yield Ok(Bytes::from(sse_data));
                                            }
                                        }

                                        // 处理文本内容
                                        // content 与 refusal 都映射到 Anthropic 的
                                        // text 块（非流式路径亦然）；同一 chunk 只会
                                        // 有其一，故复用同一段逻辑，取非空的那个。
                                        let text_piece = choice
                                            .delta
                                            .content
                                            .as_deref()
                                            .filter(|s| !s.is_empty())
                                            .or_else(|| {
                                                choice
                                                    .delta
                                                    .refusal
                                                    .as_deref()
                                                    .filter(|s| !s.is_empty())
                                            });
                                        if let Some(content) = text_piece {
                                            if !content.is_empty() {
                                                if current_non_tool_block_type != Some("text") {
                                                    if let Some(index) = current_non_tool_block_index.take() {
                                                        let event = json!({
                                                            "type": "content_block_stop",
                                                            "index": index
                                                        });
                                                        let sse_data = format!("event: content_block_stop\ndata: {}\n\n",
                                                            serde_json::to_string(&event).unwrap_or_default());
                                                        yield Ok(Bytes::from(sse_data));
                                                    }

                                                    let index = next_content_index;
                                                    next_content_index += 1;
                                                    let event = json!({
                                                        "type": "content_block_start",
                                                        "index": index,
                                                        "content_block": {
                                                            "type": "text",
                                                            "text": ""
                                                        }
                                                    });
                                                    let sse_data = format!("event: content_block_start\ndata: {}\n\n",
                                                        serde_json::to_string(&event).unwrap_or_default());
                                                    yield Ok(Bytes::from(sse_data));
                                                    current_non_tool_block_type = Some("text");
                                                    current_non_tool_block_index = Some(index);
                                                }

                                                if let Some(index) = current_non_tool_block_index {
                                                    let event = json!({
                                                        "type": "content_block_delta",
                                                        "index": index,
                                                        "delta": {
                                                            "type": "text_delta",
                                                            "text": content
                                                        }
                                                    });
                                                    let sse_data = format!("event: content_block_delta\ndata: {}\n\n",
                                                        serde_json::to_string(&event).unwrap_or_default());
                                                    yield Ok(Bytes::from(sse_data));
                                                }
                                            }
                                        }

                                        // 处理工具调用
                                        if let Some(tool_calls) = &choice.delta.tool_calls {
                                            if !tool_calls.is_empty() {
                                                if let Some(index) = current_non_tool_block_index.take() {
                                                    let event = json!({
                                                        "type": "content_block_stop",
                                                        "index": index
                                                    });
                                                    let sse_data = format!("event: content_block_stop\ndata: {}\n\n",
                                                        serde_json::to_string(&event).unwrap_or_default());
                                                    yield Ok(Bytes::from(sse_data));
                                                }
                                                current_non_tool_block_type = None;

                                                // 1) 累积 delta 到 map：anthropic index 推迟到 start 发出时
                                                //    分配（保序），此处只累积 id/name/arguments。
                                                //    abort（无限空白 bug）后的块跳过后续处理。
                                                let mut immediate_deltas: Vec<(u32, String)> = Vec::new();
                                                for tool_call in tool_calls {
                                                    let state = tool_blocks_by_index
                                                        .entry(tool_call.index)
                                                        .or_insert_with(|| ToolBlockState {
                                                            anthropic_index: 0,
                                                            id: String::new(),
                                                            name: String::new(),
                                                            started: false,
                                                            pending_args: String::new(),
                                                            consecutive_whitespace: 0,
                                                            aborted: false,
                                                        });

                                                    if state.aborted {
                                                        continue;
                                                    }

                                                    if let Some(id) = &tool_call.id {
                                                        state.id = id.clone();
                                                    }
                                                    if let Some(function) = &tool_call.function {
                                                        if let Some(name) = &function.name {
                                                            state.name = name.clone();
                                                        }
                                                    }

                                                    if let Some(args) = tool_call
                                                        .function
                                                        .as_ref()
                                                        .and_then(|f| f.arguments.clone())
                                                    {
                                                        // 无限空白 bug 检测：跟踪连续空白字符
                                                        for ch in args.chars() {
                                                            if ch.is_whitespace() {
                                                                state.consecutive_whitespace += 1;
                                                            } else {
                                                                state.consecutive_whitespace = 0;
                                                            }
                                                        }
                                                        if state.consecutive_whitespace
                                                            >= INFINITE_WHITESPACE_THRESHOLD
                                                        {
                                                            log::warn!(
                                                                "[Copilot] 检测到无限空白 bug (tool: {}), 中止此 tool call 流",
                                                                state.name
                                                            );
                                                            state.aborted = true;
                                                            continue;
                                                        }
                                                        if state.started {
                                                            immediate_deltas.push((
                                                                state.anthropic_index,
                                                                args,
                                                            ));
                                                        } else {
                                                            state.pending_args.push_str(&args);
                                                        }
                                                    }
                                                }

                                                // 2) 按连续 Chat index 释放 ready 块（保序）：
                                                //    只有当前最小未释放 index 的身份碎片齐备时才发出
                                                //    content_block_start，晚到的碎片不会重排已发出的块。
                                                let mut ready_starts: Vec<(u32, String, String, String)> =
                                                    Vec::new();
                                                loop {
                                                    let Some(state) = tool_blocks_by_index
                                                        .get_mut(&next_tool_start_index)
                                                    else {
                                                        break;
                                                    };
                                                    if state.aborted {
                                                        next_tool_start_index += 1;
                                                        continue;
                                                    }
                                                    if state.started {
                                                        next_tool_start_index += 1;
                                                        continue;
                                                    }
                                                    if state.id.is_empty() || state.name.is_empty() {
                                                        // 身份碎片未到齐：等待后续 chunk
                                                        break;
                                                    }
                                                    let anthropic_index = next_content_index;
                                                    next_content_index += 1;
                                                    state.anthropic_index = anthropic_index;
                                                    state.started = true;
                                                    let pending =
                                                        std::mem::take(&mut state.pending_args);
                                                    ready_starts.push((
                                                        anthropic_index,
                                                        state.id.clone(),
                                                        state.name.clone(),
                                                        pending,
                                                    ));
                                                    next_tool_start_index += 1;
                                                }

                                                // 3) 发出 start / pending args / immediate delta
                                                for (anthropic_index, id, name, pending) in ready_starts {
                                                    let event = json!({
                                                        "type": "content_block_start",
                                                        "index": anthropic_index,
                                                        "content_block": {
                                                            "type": "tool_use",
                                                            "id": id,
                                                            "name": name
                                                        }
                                                    });
                                                    let sse_data = format!("event: content_block_start\ndata: {}\n\n",
                                                        serde_json::to_string(&event).unwrap_or_default());
                                                    yield Ok(Bytes::from(sse_data));
                                                    open_tool_block_indices.insert(anthropic_index);

                                                    if !pending.is_empty() {
                                                        let event = json!({
                                                            "type": "content_block_delta",
                                                            "index": anthropic_index,
                                                            "delta": {
                                                                "type": "input_json_delta",
                                                                "partial_json": pending
                                                            }
                                                        });
                                                        let sse_data = format!("event: content_block_delta\ndata: {}\n\n",
                                                            serde_json::to_string(&event).unwrap_or_default());
                                                        yield Ok(Bytes::from(sse_data));
                                                    }
                                                }

                                                for (anthropic_index, args) in immediate_deltas {
                                                    let event = json!({
                                                        "type": "content_block_delta",
                                                        "index": anthropic_index,
                                                        "delta": {
                                                            "type": "input_json_delta",
                                                            "partial_json": args
                                                        }
                                                    });
                                                    let sse_data = format!("event: content_block_delta\ndata: {}\n\n",
                                                        serde_json::to_string(&event).unwrap_or_default());
                                                    yield Ok(Bytes::from(sse_data));
                                                }
                                            }
                                        }

                                        // 处理 finish_reason。
                                        // 注意：OpenRouter 某些 provider 会发送多个带 finish_reason 的 chunk
                                        // （第一个 usage 为 null，后续才补全）。此处只做缓存，不立即发送，
                                        // 等到 [DONE] 或流末尾再统一发出，确保 usage 完整且只发一次。
                                        if let Some(finish_reason) = &choice.finish_reason {
                                            let stop_reason = map_stop_reason(Some(finish_reason));
                                            let usage_json =
                                                chunk_usage_json.clone().or_else(|| latest_usage.clone());

                                            if has_emitted_message_delta {
                                                // 更新缓存的 message_delta usage（如果有更完整的 usage）
                                                if let (Some((_, ref mut usage)), Some(uj)) = (&mut pending_message_delta, usage_json) {
                                                    *usage = Some(uj);
                                                }
                                                continue;
                                            }
                                            has_emitted_message_delta = true;

                                            if let Some(index) = current_non_tool_block_index.take() {
                                                let event = json!({
                                                    "type": "content_block_stop",
                                                    "index": index
                                                });
                                                let sse_data = format!("event: content_block_stop\ndata: {}\n\n",
                                                    serde_json::to_string(&event).unwrap_or_default());
                                                yield Ok(Bytes::from(sse_data));
                                            }
                                            current_non_tool_block_type = None;

                                            // Late start for blocks that accumulated args before id/name arrived.
                                            // 保序 + 防御性丢弃（对齐 streaming_codex_chat 的 finalize_tools）：
                                            // - 缺 name / 缺 id 的块直接丢弃（不伪造 unknown_tool / tool_call_N）
                                            // - 有完整身份的块按 Chat index 顺序释放（keys 已排序），
                                            //   避免乱序上游把并行工具以颠倒顺序交给 Claude Code
                                            let mut late_tool_starts: Vec<(u32, String, String, String)> =
                                                Vec::new();
                                            let mut tool_keys: Vec<usize> =
                                                tool_blocks_by_index.keys().copied().collect();
                                            tool_keys.sort_unstable();
                                            for tool_idx in tool_keys {
                                                let Some(state) = tool_blocks_by_index.get_mut(&tool_idx) else {
                                                    continue;
                                                };
                                                if state.started || state.aborted {
                                                    continue;
                                                }
                                                let has_payload = !state.pending_args.is_empty()
                                                    || !state.id.is_empty()
                                                    || !state.name.is_empty();
                                                if !has_payload {
                                                    continue;
                                                }
                                                if state.name.is_empty() {
                                                    state.started = true;
                                                    log::warn!(
                                                        "[Claude/OpenRouter] 丢弃无 name 的流式工具调用 (index={tool_idx})"
                                                    );
                                                    continue;
                                                }
                                                if state.id.is_empty() {
                                                    state.started = true;
                                                    log::warn!(
                                                        "[Claude/OpenRouter] 丢弃无 id 的流式工具调用 (index={tool_idx})"
                                                    );
                                                    continue;
                                                }
                                                let anthropic_index = next_content_index;
                                                next_content_index += 1;
                                                state.anthropic_index = anthropic_index;
                                                state.started = true;
                                                let pending = std::mem::take(&mut state.pending_args);
                                                late_tool_starts.push((
                                                    anthropic_index,
                                                    state.id.clone(),
                                                    state.name.clone(),
                                                    pending,
                                                ));
                                            }
                                            for (index, id, name, pending) in late_tool_starts {
                                                let event = json!({
                                                    "type": "content_block_start",
                                                    "index": index,
                                                    "content_block": {
                                                        "type": "tool_use",
                                                        "id": id,
                                                        "name": name
                                                    }
                                                });
                                                let sse_data = format!("event: content_block_start\ndata: {}\n\n",
                                                    serde_json::to_string(&event).unwrap_or_default());
                                                yield Ok(Bytes::from(sse_data));
                                                open_tool_block_indices.insert(index);
                                                if !pending.is_empty() {
                                                    let delta_event = json!({
                                                        "type": "content_block_delta",
                                                        "index": index,
                                                        "delta": {
                                                            "type": "input_json_delta",
                                                            "partial_json": pending
                                                        }
                                                    });
                                                    let delta_sse = format!("event: content_block_delta\ndata: {}\n\n",
                                                        serde_json::to_string(&delta_event).unwrap_or_default());
                                                    yield Ok(Bytes::from(delta_sse));
                                                }
                                            }

                                            if !open_tool_block_indices.is_empty() {
                                                let mut tool_indices: Vec<u32> =
                                                    open_tool_block_indices.iter().copied().collect();
                                                tool_indices.sort_unstable();
                                                for index in tool_indices {
                                                    let event = json!({
                                                        "type": "content_block_stop",
                                                        "index": index
                                                    });
                                                    let sse_data = format!("event: content_block_stop\ndata: {}\n\n",
                                                        serde_json::to_string(&event).unwrap_or_default());
                                                    yield Ok(Bytes::from(sse_data));
                                                }
                                                open_tool_block_indices.clear();
                                            }

                                            // 缓存 message_delta，等到 [DONE] 时发送（以便收集完整的 usage）
                                            pending_message_delta = Some((stop_reason, usage_json));
                                        }
                                    }
                                }
                    }
                }
                Err(e) => {
                    log::error!("Stream error: {e}");
                    stream_ended_with_error = true;
                    let error_event = json!({
                        "type": "error",
                        "error": {
                            "type": "stream_error",
                            "message": format!("Stream error: {e}")
                        }
                    });
                    let sse_data = format!("event: error\ndata: {}\n\n",
                        serde_json::to_string(&error_event).unwrap_or_default());
                    yield Ok(Bytes::from(sse_data));
                    break;
                }
            }
        }

        // 流自然结束但未收到 [DONE] 时，确保发送缓存的 message_delta 和 message_stop。
        // 若上游已显式报错，则只保留 error 事件，避免把失败伪装成成功完成。
        // 若 [DONE] 已发出 message_stop（终态已收尾），跳过整个终止处理。
        if !stream_ended_with_error && !has_sent_message_stop {
            // 截断流防护：已打开的内容块先闭合（顺序先于 message_delta）。
            if let Some(index) = current_non_tool_block_index.take() {
                let event = json!({
                    "type": "content_block_stop",
                    "index": index
                });
                let sse_data = format!("event: content_block_stop\ndata: {}\n\n",
                    serde_json::to_string(&event).unwrap_or_default());
                yield Ok(Bytes::from(sse_data));
            }
            current_non_tool_block_type = None;
            if !open_tool_block_indices.is_empty() {
                let mut tool_indices: Vec<u32> =
                    open_tool_block_indices.iter().copied().collect();
                tool_indices.sort_unstable();
                for index in tool_indices {
                    let event = json!({
                        "type": "content_block_stop",
                        "index": index
                    });
                    let sse_data = format!("event: content_block_stop\ndata: {}\n\n",
                        serde_json::to_string(&event).unwrap_or_default());
                    yield Ok(Bytes::from(sse_data));
                }
                open_tool_block_indices.clear();
            }

            let emitted_pending_message_delta = if let Some((stop_reason, usage_json)) =
                pending_message_delta.take()
            {
                let event = build_message_delta_event(stop_reason, usage_json);
                let sse_data = format!("event: message_delta\ndata: {}\n\n",
                    serde_json::to_string(&event).unwrap_or_default());
                log::debug!("[Claude/OpenRouter] >>> Anthropic SSE: message_delta (at stream end)");
                yield Ok(Bytes::from(sse_data));
                true
            } else if has_sent_message_start {
                // 上游流结束但从未发 finish_reason：有实质输出时补 end_turn，
                // 避免 Claude Code 静默挂起（无任何输出的场景由下方 error 兜底）。
                log::warn!(
                    "[Claude/OpenRouter] 上游流结束但未发送 finish_reason，补发 end_turn 终止事件"
                );
                let event = build_message_delta_event(
                    Some("end_turn".to_string()),
                    latest_usage.clone(),
                );
                let sse_data = format!("event: message_delta\ndata: {}\n\n",
                    serde_json::to_string(&event).unwrap_or_default());
                yield Ok(Bytes::from(sse_data));
                true
            } else {
                false
            };

            if emitted_pending_message_delta && !has_sent_message_stop {
                let event = json!({"type": "message_stop"});
                let sse_data = format!("event: message_stop\ndata: {}\n\n",
                    serde_json::to_string(&event).unwrap_or_default());
                log::debug!("[Claude/OpenRouter] >>> Anthropic SSE: message_stop (at stream end)");
                yield Ok(Bytes::from(sse_data));
            }

            // 止损：客户端请求了流式，但网关无视 stream 标志、返回了一份完整
            // JSON（无 `data:`/`\n\n`）。这些字节从没被 SSE 分块消费，留在 buffer
            // 里，转换器一个事件都没产出（has_sent_message_start 仍为 false）。
            // 不发任何东西的话客户端会静默挂起。这里补一个 Anthropic error 事件，
            // 让客户端和用量收集器都能看到一个明确的终止结果。
            //
            // 注意：这只是把「静默挂起」变成「明确报错」，并没有把那份 JSON 应答
            // 真正转成 Anthropic 流式格式（完整方案需仿 streaming_responses 的
            // whole-JSON 回退，改动面大，另行处理）。
            if !has_sent_message_start && !buffer.trim().is_empty() {
                log::warn!(
                    "[Claude/OpenRouter] 上游对流式请求返回了非 SSE 响应，转换器零产出；\
                     发送 error 事件避免客户端静默挂起"
                );
                let error_event = json!({
                    "type": "error",
                    "error": {
                        "type": "api_error",
                        "message": "Upstream returned a non-streaming response to a streaming request; \
                                    the proxy could not convert it to Anthropic SSE.",
                    }
                });
                let sse_data = format!("event: error\ndata: {}\n\n",
                    serde_json::to_string(&error_event).unwrap_or_default());
                yield Ok(Bytes::from(sse_data));
            }
        }
    }
}

/// Extract cache_read tokens from Usage, checking both direct field and nested details
fn extract_cache_read_tokens(usage: &Usage) -> Option<u32> {
    // Direct field takes priority (compatible servers)
    if let Some(v) = usage.cache_read_input_tokens {
        return Some(v);
    }
    // OpenAI standard: prompt_tokens_details.cached_tokens
    usage
        .prompt_tokens_details
        .as_ref()
        .map(|d| d.cached_tokens)
        .filter(|&v| v > 0)
}

/// Extract cache-write tokens from direct compatibility fields or OpenAI details.
fn extract_cache_write_tokens(usage: &Usage) -> Option<u32> {
    if let Some(value) = usage.cache_creation_input_tokens {
        return Some(value);
    }
    usage
        .prompt_tokens_details
        .as_ref()
        .map(|details| details.cache_write_tokens)
        .filter(|value| *value > 0)
}

/// 映射停止原因
fn map_stop_reason(finish_reason: Option<&str>) -> Option<String> {
    finish_reason.map(|r| {
        match r {
            "tool_calls" | "function_call" => "tool_use",
            "stop" => "end_turn",
            "length" => "max_tokens",
            "content_filter" => "end_turn",
            other => {
                log::warn!("[Claude/OpenRouter] Unknown finish_reason in streaming: {other}");
                "end_turn"
            }
        }
        .to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use futures::StreamExt;
    use serde_json::Value;
    use std::collections::HashMap;

    async fn collect_anthropic_events(input: &str) -> Vec<Value> {
        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;
        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        merged
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .find_map(|line| strip_sse_field(line, "data"))?;
                serde_json::from_str::<Value>(data).ok()
            })
            .collect()
    }

    fn event_type(event: &Value) -> Option<&str> {
        event.get("type").and_then(|v| v.as_str())
    }

    #[test]
    fn test_map_stop_reason_legacy_and_filtered_values() {
        assert_eq!(
            map_stop_reason(Some("function_call")),
            Some("tool_use".to_string())
        );
        assert_eq!(
            map_stop_reason(Some("content_filter")),
            Some("end_turn".to_string())
        );
    }

    #[tokio::test]
    async fn streamed_refusal_becomes_anthropic_text() {
        // OpenAI 结构化输出的流式拒绝走 delta.refusal。此前完全没读，拒绝时
        // 客户端收到空的成功消息。现在应转成 text_delta。
        let input = concat!(
            "data: {\"id\":\"c1\",\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"refusal\":\"I can't help with that.\"}}]}\n\n",
            "data: {\"id\":\"c1\",\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let events = collect_anthropic_events(input).await;
        let text: String = events
            .iter()
            .filter(|e| e["type"] == "content_block_delta")
            .filter_map(|e| e["delta"]["text"].as_str())
            .collect();
        assert!(
            text.contains("I can't help with that."),
            "流式 refusal 应转成 text_delta，实际事件: {events:?}"
        );
    }

    #[tokio::test]
    async fn test_streaming_tool_calls_routed_by_index() {
        let input = concat!(
            "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_0\",\"type\":\"function\",\"function\":{\"name\":\"first_tool\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"second_tool\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"arguments\":\"{\\\"b\\\":2}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"a\\\":1}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":4}}\n\n",
            "data: [DONE]\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;

        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        let events: Vec<Value> = merged
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .find_map(|line| strip_sse_field(line, "data"))?;
                serde_json::from_str::<Value>(data).ok()
            })
            .collect();

        let mut tool_index_by_call: HashMap<String, u64> = HashMap::new();
        for event in &events {
            if event.get("type").and_then(|v| v.as_str()) == Some("content_block_start")
                && event
                    .pointer("/content_block/type")
                    .and_then(|v| v.as_str())
                    == Some("tool_use")
            {
                if let (Some(call_id), Some(index)) = (
                    event.pointer("/content_block/id").and_then(|v| v.as_str()),
                    event.get("index").and_then(|v| v.as_u64()),
                ) {
                    tool_index_by_call.insert(call_id.to_string(), index);
                }
            }
        }

        assert_eq!(tool_index_by_call.len(), 2);
        assert_ne!(
            tool_index_by_call.get("call_0"),
            tool_index_by_call.get("call_1")
        );

        let deltas: Vec<(u64, String)> = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(|v| v.as_str()) == Some("content_block_delta")
                    && event.pointer("/delta/type").and_then(|v| v.as_str())
                        == Some("input_json_delta")
            })
            .filter_map(|event| {
                let index = event.get("index").and_then(|v| v.as_u64())?;
                let partial_json = event
                    .pointer("/delta/partial_json")
                    .and_then(|v| v.as_str())?
                    .to_string();
                Some((index, partial_json))
            })
            .collect();

        assert_eq!(deltas.len(), 2);
        let second_idx = deltas
            .iter()
            .find_map(|(index, payload)| (payload == "{\"b\":2}").then_some(*index))
            .unwrap();
        let first_idx = deltas
            .iter()
            .find_map(|(index, payload)| (payload == "{\"a\":1}").then_some(*index))
            .unwrap();

        assert_eq!(second_idx, *tool_index_by_call.get("call_1").unwrap());
        assert_eq!(first_idx, *tool_index_by_call.get("call_0").unwrap());

        assert!(events.iter().any(|event| {
            event.get("type").and_then(|v| v.as_str()) == Some("message_delta")
                && event.pointer("/delta/stop_reason").and_then(|v| v.as_str()) == Some("tool_use")
        }));
    }

    #[tokio::test]
    async fn test_streaming_delays_tool_start_until_id_and_name_ready() {
        let input = concat!(
            "data: {\"id\":\"chatcmpl_2\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"a\\\":\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_2\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_0\",\"type\":\"function\",\"function\":{\"name\":\"first_tool\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_2\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"1}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_2\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":6,\"completion_tokens\":2}}\n\n",
            "data: [DONE]\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;
        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        let events: Vec<Value> = merged
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .find_map(|line| strip_sse_field(line, "data"))?;
                serde_json::from_str::<Value>(data).ok()
            })
            .collect();

        let starts: Vec<&Value> = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(|v| v.as_str()) == Some("content_block_start")
                    && event
                        .pointer("/content_block/type")
                        .and_then(|v| v.as_str())
                        == Some("tool_use")
            })
            .collect();
        assert_eq!(starts.len(), 1);
        assert_eq!(
            starts[0]
                .pointer("/content_block/id")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            "call_0"
        );
        assert_eq!(
            starts[0]
                .pointer("/content_block/name")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            "first_tool"
        );

        let deltas: Vec<&str> = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(|v| v.as_str()) == Some("content_block_delta")
                    && event.pointer("/delta/type").and_then(|v| v.as_str())
                        == Some("input_json_delta")
            })
            .filter_map(|event| {
                event
                    .pointer("/delta/partial_json")
                    .and_then(|v| v.as_str())
            })
            .collect();
        assert!(deltas.contains(&"{\"a\":"));
        assert!(deltas.contains(&"1}"));
    }

    #[tokio::test]
    async fn test_streaming_preserves_parallel_tool_order_when_earlier_name_arrives_late() {
        // 上游 index 1 的身份先到、index 0 后到（DeepSeek 系实测存在）。
        // content_block_start 必须按 Chat index 顺序释放：index 0 的块必须先于
        // index 1 发出，否则 Claude Code 按错误顺序执行并行工具。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_4\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"second_tool\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_4\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_0\",\"type\":\"function\",\"function\":{\"name\":\"first_tool\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_4\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"arguments\":\"{\\\"b\\\":2}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_4\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"a\\\":1}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_4\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":4}}\n\n",
            "data: [DONE]\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;
        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        let events: Vec<Value> = merged
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .find_map(|line| strip_sse_field(line, "data"))?;
                serde_json::from_str::<Value>(data).ok()
            })
            .collect();

        let start_names: Vec<String> = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(|v| v.as_str()) == Some("content_block_start")
                    && event
                        .pointer("/content_block/type")
                        .and_then(|v| v.as_str())
                        == Some("tool_use")
            })
            .filter_map(|event| {
                event
                    .pointer("/content_block/name")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
            .collect();

        assert_eq!(start_names, vec!["first_tool", "second_tool"],
            "index 0 晚到身份碎片也不得重排：start 顺序必须与 Chat index 一致");
    }

    #[tokio::test]
    async fn test_streaming_drops_tool_call_without_name_at_finish() {
        // 上游 finish 时仍未提供 name（异常上游）。不得伪造 unknown_tool：
        // 该块应被丢弃（不发出 content_block_start），已完成的其它块不受影响。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_5\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_0\",\"type\":\"function\",\"function\":{\"name\":\"first_tool\",\"arguments\":\"{\\\"a\\\":1}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_5\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_1\",\"type\":\"function\"}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_5\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":4}}\n\n",
            "data: [DONE]\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;
        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        let events: Vec<Value> = merged
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .find_map(|line| strip_sse_field(line, "data"))?;
                serde_json::from_str::<Value>(data).ok()
            })
            .collect();

        let start_names: Vec<&str> = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(|v| v.as_str()) == Some("content_block_start")
                    && event
                        .pointer("/content_block/type")
                        .and_then(|v| v.as_str())
                        == Some("tool_use")
            })
            .filter_map(|event| {
                event
                    .pointer("/content_block/name")
                    .and_then(|v| v.as_str())
            })
            .collect();

        assert_eq!(start_names, vec!["first_tool"]);
        assert!(
            !start_names.iter().any(|name| *name == "unknown_tool"),
            "不得伪造 unknown_tool 工具名"
        );
    }

    #[tokio::test]
    async fn test_streaming_chinese_split_across_chunks_no_replacement_chars() {
        // "你好" split across two TCP chunks inside a streaming text delta.
        // Before the fix, from_utf8_lossy would produce U+FFFD for each half.
        let full = concat!(
            "data: {\"id\":\"chatcmpl_3\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_3\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\n",
            "data: [DONE]\n\n"
        );
        let bytes = full.as_bytes();

        // Find "你" in the byte stream and split inside it
        let ni_start = bytes.windows(3).position(|w| w == "你".as_bytes()).unwrap();
        let split_point = ni_start + 1; // split after first byte of "你"

        let chunk1 = Bytes::from(bytes[..split_point].to_vec());
        let chunk2 = Bytes::from(bytes[split_point..].to_vec());

        let upstream = stream::iter(vec![
            Ok::<_, std::io::Error>(chunk1),
            Ok::<_, std::io::Error>(chunk2),
        ]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;

        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        // Must contain the original Chinese characters, not replacement chars
        assert!(
            merged.contains("你好"),
            "expected '你好' in output, got replacement chars (U+FFFD)"
        );
        assert!(
            !merged.contains('\u{FFFD}'),
            "output must not contain U+FFFD replacement characters"
        );
    }

    #[tokio::test]
    async fn test_duplicate_finish_reason_emits_only_one_message_delta() {
        // Simulates OpenRouter behavior where two chunks carry finish_reason:
        // first with null usage, second with populated usage.
        let input = concat!(
            "data: {\"id\":\"chatcmpl_dup\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"id\":\"chatcmpl_dup\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n\n",
            "data: [DONE]\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;

        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        let events: Vec<Value> = merged
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .find_map(|line| strip_sse_field(line, "data"))?;
                serde_json::from_str::<Value>(data).ok()
            })
            .collect();

        let message_deltas: Vec<&Value> = events
            .iter()
            .filter(|e| e.get("type").and_then(|v| v.as_str()) == Some("message_delta"))
            .collect();

        assert_eq!(
            message_deltas.len(),
            1,
            "duplicate finish_reason chunks must produce exactly one message_delta, got {}: {:?}",
            message_deltas.len(),
            message_deltas
        );

        assert_eq!(message_deltas[0]["usage"]["input_tokens"], 10);
        assert_eq!(message_deltas[0]["usage"]["output_tokens"], 5);

        let message_stops = events
            .iter()
            .filter(|e| e.get("type").and_then(|v| v.as_str()) == Some("message_stop"))
            .count();
        assert_eq!(message_stops, 1, "message_stop must only be emitted once");
    }

    #[tokio::test]
    async fn test_usage_only_chunk_after_finish_reason_updates_message_delta_usage() {
        let input = concat!(
            "data: {\"id\":\"chatcmpl_split\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"tool-0924\",\"type\":\"function\",\"function\":{\"name\":\"Bash\",\"arguments\":\"{\\\"command\\\":\\\"pwd\\\"}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_split\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":13312,\"completion_tokens\":79,\"prompt_tokens_details\":{\"cached_tokens\":100}}}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;
        let message_deltas: Vec<&Value> = events
            .iter()
            .filter(|event| event_type(event) == Some("message_delta"))
            .collect();
        let message_stops = events
            .iter()
            .filter(|event| event_type(event) == Some("message_stop"))
            .count();

        assert_eq!(message_deltas.len(), 1);
        assert_eq!(message_stops, 1);

        let message_delta = message_deltas[0];
        assert_eq!(
            message_delta
                .pointer("/delta/stop_reason")
                .and_then(|v| v.as_str()),
            Some("tool_use")
        );
        assert_eq!(
            message_delta
                .pointer("/usage/input_tokens")
                .and_then(|v| v.as_u64()),
            Some(13212)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/output_tokens")
                .and_then(|v| v.as_u64()),
            Some(79)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/cache_read_input_tokens")
                .and_then(|v| v.as_u64()),
            Some(100)
        );
    }

    #[tokio::test]
    async fn test_usage_chunk_subtracts_cache_read_and_creation_from_input() {
        // prompt_tokens(1000) 含 cache_read(600) 与 cache_creation(300)；转 Anthropic 后
        // input 应为 fresh，守恒：input(100) + cache_read(600) + cache_creation(300) == prompt(1000)。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_cc\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"tool-1\",\"type\":\"function\",\"function\":{\"name\":\"Bash\",\"arguments\":\"{\\\"command\\\":\\\"pwd\\\"}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_cc\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":50,\"prompt_tokens_details\":{\"cached_tokens\":600,\"cache_write_tokens\":300}}}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;
        let message_delta = events
            .iter()
            .find(|event| event_type(event) == Some("message_delta"))
            .expect("should emit message_delta with usage");

        // fresh input = 1000 - 600 - 300 = 100
        assert_eq!(
            message_delta
                .pointer("/usage/input_tokens")
                .and_then(|v| v.as_u64()),
            Some(100)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/cache_read_input_tokens")
                .and_then(|v| v.as_u64()),
            Some(600)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/cache_creation_input_tokens")
                .and_then(|v| v.as_u64()),
            Some(300)
        );
    }

    #[tokio::test]
    async fn test_usage_chunk_clamps_input_to_zero_when_cache_exceeds_prompt() {
        // prompt(100) < cache_read(80)+cache_creation(50)=130：saturating 钳到 0，防下溢。
        // 钉桩：阻止未来把 saturating_sub 误改成普通减法(debug panic / release wrap)。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_uf\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"tool-1\",\"type\":\"function\",\"function\":{\"name\":\"Bash\",\"arguments\":\"{\\\"command\\\":\\\"pwd\\\"}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_uf\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":50,\"prompt_tokens_details\":{\"cached_tokens\":80},\"cache_creation_input_tokens\":50}}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;
        let message_delta = events
            .iter()
            .find(|event| event_type(event) == Some("message_delta"))
            .expect("should emit message_delta with usage");

        assert_eq!(
            message_delta
                .pointer("/usage/input_tokens")
                .and_then(|v| v.as_u64()),
            Some(0)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/cache_read_input_tokens")
                .and_then(|v| v.as_u64()),
            Some(80)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/cache_creation_input_tokens")
                .and_then(|v| v.as_u64()),
            Some(50)
        );
    }

    #[tokio::test]
    async fn test_message_delta_includes_zero_usage_when_stream_has_no_usage() {
        let input = concat!(
            "data: {\"id\":\"chatcmpl_no_usage\",\"model\":\"gpt-5.5\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_0\",\"type\":\"function\",\"function\":{\"name\":\"get_time\",\"arguments\":\"{}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_no_usage\",\"model\":\"gpt-5.5\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;
        let message_deltas: Vec<&Value> = events
            .iter()
            .filter(|event| event_type(event) == Some("message_delta"))
            .collect();

        assert_eq!(message_deltas.len(), 1);
        let message_delta = message_deltas[0];
        assert_eq!(
            message_delta
                .pointer("/delta/stop_reason")
                .and_then(|v| v.as_str()),
            Some("tool_use")
        );
        assert_eq!(
            message_delta
                .pointer("/usage/input_tokens")
                .and_then(|v| v.as_u64()),
            Some(0)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/output_tokens")
                .and_then(|v| v.as_u64()),
            Some(0)
        );
    }

    #[tokio::test]
    async fn test_duplicate_done_emits_single_terminal_sequence() {
        // 异常上游发多个 [DONE]：终态（message_delta + message_stop）只能出现一次，
        // 否则 Claude Code 按「每个消息流一个终止序列」解析会失败。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_dup_done\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":1}}\n\n",
            "data: [DONE]\n\n",
            "data: [DONE]\n\n"
        );
        let events = collect_anthropic_events(input).await;
        let stops = events
            .iter()
            .filter(|e| event_type(e) == Some("message_stop"))
            .count();
        let deltas = events
            .iter()
            .filter(|e| event_type(e) == Some("message_delta"))
            .count();
        assert_eq!(stops, 1, "重复 [DONE] 不得重复 message_stop");
        assert_eq!(deltas, 1, "重复 [DONE] 不得重复 message_delta");
    }

    #[tokio::test]
    async fn test_streaming_finalizes_after_finish_when_done_is_missing() {
        let input = concat!(
            "data: {\"id\":\"chatcmpl_no_done\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_no_done\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n"
        );

        let events = collect_anthropic_events(input).await;

        assert!(events.iter().any(|event| {
            event_type(event) == Some("message_delta")
                && event.pointer("/delta/stop_reason").and_then(|v| v.as_str()) == Some("end_turn")
        }));
        assert_eq!(
            events.last().and_then(|event| event_type(event)),
            Some("message_stop")
        );
    }

    #[tokio::test]
    async fn test_stream_end_without_finish_reason_does_not_emit_success_terminal_events() {
        let input = "data: {\"id\":\"chatcmpl_truncated\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n";

        let events = collect_anthropic_events(input).await;

        // 修复后行为：上游流结束但从未发 finish_reason 时，有实质输出的流必须
        // 补发终止事件（end_turn + message_stop），否则 Claude Code 静默挂起。
        assert!(
            events.iter().any(|event| event_type(event) == Some("message_delta")),
            "截断流（无 finish_reason）必须补发 message_delta，实际: {events:?}"
        );
        assert!(
            events.iter().any(|event| event_type(event) == Some("message_stop")),
            "截断流（无 finish_reason）必须补发 message_stop"
        );
        // 内容块必须闭合（stop 在 message_delta 之前）
        let delta_pos = events
            .iter()
            .position(|e| event_type(e) == Some("message_delta"))
            .unwrap();
        let text_stop_pos = events
            .iter()
            .position(|e| {
                event_type(e) == Some("content_block_stop")
                    && e.pointer("/index").and_then(|v| v.as_u64()) == Some(0)
            })
            .unwrap();
        assert!(text_stop_pos < delta_pos, "content_block_stop 必须先于 message_delta");
    }

    #[tokio::test]
    async fn test_non_sse_json_body_emits_error_not_silence() {
        // 客户端请求流式，网关却返回一份完整 JSON（无 data:/\n\n）。这些字节
        // 从不被 SSE 分块消费，转换器零产出。旧行为是空流——客户端静默挂起。
        // 现在应至少产出一个 error 事件。
        let json_body = r#"{"id":"chatcmpl-1","object":"chat.completion","choices":[{"message":{"role":"assistant","content":"hi"}}]}"#;
        let events = collect_anthropic_events(json_body).await;

        assert!(
            events.iter().any(|e| event_type(e) == Some("error")),
            "非 SSE 的 JSON 应答应产出 error 事件，实际: {events:?}"
        );
        // 且不得伪装成成功完成
        assert!(
            !events.iter().any(|e| event_type(e) == Some("message_stop")),
            "不应发出 message_stop（那会把失败伪装成成功）"
        );
    }

    #[tokio::test]
    async fn test_stream_error_does_not_emit_success_terminal_events() {
        let upstream = stream::iter(vec![Err::<Bytes, _>(std::io::Error::other(
            "upstream disconnected",
        ))]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;

        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        let events: Vec<Value> = merged
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .find_map(|line| strip_sse_field(line, "data"))?;
                serde_json::from_str::<Value>(data).ok()
            })
            .collect();

        assert!(events
            .iter()
            .any(|e| e.get("type").and_then(|v| v.as_str()) == Some("error")));
        assert!(!events
            .iter()
            .any(|e| e.get("type").and_then(|v| v.as_str()) == Some("message_delta")));
        assert!(!events
            .iter()
            .any(|e| e.get("type").and_then(|v| v.as_str()) == Some("message_stop")));
    }
}
