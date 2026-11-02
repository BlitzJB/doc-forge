//! AI provider configuration for Doc Forge.
//!
//! - Keys live in a private file in the app's config folder (or come from an environment variable), never in
//!   config files. The frontend only ever sees a short hint like `…nJni`.
//! - Models are discovered live from each provider's `/models` endpoint, keeping the
//!   capability metadata providers expose (tools, vision, context window).
//! - A key is only saved after the provider accepts it.

pub mod agent;
pub mod config;
pub mod history;
pub mod oauth;
pub mod search;
pub mod secrets;
pub mod service;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub use config::{AiConfig, ProviderConfig, RoleChoice, Roles};
pub use secrets::{FileSecrets, MemorySecrets, Secrets};
pub use service::{AiService, AiView, Endpoint, KeySource, ProviderView, Role};

// ---- Catalog -----------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Fireworks,
    Anthropic,
    Openai,
    Openrouter,
    Ollama,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    /// OpenAI-style `/models` + `/chat/completions`.
    OpenAi,
    /// Anthropic `/models` + `/messages`.
    Anthropic,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KindInfo {
    pub label: &'static str,
    pub default_base_url: &'static str,
    /// Environment variable consulted when no key is stored.
    pub env_var: Option<&'static str>,
    /// Whether the provider requires a key at all (local servers don't).
    pub needs_key: bool,
    pub key_url: Option<&'static str>,
    pub key_placeholder: &'static str,
    pub local: bool,
    #[serde(skip)]
    pub protocol: Protocol,
}

impl Kind {
    pub const BUILT_IN: [Kind; 5] = [
        Kind::Fireworks,
        Kind::Anthropic,
        Kind::Openai,
        Kind::Openrouter,
        Kind::Ollama,
    ];

    pub fn info(self) -> KindInfo {
        match self {
            Kind::Fireworks => KindInfo {
                label: "Fireworks",
                default_base_url: "https://api.fireworks.ai/inference/v1",
                env_var: Some("FIREWORKS_API_KEY"),
                needs_key: true,
                key_url: Some("https://app.fireworks.ai/settings/users/api-keys"),
                key_placeholder: "fw_…",
                local: false,
                protocol: Protocol::OpenAi,
            },
            Kind::Anthropic => KindInfo {
                label: "Anthropic",
                default_base_url: "https://api.anthropic.com/v1",
                env_var: Some("ANTHROPIC_API_KEY"),
                needs_key: true,
                key_url: Some("https://console.anthropic.com/settings/keys"),
                key_placeholder: "sk-ant-…",
                local: false,
                protocol: Protocol::Anthropic,
            },
            Kind::Openai => KindInfo {
                label: "OpenAI",
                default_base_url: "https://api.openai.com/v1",
                env_var: Some("OPENAI_API_KEY"),
                needs_key: true,
                key_url: Some("https://platform.openai.com/api-keys"),
                key_placeholder: "sk-…",
                local: false,
                protocol: Protocol::OpenAi,
            },
            Kind::Openrouter => KindInfo {
                label: "OpenRouter",
                default_base_url: "https://openrouter.ai/api/v1",
                env_var: Some("OPENROUTER_API_KEY"),
                needs_key: true,
                key_url: Some("https://openrouter.ai/settings/keys"),
                key_placeholder: "sk-or-…",
                local: false,
                protocol: Protocol::OpenAi,
            },
            Kind::Ollama => KindInfo {
                label: "Ollama",
                default_base_url: "http://localhost:11434/v1",
                env_var: None,
                needs_key: false,
                key_url: None,
                key_placeholder: "",
                local: true,
                protocol: Protocol::OpenAi,
            },
            Kind::Custom => KindInfo {
                label: "OpenAI-compatible",
                default_base_url: "",
                env_var: None,
                needs_key: false,
                key_url: None,
                key_placeholder: "Optional",
                local: false,
                protocol: Protocol::OpenAi,
            },
        }
    }
}

// ---- Results -------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    /// Short display name, e.g. `kimi-k3` for `accounts/fireworks/models/kimi-k3`.
    pub name: String,
    pub context_length: Option<u64>,
    /// None when the provider doesn't say.
    pub tools: Option<bool>,
    pub vision: Option<bool>,
    /// Provider-side routers ("auto", Fireworks routers) rather than a single model.
    pub router: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Connected,
    Unauthorized,
    Unreachable,
    Error,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub status: Status,
    pub message: String,
    pub latency_ms: u64,
    pub models: Vec<ModelInfo>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub status: Status,
    pub reply: String,
    /// The model produced separate reasoning before answering.
    pub reasoned: bool,
    pub latency_ms: u64,
    pub usage: Usage,
    pub message: String,
}

// ---- HTTP ------------------------------------------------------------------------

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(60))
        .user_agent(concat!("DocForge/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

fn trim_base(base: &str) -> &str {
    base.trim().trim_end_matches('/')
}

fn authorize(
    req: reqwest::RequestBuilder,
    protocol: Protocol,
    key: Option<&str>,
) -> reqwest::RequestBuilder {
    match (protocol, key) {
        (Protocol::Anthropic, Some(k)) => req
            .header("x-api-key", k)
            .header("anthropic-version", "2023-06-01"),
        (Protocol::Anthropic, None) => req.header("anthropic-version", "2023-06-01"),
        (Protocol::OpenAi, Some(k)) => req.bearer_auth(k),
        (Protocol::OpenAi, None) => req,
    }
}

/// Map a failed HTTP exchange onto a status and a message a person can act on.
pub(crate) fn classify(
    status: Option<reqwest::StatusCode>,
    body: &str,
    transport: Option<&reqwest::Error>,
    label: &str,
) -> (Status, String) {
    if let Some(e) = transport {
        let what = if e.is_timeout() {
            "timed out"
        } else if e.is_connect() {
            "could not be reached"
        } else {
            "request failed"
        };
        return (Status::Unreachable, format!("{label} {what}."));
    }
    let code = status.map(|s| s.as_u16()).unwrap_or(0);
    let detail = provider_error(body);
    match code {
        401 | 403 => (Status::Unauthorized, format!("{label} rejected this key.")),
        404 => (
            Status::Error,
            format!("Not found at this address{}", suffix(&detail)),
        ),
        429 => (
            Status::Error,
            format!("Rate limited by {label}{}", suffix(&detail)),
        ),
        _ => (
            Status::Error,
            format!("{label} returned {code}{}", suffix(&detail)),
        ),
    }
}

fn suffix(detail: &str) -> String {
    if detail.is_empty() {
        ".".into()
    } else {
        format!(": {detail}")
    }
}

/// Pull a readable message out of common error bodies.
fn provider_error(body: &str) -> String {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let msg = v
        .pointer("/error/message")
        .or_else(|| v.pointer("/error"))
        .or_else(|| v.pointer("/message"))
        .and_then(|m| m.as_str())
        .unwrap_or("");
    let msg = if msg.is_empty() { body.trim() } else { msg };
    msg.chars().take(160).collect()
}

// ---- Model discovery ---------------------------------------------------------------

pub async fn list_models(
    http: &reqwest::Client,
    kind: Kind,
    base_url: &str,
    key: Option<&str>,
) -> Check {
    let info = kind.info();
    let label = info.label;
    let url = format!("{}/models", trim_base(base_url));
    let started = Instant::now();
    let res = authorize(http.get(&url), info.protocol, key).send().await;
    let latency_ms = started.elapsed().as_millis() as u64;
    let res = match res {
        Ok(r) => r,
        Err(e) => {
            let (status, message) = classify(None, "", Some(&e), label);
            return Check {
                status,
                message,
                latency_ms,
                models: vec![],
            };
        }
    };
    let code = res.status();
    let body = res.text().await.unwrap_or_default();
    if !code.is_success() {
        let (status, message) = classify(Some(code), &body, None, label);
        return Check {
            status,
            message,
            latency_ms,
            models: vec![],
        };
    }
    let value: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => {
            return Check {
                status: Status::Error,
                message: format!("{label} sent a model list Doc Forge couldn't read."),
                latency_ms,
                models: vec![],
            }
        }
    };
    let mut models = parse_models(kind, &value);
    models.sort_by(|a, b| a.router.cmp(&b.router).then_with(|| a.name.cmp(&b.name)));
    let n = models.len();
    Check {
        status: Status::Connected,
        message: format!("{n} chat model{}", if n == 1 { "" } else { "s" }),
        latency_ms,
        models,
    }
}

/// Normalise `/models` responses. Keeps only models that can chat.
pub fn parse_models(kind: Kind, value: &Value) -> Vec<ModelInfo> {
    let items = value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    items
        .iter()
        .filter_map(|m| {
            let id = m.get("id").and_then(|v| v.as_str())?.to_string();
            let model_kind = m.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            // Fireworks lists embedders/rerankers with supports_chat=true; trust `kind`.
            if model_kind.contains("EMBEDDING")
                || m.get("supports_chat").and_then(|v| v.as_bool()) == Some(false)
            {
                return None;
            }
            if kind == Kind::Openai && !looks_like_chat_model(&id) {
                return None;
            }
            let context_length = m
                .get("context_length")
                .or_else(|| m.pointer("/top_provider/context_length"))
                .or_else(|| m.get("max_input_tokens"))
                .and_then(|v| v.as_u64());
            let tools = match kind {
                Kind::Anthropic => Some(true),
                _ => m
                    .get("supports_tools")
                    .and_then(|v| v.as_bool())
                    .or_else(|| {
                        m.get("supported_parameters")
                            .and_then(|p| p.as_array())
                            .map(|p| p.iter().any(|x| x.as_str() == Some("tools")))
                    }),
            };
            let vision = match kind {
                Kind::Anthropic => Some(true),
                _ => m
                    .get("supports_image_input")
                    .and_then(|v| v.as_bool())
                    .or_else(|| {
                        m.pointer("/architecture/input_modalities")
                            .and_then(|p| p.as_array())
                            .map(|p| p.iter().any(|x| x.as_str() == Some("image")))
                    }),
            };
            let router = model_kind == "ROUTER"
                || id.contains("/routers/")
                || id == "auto"
                || id.ends_with("/auto")
                || id == "openrouter/auto";
            let name = match kind {
                Kind::Anthropic => m
                    .get("display_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&id)
                    .to_string(),
                Kind::Openrouter => m
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&id)
                    .to_string(),
                _ => short_name(&id),
            };
            Some(ModelInfo {
                id,
                name,
                context_length,
                tools,
                vision,
                router,
            })
        })
        .collect()
}

fn short_name(id: &str) -> String {
    id.rsplit('/').next().unwrap_or(id).to_string()
}

/// OpenAI's list mixes in embeddings, audio, image and moderation models without metadata.
fn looks_like_chat_model(id: &str) -> bool {
    const NOT_CHAT: [&str; 11] = [
        "embedding",
        "tts",
        "whisper",
        "dall-e",
        "moderation",
        "image",
        "audio",
        "transcribe",
        "realtime",
        "davinci",
        "babbage",
    ];
    !NOT_CHAT.iter().any(|w| id.contains(w))
}

// ---- Round-trip test -----------------------------------------------------------------

const TEST_PROMPT: &str = "Reply with exactly: connection ok";
/// Reasoning models spend part of the budget thinking before they answer.
const TEST_MAX_TOKENS: u32 = 1024;

pub async fn test_model(
    http: &reqwest::Client,
    kind: Kind,
    base_url: &str,
    key: Option<&str>,
    model: &str,
) -> TestResult {
    complete(
        http,
        kind,
        base_url,
        key,
        model,
        None,
        TEST_PROMPT,
        TEST_MAX_TOKENS,
    )
    .await
}

/// One non-streaming completion (an optional system prompt and one user message).
/// `max_tokens` must leave room for reasoning models to think before they answer.
#[allow(clippy::too_many_arguments)]
pub async fn complete(
    http: &reqwest::Client,
    kind: Kind,
    base_url: &str,
    key: Option<&str>,
    model: &str,
    system: Option<&str>,
    prompt: &str,
    max_tokens: u32,
) -> TestResult {
    let info = kind.info();
    let base = trim_base(base_url);
    let (url, body) = match info.protocol {
        Protocol::OpenAi => {
            let mut messages = vec![];
            if let Some(s) = system {
                messages.push(json!({ "role": "system", "content": s }));
            }
            messages.push(json!({ "role": "user", "content": prompt }));
            (
                format!("{base}/chat/completions"),
                json!({ "model": model, "max_tokens": max_tokens, "messages": messages }),
            )
        }
        Protocol::Anthropic => {
            let mut body = json!({ "model": model, "max_tokens": max_tokens, "messages": [{ "role": "user", "content": prompt }] });
            if let Some(s) = system {
                body["system"] = json!(s);
            }
            (format!("{base}/messages"), body)
        }
    };
    let started = Instant::now();
    let res = authorize(http.post(&url), info.protocol, key)
        .json(&body)
        .send()
        .await;
    let fail = |status: Status, message: String, latency_ms: u64| TestResult {
        status,
        reply: String::new(),
        reasoned: false,
        latency_ms,
        usage: Usage::default(),
        message,
    };
    let res = match res {
        Ok(r) => r,
        Err(e) => {
            let (s, m) = classify(None, "", Some(&e), info.label);
            return fail(s, m, started.elapsed().as_millis() as u64);
        }
    };
    let code = res.status();
    let text = res.text().await.unwrap_or_default();
    let latency_ms = started.elapsed().as_millis() as u64;
    if !code.is_success() {
        let (s, m) = classify(Some(code), &text, None, info.label);
        return fail(s, m, latency_ms);
    }
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let (reply, reasoned, usage, finish) = match info.protocol {
        Protocol::OpenAi => {
            let msg = v.pointer("/choices/0/message");
            let reply = msg
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let reasoned = msg
                .and_then(|m| m.get("reasoning_content").or_else(|| m.get("reasoning")))
                .and_then(|r| r.as_str())
                .is_some_and(|r| !r.is_empty());
            let u = v.get("usage");
            let usage = Usage {
                input_tokens: u
                    .and_then(|u| u.get("prompt_tokens"))
                    .and_then(|x| x.as_u64()),
                output_tokens: u
                    .and_then(|u| u.get("completion_tokens"))
                    .and_then(|x| x.as_u64()),
                reasoning_tokens: u
                    .and_then(|u| u.pointer("/completion_tokens_details/reasoning_tokens"))
                    .and_then(|x| x.as_u64()),
            };
            let finish = v
                .pointer("/choices/0/finish_reason")
                .and_then(|f| f.as_str())
                .unwrap_or("")
                .to_string();
            (reply, reasoned, usage, finish)
        }
        Protocol::Anthropic => {
            let blocks = v
                .get("content")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default();
            let reply = blocks
                .iter()
                .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<String>()
                .trim()
                .to_string();
            let reasoned = blocks
                .iter()
                .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("thinking"));
            let u = v.get("usage");
            let usage = Usage {
                input_tokens: u
                    .and_then(|u| u.get("input_tokens"))
                    .and_then(|x| x.as_u64()),
                output_tokens: u
                    .and_then(|u| u.get("output_tokens"))
                    .and_then(|x| x.as_u64()),
                reasoning_tokens: None,
            };
            let finish = v
                .get("stop_reason")
                .and_then(|f| f.as_str())
                .unwrap_or("")
                .to_string();
            (reply, reasoned, usage, finish)
        }
    };
    if reply.is_empty() {
        let message = if finish == "length" || finish == "max_tokens" {
            "The model used its whole budget before answering.".to_string()
        } else {
            "The model returned an empty reply.".to_string()
        };
        return TestResult {
            status: Status::Error,
            reply,
            reasoned,
            latency_ms,
            usage,
            message,
        };
    }
    TestResult {
        status: Status::Connected,
        reply,
        reasoned,
        latency_ms,
        usage,
        message: String::new(),
    }
}

/// Last four characters, for display only.
pub fn key_hint(key: &str) -> String {
    let tail: String = key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fireworks_list_and_drops_embedders() {
        let v = json!({ "data": [
            { "id": "accounts/fireworks/models/kimi-k3", "kind": "HF_BASE_MODEL", "supports_chat": true, "supports_tools": true, "supports_image_input": true, "context_length": 1048576 },
            { "id": "accounts/fireworks/models/qwen3-embedding-8b", "kind": "EMBEDDING_MODEL", "supports_chat": true },
            { "id": "accounts/fireworks/models/rune", "kind": "HF_BASE_MODEL", "supports_chat": false },
            { "id": "accounts/fireworks/routers/glm-5p3-fast", "kind": "HF_BASE_MODEL", "supports_chat": true, "supports_tools": true },
        ]});
        let m = parse_models(Kind::Fireworks, &v);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].name, "kimi-k3");
        assert_eq!(m[0].context_length, Some(1048576));
        assert_eq!(m[0].tools, Some(true));
        assert_eq!(m[0].vision, Some(true));
        assert!(!m[0].router);
        assert!(m[1].router);
    }

    #[test]
    fn parses_openrouter_capabilities() {
        let v = json!({ "data": [{ "id": "x/y", "name": "Y", "context_length": 8000, "supported_parameters": ["tools", "temperature"], "architecture": { "input_modalities": ["text"] } }]});
        let m = parse_models(Kind::Openrouter, &v);
        assert_eq!(m[0].tools, Some(true));
        assert_eq!(m[0].vision, Some(false));
        assert_eq!(m[0].name, "Y");
    }

    #[test]
    fn filters_openai_non_chat() {
        let v = json!({ "data": [{ "id": "text-embedding-3-large" }, { "id": "gpt-x" }, { "id": "whisper-1" }]});
        let m = parse_models(Kind::Openai, &v);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].tools, None);
    }

    #[test]
    fn readable_errors() {
        assert_eq!(
            classify(
                Some(reqwest::StatusCode::UNAUTHORIZED),
                "",
                None,
                "Fireworks"
            )
            .0,
            Status::Unauthorized
        );
        let (s, m) = classify(
            Some(reqwest::StatusCode::TOO_MANY_REQUESTS),
            r#"{"error":{"message":"slow down"}}"#,
            None,
            "X",
        );
        assert_eq!(s, Status::Error);
        assert_eq!(m, "Rate limited by X: slow down");
        assert_eq!(key_hint("fw_abcdefgh"), "…efgh");
    }
}
