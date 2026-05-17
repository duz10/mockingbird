//! Transcript cleanup — the post-STT polish layer.
//!
//! Wave 4 shipped the **trait + passthrough** stub. Phase 4 fills in
//! the real pipeline: provider abstraction ([`CleanupProvider`])
//! with Ollama + Claude implementations, token-budget enforcement
//! ([`token_budget`]), few-shot example selection ([`few_shot`]),
//! prompt assembly ([`prompt_builder`]), and the orchestrator-facing
//! glue ([`LlmCleaner`]).
//!
//! The Wave-4 orchestrator interface ([`Cleaner`]) is unchanged —
//! `LlmCleaner` slots in next to `PassthroughCleaner` via the same
//! `Box<dyn Cleaner>` the dictation thread already holds.
//!
//! See [ADR 0021](../../../docs/adr/0021-sync-cleanup-provider.md)
//! for the sync-vs-async-trait decision.

pub mod claude;
pub mod few_shot;
pub mod llm_cleaner;
pub mod ollama;
pub mod prompt_builder;
pub mod provider;
pub mod token_budget;

pub use claude::ClaudeProvider;
pub use llm_cleaner::LlmCleaner;
pub use ollama::OllamaProvider;
pub use provider::{CleanupProvider, CleanupRequest, CleanupResult, StubCleanupProvider};

use crate::error::AppResult;

/// Cleanup trait. `clean(raw, mode_slug)` returns the polished text
/// that will be injected.
pub trait Cleaner: Send {
    /// Return cleaned text for the given raw transcript + mode.
    ///
    /// `mode_slug` is the canonical mode identifier (`"normal"`,
    /// `"fragment"`, `"verbose"`) — the LLM impl selects its prompt
    /// file based on this. The passthrough impl ignores it.
    fn clean(&mut self, raw: &str, mode_slug: &str) -> AppResult<String>;

    /// Identifier for the model that produced the cleaned text,
    /// persisted in `transcripts.model_used` for provenance.
    ///
    /// Default returns `"passthrough"`. The Phase-4 LLM cleaner
    /// overrides this with its actual model identifier (e.g.
    /// `"qwen2.5-7b-instruct-q5_k_m"`).
    fn model_name(&self) -> &str {
        "passthrough"
    }
}

/// Default cleaner — returns the input verbatim.
///
/// This is the Wave-4 default until the LLM cleaner lands in Phase 4.
/// Useful in tests + in CI environments without an LLM endpoint.
#[derive(Debug, Default, Clone, Copy)]
pub struct PassthroughCleaner;

impl PassthroughCleaner {
    /// Construct. No state.
    pub fn new() -> Self {
        Self
    }
}

impl Cleaner for PassthroughCleaner {
    fn clean(&mut self, raw: &str, _mode_slug: &str) -> AppResult<String> {
        Ok(raw.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_returns_input_verbatim() {
        let mut c = PassthroughCleaner::new();
        assert_eq!(c.clean("hello world", "normal").unwrap(), "hello world");
    }

    #[test]
    fn passthrough_handles_empty_string() {
        let mut c = PassthroughCleaner::new();
        assert_eq!(c.clean("", "normal").unwrap(), "");
    }

    #[test]
    fn passthrough_ignores_mode_slug() {
        let mut c = PassthroughCleaner::new();
        // Same input, different modes → same output.
        let a = c.clean("text", "normal").unwrap();
        let b = c.clean("text", "fragment").unwrap();
        let z = c.clean("text", "verbose").unwrap();
        assert_eq!(a, b);
        assert_eq!(b, z);
    }
}
