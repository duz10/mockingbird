//! Local embeddings dispatcher — graduated from the sandbox under
//! Wave 2 Task 5 (`mb-ygds`).
//!
//! Per ADR 0049 Amendment A1, this module is preserved for **entity
//! disambiguation** (NOT classification). It is wired in this module
//! tree but is **not yet consumed by `run_pipeline`** in 1A; a
//! follow-up wave will connect it to the entity-extraction path.
//! That's the reason the parent `kg::mod` puts an `#[allow(dead_code)]`
//! on the `embeddings` declaration — the dispatcher trait + cosine
//! helper + mock all exist, intentionally, ahead of their first
//! caller in the production crate.
//!
//! ## What changed from the sandbox
//!
//! - `anyhow::Result` → `Result<_, EmbedError>` (thiserror enum;
//!   binding parameter D3).
//! - `reqwest::blocking::Client` → `ureq::Agent` (binding parameter D1).
//! - Otherwise byte-stable: same `EmbeddingsDispatcher` trait, same
//!   `cosine_similarity` helper, same `MockEmbedder` shape in
//!   `#[cfg(test)] pub mod testing`.
//!
//! ## Wire format
//!
//! POST `/api/embed` with `{model, input}` where `input` is a string.
//! Response is `{embeddings: [[f32; dim]]}`. We use the single-input
//! form because per-call instrumentation is cleaner than batches.
//! `nomic-embed-text` returns 768-dim float vectors. Cosine similarity
//! is the natural distance for normalized text embeddings.

use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Errors the embeddings dispatcher can produce.
#[derive(Debug, Error)]
pub enum EmbedError {
    #[error("ollama transport error against {url}: {source}")]
    Transport {
        url: String,
        source: Box<ureq::Error>,
    },
    #[error("ollama {url} returned HTTP {status}: {body}")]
    BadStatus {
        url: String,
        status: u16,
        body: String,
    },
    #[error("ollama body read of {url} failed: {error}")]
    Body { url: String, error: String },
    #[error("ollama embed returned non-JSON from {url}: {error}\nraw: {raw}")]
    Json {
        url: String,
        error: String,
        raw: String,
    },
    #[error("missing `.embeddings` array in response: {body}")]
    MissingEmbeddings { body: String },
    #[error("some embedding values were not parseable as f64 (got {got} of {expected})")]
    ParseValues { got: usize, expected: usize },
    /// Test-only mock failure path (kept always-compiled for the
    /// same simplicity reason as `OllamaError::Mock`).
    #[error("MockEmbedder: {0}")]
    Mock(String),
}

/// Single-method trait so the embeddings classifier can be unit-tested
/// without a live Ollama. `Send + Sync` for future parallel harness use.
pub trait EmbeddingsDispatcher: Send + Sync {
    /// Embed a single text. Returns the f32 vector. Caller picks the
    /// model so the same dispatcher can serve multiple embedding
    /// models (we currently only use `nomic-embed-text`, but the
    /// trait stays generic).
    fn embed(&self, model: &str, text: &str) -> Result<Vec<f32>, EmbedError>;
}

/// Cosine similarity for two equal-length vectors. Embeddings are
/// not pre-normalized by Ollama, so we normalize on the fly.
///
/// Returns `f32::NAN` if either vector is zero-length or has zero
/// norm (defensive — both indicate upstream bugs we'd want to see
/// surface).
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return f32::NAN;
    }
    let mut dot = 0f32;
    let mut na = 0f32;
    let mut nb = 0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return f32::NAN;
    }
    dot / (na.sqrt() * nb.sqrt())
}

#[derive(Debug, Clone, Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a str,
}

/// Concrete client that POSTs to a running Ollama daemon. Mirrors
/// the `OllamaClient` setup in `ollama.rs` — generous timeout (cold
/// model load), blocking client (harness is single-threaded).
pub struct OllamaEmbedder {
    agent: ureq::Agent,
    base_url: String,
}

impl OllamaEmbedder {
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

impl Default for OllamaEmbedder {
    fn default() -> Self {
        Self::new()
    }
}

impl EmbeddingsDispatcher for OllamaEmbedder {
    fn embed(&self, model: &str, text: &str) -> Result<Vec<f32>, EmbedError> {
        let body = EmbedRequest { model, input: text };
        let url = format!("{}/api/embed", self.base_url.trim_end_matches('/'));

        let resp = match self.agent.post(&url).send_json(
            serde_json::to_value(&body)
                .expect("EmbedRequest is trivially serializable; serde_json::to_value cannot fail"),
        ) {
            Ok(r) => r,
            Err(ureq::Error::Status(status, response)) => {
                let body = response
                    .into_string()
                    .unwrap_or_else(|e| format!("<body read failed: {e}>"));
                return Err(EmbedError::BadStatus { url, status, body });
            }
            Err(e) => {
                return Err(EmbedError::Transport {
                    url,
                    source: Box::new(e),
                });
            }
        };

        let raw = resp.into_string().map_err(|e| EmbedError::Body {
            url: url.clone(),
            error: e.to_string(),
        })?;

        let parsed: Value = serde_json::from_str(&raw).map_err(|e| EmbedError::Json {
            url: url.clone(),
            error: e.to_string(),
            raw: raw.clone(),
        })?;
        let arr = parsed
            .get("embeddings")
            .and_then(|v| v.as_array())
            .ok_or_else(|| EmbedError::MissingEmbeddings {
                body: parsed.to_string(),
            })?;
        let first = arr.first().and_then(|v| v.as_array()).ok_or_else(|| {
            EmbedError::MissingEmbeddings {
                body: parsed.to_string(),
            }
        })?;
        let vec: Vec<f32> = first
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();
        if vec.len() != first.len() {
            return Err(EmbedError::ParseValues {
                got: vec.len(),
                expected: first.len(),
            });
        }
        Ok(vec)
    }
}

// ────────────────────────────────────────────────────────────────────
// Test-only mock — used by exemplar-pool tests + reclassify-binary
// tests. Same pattern as `ollama::testing::MockOllama`.
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Deterministic mock: every text gets a stable vector from a
    /// caller-supplied lookup. Tests can register `(text, vector)`
    /// pairs and any unregistered text errors loudly (so test
    /// failures point at missing setup, not silent fallback
    /// weirdness).
    pub struct MockEmbedder {
        vectors: Mutex<HashMap<String, Vec<f32>>>,
        calls: Mutex<Vec<String>>,
    }

    impl MockEmbedder {
        pub fn new() -> Self {
            Self {
                vectors: Mutex::new(HashMap::new()),
                calls: Mutex::new(Vec::new()),
            }
        }

        pub fn with(self, text: impl Into<String>, vec: Vec<f32>) -> Self {
            self.vectors.lock().unwrap().insert(text.into(), vec);
            self
        }

        pub fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Default for MockEmbedder {
        fn default() -> Self {
            Self::new()
        }
    }

    impl EmbeddingsDispatcher for MockEmbedder {
        fn embed(&self, _model: &str, text: &str) -> Result<Vec<f32>, EmbedError> {
            self.calls.lock().unwrap().push(text.to_string());
            self.vectors
                .lock()
                .unwrap()
                .get(text)
                .cloned()
                .ok_or_else(|| EmbedError::Mock(format!("no registered vector for: {text}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_identical_vectors_is_one() {
        let v = vec![1.0_f32, 2.0, 3.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_orthogonal_is_zero() {
        let a = vec![1.0_f32, 0.0];
        let b = vec![0.0_f32, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn cosine_opposite_is_minus_one() {
        let a = vec![1.0_f32, 2.0];
        let b = vec![-1.0_f32, -2.0];
        assert!((cosine_similarity(&a, &b) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_handles_mismatched_lengths() {
        assert!(cosine_similarity(&[1.0], &[1.0, 2.0]).is_nan());
    }

    #[test]
    fn cosine_handles_zero_vector() {
        assert!(cosine_similarity(&[0.0, 0.0], &[1.0, 2.0]).is_nan());
    }

    #[test]
    fn mock_embedder_returns_registered_vectors() {
        use testing::MockEmbedder;
        let m = MockEmbedder::new()
            .with("foo", vec![1.0, 0.0])
            .with("bar", vec![0.0, 1.0]);
        assert_eq!(m.embed("any", "foo").unwrap(), vec![1.0, 0.0]);
        assert_eq!(m.embed("any", "bar").unwrap(), vec![0.0, 1.0]);
        assert_eq!(m.calls(), vec!["foo", "bar"]);
    }

    #[test]
    fn mock_embedder_errors_on_unregistered_text() {
        use testing::MockEmbedder;
        let m = MockEmbedder::new();
        let err = m.embed("any", "missing").unwrap_err().to_string();
        assert!(err.contains("missing"), "got: {err}");
    }
}
