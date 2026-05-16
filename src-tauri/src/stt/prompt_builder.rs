#![allow(missing_docs)] // Scaffold; Wave-4 brief will document.

//! Whisper `initial_prompt` builder.
//!
//! Wave 1 ships the scaffold + the 224-token cap constant. Wave 4
//! fills in the recency × frequency × app-match scoring per PLAN
//! line 1365.

/// Whisper's hard cap on the `initial_prompt` field. Beyond this,
/// Whisper truncates (silently); we truncate explicitly here.
pub const PROMPT_TOKEN_CAP: usize = 224;

/// Inputs to the prompt builder. Wave 4 fills in scoring; for Wave 1
/// this exists so call sites can be written against the final shape.
#[derive(Debug, Clone, Default)]
pub struct PromptBuilderInput<'a> {
    /// Dictionary entries to consider seeding into the prompt.
    /// Scored on (use_count, last_used_at, app_context match).
    pub dictionary: &'a [DictionaryView<'a>],
    /// Current foreground-app context (e.g. "vscode.exe").
    /// Used to boost dictionary entries with matching `app_context`.
    pub foreground_app: Option<&'a str>,
    /// Recent transcripts (most-recent first), used as a recency
    /// signal in the scoring model. Defaults to empty.
    pub recent_transcripts: &'a [&'a str],
}

/// Read-only view of a dictionary entry. Wave 4 pulls this from
/// `db::dictionary::DictionaryEntry`. Keeping a view type here avoids
/// forcing the stt module to depend on db.
#[derive(Debug, Clone)]
pub struct DictionaryView<'a> {
    pub term: &'a str,
    pub canonical: Option<&'a str>,
    pub use_count: i64,
    pub last_used_at: Option<&'a str>,
    pub app_context: Option<&'a str>,
}

/// Assemble a `initial_prompt` for Whisper. Caps at
/// [`PROMPT_TOKEN_CAP`] tokens (approximated by whitespace splits
/// for now; Wave 4 may swap in a real tokenizer if Whisper's BPE
/// truncation produces visible drift).
///
/// Returns `None` if nothing scored above the inclusion threshold.
pub fn build_prompt(_input: &PromptBuilderInput<'_>) -> Option<String> {
    // Wave 4 will:
    //   1. Score each dictionary entry: recency × frequency × app-match
    //   2. Sort descending; take until token budget exhausted
    //   3. Comma-join the canonical forms (or term where canonical is None)
    //   4. Apply PROMPT_TOKEN_CAP via whitespace-split truncation
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_token_cap_is_224() {
        assert_eq!(PROMPT_TOKEN_CAP, 224);
    }

    #[test]
    fn build_prompt_returns_none_for_empty_input() {
        let input = PromptBuilderInput::default();
        assert!(build_prompt(&input).is_none());
    }
}
