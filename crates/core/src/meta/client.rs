//! HTTP client. OWNER: agent "meta".
//!
//! Every request goes through `send`, which retries network errors, 429 and 5xx with
//! exponential backoff (honours `Retry-After`, capped). Token usage of every Muse Spark
//! answer is added to a counter shared by all clones of the client.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::Result;
use futures_util::{Stream, StreamExt};
use serde_json::Value;

use super::responses::{ResponseEvent, ResponseOutput, ResponseRequest, Usage, parse_response};
use super::sse::{SseParser, map_event};

/// Client for the Meta Model API. The key is never printed (manual Debug).
#[derive(Clone)]
pub struct MetaClient {
    pub(crate) http: reqwest::Client,
    pub(crate) base: String,
    pub(crate) key: String,
    usage: Arc<UsageCounter>,
    retry: RetryPolicy,
}

impl fmt::Debug for MetaClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MetaClient").field("base", &self.base).field("key", &"<redacted>").finish()
    }
}

/// How often and how patiently a failed request is repeated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total tries including the first one (>= 1).
    pub max_attempts: u32,
    /// Wait before the 2nd try; doubles each time.
    pub base_delay: Duration,
    /// Upper bound for one wait (also for `Retry-After`).
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self { max_attempts: 4, base_delay: Duration::from_millis(500), max_delay: Duration::from_secs(10) }
    }
}

impl RetryPolicy {
    fn delay(&self, attempt: u32, retry_after: Option<Duration>) -> Duration {
        let exp = self.base_delay.saturating_mul(1u32 << (attempt.saturating_sub(1)).min(16));
        retry_after.unwrap_or(exp).min(self.max_delay)
    }
}

#[derive(Debug, Default)]
struct UsageCounter {
    input: AtomicU64,
    output: AtomicU64,
}

/// Errors the UI turns into friendly sentences (never shown as codes).
#[derive(Debug, thiserror::Error)]
pub enum MetaError {
    #[error("no internet connection or the server did not answer")]
    Network(#[source] reqwest::Error),
    #[error("the API answered with status {status}: {body}")]
    Http { status: u16, body: String },
    #[error("unexpected answer from the API: {0}")]
    Parse(String),
    #[error("the answer stopped before it was complete")]
    StreamCut,
    #[error("MODEL_API_KEY is not set")]
    MissingKey,
}

impl MetaError {
    /// True for network problems, 429 and 5xx: worth a retry button.
    pub fn is_retryable(&self) -> bool {
        match self {
            MetaError::Network(e) => !e.is_builder(),
            MetaError::StreamCut => true,
            MetaError::Http { status, .. } => *status == 429 || (500..600).contains(status),
            MetaError::Parse(_) | MetaError::MissingKey => false,
        }
    }

    /// Finds a `MetaError` inside an `anyhow::Error` chain (for the UI's retry decision).
    pub fn find(err: &anyhow::Error) -> Option<&MetaError> {
        err.chain().find_map(|e| e.downcast_ref::<MetaError>())
    }
}

impl MetaClient {
    /// Loads `.env` (see `load_dotenv`), then MODEL_API_KEY (required) and MODEL_API_BASE
    /// (default `meta::DEFAULT_BASE`).
    pub fn from_env() -> Result<Self> {
        load_dotenv();
        let key = std::env::var("MODEL_API_KEY").ok().filter(|k| !k.trim().is_empty()).ok_or(MetaError::MissingKey)?;
        let base = std::env::var("MODEL_API_BASE")
            .ok()
            .filter(|b| !b.trim().is_empty())
            .unwrap_or_else(|| super::DEFAULT_BASE.to_string());
        Ok(Self::new(base, key.trim()))
    }

    /// Explicit constructor (tests point `base` at a wiremock server).
    pub fn new(base: impl Into<String>, key: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(180))
            .user_agent(concat!("hey-pip/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest client with rustls");
        Self {
            http,
            base: base.into().trim_end_matches('/').to_string(),
            key: key.into(),
            usage: Arc::default(),
            retry: RetryPolicy::default(),
        }
    }

    /// Replace the retry policy (tests use tiny delays).
    pub fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// Tokens used by all Muse Spark answers of this client (and its clones) so far.
    pub fn usage_total(&self) -> Usage {
        Usage {
            input_tokens: self.usage.input.load(Ordering::Relaxed),
            output_tokens: self.usage.output.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn add_usage(&self, u: Usage) {
        self.usage.input.fetch_add(u.input_tokens, Ordering::Relaxed);
        self.usage.output.fetch_add(u.output_tokens, Ordering::Relaxed);
        tracing::debug!(input = u.input_tokens, output = u.output_tokens, "meta usage");
    }

    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base, path.trim_start_matches('/'))
    }

    /// Send with bearer auth and retries. `build` is called once per try (multipart bodies
    /// cannot be cloned). Non-2xx answers become `MetaError::Http` with the API's message.
    pub(crate) async fn send(
        &self,
        what: &str,
        build: impl Fn() -> reqwest::RequestBuilder,
    ) -> std::result::Result<reqwest::Response, MetaError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let started = std::time::Instant::now();
            let (err, retry_after) = match build().bearer_auth(&self.key).send().await {
                Ok(r) if r.status().is_success() => {
                    tracing::debug!(what, ms = started.elapsed().as_millis() as u64, "meta ok");
                    return Ok(r);
                }
                Ok(r) => {
                    let status = r.status().as_u16();
                    let retry_after = r
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|h| h.to_str().ok())
                        .and_then(|s| s.trim().parse::<u64>().ok())
                        .map(Duration::from_secs);
                    let body = r.text().await.unwrap_or_default();
                    let body = redact(error_message(&body), &self.key);
                    (MetaError::Http { status, body }, retry_after)
                }
                Err(e) => (MetaError::Network(e.without_url()), None),
            };
            if !err.is_retryable() || attempt >= self.retry.max_attempts.max(1) {
                tracing::warn!(what, attempt, error = %err, "meta request failed");
                return Err(err);
            }
            let delay = self.retry.delay(attempt, retry_after);
            tracing::warn!(what, attempt, error = %err, delay_ms = delay.as_millis() as u64, "meta retry");
            tokio::time::sleep(delay).await;
        }
    }

    /// POST a JSON body, return the JSON answer.
    pub(crate) async fn post_json(&self, path: &str, body: &Value) -> Result<Value> {
        let url = self.url(path);
        let resp = self.send(path, || self.http.post(&url).json(body)).await?;
        let text = resp.text().await.map_err(|e| MetaError::Network(e.without_url()))?;
        Ok(serde_json::from_str(&text).map_err(|e| MetaError::Parse(format!("{path}: {e}")))?)
    }

    /// POST {base}/responses, stream:false.
    pub async fn respond(&self, req: &ResponseRequest) -> Result<ResponseOutput> {
        let mut body = req.to_json();
        body["stream"] = Value::Bool(false);
        // PIP_DUMP_REQ=folder: keep each request body for latency tests (contains the screenshot).
        if let Ok(dir) = std::env::var("PIP_DUMP_REQ") {
            let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
            let _ = std::fs::write(std::path::Path::new(&dir).join(format!("req-{n}.json")), body.to_string());
        }
        let started = std::time::Instant::now();
        let v = self.post_json("responses", &body).await?;
        tracing::debug!(ms = started.elapsed().as_millis() as u64, "responses call");
        let out = parse_response(&v)?;
        self.add_usage(out.usage);
        Ok(out)
    }

    /// POST {base}/responses with stream:true; parses SSE via `sse.rs`.
    pub fn respond_stream(&self, req: &ResponseRequest) -> impl Stream<Item = Result<ResponseEvent>> + Send + 'static {
        let mut body = req.to_json();
        body["stream"] = Value::Bool(true);
        self.stream_json(body)
    }

    /// Stream any /responses body (also used for SAM, whose body has no instructions/reasoning).
    pub(crate) fn stream_json(&self, body: Value) -> impl Stream<Item = Result<ResponseEvent>> + Send + 'static {
        let this = self.clone();
        async_stream::try_stream! {
            let url = this.url("responses");
            let resp = this
                .send("responses(stream)", || {
                    this.http.post(&url).header(reqwest::header::ACCEPT, "text/event-stream").json(&body)
                })
                .await?;
            let mut bytes = resp.bytes_stream();
            let mut parser = SseParser::new();
            let mut pending: Vec<u8> = Vec::new();
            let mut completed = false;
            while let Some(chunk) = bytes.next().await {
                let chunk = chunk.map_err(|e| MetaError::Network(e.without_url()))?;
                pending.extend_from_slice(&chunk);
                let text = take_utf8(&mut pending);
                for data in parser.push(&text) {
                    if let Some(ev) = map_event(&data)? {
                        if let ResponseEvent::Completed(o) = &ev {
                            this.add_usage(o.usage);
                            completed = true;
                        }
                        yield ev;
                    }
                }
            }
            if let Some(data) = parser.finish() {
                if let Some(ev) = map_event(&data)? {
                    if let ResponseEvent::Completed(o) = &ev {
                        this.add_usage(o.usage);
                        completed = true;
                    }
                    yield ev;
                }
            }
            // The connection closed without `response.completed`: the text (e.g. SAM boxes)
            // may be partial, so the caller must not act on it.
            if !completed {
                tracing::warn!("meta stream ended without response.completed");
                Err(MetaError::StreamCut)?;
            }
        }
    }
}

/// `dotenvy::dotenv()` (current folder and its parents), then `.env` next to the program
/// (a double-clicked or shortcut-started exe may run in another folder). Never overrides
/// variables that are already set.
pub fn load_dotenv() {
    dotenvy::dotenv().ok();
    if let Some(env) = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(".env")))
        && env.is_file()
    {
        dotenvy::from_path(env).ok();
    }
    // Where Pip's own "Schlüssel einfügen" field stores the key (%APPDATA%\Pip\.env).
    let env = crate::config::env_file();
    if env.is_file() {
        dotenvy::from_path(env).ok();
    }
}

/// Take the longest valid UTF-8 prefix out of `buf` (a chunk may end inside a character).
fn take_utf8(buf: &mut Vec<u8>) -> String {
    match std::str::from_utf8(buf) {
        Ok(s) => {
            let s = s.to_string();
            buf.clear();
            s
        }
        Err(e) if e.error_len().is_none() => {
            let rest = buf.split_off(e.valid_up_to());
            String::from_utf8(std::mem::replace(buf, rest)).unwrap_or_default()
        }
        Err(_) => String::from_utf8_lossy(&std::mem::take(buf)).into_owned(),
    }
}

/// The API's `error.message` (or the raw body), max 300 chars. Never contains our key.
fn error_message(body: &str) -> String {
    let msg = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.get("message"))
                .or_else(|| v.get("detail"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.to_string());
    msg.chars().take(300).collect()
}

/// Remove the key in case an error answer echoes it.
fn redact(msg: String, key: &str) -> String {
    if key.len() < 8 || !msg.contains(key) { msg } else { msg.replace(key, "<redacted>") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_key() {
        let c = MetaClient::new("https://x/v1/", "sk-secret");
        let d = format!("{c:?}");
        assert!(!d.contains("sk-secret"));
        assert!(d.contains("redacted"));
        assert_eq!(c.url("/responses"), "https://x/v1/responses");
    }

    #[test]
    fn retryable() {
        let http = |s| MetaError::Http { status: s, body: String::new() };
        assert!(http(429).is_retryable());
        assert!(http(503).is_retryable());
        assert!(!http(400).is_retryable());
        assert!(!http(401).is_retryable());
        assert!(!MetaError::MissingKey.is_retryable());
        assert!(!MetaError::Parse("x".into()).is_retryable());
        assert!(MetaError::StreamCut.is_retryable());
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let p = RetryPolicy::default();
        assert_eq!(p.delay(1, None), Duration::from_millis(500));
        assert_eq!(p.delay(2, None), Duration::from_millis(1000));
        assert_eq!(p.delay(3, None), Duration::from_millis(2000));
        assert_eq!(p.delay(30, None), Duration::from_secs(10));
        assert_eq!(p.delay(1, Some(Duration::from_secs(60))), Duration::from_secs(10));
    }

    #[test]
    fn utf8_split() {
        let bytes = "aä".as_bytes();
        let mut buf = bytes[..2].to_vec();
        assert_eq!(take_utf8(&mut buf), "a");
        buf.extend_from_slice(&bytes[2..]);
        assert_eq!(take_utf8(&mut buf), "ä");
        assert!(buf.is_empty());
    }

    #[test]
    fn redacts_echoed_key() {
        assert_eq!(redact("bad key sk-12345678 given".into(), "sk-12345678"), "bad key <redacted> given");
        assert_eq!(redact("abc".into(), ""), "abc");
    }

    #[test]
    fn error_message_extracts() {
        assert_eq!(error_message(r#"{"error":{"message":"bad effort"}}"#), "bad effort");
        assert_eq!(error_message("plain"), "plain");
    }
}
