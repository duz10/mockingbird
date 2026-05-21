//! Static filler-word set for the deterministic formatter.
//!
//! Compile-time `phf::Set` — zero allocation, perfect hash lookup,
//! ASCII lowercase keys. The formatter normalises its input to ASCII
//! lowercase before lookup; non-ASCII tokens (CJK, accented Latin,
//! emoji) are never fillers by definition.
//!
//! Multi-word fillers ("you know", "i mean") live in [`FILLER_PHRASES`]
//! and are matched greedy-longest by the formatter sliding a window
//! over the token stream. Phrases are stored space-separated; the
//! formatter does the join.
//!
//! Wave 1 ships the initial set; Wave 2's formatter test suite is the
//! contract that pins it. Adding a filler later is an additive change
//! (no existing test changes shape).

use phf::{phf_set, Set};

/// Single-word fillers. Lowercase.
///
/// Sources: common dictation/meeting filler lists (Brown corpus
/// disfluencies + casual-speech research). Conservative — words like
/// "okay" and "right" are NOT included because they often carry real
/// semantic weight in meetings ("okay so the deadline is…").
pub static FILLERS: Set<&'static str> = phf_set! {
    "um", "uh", "umm", "uhh", "ummm", "uhhh",
    "er", "erm", "ah", "ahh", "eh",
    "like",   // contested; included because the formatter strips on
              // exact match and "like" as a verb is rare relative to
              // its filler-use in meetings. Power users can opt out
              // via `MeetingFillerStripEnabled = false`.
    "basically", // added in ADR 0032 / MC v1.1: called out in the
                 // original plan filler list (mb-tn5) but missed
                 // during initial set authoring. Same opt-out as
                 // "like" applies.
    "hmm", "mhm", "mm",
};

/// Multi-word filler phrases. Lowercase, space-separated.
///
/// The formatter slides a 3-token window over its token stream and
/// looks up the longest prefix that appears in this set. Wave 1 ships
/// the initial set; Wave 2 ships the lookup helper.
pub static FILLER_PHRASES: Set<&'static str> = phf_set! {
    "you know",
    "i mean",
    "sort of",
    "kind of",
    "you see",
};

/// Maximum number of tokens in any phrase in [`FILLER_PHRASES`].
/// Used by the formatter as the sliding-window cap (avoids quadratic
/// behavior on long token streams). Bump when adding a 4+-token
/// phrase.
pub const MAX_PHRASE_TOKENS: usize = 3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn um_uh_are_fillers() {
        assert!(FILLERS.contains("um"));
        assert!(FILLERS.contains("uh"));
    }

    #[test]
    fn basically_is_a_filler() {
        // mb-tn5 / ADR 0032: "basically" was named in the original
        // Phase MC plan filler list but missed during the Wave 1
        // implementation. Pin it here so it never regresses out.
        assert!(FILLERS.contains("basically"));
    }

    #[test]
    fn okay_is_not_a_filler() {
        // Documented design choice — "okay" carries semantic weight in
        // meetings ("okay so the deadline is…"). Stripping it would
        // change meaning. If you want it gone, opt out via
        // MeetingFillerStripEnabled.
        assert!(!FILLERS.contains("okay"));
        assert!(!FILLERS.contains("right"));
        assert!(!FILLERS.contains("so"));
    }

    #[test]
    fn filler_phrases_include_you_know() {
        assert!(FILLER_PHRASES.contains("you know"));
        assert!(FILLER_PHRASES.contains("i mean"));
    }

    #[test]
    fn all_filler_phrases_within_window_cap() {
        for phrase in FILLER_PHRASES.iter() {
            let tokens = phrase.split_whitespace().count();
            assert!(
                tokens <= MAX_PHRASE_TOKENS,
                "phrase {phrase:?} exceeds MAX_PHRASE_TOKENS={MAX_PHRASE_TOKENS}"
            );
        }
    }

    #[test]
    fn filler_set_keys_are_lowercase() {
        for k in FILLERS.iter() {
            assert_eq!(
                *k,
                k.to_ascii_lowercase(),
                "non-lowercase filler key: {k:?}"
            );
        }
        for k in FILLER_PHRASES.iter() {
            assert_eq!(
                *k,
                k.to_ascii_lowercase(),
                "non-lowercase phrase key: {k:?}"
            );
        }
    }
}
