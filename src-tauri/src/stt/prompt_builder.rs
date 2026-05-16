#![allow(missing_docs)] // Public types doc-commented; method-level docs are the API.

//! Whisper `initial_prompt` builder — recency x frequency x app-match.
//!
//! Phase 2 Wave 4 implements the body Wave 1 scaffolded. The
//! pipeline:
//!
//!   1. Score each dictionary entry by `recency * frequency * app_match`.
//!      - recency: 1.0 if used within ~24h, decays linearly to 0.1 over
//!        7 days, then floors at 0.1.
//!      - frequency: `ln(1 + use_count)` (logarithmic so a single 1000x
//!        outlier doesn't dominate).
//!      - app_match: 2.0 multiplier if entry's `app_context` matches the
//!        current `foreground_app`; 1.0 otherwise.
//!   2. Sort entries descending by score; break ties on `term` lex for
//!      determinism.
//!   3. Greedily add `canonical` (or `term` if `canonical` is None) into
//!      the prompt until the token budget runs out. Token estimate uses
//!      whitespace splits (Whisper's BPE averages ~1 token per word for
//!      English; this errs on the side of under-stuffing).
//!   4. Apply hard [`PROMPT_TOKEN_CAP`] truncation at the end.

/// Whisper's hard cap on the `initial_prompt` field. Beyond this,
/// Whisper truncates (silently); we truncate explicitly here.
pub const PROMPT_TOKEN_CAP: usize = 224;

/// Recency window: anything used within the last 24h gets weight 1.0.
const RECENCY_HOT_HOURS: f64 = 24.0;
/// After this many hours, the recency multiplier floors at 0.1.
const RECENCY_COLD_HOURS: f64 = 24.0 * 7.0;
/// Cold-floor for entries with no `last_used_at` or older than cold horizon.
const RECENCY_FLOOR: f64 = 0.1;
/// Multiplier when an entry's `app_context` matches `foreground_app`.
const APP_MATCH_BOOST: f64 = 2.0;

#[derive(Debug, Clone, Default)]
pub struct PromptBuilderInput<'a> {
    pub dictionary: &'a [DictionaryView<'a>],
    pub foreground_app: Option<&'a str>,
    pub recent_transcripts: &'a [&'a str],
}

#[derive(Debug, Clone)]
pub struct DictionaryView<'a> {
    pub term: &'a str,
    pub canonical: Option<&'a str>,
    pub use_count: i64,
    pub last_used_at: Option<&'a str>,
    pub app_context: Option<&'a str>,
}

/// Build a Whisper `initial_prompt` from the supplied inputs.
///
/// Returns `None` if no input would produce any prompt content.
pub fn build_prompt(input: &PromptBuilderInput<'_>) -> Option<String> {
    let now_secs = now_unix_seconds();
    build_prompt_at(input, now_secs)
}

/// Test-friendly variant — caller supplies "now" so recency math is
/// deterministic.
pub fn build_prompt_at(input: &PromptBuilderInput<'_>, now_unix_seconds: i64) -> Option<String> {
    if input.dictionary.is_empty() {
        return None;
    }

    // Score every entry.
    let mut scored: Vec<(f64, &DictionaryView<'_>)> = input
        .dictionary
        .iter()
        .map(|d| (score(d, input.foreground_app, now_unix_seconds), d))
        .collect();

    // Descending by score; ties broken by term for determinism.
    scored.sort_by(|(s1, d1), (s2, d2)| {
        s2.partial_cmp(s1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| d1.term.cmp(d2.term))
    });

    // Greedy pack until token cap.
    let mut tokens_used = 0usize;
    let mut chosen: Vec<&str> = Vec::new();
    for (_, entry) in &scored {
        let word = entry.canonical.unwrap_or(entry.term);
        let cost = approx_tokens(word) + 1; // +1 for joining comma
        if tokens_used + cost > PROMPT_TOKEN_CAP {
            break;
        }
        chosen.push(word);
        tokens_used += cost;
    }

    if chosen.is_empty() {
        return None;
    }

    Some(chosen.join(", "))
}

fn score(entry: &DictionaryView<'_>, foreground_app: Option<&str>, now_unix_seconds: i64) -> f64 {
    let recency = recency_weight(entry.last_used_at, now_unix_seconds);
    let frequency = (1.0 + entry.use_count.max(0) as f64).ln();
    let app_match = if let (Some(a), Some(b)) = (entry.app_context, foreground_app) {
        if a.eq_ignore_ascii_case(b) {
            APP_MATCH_BOOST
        } else {
            1.0
        }
    } else {
        1.0
    };
    recency * frequency * app_match
}

fn recency_weight(last_used_at: Option<&str>, now_unix_seconds: i64) -> f64 {
    let Some(ts) = last_used_at else {
        return RECENCY_FLOOR;
    };
    let Some(used_secs) = parse_iso8601_seconds(ts) else {
        return RECENCY_FLOOR;
    };
    let age_hours = (now_unix_seconds - used_secs).max(0) as f64 / 3600.0;
    if age_hours <= RECENCY_HOT_HOURS {
        1.0
    } else if age_hours >= RECENCY_COLD_HOURS {
        RECENCY_FLOOR
    } else {
        // Linear decay from 1.0 (at HOT_HOURS) to FLOOR (at COLD_HOURS).
        let t = (age_hours - RECENCY_HOT_HOURS) / (RECENCY_COLD_HOURS - RECENCY_HOT_HOURS);
        1.0 + t * (RECENCY_FLOOR - 1.0)
    }
}

/// Cheap approximation of Whisper's BPE token count: whitespace splits.
fn approx_tokens(text: &str) -> usize {
    text.split_whitespace().count().max(1)
}

/// Parse a SQLite `CURRENT_TIMESTAMP` string ("YYYY-MM-DD HH:MM:SS")
/// or an ISO-8601 datetime ("YYYY-MM-DDTHH:MM:SS[Z]") into Unix seconds.
/// Returns None if parsing fails.
fn parse_iso8601_seconds(ts: &str) -> Option<i64> {
    // Hand-rolled parser to avoid pulling a date crate just for this.
    // Format: YYYY-MM-DD[T ]HH:MM:SS[.fff][Z]
    let bytes = ts.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let year = std::str::from_utf8(&bytes[0..4])
        .ok()?
        .parse::<i64>()
        .ok()?;
    if bytes[4] != b'-' {
        return None;
    }
    let month = std::str::from_utf8(&bytes[5..7])
        .ok()?
        .parse::<u32>()
        .ok()?;
    if bytes[7] != b'-' {
        return None;
    }
    let day = std::str::from_utf8(&bytes[8..10])
        .ok()?
        .parse::<u32>()
        .ok()?;
    if bytes[10] != b' ' && bytes[10] != b'T' {
        return None;
    }
    let hour = std::str::from_utf8(&bytes[11..13])
        .ok()?
        .parse::<u32>()
        .ok()?;
    if bytes[13] != b':' {
        return None;
    }
    let minute = std::str::from_utf8(&bytes[14..16])
        .ok()?
        .parse::<u32>()
        .ok()?;
    if bytes[16] != b':' {
        return None;
    }
    let second = std::str::from_utf8(&bytes[17..19])
        .ok()?
        .parse::<u32>()
        .ok()?;
    Some(
        days_from_civil(year, month, day) * 86_400
            + hour as i64 * 3_600
            + minute as i64 * 60
            + second as i64,
    )
}

/// Howard Hinnant's days_from_civil algorithm (public domain).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m as i64 + (if m > 2 { -3 } else { 9 })) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn now_unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry<'a>(
        term: &'a str,
        canonical: Option<&'a str>,
        use_count: i64,
        last_used_at: Option<&'a str>,
        app_context: Option<&'a str>,
    ) -> DictionaryView<'a> {
        DictionaryView {
            term,
            canonical,
            use_count,
            last_used_at,
            app_context,
        }
    }

    /// Reference "now" used across deterministic tests: 2026-05-16 12:00:00 UTC.
    /// Computed via our own parser so the constant and the impl stay in sync.
    fn now() -> i64 {
        parse_iso8601_seconds("2026-05-16 12:00:00").expect("parse NOW")
    }

    #[test]
    fn prompt_token_cap_is_224() {
        assert_eq!(PROMPT_TOKEN_CAP, 224);
    }

    #[test]
    fn build_prompt_returns_none_for_empty_input() {
        let input = PromptBuilderInput::default();
        assert!(build_prompt(&input).is_none());
    }

    #[test]
    fn single_entry_renders_canonical_form() {
        let dict = [entry(
            "k8s",
            Some("Kubernetes"),
            5,
            Some("2026-05-16 11:00:00"),
            None,
        )];
        let input = PromptBuilderInput {
            dictionary: &dict,
            foreground_app: None,
            recent_transcripts: &[],
        };
        let out = build_prompt_at(&input, now()).unwrap();
        assert_eq!(out, "Kubernetes");
    }

    #[test]
    fn falls_back_to_term_when_canonical_missing() {
        let dict = [entry(
            "rustacean",
            None,
            3,
            Some("2026-05-16 11:00:00"),
            None,
        )];
        let input = PromptBuilderInput {
            dictionary: &dict,
            foreground_app: None,
            recent_transcripts: &[],
        };
        let out = build_prompt_at(&input, now()).unwrap();
        assert_eq!(out, "rustacean");
    }

    #[test]
    fn higher_use_count_ranks_higher_for_same_recency() {
        let dict = [
            entry("rare", None, 1, Some("2026-05-16 11:00:00"), None),
            entry("common", None, 1000, Some("2026-05-16 11:00:00"), None),
        ];
        let input = PromptBuilderInput {
            dictionary: &dict,
            foreground_app: None,
            recent_transcripts: &[],
        };
        let out = build_prompt_at(&input, now()).unwrap();
        assert!(out.starts_with("common"), "got: {out}");
    }

    #[test]
    fn fresher_entry_ranks_higher_for_same_use_count() {
        let dict = [
            // 10 days old → cold floor
            entry("stale", None, 5, Some("2026-05-06 12:00:00"), None),
            // 1 hour old → hot
            entry("fresh", None, 5, Some("2026-05-16 11:00:00"), None),
        ];
        let input = PromptBuilderInput {
            dictionary: &dict,
            foreground_app: None,
            recent_transcripts: &[],
        };
        let out = build_prompt_at(&input, now()).unwrap();
        assert!(out.starts_with("fresh"), "got: {out}");
    }

    #[test]
    fn matching_app_context_boosts_score() {
        let dict = [
            entry(
                "vsterm",
                None,
                5,
                Some("2026-05-16 11:00:00"),
                Some("vscode.exe"),
            ),
            entry("global", None, 5, Some("2026-05-16 11:00:00"), None),
        ];
        let input = PromptBuilderInput {
            dictionary: &dict,
            foreground_app: Some("vscode.exe"),
            recent_transcripts: &[],
        };
        let out = build_prompt_at(&input, now()).unwrap();
        assert!(out.starts_with("vsterm"), "got: {out}");
    }

    #[test]
    fn non_matching_app_context_does_not_boost() {
        let dict = [
            entry(
                "vsterm",
                None,
                5,
                Some("2026-05-16 11:00:00"),
                Some("vscode.exe"),
            ),
            entry("hot-global", None, 5, Some("2026-05-16 11:30:00"), None),
        ];
        let input = PromptBuilderInput {
            dictionary: &dict,
            foreground_app: Some("notepad.exe"),
            recent_transcripts: &[],
        };
        // Both are equally hot; vsterm's app_context mismatches notepad
        // so no boost. Alphabetical tiebreak → "hot-global" comes first.
        let out = build_prompt_at(&input, now()).unwrap();
        assert!(out.starts_with("hot-global"), "got: {out}");
    }

    #[test]
    fn token_cap_truncates_long_dictionaries() {
        // Build a 500-entry dict; each entry is one token. We should
        // stop at PROMPT_TOKEN_CAP entries (with the +1 comma-cost
        // calculation, that's ~112 entries).
        let terms: Vec<String> = (0..500).map(|i| format!("term{i:03}")).collect();
        let dict: Vec<DictionaryView<'_>> = terms
            .iter()
            .map(|t| entry(t, None, 1, Some("2026-05-16 11:00:00"), None))
            .collect();
        let input = PromptBuilderInput {
            dictionary: &dict,
            foreground_app: None,
            recent_transcripts: &[],
        };
        let out = build_prompt_at(&input, now()).unwrap();
        let token_count = out.split_whitespace().count();
        assert!(
            token_count <= PROMPT_TOKEN_CAP,
            "produced {token_count} tokens, cap is {PROMPT_TOKEN_CAP}"
        );
        // Should still get a reasonable number of entries packed in.
        assert!(
            token_count >= 100,
            "only {token_count} tokens — too aggressive"
        );
    }

    #[test]
    fn iso8601_parser_handles_both_separators() {
        // SQLite's CURRENT_TIMESTAMP yields "YYYY-MM-DD HH:MM:SS"; we
        // also accept ISO-8601's "T" separator.
        let a = parse_iso8601_seconds("2026-05-16 12:00:00").unwrap();
        let b = parse_iso8601_seconds("2026-05-16T12:00:00").unwrap();
        assert_eq!(a, b);
        // Self-consistency: parser must agree with itself across invocations.
        assert_eq!(a, now());
    }

    #[test]
    fn missing_last_used_at_falls_to_cold_floor() {
        let dict = [
            entry("a", None, 100, None, None),
            entry("b", None, 1, Some("2026-05-16 11:00:00"), None),
        ];
        let input = PromptBuilderInput {
            dictionary: &dict,
            foreground_app: None,
            recent_transcripts: &[],
        };
        // "a" has 100x frequency but floors at 0.1 recency → 0.1 * ln(101) ~= 0.46.
        // "b" has hot recency (1.0) but ln(2) ~= 0.69 → score ~0.69.
        // So "b" wins despite lower use_count.
        let out = build_prompt_at(&input, now()).unwrap();
        assert!(out.starts_with("b"), "got: {out}");
    }
}
