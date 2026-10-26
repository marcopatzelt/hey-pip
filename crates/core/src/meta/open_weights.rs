//! Private mode: a Meta open-weights model (Muse Glimmer on this computer via llama.cpp
//! `llama-server`, or Muse Spark Open on Baseten) behind an OpenAI-compatible
//! `/chat/completions` endpoint. OWNER: agent "meta".
//!
//! The key (PRIVATE_MODEL_KEY, optional: llama.cpp needs none) is read only here, never
//! printed (manual Debug) and removed from error texts. Errors are `MetaError`s, so the
//! UI's friendly sentences work unchanged.

use std::fmt;
use std::time::Duration;

use anyhow::Result;
use serde_json::{Value, json};

use super::client::MetaError;
use super::responses::data_url;
use crate::config::Config;

/// Muse Glimmer (open weights, runs locally). Default for PRIVATE_MODEL.
pub const MODEL_GLIMMER: &str = "muse-glimmer";
/// Muse Spark Open (open weights) as Baseten names it.
pub const MODEL_SPARK_OPEN: &str = "meta-models/muse-spark-open-1026";

/// Answer length for one letter (JSON with the letter text).
const MAX_TOKENS: u32 = 1500;

/// Client for an OpenAI-compatible chat endpoint serving a Meta open-weights model.
#[derive(Clone)]
pub struct OpenWeightsClient {
    http: reqwest::Client,
    base: String,
    key: Option<String>,
    model: String,
}

impl fmt::Debug for OpenWeightsClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenWeightsClient")
            .field("base", &self.base)
            .field("model", &self.model)
            .field("key", &self.key.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl OpenWeightsClient {
    /// `base` like `http://127.0.0.1:8080/v1`; `key` only for hosted endpoints (Baseten).
    pub fn new(base: impl Into<String>, key: Option<String>, model: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            // A 30B model on a laptop CPU can take minutes for one letter.
            .timeout(Duration::from_secs(600))
            .user_agent(concat!("hey-pip/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest client with rustls");
        Self {
            http,
            base: base.into().trim().trim_end_matches('/').to_string(),
            key: key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()),
            model: model.into(),
        }
    }

    /// None unless private mode is on (`Config::private_mode`). The key comes from
    /// PRIVATE_MODEL_KEY (the `.env` was loaded by `Config::from_env`).
    pub fn from_config(cfg: &Config) -> Option<Self> {
        if !cfg.private_mode() {
            return None;
        }
        let base = cfg.private_model_base.clone()?;
        let key = std::env::var("PRIVATE_MODEL_KEY").ok();
        Some(Self::new(base, key, cfg.private_model.clone()))
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// One chat turn (temperature 0, no streaming) -> the assistant's text (expected: one JSON object).
    pub async fn chat_json(&self, system: &str, user_text: &str, image_png: Option<&[u8]>) -> Result<String> {
        let mut user = vec![json!({"type": "text", "text": user_text})];
        if let Some(png) = image_png {
            user.push(json!({"type": "image_url", "image_url": {"url": data_url("image/png", png)}}));
        }
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "temperature": 0,
            "max_tokens": MAX_TOKENS,
            "stream": false,
        });
        let url = format!("{}/chat/completions", self.base);
        let mut req = self.http.post(&url).json(&body);
        if let Some(key) = &self.key {
            req = req.bearer_auth(key);
        }
        let started = std::time::Instant::now();
        let resp = req.send().await.map_err(|e| MetaError::Network(e.without_url()))?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| MetaError::Network(e.without_url()))?;
        if !status.is_success() {
            let body = self.redact(error_message(&text));
            tracing::warn!(model = %self.model, status = status.as_u16(), "open-weights request failed");
            return Err(MetaError::Http { status: status.as_u16(), body }.into());
        }
        tracing::debug!(model = %self.model, ms = started.elapsed().as_millis() as u64, "open-weights ok");
        let v: Value = serde_json::from_str(&text).map_err(|e| MetaError::Parse(format!("chat/completions: {e}")))?;
        content_text(&v).ok_or_else(|| MetaError::Parse("chat/completions: no message content".into()).into())
    }

    /// Remove the key in case an error answer echoes it.
    fn redact(&self, msg: String) -> String {
        match &self.key {
            Some(k) if k.len() >= 4 && msg.contains(k.as_str()) => msg.replace(k.as_str(), "<redacted>"),
            _ => msg,
        }
    }
}

/// `choices[0].message.content`: a string, or a list of `{type:"text", text}` parts.
fn content_text(v: &Value) -> Option<String> {
    let content = v.pointer("/choices/0/message/content")?;
    match content {
        Value::String(s) => Some(s.clone()),
        Value::Array(parts) => {
            let s: String = parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect();
            Some(s).filter(|s| !s.is_empty())
        }
        _ => None,
    }
}

/// The server's `error.message` (or the raw body), max 300 chars.
fn error_message(body: &str) -> String {
    let msg = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.get("message"))
                .or_else(|| v.get("detail"))
                .or_else(|| v.get("error"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.to_string());
    msg.chars().take(300).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_key_and_base_is_trimmed() {
        let c = OpenWeightsClient::new("http://127.0.0.1:8080/v1/", Some("k-secret".into()), MODEL_GLIMMER);
        let d = format!("{c:?}");
        assert!(!d.contains("k-secret") && d.contains("redacted"), "{d}");
        assert_eq!(c.base(), "http://127.0.0.1:8080/v1");
        assert_eq!(c.model(), "muse-glimmer");
        assert_eq!(c.redact("bad key k-secret".into()), "bad key <redacted>");
        let none = OpenWeightsClient::new("http://x/v1", Some("  ".into()), MODEL_SPARK_OPEN);
        assert!(none.key.is_none(), "a blank key is no key");
    }

    #[test]
    fn content_as_string_or_parts() {
        let s = json!({"choices": [{"message": {"content": "{\"a\":1}"}}]});
        assert_eq!(content_text(&s).as_deref(), Some("{\"a\":1}"));
        let parts = json!({"choices": [{"message": {"content": [{"type": "text", "text": "{}"}]}}]});
        assert_eq!(content_text(&parts).as_deref(), Some("{}"));
        assert_eq!(content_text(&json!({"choices": []})), None);
        assert_eq!(error_message(r#"{"error":{"message":"model not loaded"}}"#), "model not loaded");
        assert_eq!(error_message(r#"{"error":"busy"}"#), "busy");
    }
}
