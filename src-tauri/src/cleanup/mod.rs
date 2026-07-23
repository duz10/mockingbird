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
// ADR 0064 — RAM-aware effective-model selection (macOS unified-memory
// tier + shared, Windows-byte-identical selector).
pub mod model_select;
pub mod ollama;
pub mod preprocessor;
pub mod prompt_builder;
pub mod provider;
pub mod token_budget;
// ADR 0047 §Wave 2.4 — VRAM probe for the Q5_K_M opt-in gate.
pub mod vram_probe;

pub use claude::ClaudeProvider;
pub use llm_cleaner::LlmCleaner;
pub use ollama::OllamaProvider;
pub use preprocessor::{Preprocessor, Processed, ProcessedNotes, PREPROCESSOR_VERSION};
pub use provider::{CleanupProvider, CleanupRequest, CleanupResult, StubCleanupProvider};

use serde::{Deserialize, Serialize};

use crate::error::AppResult;

/// How aggressively the dictation cleanup pipeline runs (ADR 0047 §Wave 2.1).
///
/// Orthogonal to the tone mode (`casual` / `normal` / `formal`):
/// tone selects voice + register, level selects how much the pipeline
/// is allowed to touch the text. Defaults to [`Self::High`] so existing
/// installs see no behaviour change at upgrade.
///
/// Serializes lowercase (`"none"` / `"light"` / `"medium"` / `"high"`)
/// to match the existing settings string-value convention
/// (`MeetingDefaultSource`, `Theme`, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DictationCleanupLevel {
    /// Raw STT output, byte-for-byte. Skip preprocessor + LLM.
    /// Provenance: `"raw-passthrough"`.
    None,
    /// Deterministic preprocessor only. No LLM call.
    /// Provenance: `"preprocessor-only+<ver>"`.
    Light,
    /// Preprocessor + LLM with the additive-only prompt
    /// (`normal_v6_additive`). LLM may insert punctuation, paragraph
    /// breaks, and list structure, but may not remove or modify
    /// content words. Tone mode is ignored at this level.
    /// Provenance: `"<model>+additive+<ver>"`.
    Medium,
    /// Existing pre-Wave-2 behaviour: preprocessor + LLM with the
    /// mode-specific prompt (`casual_v2` / `normal_v5` / `formal_v2`).
    /// The LLM has full register-shift + list-rendering authority.
    /// Provenance: `"<model>+<ver>"` (unchanged from pre-Wave-2).
    #[default]
    High,
}

/// Mode slug under which the Medium-level additive prompt is stored
/// in the `prompts` table. Reserved here as a `pub const` so the
/// migration 020 SQL, the `LlmCleaner` Medium-branch lookup, and any
/// future provenance-grepping code all reference the same string.
pub const ADDITIVE_PROMPT_MODE_SLUG: &str = "normal_additive";

/// Mode slug for the tier-gated small-model Normal prompt (ADR 0065).
///
/// A hardened variant of `normal@v5` seeded by migration 027 under a
/// parallel slug (same pattern as [`ADDITIVE_PROMPT_MODE_SLUG`]).
/// Selected ONLY at the macOS RAM-aware downsize seam in
/// `dictation/runtime_cleaner.rs::make_default_cleaner` — when the
/// effective model was downsized off the parity model AND the active
/// mode is `normal` — via [`LlmCleaner::with_prompt_mode_override`].
/// On non-macOS the override is never set, so `normal` keeps resolving
/// to `normal@v5` and the 7B / Windows cleanup path is byte-identical.
pub const SMALL_MODEL_PROMPT_MODE_SLUG: &str = "normal_small";

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

    /// The prompt (slug + version) this cleaner actually resolved for
    /// the most recent [`Self::clean`] call, e.g. `"normal_small v2"`.
    ///
    /// Persisted into `sessions.effective_prompt_label` so the dictation
    /// Metadata reports the prompt that REALLY ran rather than inferring
    /// it from the mode's canonical `prompt_id` (which is wrong on the
    /// macOS RAM-aware downsize path, where the override runs
    /// `normal_small` while `prompt_id` still points at normal@v5).
    ///
    /// Default `None` — the passthrough cleaner resolves no prompt, and
    /// any cleaner that doesn't opt in simply leaves the column NULL
    /// (the Metadata then falls back to the canonical version).
    fn prompt_label(&self) -> Option<String> {
        None
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
