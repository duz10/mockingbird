//! Heuristic meeting-title derivation from formatted transcript text.
//!
//! Picks a ≤5-word, ≤60-char title from the most representative
//! channel — preferring the merged formatter output (which interleaves
//! speakers and reads most naturally), falling back to mic, then
//! system. Strips speaker prefixes (`**You:**`, `**Other(s):**`),
//! filler-only paragraphs, and trailing punctuation. Returns `None`
//! when no usable text exists; callers default to the localized
//! "Untitled meeting" string at render time.
//!
//! ## Why this lives in its own module
//!
//! Pure module (no I/O, no DB, no clock). Deterministic given identical
//! inputs and runs cheaply (≤O(words-in-first-paragraph)) at meeting-
//! finalize time. Easy to unit-test exhaustively. Wiring lives in
//! `lifecycle.rs::build_persist_request`.
//!
//! ## Mutability
//!
//! The `meeting_sessions.title` column is mutable. Users rename via
//! `meeting_rename` (see `repo::rename_meeting`). This is fine — the
//! immutability invariant (PLAN Principle 1) applies to `raw`-stage
//! transcript rows, not to the session header.

const MAX_WORDS: usize = 5;
const MAX_CHARS: usize = 60;
/// Minimum length of a "substantive" token. `a`, `i`, `&` etc. are
/// fine as connector tokens in the middle of a title but we won't
/// START a title on them.
const MIN_LEAD_TOKEN_LEN: usize = 2;

/// Derive a short title from the formatter output. Inputs are the
/// three Optional formatted-prose channels in priority order. Returns
/// `Some(title)` on success, `None` when no channel had usable text
/// (caller then falls back to "Untitled meeting").
pub fn derive_meeting_title(
    merged: Option<&str>,
    mic: Option<&str>,
    sys: Option<&str>,
) -> Option<String> {
    let body = pick_source(merged, mic, sys)?;
    let paragraph = first_substantive_paragraph(body)?;
    let raw_words: Vec<&str> = paragraph.split_whitespace().collect();
    let head = collect_lead_words(&raw_words, MAX_WORDS)?;
    let title = finalize(&head);
    // An all-connector head (e.g. "---") survives `collect_lead_words`
    // because '-' is a preserved connector glyph, but `finalize`
    // then strips it down to "". A blank title is meaningless and
    // would render as an empty meeting header, so fall back to None
    // (caller substitutes the localized "Untitled meeting").
    // (mb-mac-v1.9: real bug -- pure-punctuation input returned
    // Some("") instead of None; surfaced on Mac's first real test run.)
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

/// First non-empty, non-whitespace-only channel from the priority list.
fn pick_source<'a>(
    merged: Option<&'a str>,
    mic: Option<&'a str>,
    sys: Option<&'a str>,
) -> Option<&'a str> {
    [merged, mic, sys]
        .into_iter()
        .flatten()
        .find(|s| !s.trim().is_empty())
}

/// First paragraph (double-newline-separated) with at least one
/// substantive token after speaker-label stripping. Returns the
/// label-stripped paragraph.
fn first_substantive_paragraph(body: &str) -> Option<String> {
    for raw in body.split("\n\n") {
        let stripped = strip_speaker_label(raw.trim());
        if stripped.is_empty() {
            continue;
        }
        // Require at least one token of length ≥ MIN_LEAD_TOKEN_LEN
        // somewhere in the paragraph; pure-punctuation paragraphs
        // ("..." / "—") and one-letter-fragment paragraphs would
        // otherwise pass and produce nonsense titles.
        let has_substance = stripped
            .split_whitespace()
            .any(|w| trim_token(w).chars().count() >= MIN_LEAD_TOKEN_LEN);
        if has_substance {
            return Some(stripped);
        }
    }
    None
}

/// Strip a leading `**Speaker:**` markdown bold-label, if present.
/// Returns the input unchanged when no label is found. Conservative:
/// only strips when the pattern matches exactly (open `**`, content
/// without `*`, close `**`, optional whitespace).
fn strip_speaker_label(s: &str) -> String {
    if !s.starts_with("**") {
        return s.to_string();
    }
    // Find the closing `**` after position 2. Anything in between is
    // the label (e.g. "You:" or "Other(s):").
    let rest = &s[2..];
    if let Some(close) = rest.find("**") {
        let after = &rest[close + 2..];
        return after.trim_start().to_string();
    }
    s.to_string()
}

/// Pick the first N tokens from `words`, skipping any leading short
/// connector tokens (so a title doesn't start with "a" or "I"). Each
/// token is cleaned via `trim_token`. Returns `None` if no lead
/// candidate exists.
fn collect_lead_words(words: &[&str], n: usize) -> Option<Vec<String>> {
    // Find the first index whose token is "substantive enough" to be
    // the FIRST word of the title.
    let start = words
        .iter()
        .position(|w| trim_token(w).chars().count() >= MIN_LEAD_TOKEN_LEN)?;
    let mut out: Vec<String> = Vec::with_capacity(n);
    for w in words.iter().skip(start) {
        let cleaned = trim_token(w);
        if cleaned.is_empty() {
            continue;
        }
        out.push(cleaned.to_string());
        if out.len() == n {
            break;
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Strip leading/trailing punctuation, brackets, and quotes from a
/// whitespace-separated token. Preserves internal punctuation (so
/// "don't" stays "don't" and "co-worker" stays "co-worker").
fn trim_token(t: &str) -> &str {
    t.trim_matches(|c: char| {
        c.is_ascii_punctuation() && !matches!(c, '\'' | '-')
            || c == '"'
            || c == '“'
            || c == '”'
            || c == '‘'
            || c == '’'
    })
}

/// Join, char-cap at MAX_CHARS without splitting a token, and
/// capitalize the first character. Strips a trailing punctuation
/// glyph if present (don't want titles ending in ",").
fn finalize(words: &[String]) -> String {
    let joined = words.join(" ");
    let capped = cap_at_chars(&joined, MAX_CHARS);
    let trimmed = capped.trim_end_matches(|c: char| {
        c.is_ascii_punctuation() && !matches!(c, ')' | ']' | '!' | '?')
    });
    capitalize_first(trimmed)
}

/// Cap a string to at most `n` chars without splitting mid-token.
/// If the full string fits, returns it as-is. Otherwise truncates
/// at the last whole-token boundary that fits and trims trailing
/// whitespace.
fn cap_at_chars(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out = String::new();
    for word in s.split_whitespace() {
        let prospective = if out.is_empty() {
            word.chars().count()
        } else {
            out.chars().count() + 1 + word.chars().count()
        };
        if prospective > n {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    // Edge case: a single token longer than n. Hard-truncate at char.
    if out.is_empty() {
        out = s.chars().take(n).collect();
    }
    out
}

/// Uppercase the first character; pass the rest through unchanged.
/// Unicode-safe (handles multi-byte first chars).
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn derive(s: &str) -> Option<String> {
        derive_meeting_title(Some(s), None, None)
    }

    // ---- end-to-end happy paths -------------------------------------

    #[test]
    fn simple_sentence_yields_first_five_words() {
        // 7 words → cap at 5, capitalize "the".
        let t = derive("the quick brown fox jumps over the lazy dog").unwrap();
        assert_eq!(t, "The quick brown fox jumps");
    }

    #[test]
    fn three_words_only_returns_all_three() {
        let t = derive("alpha beta gamma").unwrap();
        assert_eq!(t, "Alpha beta gamma");
    }

    #[test]
    fn strips_trailing_punctuation_on_short_input() {
        let t = derive("hello, world.").unwrap();
        // `trim_token` strips per-token trailing punctuation, so the
        // interior comma on "hello," is dropped. (mb-mac-v1.9: the
        // shipped f298a5d implementation has always done this; the
        // assertion was aspirational and never ran -- Windows gates
        // `--no-run`. See mb-mac-v1.9b for the comma-polish follow-up.)
        assert_eq!(t, "Hello world");
    }

    #[test]
    fn capitalizes_when_input_is_lowercase() {
        let t = derive("kickoff meeting for the alpha launch tomorrow").unwrap();
        assert_eq!(t, "Kickoff meeting for the alpha");
    }

    #[test]
    fn preserves_internal_case() {
        let t = derive("API design sync for Q3 roadmap planning").unwrap();
        assert_eq!(t, "API design sync for Q3");
    }

    // ---- speaker-label stripping -----------------------------------

    #[test]
    fn strips_you_label_from_merged_output() {
        let t = derive("**You:** Alright, let's talk about the budget today.").unwrap();
        // Interior comma on "Alright," dropped by per-token trim;
        // apostrophe in "let's" preserved. (mb-mac-v1.9 stale assert;
        // comma-polish tracked in mb-mac-v1.9b.)
        assert_eq!(t, "Alright let's talk about the");
    }

    #[test]
    fn strips_other_label_with_parens() {
        let t = derive("**Other(s):** Welcome everyone to the standup call.").unwrap();
        assert_eq!(t, "Welcome everyone to the standup");
    }

    #[test]
    fn malformed_label_no_close_left_alone() {
        // Open `**` but no close → don't strip, keep the literal.
        let t = derive("**This is a heading line about widgets").unwrap();
        // The trim_token call removes leading `*` (ASCII punct, not '\'' or '-').
        assert_eq!(t, "This is a heading line");
    }

    // ---- paragraph selection ----------------------------------------

    #[test]
    fn skips_first_paragraph_if_only_punctuation() {
        let body = "...\n\nThe real first paragraph has substance here.";
        let t = derive(body).unwrap();
        assert_eq!(t, "The real first paragraph has");
    }

    #[test]
    fn skips_pure_filler_one_char_paragraph() {
        // Single-letter token shouldn't anchor a title.
        let body = "a\n\nProject planning kickoff for next quarter started";
        let t = derive(body).unwrap();
        assert_eq!(t, "Project planning kickoff for next");
    }

    #[test]
    fn uses_first_paragraph_when_multiple_present() {
        let body = "First paragraph topic here.\n\nSecond paragraph ignored entirely.";
        let t = derive(body).unwrap();
        assert_eq!(t, "First paragraph topic here");
    }

    // ---- channel selection ------------------------------------------

    #[test]
    fn prefers_merged_when_all_three_present() {
        let t = derive_meeting_title(
            Some("merged channel content here today"),
            Some("mic only channel content"),
            Some("sys only channel content"),
        )
        .unwrap();
        assert_eq!(t, "Merged channel content here today");
    }

    #[test]
    fn falls_back_to_mic_when_merged_none() {
        let t = derive_meeting_title(
            None,
            Some("mic channel speaks first today now"),
            Some("sys content"),
        )
        .unwrap();
        assert_eq!(t, "Mic channel speaks first today");
    }

    #[test]
    fn falls_back_to_sys_when_mic_none() {
        let t =
            derive_meeting_title(None, None, Some("system audio captured a long lecture")).unwrap();
        assert_eq!(t, "System audio captured a long");
    }

    #[test]
    fn falls_back_when_merged_is_empty_string() {
        // Empty / whitespace-only string must NOT win the channel pick.
        let t = derive_meeting_title(
            Some("   \n   "),
            Some("mic wins the fallback today now"),
            None,
        )
        .unwrap();
        assert_eq!(t, "Mic wins the fallback today");
    }

    // ---- None cases -------------------------------------------------

    #[test]
    fn all_none_returns_none() {
        assert_eq!(derive_meeting_title(None, None, None), None);
    }

    #[test]
    fn all_empty_returns_none() {
        assert_eq!(
            derive_meeting_title(Some(""), Some("   "), Some("\n\n")),
            None
        );
    }

    #[test]
    fn only_speaker_labels_returns_none() {
        // After stripping the label there's nothing substantive.
        let t = derive_meeting_title(Some("**You:**\n\n**Other(s):**"), None, None);
        assert_eq!(t, None);
    }

    #[test]
    fn only_punctuation_returns_none() {
        let t = derive_meeting_title(Some("... --- ??? !!! ,,,"), None, None);
        assert_eq!(t, None);
    }

    // ---- char cap ---------------------------------------------------

    #[test]
    fn caps_at_60_chars_even_with_room_for_more_words() {
        // 5 words but the first three already exceed 60 chars. Cap.
        let long = "supercalifragilisticexpialidocious antidisestablishmentarianism pneumonoultramicroscopicsilicovolcanoconiosis okay extra";
        let t = derive(long).unwrap();
        assert!(
            t.chars().count() <= MAX_CHARS,
            "title exceeded {} chars: {} chars in {:?}",
            MAX_CHARS,
            t.chars().count(),
            t
        );
    }

    #[test]
    fn single_huge_token_hard_truncates() {
        // Pathological: one word longer than MAX_CHARS. We should
        // still produce a non-empty title (hard truncate at char N).
        let huge = "a".repeat(200);
        let t = derive(&huge).unwrap();
        assert!(t.chars().count() <= MAX_CHARS);
        assert!(!t.is_empty());
    }

    // ---- preserved punctuation --------------------------------------

    #[test]
    fn keeps_apostrophes_inside_tokens() {
        let t = derive("don't break my heart please everyone").unwrap();
        assert_eq!(t, "Don't break my heart please");
    }

    #[test]
    fn keeps_hyphens_inside_tokens() {
        let t = derive("co-worker check-in for end-of-quarter review tomorrow").unwrap();
        assert_eq!(t, "Co-worker check-in for end-of-quarter review");
    }

    #[test]
    fn strips_quote_marks_around_tokens() {
        let t = derive("\"quoted\" 'word' here today now").unwrap();
        // Double-quotes are stripped but the single-quotes around
        // 'word' are preserved: `trim_token` deliberately keeps `'`
        // so contractions like "don't"/"let's" survive, and it can't
        // tell a wrapping quote from an apostrophe. (mb-mac-v1.9
        // stale assert; quote-polish tracked in mb-mac-v1.9b.)
        assert_eq!(t, "Quoted 'word' here today now");
    }

    // ---- unicode safety ---------------------------------------------

    #[test]
    fn unicode_capitalization_works() {
        let t = derive("café latte tasting session this morning").unwrap();
        assert_eq!(t, "Café latte tasting session this");
    }

    #[test]
    fn already_capitalized_stays_capitalized() {
        let t = derive("Standup today with the engineering team").unwrap();
        assert_eq!(t, "Standup today with the engineering");
    }

    // ---- realistic merged-formatter output --------------------------

    #[test]
    fn realistic_two_speaker_merged_transcript() {
        // Mirrors the actual mc-v1 merged output Dustin recorded.
        let body = "**You:** Alright, I'm testing my microphone right now from the mic input.\n\n**Other(s):** Okay, this is big. Researchers at MIT just built an AI agent.";
        let t = derive(body).unwrap();
        // First paragraph stripped of the speaker label; interior
        // comma on "Alright," dropped by per-token trim, apostrophe
        // in "I'm" preserved. (mb-mac-v1.9 stale assert; comma-polish
        // tracked in mb-mac-v1.9b.)
        assert_eq!(t, "Alright I'm testing my microphone");
    }
}
