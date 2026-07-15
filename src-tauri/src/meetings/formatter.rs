//! Deterministic meeting-transcript formatter (Section MC.3).
//!
//! **Pure Rust. No RNG. No system clock. No global state. Same input
//! → same output, byte-for-byte.** This is the canonical pass for
//! meeting transcripts — the optional [`super::llm_pass`] runs AFTER
//! persist and its output is explicitly not written back to the DB.
//!
//! The algorithm walks a token stream once per segment, drops fillers
//! (single-token and multi-token phrases, greedy-longest), optionally
//! collapses exact repeats, joins segments with either a single space
//! or `"\n\n"` based on the inter-segment timestamp gap, and runs a
//! single forward capitalization pass over the result. It's a
//! fixpoint (`format(format(x)) == format(x)` for the wrap-as-single-
//! segment encoding) — see the `proptest` invariant at the bottom of
//! this file.
//!
//! The `mc-formatter-deterministic` judge (Wave 6) feeds off the test
//! suite below plus that proptest.

use crate::error::AppResult;

use super::filler_words::{FILLER_PHRASES, MAX_PHRASE_TOKENS};
use super::long_form_stt::TimedSegment;

/// Formatter options. Defaults reflect Section MC.3's documented
/// defaults; the runtime sources overrides from `SettingKey::*` keys
/// (`MeetingFillerStripEnabled`, `MeetingParagraphGapMs`, …).
#[derive(Debug, Clone)]
pub struct FormatOpts {
    /// Gap in ms between segments that triggers a paragraph break.
    pub paragraph_gap_ms: u32,
    /// Drop tokens matching the filler set (greedy-longest for
    /// multi-word phrases like "you know").
    pub strip_fillers: bool,
    /// Collapse exact-match consecutive tokens after lowercase
    /// normalization ("the the" → "the"; preserves first occurrence's
    /// case).
    pub strip_repeats: bool,
    /// Uppercase the first non-whitespace char of each paragraph.
    pub capitalize_paragraph_starts: bool,
    /// Uppercase the first non-whitespace char after [.!?] + ws.
    pub capitalize_sentence_starts: bool,
    /// Trim leading/trailing whitespace of the final string.
    pub strip_leading_trailing_ws: bool,
}

impl Default for FormatOpts {
    fn default() -> Self {
        Self {
            paragraph_gap_ms: 2_000,
            strip_fillers: true,
            strip_repeats: true,
            capitalize_paragraph_starts: true,
            capitalize_sentence_starts: true,
            strip_leading_trailing_ws: true,
        }
    }
}

/// Format a segment stream into prose per Section MC.3.
///
/// `filler_set` is taken by reference so callers pass
/// [`super::filler_words::FILLERS`] without copying.
///
/// Returns `Ok(String)` always today; the `AppResult` return type is
/// preserved so a future invariant violation (e.g. caller hands in
/// non-monotonic segments) can surface as `AppError::Formatter`
/// without a struct churn.
pub fn format(
    segments: &[TimedSegment],
    filler_set: &phf::Set<&'static str>,
    opts: &FormatOpts,
) -> AppResult<String> {
    if segments.is_empty() {
        return Ok(String::new());
    }

    // Step 1 — GROUP raw tokens into paragraphs. A paragraph is a run of
    // segments whose inter-segment gap stays below `paragraph_gap_ms`
    // (measured off the last segment that CONTRIBUTED words); a gap at or
    // above the threshold starts a fresh paragraph. Segment text is ALSO
    // split on any embedded blank line ("\n\n") — which only appears when
    // already-formatted prose is fed back through `format` — so a prior
    // pass's paragraph breaks are preserved rather than flattened by
    // `split_whitespace`. That preservation is one half of what makes
    // `format` idempotent; `strip_to_fixpoint` (Step 2) is the other. (mb-7k6)
    let mut paragraphs: Vec<Vec<&str>> = Vec::new();
    let mut prev_t1_ms: Option<u32> = None;
    for seg in segments {
        let mut contributed = false;
        for block in split_into_paragraphs(&seg.text) {
            let tokens: Vec<&str> = block.split_whitespace().collect();
            if tokens.is_empty() {
                continue;
            }
            let start_new = if contributed {
                // A second+ block within one segment is an explicit
                // paragraph break carried in the (re-fed) segment text.
                true
            } else if let Some(pt1) = prev_t1_ms {
                seg.t0_ms.saturating_sub(pt1) >= opts.paragraph_gap_ms
            } else {
                true // the very first paragraph
            };
            if start_new || paragraphs.is_empty() {
                paragraphs.push(tokens);
            } else if let Some(last) = paragraphs.last_mut() {
                last.extend(tokens);
            }
            contributed = true;
        }
        if contributed {
            prev_t1_ms = Some(seg.t1_ms);
        }
    }

    // Step 2 — strip each paragraph's MERGED token stream to a fixpoint
    // (phrase + filler + repeat passes iterated until nothing more is
    // removed). Stripping the merged stream (not per-segment) also cleans
    // fillers/repeats that only appear once two segments are glued (e.g.
    // "you" + "know" -> the filler phrase "you know"), which is both
    // strictly better and required for idempotency. Empty (all-filler)
    // paragraphs drop out.
    let rendered: Vec<String> = paragraphs
        .iter()
        .map(|toks| strip_to_fixpoint(toks, filler_set, opts).join(" "))
        .filter(|p| !p.is_empty())
        .collect();

    // Step 3 — join paragraphs with "\n\n", then capitalize + trim.
    let joined = rendered.join("\n\n");
    let cased = capitalize(&joined, opts);
    let final_out = if opts.strip_leading_trailing_ws {
        cased.trim().to_string()
    } else {
        cased
    };

    Ok(final_out)
}

/// Split `text` into paragraph slices on any whitespace run containing a
/// blank line (>= 2 newlines). Single newlines and spaces stay *inside*
/// a paragraph (the tokenizer collapses them). Raw STT segment text never
/// carries "\n\n", so on the production single-pass path this returns
/// exactly one slice; only already-formatted prose fed back through
/// `format` exercises the multi-slice branch. Empty / whitespace-only
/// input yields no slices. (mb-7k6)
fn split_into_paragraphs(text: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    let mut para_start: Option<usize> = None;
    let mut run_start = 0usize;
    let mut run_newlines = 0usize;
    let mut in_ws = false;
    for (idx, c) in text.char_indices() {
        if c.is_whitespace() {
            if !in_ws {
                in_ws = true;
                run_start = idx;
                run_newlines = 0;
            }
            if c == '\n' {
                run_newlines += 1;
            }
        } else {
            if in_ws && run_newlines >= 2 {
                if let Some(start) = para_start.take() {
                    out.push(&text[start..run_start]);
                }
            }
            in_ws = false;
            if para_start.is_none() {
                para_start = Some(idx);
            }
        }
    }
    if let Some(start) = para_start {
        out.push(&text[start..]);
    }
    out
}

/// Apply the phrase/filler pass and (when enabled) the repeat pass
/// repeatedly until the token list stops shrinking. A SINGLE pass is not
/// a fixpoint: removing a middle filler can bring two survivors together
/// into a *new* filler phrase or repeat (e.g. `you <um> know` -> `you
/// know`). Iterating to a fixpoint is what lets `format(format(x))`
/// equal `format(x)` — re-running finds nothing left to remove. Each
/// pass only deletes tokens, so length is monotonically non-increasing;
/// equal length means no change, so this terminates in <= tokens.len()
/// iterations. (mb-7k6)
fn strip_to_fixpoint(
    tokens: &[&str],
    filler_set: &phf::Set<&'static str>,
    opts: &FormatOpts,
) -> Vec<String> {
    let mut current: Vec<String> = tokens.iter().map(|t| t.to_string()).collect();
    loop {
        let refs: Vec<&str> = current.iter().map(String::as_str).collect();
        let mut next = strip_phrases_and_fillers(&refs, filler_set, opts);
        if opts.strip_repeats {
            next = strip_repeats(&next);
        }
        if next.len() == current.len() {
            return next;
        }
        current = next;
    }
}

// ---------- internal helpers (pub(super) for fine-grained tests) ----------

/// Lowercase ASCII + strip leading/trailing ASCII punctuation. Used
/// ONLY for filler-set / repeat-set lookups; the original token is
/// preserved in the output stream so Whisper's punctuation survives.
fn normalize_for_lookup(token: &str) -> String {
    let trimmed = token.trim_matches(|c: char| ".,!?;:\"'()[]{}".contains(c));
    trimmed.to_ascii_lowercase()
}

/// Phrase + filler pass. Slides a window of length
/// `min(MAX_PHRASE_TOKENS, remaining)` starting at the longest length
/// and stepping down; drops the entire window on a phrase hit.
/// Falls back to single-token filler check on no-hit (only when
/// `opts.strip_fillers`).
fn strip_phrases_and_fillers(
    tokens: &[&str],
    filler_set: &phf::Set<&'static str>,
    opts: &FormatOpts,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        // Try phrases from longest (capped by MAX_PHRASE_TOKENS) down
        // to length 2. Single-token "phrases" are handled by the
        // filler-set check below; we never put 1-token entries in
        // FILLER_PHRASES.
        let max_len = MAX_PHRASE_TOKENS.min(tokens.len() - i);
        let mut consumed = 0usize;
        for len in (2..=max_len).rev() {
            let phrase: String = (0..len)
                .map(|j| normalize_for_lookup(tokens[i + j]))
                .collect::<Vec<_>>()
                .join(" ");
            if FILLER_PHRASES.contains(phrase.as_str()) {
                consumed = len;
                break;
            }
        }
        if consumed > 0 {
            i += consumed;
            continue;
        }

        // Single-token filler check.
        if opts.strip_fillers {
            let norm = normalize_for_lookup(tokens[i]);
            if filler_set.contains(norm.as_str()) {
                i += 1;
                continue;
            }
        }

        out.push(tokens[i].to_string());
        i += 1;
    }
    out
}

/// Repeat-collapse pass. Two consecutive tokens whose
/// [`normalize_for_lookup`] values match are collapsed to the first
/// (preserves original case + punctuation).
fn strip_repeats(tokens: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(tokens.len());
    let mut prev_norm: Option<String> = None;
    for tok in tokens {
        let norm = normalize_for_lookup(tok);
        if prev_norm.as_deref() == Some(norm.as_str()) {
            // Skip; same as previous after normalization.
            continue;
        }
        out.push(tok.clone());
        prev_norm = Some(norm);
    }
    out
}

/// Single-forward-walk capitalization pass.
///
/// - Initial char (paragraph start) → uppercase, if
///   `capitalize_paragraph_starts`.
/// - First non-whitespace char after a whitespace run containing `\n`
///   → uppercase, if `capitalize_paragraph_starts`.
/// - First non-whitespace char after any `[.!?]` then whitespace →
///   uppercase, if `capitalize_sentence_starts`.
///
/// Non-alphabetic chars don't consume the flag — `"the cat"` would
/// still capitalize the `t` of `the` if a leading quote were inserted.
fn capitalize(s: &str, opts: &FormatOpts) -> String {
    if !opts.capitalize_paragraph_starts && !opts.capitalize_sentence_starts {
        return s.to_string();
    }

    let mut out = String::with_capacity(s.len());
    let mut next_uppercase = opts.capitalize_paragraph_starts;
    let mut sentence_end_pending = false;
    let mut newline_in_ws_run = false;

    for c in s.chars() {
        if c.is_whitespace() {
            if c == '\n' {
                newline_in_ws_run = true;
            }
            out.push(c);
            continue;
        }

        // Non-whitespace char: resolve any pending sentence/paragraph
        // flag that we accumulated during the preceding whitespace run.
        if sentence_end_pending && opts.capitalize_sentence_starts {
            next_uppercase = true;
        }
        if newline_in_ws_run && opts.capitalize_paragraph_starts {
            next_uppercase = true;
        }
        sentence_end_pending = false;
        newline_in_ws_run = false;

        if c.is_alphabetic() {
            if next_uppercase {
                for uc in c.to_uppercase() {
                    out.push(uc);
                }
            } else {
                out.push(c);
            }
            next_uppercase = false;
        } else {
            // Non-alpha, non-whitespace (punctuation, digits, symbols,
            // emoji). Preserve as-is; don't consume next_uppercase so
            // leading quotes / parens still let the first letter cap.
            out.push(c);
        }

        if matches!(c, '.' | '!' | '?') {
            sentence_end_pending = true;
        }
    }

    out
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meetings::filler_words::FILLERS;

    // ---------- helpers ----------

    fn seg(text: &str, t0: u32, t1: u32) -> TimedSegment {
        TimedSegment {
            text: text.to_string(),
            t0_ms: t0,
            t1_ms: t1,
        }
    }

    fn fmt(segments: &[TimedSegment]) -> String {
        format(segments, &FILLERS, &FormatOpts::default()).expect("format never errs in W2")
    }

    fn fmt_opts(segments: &[TimedSegment], opts: &FormatOpts) -> String {
        format(segments, &FILLERS, opts).expect("format never errs in W2")
    }

    // ---------- empty / trivial inputs ----------

    #[test]
    fn empty_input_is_empty() {
        let out = fmt(&[]);
        assert_eq!(out, "");
    }

    #[test]
    fn single_token_no_fillers() {
        let out = fmt(&[seg("hello world", 0, 1000)]);
        assert_eq!(out, "Hello world");
    }

    #[test]
    fn first_char_is_uppercased() {
        let out = fmt(&[seg("the cat", 0, 1000)]);
        assert_eq!(out, "The cat");
    }

    // ---------- filler stripping ----------

    #[test]
    fn strip_fillers_removes_um() {
        let out = fmt(&[seg("um the cat", 0, 1000)]);
        assert_eq!(out, "The cat");
    }

    #[test]
    fn strip_fillers_removes_multiple_um() {
        let out = fmt(&[seg("um uh um the cat", 0, 1000)]);
        assert_eq!(out, "The cat");
    }

    #[test]
    fn strip_repeats_collapses_the_the() {
        let out = fmt(&[seg("the the cat", 0, 1000)]);
        assert_eq!(out, "The cat");
    }

    #[test]
    fn combined_um_uh_um_the_the_cat() {
        let out = fmt(&[seg("um uh um um the the cat", 0, 1000)]);
        assert_eq!(out, "The cat");
    }

    // ---------- multi-word phrase stripping ----------

    #[test]
    fn phrase_you_know_at_start_dropped() {
        let out = fmt(&[seg("you know the cat", 0, 1000)]);
        assert_eq!(out, "The cat");
    }

    #[test]
    fn phrase_i_mean_mid_sentence_dropped() {
        let out = fmt(&[seg("the i mean cat", 0, 1000)]);
        assert_eq!(out, "The cat");
    }

    #[test]
    fn phrase_sort_of_dropped() {
        let out = fmt(&[seg("it was sort of weird", 0, 1000)]);
        assert_eq!(out, "It was weird");
    }

    #[test]
    fn greedy_longest_match_for_you_see() {
        let out = fmt(&[seg("you see this", 0, 1000)]);
        assert_eq!(out, "This");
    }

    #[test]
    fn filler_at_end_of_segment_no_double_space() {
        let out = fmt(&[seg("the cat um", 0, 1000)]);
        assert_eq!(out, "The cat");
    }

    // ---------- segment-boundary timing → paragraph break decisions ----------

    #[test]
    fn two_segments_short_gap_single_space() {
        let out = fmt(&[seg("the cat", 0, 1000), seg("ran fast", 1500, 2500)]);
        assert_eq!(out, "The cat ran fast");
    }

    #[test]
    fn two_segments_long_gap_paragraph() {
        let out = fmt(&[seg("the cat", 0, 1000), seg("dog barked", 3500, 4500)]);
        assert_eq!(out, "The cat\n\nDog barked");
    }

    #[test]
    fn gap_exactly_paragraph_gap_ms_is_paragraph() {
        // Gap = 3000 - 1000 = 2000 = default threshold (>= triggers).
        let out = fmt(&[seg("a", 0, 1000), seg("b", 3000, 4000)]);
        assert_eq!(out, "A\n\nB");
    }

    #[test]
    fn gap_one_less_than_paragraph_gap_is_space() {
        let out = fmt(&[seg("a", 0, 1000), seg("b", 2999, 4000)]);
        assert_eq!(out, "A b");
    }

    #[test]
    fn three_segments_mixed_gaps() {
        let out = fmt(&[
            seg("a", 0, 1000),
            seg("b", 1500, 2500),
            seg("c", 5000, 6000),
        ]);
        assert_eq!(out, "A b\n\nC");
    }

    // ---------- punctuation + sentence capitalization ----------

    #[test]
    fn whisper_punctuation_preserved() {
        let out = fmt(&[seg("hello, world. how are you?", 0, 1000)]);
        assert_eq!(out, "Hello, world. How are you?");
    }

    #[test]
    fn sentence_start_after_period_capitalized() {
        let out = fmt(&[seg("hello. world", 0, 1000)]);
        assert_eq!(out, "Hello. World");
    }

    #[test]
    fn sentence_start_after_question_capitalized() {
        let out = fmt(&[seg("ok? then go", 0, 1000)]);
        assert_eq!(out, "Ok? Then go");
    }

    // ---------- UTF-8 safety ----------

    #[test]
    fn utf8_cjk_passes_through_without_panic() {
        let out = fmt(&[seg("hello 你好 world", 0, 1000)]);
        assert_eq!(out, "Hello 你好 world");
    }

    #[test]
    fn utf8_emoji_passes_through() {
        let out = fmt(&[seg("hi 🎉 there", 0, 1000)]);
        assert_eq!(out, "Hi 🎉 there");
    }

    // ---------- whitespace + opts toggles ----------

    #[test]
    fn leading_trailing_ws_stripped() {
        let out = fmt(&[seg("  hello  ", 0, 1000)]);
        assert_eq!(out, "Hello");
    }

    #[test]
    fn strip_fillers_false_keeps_um() {
        let opts = FormatOpts {
            strip_fillers: false,
            ..FormatOpts::default()
        };
        let out = fmt_opts(&[seg("um the cat", 0, 1000)], &opts);
        assert_eq!(out, "Um the cat");
    }

    #[test]
    fn strip_repeats_false_keeps_the_the() {
        let opts = FormatOpts {
            strip_repeats: false,
            ..FormatOpts::default()
        };
        let out = fmt_opts(&[seg("the the cat", 0, 1000)], &opts);
        assert_eq!(out, "The the cat");
    }

    #[test]
    fn mid_word_case_preserved() {
        let out = fmt(&[seg("the iPhone is great", 0, 1000)]);
        assert_eq!(out, "The iPhone is great");
    }

    #[test]
    fn filler_only_input_emits_empty() {
        // Trim wins over capitalize-empty (no chars to walk).
        let out = fmt(&[seg("um uh um", 0, 1000)]);
        assert_eq!(out, "");
    }

    // ---------- Wave-1 smoke kept (defaults pin) ----------

    // ---------- idempotency (mb-7k6) ----------

    #[test]
    fn format_is_idempotent_on_paragraph_breaks() {
        // A >= threshold gap between two segments produces a real "\n\n"
        // paragraph break. Re-feeding that output as a single segment
        // must reproduce it byte-for-byte -- pre-fix the break collapsed
        // back to a single space.
        let once = fmt(&[
            seg("the cat sat", 0, 1_000),
            seg("the dog ran", 4_000, 5_000),
        ]);
        assert_eq!(once, "The cat sat\n\nThe dog ran");
        let twice = fmt(&[seg(&once, 0, 1_000)]);
        assert_eq!(twice, once);
    }

    #[test]
    fn cross_segment_filler_phrase_stripped_and_idempotent() {
        // "you" (seg 1) + "know" (seg 2) merge into the filler phrase
        // "you know" within one paragraph -> stripped. Pre-fix this
        // survived per-segment stripping and broke idempotency.
        let once = fmt(&[seg("you", 0, 1_000), seg("know this", 1_200, 2_000)]);
        assert_eq!(once, "This");
        assert_eq!(fmt(&[seg(&once, 0, 1_000)]), once);
    }

    #[test]
    fn default_opts_match_section_mc_3() {
        let opts = FormatOpts::default();
        assert_eq!(opts.paragraph_gap_ms, 2_000);
        assert!(opts.strip_fillers);
        assert!(opts.strip_repeats);
        assert!(opts.capitalize_paragraph_starts);
        assert!(opts.capitalize_sentence_starts);
        assert!(opts.strip_leading_trailing_ws);
    }

    // =================================================================
    // proptest invariants
    // =================================================================

    use proptest::prelude::*;

    /// Generate a token that's safe to test against — printable ASCII
    /// and a sprinkle of high-Unicode. Specifically avoids whitespace
    /// inside a token (whitespace is the segment tokenizer's
    /// delimiter; including it would make the test compare apples to
    /// orange-juice).
    fn token_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            "[a-zA-Z0-9.,!?;:'\"()\\[\\]{}]{1,8}".prop_map(|s| s),
            Just("um".to_string()),
            Just("uh".to_string()),
            Just("you".to_string()),
            Just("know".to_string()),
            Just("the".to_string()),
            Just("🎉".to_string()),
            Just("你好".to_string()),
        ]
    }

    fn segments_strategy() -> impl Strategy<Value = Vec<TimedSegment>> {
        // Generate sorted, non-overlapping segments with bounded gaps.
        prop::collection::vec(
            (
                prop::collection::vec(token_strategy(), 0..6),
                0u32..1_000,
                100u32..2_000,
            ),
            0..6,
        )
        .prop_map(|raws| {
            let mut t = 0u32;
            raws.into_iter()
                .map(|(toks, gap, dur)| {
                    t = t.saturating_add(gap);
                    let t0 = t;
                    t = t.saturating_add(dur);
                    let t1 = t;
                    TimedSegment {
                        text: toks.join(" "),
                        t0_ms: t0,
                        t1_ms: t1,
                    }
                })
                .collect()
        })
    }

    proptest! {
        /// Re-wrapping the formatted output as a single segment and
        /// re-running the formatter is a fixpoint. This catches
        /// double-stripping bugs, capitalization creep, and
        /// trim-of-trim being non-identity.
        ///
        /// mb-7k6: the formatter IS now a fixpoint. Previously it was
        /// not -- re-formatting collapsed paragraph breaks (".\n\nA" ->
        /// ". A") and re-stripped fillers/repeats that only appeared
        /// once segments were glued. The fix preserves "\n\n" through the
        /// tokenizer (`split_into_paragraphs`) and strips each merged
        /// paragraph to a fixpoint (`strip_to_fixpoint`).
        #[test]
        fn format_is_idempotent_fixpoint(segments in segments_strategy()) {
            let once = fmt(&segments);
            let rewrapped = vec![seg(&once, 0, 1_000)];
            let twice = fmt(&rewrapped);
            prop_assert_eq!(once, twice);
        }

        /// Formatter never panics, never errs on arbitrary segment
        /// text (including the proptest strategy's sprinkle of
        /// punctuation + non-ASCII).
        #[test]
        fn format_never_panics_on_arbitrary_unicode(segments in segments_strategy()) {
            let _ = fmt(&segments);
        }
    }
}
