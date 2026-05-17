//! Anthropic Claude provider — cloud cleanup with BYO key.
//!
//! Per PLAN §8: the user provides their API key via Settings; we
//! validate by hitting `GET /v1/models` before storing. Models per
//! pre-flight decision 6.
//!
//! Retry policy: 429 / 503 / network blip → exponential backoff
//! (200 ms → 400 ms → 800 ms) with full jitter; max 3 attempts. Any
//! 4xx other than 429 / 401 surfaces immediately.
//!
//! API key is **never** logged. The provider holds it in a private
//! `String`; `Debug` is custom-implemented to redact.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

use super::provider::{provider_http_error, CleanupProvider, CleanupRequest, CleanupResult};

/// Anthropic API base URL.
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

/// Anthropic version header required on every request.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_RETRIES: u32 = 3;

/// Cloud-side cleanup provider.
pub struct ClaudeProvider {
    base_url: String,
    api_key: String,
    agent: ureq::Agent,
}

impl ClaudeProvider {
    /// Construct with a freshly-validated API key. Callers should
    /// have already round-tripped `validate_key` before reaching here.
    pub fn new(api_key: String) -> Self {
        Self::with_base_url(DEFAULT_BASE_URL.to_string(), api_key)
    }

    /// Custom base URL (testing only).
    pub fn with_base_url(base_url: String, api_key: String) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build();
        Self {
            base_url,
            api_key,
            agent,
        }
    }

    /// Hit `GET /v1/models` to verify the key works. Per PLAN §8 the
    /// settings UI calls this before storing the key in DPAPI.
    /// Returns `Ok(())` on 200; any other status maps to
    /// `AppError::Cleanup`.
    pub fn validate_key(&self) -> AppResult<()> {
        let url = format!("{}/v1/models", self.base_url);
        self.agent
            .get(&url)
            .set("x-api-key", &self.api_key)
            .set("anthropic-version", ANTHROPIC_VERSION)
            .call()
            .map_err(provider_http_error)?;
        Ok(())
    }
}

impl std::fmt::Debug for ClaudeProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaudeProvider")
            .field("base_url", &self.base_url)
            .field("api_key", &"<REDACTED>")
            .finish()
    }
}

impl CleanupProvider for ClaudeProvider {
    fn cleanup(&self, request: CleanupRequest<'_>) -> AppResult<CleanupResult> {
        let url = format!("{}/v1/messages", self.base_url);
        let body = MessagesRequest {
            model: request.model_id,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            messages: vec![Message {
                role: "user",
                content: request.prompt,
            }],
        };
        let payload = serde_json::to_value(&body)
            .map_err(|e| AppError::Cleanup(format!("claude request serialize: {e}")))?;

        let mut attempt = 0_u32;
        let start = Instant::now();
        loop {
            attempt += 1;
            let result = self
                .agent
                .post(&url)
                .set("x-api-key", &self.api_key)
                .set("anthropic-version", ANTHROPIC_VERSION)
                .set("Content-Type", "application/json")
                .send_json(payload.clone());

            match result {
                Ok(resp) => {
                    let parsed: MessagesResponse = resp
                        .into_json()
                        .map_err(|e| AppError::Cleanup(format!("claude invalid response: {e}")))?;
                    let text = parsed
                        .content
                        .into_iter()
                        .filter_map(|c| if c.kind == "text" { Some(c.text) } else { None })
                        .collect::<Vec<_>>()
                        .join("");
                    return Ok(CleanupResult {
                        text,
                        model_used: parsed.model,
                        latency_ms: start.elapsed().as_millis() as u64,
                        input_tokens: parsed.usage.as_ref().map(|u| u.input_tokens),
                        output_tokens: parsed.usage.as_ref().map(|u| u.output_tokens),
                    });
                }
                Err(e) if should_retry(&e) && attempt < MAX_RETRIES => {
                    let backoff = backoff_for_attempt(attempt);
                    tracing::warn!(
                        attempt,
                        backoff_ms = backoff.as_millis() as u64,
                        "claude retryable error; backing off"
                    );
                    std::thread::sleep(backoff);
                    continue;
                }
                Err(e) => return Err(provider_http_error(e)),
            }
        }
    }

    fn provider_name(&self) -> &'static str {
        "claude"
    }

    fn supports_model(&self, model_id: &str) -> bool {
        model_id.starts_with("claude-")
    }
}

/// Decide whether a `ureq::Error` is in the "retry" bucket.
///
/// Retry: 429 (rate limit), 503 (service unavailable), any transport
/// error (TCP reset, DNS blip, connection refused — possibly a flaky
/// network rather than a permanent outage).
///
/// Don't retry: 401 (bad key — would just burn through retries),
/// 400 (bad request — won't get better), 5xx other than 503.
fn should_retry(e: &ureq::Error) -> bool {
    match e {
        ureq::Error::Status(429, _) => true,
        ureq::Error::Status(503, _) => true,
        ureq::Error::Status(_, _) => false,
        ureq::Error::Transport(_) => true,
    }
}

/// Exponential backoff with full jitter. attempt=1 → 100-200 ms;
/// attempt=2 → 200-400 ms; attempt=3 → 400-800 ms.
fn backoff_for_attempt(attempt: u32) -> Duration {
    let base_ms = 100_u64 << (attempt.saturating_sub(1));
    // Simple deterministic-ish jitter from process clock — avoids
    // pulling in `rand` for this single use.
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let jitter = (now_ns % base_ms) + 1;
    Duration::from_millis(base_ms + jitter)
}

// --------------------------------------------------------------------
// Serde DTOs.
// --------------------------------------------------------------------

#[derive(Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    temperature: f32,
    messages: Vec<Message<'a>>,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct MessagesResponse {
    model: String,
    content: Vec<ContentBlock>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: u32,
    output_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_name_is_stable() {
        let p = ClaudeProvider::new("test".into());
        assert_eq!(p.provider_name(), "claude");
    }

    #[test]
    fn supports_model_accepts_claude_rejects_local() {
        let p = ClaudeProvider::new("test".into());
        assert!(p.supports_model("claude-3-5-haiku-20241022"));
        assert!(p.supports_model("claude-3-5-sonnet-20241022"));
        assert!(!p.supports_model("qwen2.5:3b"));
        assert!(!p.supports_model("gpt-4"));
    }

    #[test]
    fn debug_redacts_api_key() {
        let p = ClaudeProvider::new("sk-ant-secretkeyhere".into());
        let dbg = format!("{p:?}");
        assert!(!dbg.contains("secretkeyhere"));
        assert!(dbg.contains("REDACTED"));
    }

    #[test]
    fn should_retry_categorises_correctly() {
        // We can construct fake transport errors via a deliberately
        // bad URL — but for unit-test purity, just exercise the match
        // arms directly via the Status variant builder helper.
        // ureq::Error::Status is constructable.
        // Build a minimal Response for the Status variant.
        let resp = ureq::Response::new(429, "Too Many", "rate limit").unwrap();
        let e = ureq::Error::Status(429, resp);
        assert!(should_retry(&e));

        let resp = ureq::Response::new(503, "Unavailable", "service unavailable").unwrap();
        let e = ureq::Error::Status(503, resp);
        assert!(should_retry(&e));

        let resp = ureq::Response::new(401, "Unauthorized", "bad key").unwrap();
        let e = ureq::Error::Status(401, resp);
        assert!(!should_retry(&e));

        let resp = ureq::Response::new(400, "Bad Request", "bad body").unwrap();
        let e = ureq::Error::Status(400, resp);
        assert!(!should_retry(&e));
    }

    #[test]
    fn backoff_grows_exponentially() {
        let b1 = backoff_for_attempt(1).as_millis() as u64;
        let b2 = backoff_for_attempt(2).as_millis() as u64;
        let b3 = backoff_for_attempt(3).as_millis() as u64;
        // Lower bounds (no jitter) double per attempt.
        assert!((100..300).contains(&b1), "b1={b1}");
        assert!((200..500).contains(&b2), "b2={b2}");
        assert!((400..900).contains(&b3), "b3={b3}");
    }

    /// Live; requires `$env:ANTHROPIC_API_KEY` set. `#[ignore]`d.
    #[test]
    #[ignore = "requires ANTHROPIC_API_KEY env"]
    fn live_validate_key_succeeds() {
        let key = std::env::var("ANTHROPIC_API_KEY").expect("set ANTHROPIC_API_KEY");
        let p = ClaudeProvider::new(key);
        let r = p.validate_key();
        assert!(r.is_ok(), "validate_key returned {r:?}");
    }
}
