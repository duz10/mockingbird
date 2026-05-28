//! Local Ollama dispatcher — the G1 carve-out from ADR 0048.
//!
//! Phase 0 deliberately **does not** reuse the production
//! `cleanup::OllamaProvider`. The validation harness must remain
//! deletable in one stroke (`rm -rf experimental/kg-validation/`)
//! without dragging in CUDA / whisper-rs / the full cleanup trait
//! surface — see spec §5 and ADR 0048 §5.1.
//!
//! ## Wire format
//!
//! POST `/api/generate` with `{model, prompt, system, stream: false,
//! options: {temperature, seed, num_ctx}}`. Response is a single JSON
//! object with a `.response` string. We parse with `serde_json::Value`
//! rather than a typed struct because Ollama is a moving target;
//! we only need one field today and forward-compat trumps strictness
//! in a throwaway sandbox.
//!
//! ## Determinism (ADR 0048 §G4 + spec §8.5)
//!
//! Temperature is pinned to 0.2 by callers (the trait carries it,
//! not the impl, so the test mock can verify the value reached the
//! wire). Seed is set per-run so two passes over the same corpus on
//! the same model produce comparable outputs — the §8.5 two-run
//! stability check depends on that contract.

use serde::Serialize;
use serde_json::Value;

/// Options forwarded into Ollama's `options` payload. Kept tiny —
/// only the knobs Phase 0 actually exercises. Anything else can be
/// added when a future wave needs it; YAGNI now.
#[derive(Debug, Clone, Serialize)]
pub struct GenerateOptions {
    /// Sampling temperature. ADR 0048 §G4 pins this at 0.2 for all
    /// production passes; tests sometimes use 0.0 for deterministic
    /// mocks.
    pub temperature: f32,
    /// Per-run seed; `None` lets Ollama pick. Spec §8.5 stability
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
    ) -> anyhow::Result<String>;
}

/// Concrete client that POSTs to a running Ollama daemon.
pub struct OllamaClient {
    client: reqwest::blocking::Client,
    base_url: String,
}

impl OllamaClient {
    /// Defaults to the standard local Ollama endpoint.
    pub fn new() -> Self {
        Self::with_base_url("http://localhost:11434")
    }

    pub fn with_base_url(url: impl Into<String>) -> Self {
        Self {
            client: reqwest::blocking::Client::builder()
                // Generous: small local models can take 20+ seconds
                // per call on cold start.
                .timeout(std::time::Duration::from_secs(180))
                .build()
                .expect("reqwest blocking client builds with default config"),
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
    ) -> anyhow::Result<String> {
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
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .map_err(|e| anyhow::anyhow!("POST {url} failed: {e}"))?;

        let status = resp.status();
        let text = resp
            .text()
            .map_err(|e| anyhow::anyhow!("read body of {url}: {e}"))?;
        if !status.is_success() {
            anyhow::bail!("ollama {url} returned HTTP {status}: {text}");
        }

        let parsed: Value = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("ollama returned non-JSON body: {e}\nraw: {text}"))?;
        let response = parsed
            .get("response")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("ollama response missing `.response` field: {parsed}"))?
            .to_string();
        Ok(response)
    }
}

// ────────────────────────────────────────────────────────────────────
// Test-only mock — shared across pass tests and harness tests.
//
// `#[cfg(test)] pub mod testing` is the simplest way to expose a
// helper to other modules' unit tests without polluting the public
// API. (See AGENTS.md "No shortcuts" — a separate `testing` feature
// flag would be cleaner if this were a real crate; for a sandbox
// it's overkill.)
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
        ) -> anyhow::Result<String> {
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
                    anyhow::anyhow!(
                        "MockOllama: no rule matched and no default_response set. Prompt was:\n{prompt}"
                    )
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
