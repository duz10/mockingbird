//! Ollama HTTP provider — local-first cleanup.
//!
//! Per PLAN §8: connects to `http://localhost:11434/api/chat`. Health
//! check on app start via `GET /api/tags`. Non-streaming responses
//! (full body waited on); streaming variant exposed for Phase 5's
//! recording-window mid-flight display.
//!
//! ## Why not the Ollama-rs crate
//!
//! `ollama-rs` is async-first and pulls in tokio + a chunk of futures
//! types. ADR 0021 keeps cleanup sync; this provider is ~150 lines of
//! `ureq` against three endpoints. Less surface area, fewer deps.
//!
//! ## Errors map cleanly
//!
//! - Connection refused → `AppError::Cleanup("ollama refused connection: ...")`
//! - 5xx → `AppError::Cleanup("http 503: ...")`
//! - Timeout → `AppError::Cleanup("transport: timed out")`
//! - JSON parse failure → `AppError::Cleanup("invalid response: ...")`

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

use super::provider::{provider_http_error, CleanupProvider, CleanupRequest, CleanupResult};

/// Default endpoint per PLAN §8.
pub const DEFAULT_BASE_URL: &str = "http://localhost:11434";

/// Connect + first-byte timeout. Ollama can take a while to load a
/// cold model; the request timeout below is the real cap.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Wall-clock cap for a single cleanup request. PLAN doesn't pin a
/// hard number; this matches the 30-s "if cleanup hangs, fall back"
/// rule in ADR 0021.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Health-check timeout — short, this is "is Ollama alive at all".
const HEALTH_TIMEOUT: Duration = Duration::from_secs(5);

/// Ollama provider state.
///
/// Holds a ureq agent so connection pooling kicks in across multiple
/// dictations to the same model. `base_url` is overridable so tests
/// (and Phase 7's per-user config) can point at a non-default host.
pub struct OllamaProvider {
    base_url: String,
    agent: ureq::Agent,
}

impl OllamaProvider {
    /// Construct with the PLAN-default base URL.
    pub fn new() -> Self {
        Self::with_base_url(DEFAULT_BASE_URL.to_string())
    }

    /// Construct with a custom base URL (test rigs, alternate
    /// localhost ports for users running multiple Ollama instances).
    pub fn with_base_url(base_url: String) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build();
        Self { base_url, agent }
    }

    /// `GET /api/tags` — returns the list of locally-pulled models.
    /// Used by the first-run wizard to decide whether to prompt a
    /// `POST /api/pull` and by the tray status icon.
    pub fn list_models(&self) -> AppResult<Vec<String>> {
        let url = format!("{}/api/tags", self.base_url);
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout(HEALTH_TIMEOUT)
            .build();
        let resp = agent.get(&url).call().map_err(provider_http_error)?;
        let body: TagsResponse = resp
            .into_json()
            .map_err(|e| AppError::Cleanup(format!("ollama /api/tags invalid response: {e}")))?;
        Ok(body.models.into_iter().map(|m| m.name).collect())
    }

    /// Quick "is Ollama running" probe. Returns `Ok(())` iff the
    /// `/api/tags` endpoint returns 2xx within `HEALTH_TIMEOUT`.
    pub fn health_check(&self) -> AppResult<()> {
        self.list_models().map(|_| ())
    }
}

impl Default for OllamaProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CleanupProvider for OllamaProvider {
    fn cleanup(&self, request: CleanupRequest<'_>) -> AppResult<CleanupResult> {
        let url = format!("{}/api/chat", self.base_url);

        // Ollama's chat schema. `stream: false` returns one body.
        let body = ChatRequest {
            model: request.model_id,
            stream: false,
            options: ChatOptions {
                temperature: request.temperature,
                num_predict: request.max_tokens as i32,
            },
            messages: vec![ChatMessage {
                role: "user",
                content: request.prompt,
            }],
        };

        let start = Instant::now();
        let resp = self
            .agent
            .post(&url)
            .set("Content-Type", "application/json")
            .send_json(
                serde_json::to_value(&body)
                    .map_err(|e| AppError::Cleanup(format!("ollama request serialize: {e}")))?,
            )
            .map_err(provider_http_error)?;

        let body: ChatResponse = resp
            .into_json()
            .map_err(|e| AppError::Cleanup(format!("ollama /api/chat invalid response: {e}")))?;
        let latency_ms = start.elapsed().as_millis() as u64;

        Ok(CleanupResult {
            text: body.message.content,
            model_used: body.model,
            latency_ms,
            input_tokens: body.prompt_eval_count,
            output_tokens: body.eval_count,
        })
    }

    fn provider_name(&self) -> &'static str {
        "ollama"
    }

    fn supports_model(&self, model_id: &str) -> bool {
        // Anything that doesn't look like a Claude model id. The exact
        // model existence check happens via `list_models` at the
        // dispatcher layer.
        !model_id.starts_with("claude-")
    }
}

// --------------------------------------------------------------------
// Serde DTOs. Kept private — outer code only sees AppResult<CleanupResult>.
// --------------------------------------------------------------------

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    stream: bool,
    options: ChatOptions,
    messages: Vec<ChatMessage<'a>>,
}

#[derive(Serialize)]
struct ChatOptions {
    temperature: f32,
    num_predict: i32,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    model: String,
    message: ChatMessageOwned,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
}

#[derive(Deserialize)]
struct ChatMessageOwned {
    content: String,
}

#[derive(Deserialize)]
struct TagsResponse {
    models: Vec<TagsModel>,
}

#[derive(Deserialize)]
struct TagsModel {
    name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_name_is_stable() {
        let p = OllamaProvider::new();
        assert_eq!(p.provider_name(), "ollama");
    }

    #[test]
    fn supports_model_accepts_local_rejects_claude() {
        let p = OllamaProvider::new();
        assert!(p.supports_model("qwen2.5:3b-instruct-q4_K_M"));
        assert!(p.supports_model("gemma2:2b-instruct-q4_K_M"));
        assert!(!p.supports_model("claude-3-5-haiku-20241022"));
    }

    #[test]
    fn with_base_url_honours_override() {
        let p = OllamaProvider::with_base_url("http://example:9999".into());
        assert_eq!(p.base_url, "http://example:9999");
    }

    #[test]
    fn default_base_url_matches_plan() {
        assert_eq!(DEFAULT_BASE_URL, "http://localhost:11434");
    }

    /// Live integration — requires Ollama running locally with the
    /// default model pulled. `#[ignore]` so CI doesn't depend on it.
    #[test]
    #[ignore = "requires local Ollama running"]
    fn live_health_check_succeeds() {
        let p = OllamaProvider::new();
        let r = p.health_check();
        assert!(r.is_ok(), "ollama not reachable: {r:?}");
    }

    #[test]
    #[ignore = "requires local Ollama running + qwen model pulled"]
    fn live_cleanup_returns_text() {
        let p = OllamaProvider::new();
        let req = CleanupRequest {
            prompt: "You are a transcript cleaner. Just echo: hi.",
            raw_transcript: "hi",
            model_id: "qwen2.5:3b-instruct-q4_K_M",
            temperature: 0.0,
            max_tokens: 32,
            mode_slug: "normal",
        };
        let result = p.cleanup(req).expect("ollama cleanup");
        assert!(!result.text.is_empty());
        assert!(result.latency_ms > 0);
    }
}
