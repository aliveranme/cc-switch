//! Reasoning parameter injection for direct `/chat/completions` requests.
//!
//! When a client sends a raw Chat Completions request without reasoning parameters,
//! this module injects appropriate parameters based on the upstream provider's
//! capabilities and the user's configured defaults.
//!
//! ## Injection logic
//!
//! 1. **Skip** if the body already has any reasoning-related field
//!    (`reasoning_effort`, `thinking`, `reasoning`, `enable_thinking`,
//!     `reasoning_split`).
//! 2. **Resolve platform-specific config** via `resolve_codex_chat_reasoning_config`.
//! 3. **With config**: inject per-platform format (thinking param + effort param
//!    with mapped default effort).
//! 4. **Without config**: inject `reasoning_effort` if the model is a known
//!    OpenAI reasoning model (o-series, GPT-5+).
//!
//! The default effort level comes from the provider's TOML `model_reasoning_effort`,
//! falling back to `"high"`.

use crate::provider::Provider;
use serde_json::Value;
use toml::Value as TomlValue;

/// Inject reasoning parameters into a `/chat/completions` request body.
///
/// Skips injection if the body already carries any reasoning-related field.
pub fn inject_chat_reasoning(body: &mut Value, provider: &Provider) {
    if has_any_reasoning_field(body) {
        return;
    }

    let config = super::providers::resolve_codex_chat_reasoning_config(provider, body);

    if let Some(ref config) = config {
        inject_with_platform_config(body, provider, config);
    } else {
        inject_default(body, provider);
    }
}

/// Check if the request body already contains reasoning-related fields.
fn has_any_reasoning_field(body: &Value) -> bool {
    // Standard OpenAI reasoning_effort
    if body.get("reasoning_effort").is_some() {
        return true;
    }
    // Anthropic-style thinking block (also DeepSeek thinking)
    if body.get("thinking").is_some() {
        return true;
    }
    // OpenRouter-style reasoning object
    if body.get("reasoning").is_some() {
        return true;
    }
    // SiliconFlow / Qwen / DeepSeek-style enable_thinking
    if body.get("enable_thinking").is_some() {
        return true;
    }
    // MiniMax-style reasoning_split
    if body.get("reasoning_split").is_some() {
        return true;
    }
    false
}

/// Inject reasoning parameters using a platform-specific `CodexChatReasoningConfig`.
///
/// For platforms that support both thinking _and_ effort (e.g. DeepSeek, OpenRouter),
/// both are injected. For platforms that only support thinking (e.g. Kimi/Moonshot,
/// GLM/Zhipu, Qwen, MiniMax), only the thinking parameter is injected.
fn inject_with_platform_config(
    body: &mut Value,
    provider: &Provider,
    config: &crate::provider::CodexChatReasoningConfig,
) {
    let supports_thinking = config.supports_thinking.unwrap_or(false);
    let supports_effort = config.supports_effort.unwrap_or(false);
    let thinking_param = config
        .thinking_param
        .as_deref()
        .unwrap_or("thinking")
        .trim()
        .to_ascii_lowercase();

    // Inject thinking parameter if supported and not explicitly set to "none"
    // (e.g. OpenRouter sets thinking_param = "none" because it does not
    //  recognise `thinking:{type}`).
    if supports_thinking && thinking_param != "none" {
        match thinking_param.as_str() {
            "thinking" => {
                body["thinking"] = serde_json::json!({"type": "enabled"});
            }
            "enable_thinking" => {
                body["enable_thinking"] = serde_json::json!(true);
            }
            "reasoning_split" => {
                body["reasoning_split"] = serde_json::json!(true);
            }
            _ => {}
        }
    }

    if !supports_effort {
        return;
    }

    let default_effort = extract_model_reasoning_effort_from_toml(provider)
        .unwrap_or_else(|| "high".to_string());

    let effort_param = config
        .effort_param
        .as_deref()
        .unwrap_or("reasoning_effort")
        .trim()
        .to_ascii_lowercase();

    let Some(mapped) =
        super::providers::transform_codex_chat::map_reasoning_effort(
            &default_effort,
            config.effort_value_mode.as_deref(),
        )
    else {
        return;
    };

    match effort_param.as_str() {
        "reasoning_effort" => {
            body["reasoning_effort"] = serde_json::json!(mapped);
        }
        "reasoning.effort" => {
            body["reasoning"] = serde_json::json!({"effort": mapped});
        }
        _ => {}
    }
}

/// Inject default `reasoning_effort` for standard OpenAI reasoning models
/// (o-series, GPT-5+).
///
/// Only fires when no platform-specific config was resolved. Uses the provider's
/// configured default effort level from TOML, falling back to `"high"`.
fn inject_default(body: &mut Value, provider: &Provider) {
    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if !super::providers::transform::supports_reasoning_effort(model) {
        return;
    }

    let effort = extract_model_reasoning_effort_from_toml(provider)
        .unwrap_or_else(|| "high".to_string());

    body["reasoning_effort"] = serde_json::json!(effort);
}

/// Extract `model_reasoning_effort` from the provider's embedded TOML config.
///
/// Codex providers carry a TOML config string in `settings_config.config` that
/// may contain `model_reasoning_effort = "high"` (set in the universal provider
/// template, see `provider.rs:to_codex_provider`).
fn extract_model_reasoning_effort_from_toml(provider: &Provider) -> Option<String> {
    let config_text = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())?;
    let doc: TomlValue = config_text.parse().ok()?;

    doc.get("model_reasoning_effort")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;
    use serde_json::json;

    // ── helpers ──

    fn make_provider(settings_config: Value) -> Provider {
        Provider {
            id: "test".to_string(),
            name: "test".to_string(),
            settings_config,
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn make_provider_with_config(config_toml: &str) -> Provider {
        make_provider(json!({ "config": config_toml }))
    }

    fn make_provider_with_base_url(base_url: &str) -> Provider {
        make_provider(json!({
            "base_url": base_url,
            "model": "test-model",
        }))
    }

    // ── has_any_reasoning_field ──

    #[test]
    fn detects_reasoning_effort_field() {
        let body = json!({"model": "gpt-5", "reasoning_effort": "high"});
        assert!(has_any_reasoning_field(&body));
    }

    #[test]
    fn detects_thinking_field() {
        let body = json!({"model": "deepseek-v4", "thinking": {"type": "enabled"}});
        assert!(has_any_reasoning_field(&body));
    }

    #[test]
    fn detects_reasoning_object() {
        let body = json!({"model": "m", "reasoning": {"effort": "high"}});
        assert!(has_any_reasoning_field(&body));
    }

    #[test]
    fn detects_enable_thinking() {
        let body = json!({"model": "m", "enable_thinking": true});
        assert!(has_any_reasoning_field(&body));
    }

    #[test]
    fn detects_reasoning_split() {
        let body = json!({"model": "m", "reasoning_split": true});
        assert!(has_any_reasoning_field(&body));
    }

    #[test]
    fn no_reasoning_field_returns_false() {
        let body = json!({"model": "gpt-5", "messages": [{"role": "user", "content": "hi"}]});
        assert!(!has_any_reasoning_field(&body));
    }

    // ── extract_model_reasoning_effort_from_toml ──

    #[test]
    fn reads_toml_value() {
        let provider = make_provider_with_config(
            "model = \"gpt-5\"\nmodel_reasoning_effort = \"medium\"\ndisable_response_storage = true",
        );
        assert_eq!(
            extract_model_reasoning_effort_from_toml(&provider).as_deref(),
            Some("medium"),
        );
    }

    #[test]
    fn returns_none_when_toml_key_missing() {
        let provider = make_provider_with_config("model = \"gpt-5\"\ndisable_response_storage = true");
        assert!(extract_model_reasoning_effort_from_toml(&provider).is_none());
    }

    #[test]
    fn returns_none_when_no_config_field() {
        let provider = make_provider(json!({"base_url": "https://api.openai.com"}));
        assert!(extract_model_reasoning_effort_from_toml(&provider).is_none());
    }

    // ── inject_default ──

    #[test]
    fn injects_for_o_series() {
        let mut body = json!({"model": "o3-mini", "messages": [{"role": "user", "content": "hi"}]});
        let provider = make_provider_with_config("model_reasoning_effort = \"high\"");
        inject_default(&mut body, &provider);
        assert_eq!(body["reasoning_effort"], "high");
    }

    #[test]
    fn injects_for_gpt5() {
        let mut body = json!({"model": "gpt-5.4", "messages": [{"role": "user", "content": "hi"}]});
        let provider = make_provider_with_config("model_reasoning_effort = \"high\"");
        inject_default(&mut body, &provider);
        assert_eq!(body["reasoning_effort"], "high");
    }

    #[test]
    fn skips_non_reasoning_model() {
        let mut body = json!({"model": "gpt-4o", "messages": [{"role": "user", "content": "hi"}]});
        let provider = make_provider_with_config("model_reasoning_effort = \"high\"");
        inject_default(&mut body, &provider);
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn uses_configured_level() {
        let mut body = json!({"model": "o1", "messages": [{"role": "user", "content": "hi"}]});
        let provider = make_provider_with_config("model_reasoning_effort = \"medium\"");
        inject_default(&mut body, &provider);
        assert_eq!(body["reasoning_effort"], "medium");
    }

    // ── inject_with_platform_config ──

    #[test]
    fn deepseek_thinking_and_effort() {
        let mut body = json!({"model": "deepseek-chat", "messages": [{"role": "user", "content": "hi"}]});
        let provider = make_provider_with_base_url("https://api.deepseek.com");
        inject_chat_reasoning(&mut body, &provider);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["reasoning_effort"], "high");
    }

    #[test]
    fn siliconflow_enable_thinking() {
        let mut body = json!({"model": "deepseek-r1", "messages": [{"role": "user", "content": "hi"}]});
        let provider = make_provider_with_base_url("https://api.siliconflow.cn");
        inject_chat_reasoning(&mut body, &provider);
        assert_eq!(body["enable_thinking"], true);
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn openrouter_reasoning_object() {
        let mut body = json!({"model": "deepseek/deepseek-chat", "messages": [{"role": "user", "content": "hi"}]});
        let provider = make_provider_with_base_url("https://openrouter.ai/api");
        inject_chat_reasoning(&mut body, &provider);
        // OpenRouter uses reasoning.effort object, NOT top-level reasoning_effort
        assert!(body.get("reasoning_effort").is_none());
        assert_eq!(body["reasoning"]["effort"], "high");
    }

    #[test]
    fn qwen_enable_thinking() {
        let mut body = json!({"model": "qwen-max", "messages": [{"role": "user", "content": "hi"}]});
        let provider = make_provider_with_base_url("https://dashscope.aliyuncs.com");
        inject_chat_reasoning(&mut body, &provider);
        assert_eq!(body["enable_thinking"], true);
    }

    #[test]
    fn minimax_reasoning_split() {
        let mut body = json!({"model": "minimax-m2", "messages": [{"role": "user", "content": "hi"}]});
        let provider = make_provider_with_base_url("https://api.minimax.chat");
        inject_chat_reasoning(&mut body, &provider);
        assert_eq!(body["reasoning_split"], true);
    }

    #[test]
    fn moonshot_thinking() {
        let mut body = json!({"model": "moonshot-v1", "messages": [{"role": "user", "content": "hi"}]});
        let provider = make_provider_with_base_url("https://api.moonshot.cn");
        inject_chat_reasoning(&mut body, &provider);
        assert_eq!(body["thinking"]["type"], "enabled");
    }

    #[test]
    fn glm_thinking() {
        let mut body = json!({"model": "glm-5", "messages": [{"role": "user", "content": "hi"}]});
        let provider = make_provider_with_base_url("https://open.bigmodel.cn");
        inject_chat_reasoning(&mut body, &provider);
        assert_eq!(body["thinking"]["type"], "enabled");
    }

    #[test]
    fn stepfun_reasoning_effort() {
        let mut body = json!({"model": "step-3.5-flash-2603", "messages": [{"role": "user", "content": "hi"}]});
        let provider = make_provider_with_base_url("https://api.stepfun.com");
        inject_chat_reasoning(&mut body, &provider);
        // StepFun doesn't write a thinking field (thinking_param = "none"), only reasoning_effort
        assert!(body.get("thinking").is_none());
        assert_eq!(body["reasoning_effort"], "high");
    }

    #[test]
    fn mimo_thinking() {
        let mut body = json!({"model": "mimo-1", "messages": [{"role": "user", "content": "hi"}]});
        let provider = make_provider_with_base_url("https://api.mimo.chat");
        inject_chat_reasoning(&mut body, &provider);
        assert_eq!(body["thinking"]["type"], "enabled");
    }

    // ── top-level inject_chat_reasoning guards ──

    #[test]
    fn skips_when_reasoning_effort_already_present() {
        let mut body = json!({
            "model": "o3-mini",
            "reasoning_effort": "low",
            "messages": [{"role": "user", "content": "hi"}],
        });
        let provider = make_provider_with_config("model_reasoning_effort = \"high\"");
        inject_chat_reasoning(&mut body, &provider);
        assert_eq!(body["reasoning_effort"], "low"); // not overridden
    }

    #[test]
    fn skips_when_thinking_already_present() {
        let mut body = json!({
            "model": "deepseek-chat",
            "thinking": {"type": "disabled"},
            "messages": [{"role": "user", "content": "hi"}],
        });
        let provider = make_provider_with_base_url("https://api.deepseek.com");
        inject_chat_reasoning(&mut body, &provider);
        assert_eq!(body["thinking"]["type"], "disabled"); // not overridden
    }

    #[test]
    fn injects_for_openai_model_via_top_level() {
        let mut body = json!({"model": "o1", "messages": [{"role": "user", "content": "hi"}]});
        let provider = make_provider_with_config("model_reasoning_effort = \"high\"");
        inject_chat_reasoning(&mut body, &provider);
        assert_eq!(body["reasoning_effort"], "high");
    }

    #[test]
    fn injects_for_deepseek_via_top_level() {
        let mut body = json!({"model": "deepseek-chat", "messages": [{"role": "user", "content": "hi"}]});
        let provider = make_provider_with_base_url("https://api.deepseek.com");
        inject_chat_reasoning(&mut body, &provider);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["reasoning_effort"], "high");
    }
}
