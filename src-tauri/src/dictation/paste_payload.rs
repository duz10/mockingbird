//! Trailing-space policy for dictation paste.
//!
//! Per user request (post-phase-MC), dictation injection appends a
//! single trailing space to the paste payload so consecutive
//! dictations flow without the user having to type a leading space
//! at the start of the next utterance.
//!
//! ## Invariants
//!
//! - The trailing space lives **only in the paste payload**. The
//!   persisted transcript text (`cleaned_text` in dictation.rs) is
//!   unchanged. We do NOT want a phantom trailing space to leak
//!   into copy-from-history, search indices, learning loops, etc.
//!
//! - If `cleaned_text` is already empty or already ends in any
//!   ASCII or unicode whitespace, the helper is a no-op (returns
//!   the slice borrowed). This avoids stacking spaces when the
//!   cleaner already terminated the text (e.g. with `\n`), and
//!   avoids producing a single " " from a completely empty STT
//!   result.
//!
//! ## Why a helper module
//!
//! `dictation.rs` is already ~1100 lines (over the 600 budget;
//! pre-existing). Extracting this as a tiny pure module keeps the
//! new logic isolated and unit-testable without further bloating
//! the orchestrator file.
//!
//! ## Cross-platform
//!
//! Pure-string transformation. No `#[cfg(target_os)]` needed.

use std::borrow::Cow;

/// Produce the string that should actually be handed to `Injector::inject`.
///
/// - If `cleaned` is empty → returns `cleaned` borrowed unchanged.
///   (Empty STT results are a separate concern; this helper is not
///   in the empty-handling business.)
/// - If the final char is whitespace (per `char::is_whitespace`,
///   which covers ASCII space/tab/newline AND unicode separators)
///   → returns `cleaned` borrowed unchanged.
/// - Otherwise → returns an owned `String` of `cleaned` with a
///   single trailing ASCII space appended.
pub fn paste_payload(cleaned: &str) -> Cow<'_, str> {
    if cleaned.is_empty() {
        return Cow::Borrowed(cleaned);
    }
    // `chars().next_back()` is O(1) for valid UTF-8 strings and gives
    // us the last *char* (not byte), so a multi-byte trailing
    // codepoint like "…" is classified correctly.
    let last = cleaned.chars().next_back();
    if last.map(|c| c.is_whitespace()).unwrap_or(false) {
        return Cow::Borrowed(cleaned);
    }
    let mut owned = String::with_capacity(cleaned.len() + 1);
    owned.push_str(cleaned);
    owned.push(' ');
    Cow::Owned(owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_borrowed_empty() {
        let got = paste_payload("");
        assert_eq!(got, "");
        assert!(matches!(got, Cow::Borrowed(_)));
    }

    #[test]
    fn appends_space_to_plain_word() {
        let got = paste_payload("hello");
        assert_eq!(got, "hello ");
        assert!(matches!(got, Cow::Owned(_)));
    }

    #[test]
    fn appends_space_after_terminal_punctuation() {
        assert_eq!(paste_payload("hello."), "hello. ");
        assert_eq!(paste_payload("hello!"), "hello! ");
        assert_eq!(paste_payload("hello?"), "hello? ");
        assert_eq!(paste_payload("hello,"), "hello, ");
    }

    #[test]
    fn noop_when_already_trailing_space() {
        // Idempotent: running the helper on already-spaced text
        // doesn't keep stacking.
        let got = paste_payload("hello ");
        assert_eq!(got, "hello ");
        assert!(matches!(got, Cow::Borrowed(_)));
    }

    #[test]
    fn noop_when_trailing_tab_or_newline() {
        assert_eq!(paste_payload("hello\t"), "hello\t");
        assert_eq!(paste_payload("hello\n"), "hello\n");
        assert_eq!(paste_payload("hello\r\n"), "hello\r\n");
    }

    #[test]
    fn noop_when_trailing_unicode_whitespace() {
        // U+00A0 NO-BREAK SPACE counts as whitespace.
        let nbsp = "hello\u{00A0}";
        let got = paste_payload(nbsp);
        assert_eq!(got, nbsp);
        assert!(matches!(got, Cow::Borrowed(_)));
    }

    #[test]
    fn appends_after_multi_byte_codepoint() {
        // Trailing ellipsis is NOT whitespace → append.
        let got = paste_payload("thinking…");
        assert_eq!(got, "thinking… ");
        // Trailing emoji likewise.
        let got = paste_payload("yay 🎉");
        assert_eq!(got, "yay 🎉 ");
    }

    #[test]
    fn whitespace_only_input_is_noop() {
        // A weird edge case but worth pinning: if the cleaner
        // somehow returns just whitespace, we still don't add more.
        assert_eq!(paste_payload(" "), " ");
        assert_eq!(paste_payload("   "), "   ");
        assert_eq!(paste_payload("\n"), "\n");
    }

    #[test]
    fn preserves_interior_whitespace() {
        // Only the *trailing* char matters; interior whitespace is
        // left alone.
        assert_eq!(paste_payload("hello world"), "hello world ");
        assert_eq!(paste_payload("two  spaces"), "two  spaces ");
    }

    #[test]
    fn long_sentence_round_trip() {
        let src = "This is a much longer dictation result with multiple sentences. \
                   It ends in a period, so a space should be appended.";
        let got = paste_payload(src);
        assert!(got.ends_with(". "));
        assert!(!got.ends_with(".  "), "must not double-space");
    }

    #[test]
    fn idempotent_when_applied_twice() {
        // paste_payload(paste_payload(x)) == paste_payload(x).
        let once = paste_payload("hello");
        let twice = paste_payload(&once);
        assert_eq!(once, twice);
    }
}
