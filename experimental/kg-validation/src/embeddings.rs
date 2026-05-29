//! Local embeddings dispatcher — ADR 0049 Wave 0.5.2 / Move 2.
//!
//! Phase 0.5 introduces an **embeddings-based classifier** to replace
//! the small-LLM classify pass on category + entry-type. The architectural
//! hypothesis (ADR 0049 §Move 2) is that small local LLMs are poor at
//! consistently applying a coarse-grained finite-set label, but a local
//! embedding model comparing the segment to exemplars of each label can
//! hit it reliably.
//!
//! ## Wire format
//!
//! POST `/api/embed` with `{model, input}` where `input` is a string or
//! string array. Response is `{embeddings: [[f32; dim]]}`. We use the
//! single-input form because batch latency benefit is marginal at this
//! scale (the corpus is ~50 short segments) and per-call instrumentation
//! is cleaner.
//!
//! `nomic-embed-text` returns 768-dim float vectors. Cosine similarity
//! is the natural distance for normalized text embeddings.
//!
//! ## Why a separate dispatcher trait
//!
//! The G1 carve-out logic (see `ollama.rs`) applies the same way here:
//! pass tests need a mock; future parallel harness wants `Send + Sync`;
//! the trait keeps the prompt-resolution path testable in isolation
//! from the network. Same pattern, different endpoint.

use serde::Serialize;
use serde_json::Value;

/// Single-method trait so the embeddings classifier can be unit-tested
/// without a live Ollama. `Send + Sync` for future parallel harness use.
pub trait EmbeddingsDispatcher: Send + Sync {
    /// Embed a single text. Returns the f32 vector. Caller picks the
    /// model so the same dispatcher can serve multiple embedding models
    /// (we currently only use `nomic-embed-text`, but the trait stays
    /// generic).
    fn embed(&self, model: &str, text: &str) -> anyhow::Result<Vec<f32>>;
}

/// Cosine similarity for two equal-length vectors. Embeddings are not
/// pre-normalized by Ollama, so we normalize on the fly.
///
/// Returns `f32::NAN` if either vector is zero-length or has zero norm
/// (defensive — both indicate upstream bugs we'd want to see surface).
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

/// Concrete client that POSTs to a running Ollama daemon. Mirrors the
/// `OllamaClient` setup in `ollama.rs` — generous timeout (cold model
/// load takes >10s), blocking client (harness is single-threaded).
pub struct OllamaEmbedder {
    client: reqwest::blocking::Client,
    base_url: String,
}

impl OllamaEmbedder {
    pub fn new() -> Self {
        Self::with_base_url("http://localhost:11434")
    }

    pub fn with_base_url(url: impl Into<String>) -> Self {
        Self {
            client: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("reqwest blocking client builds with default config"),
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
    fn embed(&self, model: &str, text: &str) -> anyhow::Result<Vec<f32>> {
        let body = EmbedRequest { model, input: text };
        let url = format!("{}/api/embed", self.base_url.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .map_err(|e| anyhow::anyhow!("POST {url} failed: {e}"))?;

        let status = resp.status();
        let raw = resp
            .text()
            .map_err(|e| anyhow::anyhow!("read body of {url}: {e}"))?;
        if !status.is_success() {
            anyhow::bail!("ollama {url} returned HTTP {status}: {raw}");
        }

        let parsed: Value = serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("ollama embed returned non-JSON: {e}\nraw: {raw}"))?;
        let arr = parsed
            .get("embeddings")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("missing `.embeddings` array in: {parsed}"))?;
        let first = arr
            .first()
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("`.embeddings[0]` not an array in: {parsed}"))?;
        let vec: Vec<f32> = first
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();
        if vec.len() != first.len() {
            anyhow::bail!(
                "some embedding values were not parseable as f64 (got {} of {})",
                vec.len(),
                first.len()
            );
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
    /// caller-supplied lookup. Tests can register `(text, vector)` pairs
    /// and any unregistered text errors loudly (so test failures point at
    /// missing setup, not silent fallback weirdness).
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
        fn embed(&self, _model: &str, text: &str) -> anyhow::Result<Vec<f32>> {
            self.calls.lock().unwrap().push(text.to_string());
            self.vectors
                .lock()
                .unwrap()
                .get(text)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("MockEmbedder has no registered vector for: {text}"))
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
