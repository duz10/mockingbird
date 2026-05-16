//! Transcript cleanup — the post-STT polish layer.
//!
//! Wave 4 ships the **trait + passthrough** only. The LLM-backed
//! cleaner (which routes raw transcripts through the prompts under
//! `cleanup/prompts/` to fix disfluencies, capitalisation, etc.) is
//! Phase 4 territory.
//!
//! Keeping the trait stable now means the Wave 4 orchestrator pipeline
//! is locked in: `audio → STT → Cleaner::clean → Injector::inject`.
//! Phase 4 swaps the [`PassthroughCleaner`] for an LLM implementation
//! without touching the orchestrator.

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
