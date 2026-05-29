//! Local Ollama dispatcher — graduated from the sandbox under
//! Wave 2 Task 3 (`mb-khqd`).
//!
//! Per binding parameters D1 + D3, the production crate uses **`ureq`**
//! (sync blocking HTTP — same as `cleanup::ollama`) and **`thiserror`**
//! (no `anyhow`). The sandbox kept `reqwest::blocking` because that's
//! what `wiggum` / the standalone harness binaries already pulled in;
//! the production crate already depends on `ureq` (Phase 4 cleanup
//! providers, ADR 0021), so reusing it here keeps the dep graph thin.
//!
//! ## What stayed bit-stable (parity contract)
//!
//! - `OllamaDispatcher` trait shape — same single `generate` method,
//!   same arg order, same `Send + Sync` bound. Chunk 3's parity probe
//!   wires a [`testing::MockOllama`] through this trait.
//! - `GenerateOptions` field set + `Default` impl.
//! - `MockOllama` semantics: first-substring-match wins, records every
//!   call, exposes `respond_when` + `default_response` + `calls()`.
//! - Wire format: POST `/api/generate`, `stream: false`, options carry
//!   `temperature` + optional `seed` + `num_ctx`, response read as
//!   `serde_json::Value`, the `.response` string is the output.
//!
//! ## What changed
//!
//! - `anyhow::Result` → `Result<_, OllamaError>` (thiserror enum). The
//!   `MockOllama::generate`'s "no rule matched" error became
//!   `OllamaError::Mock(_)`.
//! - `reqwest::blocking::Client` → `ureq::Agent` (built once,
//!   connection-pooled across calls).
//! - Timeouts: 180 s overall (same as sandbox), 5 s connect (same as
//!   `cleanup::ollama::OllamaProvider`).
//! - Adopted `tracing` in `OllamaClient` (the prior `println!` calls
//!   inside the sandbox harness binaries don't live here — only the
//!   `OllamaClient` itself does; the trace points are minimal because
//!   the upstream caller already logs per-pass progress).
//!
//! ## Why we have our own dispatcher, not `cleanup::OllamaProvider`
//!
//! Same answer as the sandbox docstring — `cleanup::OllamaProvider`
//! is a `CleanupProvider` (chat-shaped + token-budget accounting +
//! Claude-vs-Ollama dispatch decision). The KG pipeline talks to a
//! different Ollama endpoint (`/api/generate`, not `/api/chat`),
//! takes a system prompt separately, and returns the raw `.response`
//! string for per-pass JSON parsing. Wrapping `CleanupProvider` to do
//! that would be more code than the 50-line generate fn below.

use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

/// Connect timeout — short, this is "can we reach Ollama at all".
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Generous overall timeout — local small models can take 20+ seconds
/// per call on cold model load, and pass 4 (`extract_entities`) is
/// the worst offender. Same number as the sandbox.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

/// Errors the dispatcher can produce. Variants are deliberately
/// flat — every consumer either reports the failure (per-pass
/// artifact write) or aborts the dictation; there's no
/// recover-and-retry path the variants would feed.
#[derive(Debug, Error)]
pub enum OllamaError {
    /// Underlying transport failure (connection refused, DNS, TLS,
    /// timeout, etc.). String body because `ureq::Error` doesn't
    /// implement `Clone` and the consumer just wants the text.
    #[error("ollama transport error against {url}: {source}")]
    Transport {
        url: String,
        source: Box<ureq::Error>,
    },
    /// Non-2xx HTTP response. Body captured for the artifact.
    #[error("ollama {url} returned HTTP {status}: {body}")]
    BadStatus {
        url: String,
        status: u16,
        body: String,
    },
    /// Body read failed mid-response (broken pipe, etc.).
    #[error("ollama body read of {url} failed: {error}")]
    Body { url: String, error: String },
    /// Body wasn't valid JSON.
    #[error("ollama returned non-JSON body from {url}: {error}\nraw: {raw}")]
    Json {
        url: String,
        error: String,
        raw: String,
    },
    /// `/api/generate` response lacked the `response` string field.
    #[error("ollama response missing `.response` field: {body}")]
    MissingResponseField { body: String },
    /// Test-only — `MockOllama` had no rule matching the prompt AND
    /// no `default_response`. Always-compiled (cheap, simpler than
    /// `#[cfg(test)]` gating the variant + every match arm).
    #[error("MockOllama: {0}")]
    Mock(String),
}

/// Options forwarded into Ollama's `options` payload. Kept tiny —
/// only the knobs Phase 0 actually exercises. Anything else can be
/// added when a future wave needs it; YAGNI now.
#[derive(Debug, Clone, Serialize)]
pub struct GenerateOptions {
    /// Sampling temperature. ADR 0048 §G4 pins this at 0.2 for all
    /// production passes; tests sometimes use 0.0 for deterministic
    /// mocks.
    pub temperature: f32,
    /// Per-run seed; `None` lets Ollama pick. PLAN §8.5 stability
    /// requires callers to set this per run.
    pub seed: Option<i64>,
    /// Context window. 4096 is plenty for a single dictation
    /// (typically <500 words).
    pub num_ctx: usize,
}

impl Default for GenerateOptions {
    fn default() -> Self {
        Self {
            temperature: 0.2,
            seed: None,
            num_ctx: 4096,
        }
    }
}

/// Single-method trait so the four passes can be unit-tested without
/// a live Ollama. `Send + Sync` so a future parallelizing harness
/// can fan out across dictations without re-plumbing.
pub trait OllamaDispatcher: Send + Sync {
    /// Issue a single generation request. Returns the raw `.response`
    /// text — JSON parsing / validation is the caller's job because
    /// each pass has its own expected shape.
    fn generate(
        &self,
        model: &str,
        prompt: &str,
        system: Option<&str>,
        options: &GenerateOptions,
    ) -> Result<String, OllamaError>;
}

/// Concrete client that POSTs to a running Ollama daemon. Holds a
/// `ureq::Agent` so connection pooling kicks in across multiple
/// passes / dictations to the same daemon.
///
/// **Not yet wired by any in-crate caller in Phase 1A** — the
/// dictation loop integration lands in a downstream wave. The
/// `dead_code` allow is the honest signal that this is intentionally
/// unwired in the graduation commit (the sandbox + the Chunk 3
/// parity probe both go through `MockOllama`, not `OllamaClient`).
#[allow(dead_code)]
pub struct OllamaClient {
    agent: ureq::Agent,
    base_url: String,
}

#[allow(dead_code)]
impl OllamaClient {
    /// Defaults to the standard local Ollama endpoint.
    pub fn new() -> Self {
        Self::with_base_url("http://localhost:11434")
    }

    pub fn with_base_url(url: impl Into<String>) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build();
        Self {
            agent,
            base_url: url.into(),
        }
    }
}

impl Default for OllamaClient {
    fn default() -> Self {
        Self::new()
    }
}

impl OllamaDispatcher for OllamaClient {
    fn generate(
        &self,
        model: &str,
        prompt: &str,
        system: Option<&str>,
        options: &GenerateOptions,
    ) -> Result<String, OllamaError> {
        let mut body = serde_json::json!({
            "model": model,
            "prompt": prompt,
            "stream": false,
            "options": {
                "temperature": options.temperature,
                "num_ctx": options.num_ctx,
            }
        });
        if let Some(sys) = system {
            body["system"] = Value::String(sys.to_string());
        }
        if let Some(seed) = options.seed {
            body["options"]["seed"] = Value::from(seed);
        }

        let url = format!("{}/api/generate", self.base_url.trim_end_matches('/'));
        tracing::debug!(
            target: "kg::ollama",
            url = %url,
            model = %model,
            "POST /api/generate"
        );

        let resp = match self.agent.post(&url).send_json(body) {
            Ok(r) => r,
            // `ureq::Error::Status(code, response)` is a non-2xx
            // response with a readable body; everything else is a
            // transport failure (DNS, connection refused, timeout).
            Err(ureq::Error::Status(status, response)) => {
                let body = response
                    .into_string()
                    .unwrap_or_else(|e| format!("<body read failed: {e}>"));
                return Err(OllamaError::BadStatus { url, status, body });
            }
            Err(e) => {
                return Err(OllamaError::Transport {
                    url,
                    source: Box::new(e),
                });
            }
        };

        let text = resp.into_string().map_err(|e| OllamaError::Body {
            url: url.clone(),
            error: e.to_string(),
        })?;

        let parsed: Value = serde_json::from_str(&text).map_err(|e| OllamaError::Json {
            url: url.clone(),
            error: e.to_string(),
            raw: text.clone(),
        })?;
        let response = parsed
            .get("response")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OllamaError::MissingResponseField {
                body: parsed.to_string(),
            })?
            .to_string();
        Ok(response)
    }
}

// ────────────────────────────────────────────────────────────────────
// Test-only mock — shared across pass tests, pipeline tests, and the
// kg-module smoke test. Same shape as the sandbox so future probe
// fixtures slot in unmodified.
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::sync::Mutex;

    /// A canned-response dispatcher used by the pass unit tests and
    /// the pipeline integration test.
    ///
    /// Match rules: the FIRST `(needle, response)` pair whose `needle`
    /// is a substring of the prompt wins. If no needle matches, the
    /// mock returns `default_response` (or errors if none was set).
    /// Match order is insertion order — register specific cases
    /// before fallbacks.
    pub struct MockOllama {
        rules: Mutex<Vec<(String, String)>>,
        default_response: Mutex<Option<String>>,
        calls: Mutex<Vec<RecordedCall>>,
    }

    #[derive(Debug, Clone)]
    pub struct RecordedCall {
        pub model: String,
        pub prompt: String,
        pub system: Option<String>,
        pub options: GenerateOptions,
    }

    impl MockOllama {
        pub fn new() -> Self {
            Self {
                rules: Mutex::new(Vec::new()),
                default_response: Mutex::new(None),
                calls: Mutex::new(Vec::new()),
            }
        }

        /// First matching needle wins; later calls to `respond_when`
        /// append to the end of the rule list.
        pub fn respond_when(self, needle: impl Into<String>, response: impl Into<String>) -> Self {
            self.rules
                .lock()
                .unwrap()
                .push((needle.into(), response.into()));
            self
        }

        pub fn default_response(self, response: impl Into<String>) -> Self {
            *self.default_response.lock().unwrap() = Some(response.into());
            self
        }

        pub fn calls(&self) -> Vec<RecordedCall> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Default for MockOllama {
        fn default() -> Self {
            Self::new()
        }
    }

    impl OllamaDispatcher for MockOllama {
        fn generate(
            &self,
            model: &str,
            prompt: &str,
            system: Option<&str>,
            options: &GenerateOptions,
        ) -> Result<String, OllamaError> {
            self.calls.lock().unwrap().push(RecordedCall {
                model: model.to_string(),
                prompt: prompt.to_string(),
                system: system.map(str::to_string),
                options: options.clone(),
            });

            let rules = self.rules.lock().unwrap();
            for (needle, response) in rules.iter() {
                if prompt.contains(needle.as_str()) {
                    return Ok(response.clone());
                }
            }
            drop(rules);

            self.default_response
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| {
                    OllamaError::Mock(format!(
                        "no rule matched and no default_response set. Prompt was:\n{prompt}"
                    ))
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::MockOllama;
    use super::*;

    #[test]
    fn mock_first_match_wins() {
        let mock = MockOllama::new()
            .respond_when("segment", r#"["a","b"]"#)
            .respond_when("classify", r#"{"category":"personal","entry_type":"task"}"#);
        let opts = GenerateOptions::default();

        let r = mock
            .generate("m", "please segment this", None, &opts)
            .unwrap();
        assert_eq!(r, r#"["a","b"]"#);

        let r = mock
            .generate("m", "please classify this", None, &opts)
            .unwrap();
        assert_eq!(r, r#"{"category":"personal","entry_type":"task"}"#);
    }

    #[test]
    fn mock_default_fallback() {
        let mock = MockOllama::new().default_response("fallback");
        let r = mock
            .generate("m", "no rule matches", None, &GenerateOptions::default())
            .unwrap();
        assert_eq!(r, "fallback");
    }

    #[test]
    fn mock_errors_when_no_rule_and_no_default() {
        let mock = MockOllama::new();
        let err = mock
            .generate("m", "anything", None, &GenerateOptions::default())
            .unwrap_err();
        assert!(err.to_string().contains("no rule matched"));
    }

    #[test]
    fn mock_records_calls() {
        let mock = MockOllama::new().default_response("ok");
        let opts = GenerateOptions {
            temperature: 0.2,
            seed: Some(42),
            num_ctx: 4096,
        };
        mock.generate("qwen2.5:3b", "hello", Some("be terse"), &opts)
            .unwrap();
        let calls = mock.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].model, "qwen2.5:3b");
        assert_eq!(calls[0].prompt, "hello");
        assert_eq!(calls[0].system.as_deref(), Some("be terse"));
        assert_eq!(calls[0].options.seed, Some(42));
        assert!((calls[0].options.temperature - 0.2).abs() < f32::EPSILON);
    }
}
