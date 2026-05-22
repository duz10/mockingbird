//! Dictation-tuned LLM-pass prompt bodies.
//!
//! ## Why these exist separately from `meetings::prompts`
//!
//! The meeting LLM-pass engine (`meetings::llm_pass`) is prompt-agnostic
//! — it takes a `LlmPassPrompt::Custom(body)` and runs it through
//! Ollama. The MEETING built-in bodies live in
//! `src-tauri/src/meetings/prompts/*.md` and are tuned for multi-speaker
//! meeting transcripts (require explicit owners, formal task assignments,
//! etc.).
//!
//! Voice-dictation memos are a different shape entirely: one speaker,
//! talking to themselves or to a future self, with decisions and
//! intents that are perfectly valid action items even though no one
//! says "Bob, you own this by Friday". Running the meeting
//! `action_items` prompt on a dictation transcript produces "No
//! action items found" 90% of the time, because the prompt explicitly
//! demands an owner and a deadline.
//!
//! So: same three IDs, dictation-tuned bodies. The dictation IPC
//! resolves the built-in name with [`resolve_dictation_prompt`], wraps
//! the result in `LlmPassPrompt::Custom(body)`, and lets the existing
//! engine do the actual Ollama call. Zero duplicated runtime code.
//!
//! ## Why `include_str!`
//!
//! Same reason the meeting prompts do it — the markdown files ship
//! inside the binary (no `models/` style at-runtime download), they
//! get touched by `cargo build` so a typo or missing file is a
//! compile error, and the test suite can assert content without
//! filesystem fixtures.

use crate::error::{AppError, AppResult};

/// Built-in prompt: dictation-tuned summary. See `prompts/summary.md`.
pub const SUMMARY_BODY: &str = include_str!("prompts/summary.md");

/// Built-in prompt: dictation-tuned action-items extractor.
/// See `prompts/action_items.md` for the full text — TL;DR is that it
/// treats decisions and first-person intents as valid actions,
/// because in a personal voice memo they are.
pub const ACTION_ITEMS_BODY: &str = include_str!("prompts/action_items.md");

/// Built-in prompt: punctuation / paragraphing cleanup tuned for a
/// single dictating speaker. See `prompts/cleaner_punctuation.md`.
pub const CLEANER_PUNCTUATION_BODY: &str = include_str!("prompts/cleaner_punctuation.md");

/// Strip an outer markdown code fence from an LLM-pass result, if
/// one is present.
///
/// Why this exists: small instruction-tuned models (3b–7b) frequently
/// wrap their entire reply in ```` ```markdown ... ``` ```` regardless
/// of how loudly the prompt asks them not to. Prompt-engineering this
/// away is brittle — defensive postprocessing is robust. We only
/// strip the OUTER fence so legitimate inner code blocks (e.g. a
/// summary that quotes a code snippet) survive untouched.
///
/// Rules (in order):
///   1. Trim leading + trailing whitespace.
///   2. If the result starts with a fence opener (` ``` ` optionally
///      followed by a language tag like `markdown` or `text`) AND ends
///      with a closing ` ``` ` on its own line, drop both.
///   3. Otherwise return the input unchanged (trimmed).
///
/// Idempotent: applying twice yields the same result as applying once,
/// because step 2 requires the fence to be the outermost wrapper and a
/// stripped string no longer has one.
pub fn strip_markdown_code_fence(text: &str) -> String {
    let trimmed = text.trim();
    // Must start with ``` (three backticks). Anything else: bail.
    let after_open = match trimmed.strip_prefix("```") {
        Some(rest) => rest,
        None => return trimmed.to_string(),
    };
    // The fence opener may have an optional language tag on the same
    // line: ```markdown\n... or ```text\n... or just ```\n...
    // We split at the first newline; everything before it is the tag
    // we discard, everything after it is the body.
    let Some((_lang_tag, body_and_close)) = after_open.split_once('\n') else {
        // No newline after the opening fence — this isn't a multi-line
        // fenced block, leave it alone.
        return trimmed.to_string();
    };
    // The body must end with a closing fence. Trim trailing whitespace
    // first so a stray newline after ``` doesn't defeat the check.
    let body_and_close = body_and_close.trim_end();
    let Some(body) = body_and_close.strip_suffix("```") else {
        // Opener but no closer — bail, don't risk eating real content.
        return trimmed.to_string();
    };
    // Strip a single trailing newline between the body and the closing
    // ``` so we don't return a result ending in `\n` plus the fence's
    // own indentation. `trim_end` on the final return handles the rest.
    body.trim_end().to_string()
}

/// Resolve a built-in dictation prompt name to its body.
///
/// Mirrors the meeting `resolve_builtin` shape. Returning a
/// `&'static str` lets the caller drop it straight into
/// `LlmPassPrompt::Custom(...)` without an allocation that would be
/// wasted on an immutable embedded asset.
pub fn resolve_dictation_prompt(name: &str) -> AppResult<&'static str> {
    match name {
        "summary" => Ok(SUMMARY_BODY),
        "action_items" => Ok(ACTION_ITEMS_BODY),
        "cleaner_punctuation" => Ok(CLEANER_PUNCTUATION_BODY),
        other => Err(AppError::MeetingCapture(format!(
            "unknown built-in dictation prompt: {other:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_body_is_nonempty_and_first_person_framed() {
        let body = resolve_dictation_prompt("summary").unwrap();
        assert!(!body.is_empty());
        // The whole point of the dictation prompts is they avoid
        // "meeting" / "participants" framing. If someone edits the
        // markdown to talk about meetings again, this test should
        // catch it before the regression ships.
        let lower = body.to_lowercase();
        assert!(
            lower.contains("voice memo") || lower.contains("one person"),
            "summary prompt should frame the input as a voice memo, not a meeting"
        );
    }

    #[test]
    fn action_items_body_accepts_decisions() {
        let body = resolve_dictation_prompt("action_items").unwrap();
        let lower = body.to_lowercase();
        // The bug fix in this prompt: treat decisions and intents as
        // actions. Lock that intent in so a future edit can't silently
        // revert to the meeting-style "must have explicit owner".
        assert!(
            lower.contains("decision"),
            "action_items prompt must explicitly include decisions as actions"
        );
        // The meeting variant has "With a stated or strongly implied
        // deadline". Make sure THAT phrasing doesn't sneak in via a
        // copy-paste — the dictation prompt explicitly does NOT
        // require a deadline.
        assert!(
            !lower.contains("stated or strongly implied deadline"),
            "action_items prompt copied meeting deadline requirement"
        );
    }

    #[test]
    fn cleaner_punctuation_body_forbids_paraphrasing() {
        let body = resolve_dictation_prompt("cleaner_punctuation").unwrap();
        let lower = body.to_lowercase();
        // Cleanup must be punctuation-only; the deterministic
        // formatter owns word-level edits.
        assert!(lower.contains("no paraphrasing") || lower.contains("change any word"));
    }

    #[test]
    fn unknown_built_in_name_is_an_error() {
        let err = resolve_dictation_prompt("nonexistent").unwrap_err();
        assert!(format!("{err}").contains("nonexistent"));
    }

    #[test]
    fn strip_fence_removes_markdown_language_tag_wrapper() {
        // The exact shape Dustin saw in the screenshot.
        let raw = "```markdown\n- Rename the History tab to Dictations.\n- Decide whether to keep /history as a redirect.\n```";
        let out = strip_markdown_code_fence(raw);
        assert_eq!(
            out,
            "- Rename the History tab to Dictations.\n- Decide whether to keep /history as a redirect."
        );
    }

    #[test]
    fn strip_fence_removes_bare_triple_backtick_wrapper() {
        let raw = "```\nHello world.\n```";
        assert_eq!(strip_markdown_code_fence(raw), "Hello world.");
    }

    #[test]
    fn strip_fence_is_a_noop_when_no_outer_fence() {
        let raw = "- One\n- Two\n- Three";
        assert_eq!(strip_markdown_code_fence(raw), raw);
    }

    #[test]
    fn strip_fence_preserves_inner_code_blocks() {
        // A summary that legitimately contains a code snippet should
        // keep the inner fence — only the OUTER wrapper goes.
        let raw =
            "```markdown\nSee the snippet:\n\n```rust\nfn main() {}\n```\n\nThat's the gist.\n```";
        let out = strip_markdown_code_fence(raw);
        assert!(out.contains("```rust"));
        assert!(out.contains("fn main() {}"));
        assert!(out.starts_with("See the snippet:"));
        assert!(out.trim_end().ends_with("That's the gist."));
    }

    #[test]
    fn strip_fence_is_idempotent() {
        let raw = "```markdown\n- A\n- B\n```";
        let once = strip_markdown_code_fence(raw);
        let twice = strip_markdown_code_fence(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn strip_fence_handles_outer_whitespace() {
        let raw = "  \n```markdown\nfoo\n```\n  ";
        assert_eq!(strip_markdown_code_fence(raw), "foo");
    }

    #[test]
    fn strip_fence_leaves_partial_fence_alone() {
        // Opener but no closer — could be a streaming truncation.
        // Don't risk corrupting real content.
        let raw = "```markdown\n- A\n- B";
        assert_eq!(strip_markdown_code_fence(raw), raw.trim());
    }

    #[test]
    fn all_three_built_ins_resolve_distinct_bodies() {
        // Cheap regression guard against an accidental copy-paste
        // where two prompts point at the same `include_str!`.
        let s = resolve_dictation_prompt("summary").unwrap();
        let a = resolve_dictation_prompt("action_items").unwrap();
        let c = resolve_dictation_prompt("cleaner_punctuation").unwrap();
        assert_ne!(s, a);
        assert_ne!(a, c);
        assert_ne!(s, c);
    }
}
