//! Deterministic meeting-transcript formatter (Section MC.3).
//!
//! **Pure Rust. No RNG. No system clock. No global state. Same input
//! → same output, byte-for-byte.** This is the canonical pass for
//! meeting transcripts — the optional [`super::llm_pass`] runs AFTER
//! persist and its output is explicitly not written back to the DB.
//!
//! Wave 1 scaffold — `FormatOpts` + `format()` signature + `todo!()`.
//! Wave 2 implements the algorithm per Section MC.3 with ≥25 unit
//! tests + a `proptest` invariant that `format(format(x)) == format(x)`
//! (fixpoint).
//!
//! The `mc-formatter-deterministic` judge (Wave 6) feeds off the
//! Wave 2 test set plus the fixpoint proptest.

use crate::error::AppResult;

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
/// The `filler_set` parameter is taken by ref so callers can pass the
/// static [`super::filler_words::FILLERS`] without copying. Wave 2
/// adds a builder for runtime-customizable filler sets if any need
/// emerges; for now the static set covers Section MC.3's examples.
///
/// Wave 1: `todo!()` — Wave 2 implements the 6-step algorithm + ≥25
/// tests including the fixpoint proptest.
pub fn format(
    _segments: &[TimedSegment],
    _filler_set: &phf::Set<&'static str>,
    _opts: &FormatOpts,
) -> AppResult<String> {
    todo!("Wave 2: implement deterministic formatter per Section MC.3")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke: defaults match Section MC.3. Wave 2 brings the ≥25
    /// algorithmic tests + the fixpoint proptest.
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
}
