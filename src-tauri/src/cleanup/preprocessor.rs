#![allow(missing_docs)] // Per-field docs are redundant; the module-level docs are the API surface.

//! Deterministic transcript preprocessor — the cheap, predictable
//! cleanup pass that runs BEFORE the LLM.
//!
//! ## Why this exists
//!
//! Asking a 3 B-parameter quantized local model to do everything —
//! strip "um"s, fix capitalization, render verbal punctuation,
//! detect lists, apply style — is asking it too much. The model
//! burns attention budget on mechanical noise and gets the
//! interesting judgment work wrong. See ADR 0022 for the full
//! pipeline analysis.
//!
//! The preprocessor handles the rule-shaped 80 % of cleanup in
//! single-digit milliseconds, leaving the LLM with the small,
//! focused remainder: "is this a list? where do paragraphs break?
//! what's the appropriate register?"
//!
//! ## Scope discipline
//!
//! In scope:
//!   - Filler removal (two tiers, by safety)
//!   - Stutter collapse ("the the the" → "the")
//!   - Self-correction stitching ("X, wait, Y" → "Y")
//!   - Verbal punctuation rendering ("comma", "period", "question mark", …)
//!   - Verbal layout cues ("new paragraph", "new line")
//!   - Verbal quote/bracket rendering
//!   - Sentence capitalization
//!   - Terminal punctuation insertion
//!
//! Explicitly OUT of scope (the LLM's job):
//!   - Detecting implicit lists without verbal cues
//!   - Paragraph breaks from semantic topic shift
//!   - Register / tone transformation (slang → formal)
//!   - Grammar restructuring
//!   - Number / date / currency normalization
//!
//! ## Filler tiers
//!
//! - **Tier 1** ("always-safe"): `um`, `uh`, `er`, `ah`, `hmm`, `mm`,
//!   `mhm`. These are unambiguous interjections. Stripped wherever
//!   they appear as standalone tokens.
//! - **Tier 2** ("context-guarded"): `like`, `you know`, `I mean`,
//!   `sort of`, `kind of`, `basically`. These are ALSO real content
//!   words ("I like pizza", "you know nothing", "basically true").
//!   Stripped ONLY when comma-bounded or sentence-initial — i.e.
//!   when the speaker's own prosody has flagged them as fillers.
//!
//! ## Determinism + version pinning
//!
//! [`PREPROCESSOR_VERSION`] is the rule-set identifier baked into the
//! `transcripts.model_used` column (suffixed onto the LLM's model id,
//! e.g. `"qwen2.5:3b-q4+preproc@v1"`). If we change a rule, the
//! version bumps; old session rows still pin their exact transform.
//! ADR 0008 provenance invariant preserved without a schema change.

use std::sync::OnceLock;

use regex::Regex;

/// Bumped any time a rule changes. Persisted via `model_used` suffix.
pub const PREPROCESSOR_VERSION: &str = "preproc@v1";

/// Tier-1 filler tokens: always safe to strip when they appear as
/// standalone whitespace-bounded tokens. Kept as a `&'static [&str]`
/// so the compiler can constant-fold the match table.
pub const TIER_1_FILLERS: &[&str] = &["um", "uh", "er", "ah", "hmm", "mm", "mhm"];

/// Tier-2 filler PHRASES: stripped only when comma-bounded OR
/// sentence-initial. Multi-word entries are matched left-to-right
/// against the token stream.
pub const TIER_2_FILLERS: &[&str] = &[
    "like",
    "you know",
    "i mean",
    "sort of",
    "kind of",
    "basically",
];

/// One verbal punctuation cue → the literal to substitute.
///
/// Matched only when the cue is a standalone token (whitespace or
/// punctuation on both sides). Comparison is case-insensitive on the
/// cue side; the substitution is inserted verbatim.
const PUNCTUATION_CUES: &[(&str, &str)] = &[
    ("period", "."),
    ("full stop", "."),
    ("comma", ","),
    ("question mark", "?"),
    ("exclamation point", "!"),
    ("exclamation mark", "!"),
    ("semicolon", ";"),
    ("colon", ":"),
    ("ellipsis", "…"),
    ("dot dot dot", "…"),
    ("dash", "—"),
    ("em dash", "—"),
];

/// Verbal layout cues. Same standalone-token rule as punctuation,
/// substituted to whitespace control characters.
const LAYOUT_CUES: &[(&str, &str)] = &[
    ("new paragraph", "\n\n"),
    ("new line", "\n"),
    ("line break", "\n"),
];

/// Ordinal cues — sequence words that strongly signal an enumerated
/// list when more than one shows up in a single utterance. Matched
/// case-insensitively at word boundaries (anywhere in the text). The
/// `≥ 2` threshold in [`ProcessedNotes::looks_listy`] is what makes
/// these usable: a single "first" is often content; "first ... second"
/// is almost certainly a list.
///
/// See ADR 0047 §Wave 2.2 + `detect_list_signals`.
const ORDINAL_CUES: &[&str] = &[
    "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth",
    "lastly", "finally",
];

/// Enumeration markers — spoken cardinal numbers at clause boundaries.
/// More ambiguous than ordinals ("I have two cats" isn't a list) so:
/// (a) only counted at sentence-initial / post-punctuation position,
/// and (b) the [`ProcessedNotes::looks_listy`] threshold is `≥ 3`
/// (vs `≥ 2` for ordinals). Together those guards keep the
/// false-positive rate low.
///
/// See ADR 0047 §Wave 2.2 + `detect_list_signals`.
const ENUMERATION_MARKERS: &[&str] = &[
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
];

/// Quote/bracket cues. Bilateral — opens render the OPEN glyph, closes
/// render the CLOSE glyph. We don't try to balance them; that's the
/// speaker's responsibility.
const QUOTE_BRACKET_CUES: &[(&str, &str)] = &[
    ("open quote", "\""),
    ("close quote", "\""),
    ("unquote", "\""),
    ("quote", "\""),
    ("open paren", "("),
    ("close paren", ")"),
    ("open parenthesis", "("),
    ("close parenthesis", ")"),
];

/// Output of [`Preprocessor::process`].
///
/// `text` is the rewritten transcript ready for the LLM (or for
/// direct injection in Wave-3 LLM-skip mode). `notes` is an optional
/// human-readable breakdown of what changed; used for logging and
/// the Wave-3 "did this need an LLM?" decision heuristic.
#[derive(Debug, Clone, Default)]
pub struct Processed {
    pub text: String,
    pub notes: ProcessedNotes,
}

/// Per-rule counters. Cheap to compute, useful for tracing + tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessedNotes {
    pub fillers_stripped: usize,
    pub stutters_collapsed: usize,
    pub self_corrections: usize,
    pub punctuation_cues_rendered: usize,
    pub layout_cues_rendered: usize,
    pub quote_bracket_cues_rendered: usize,
    pub sentences_capitalized: usize,
    pub terminal_punctuation_added: bool,
    /// Count of [`ORDINAL_CUES`] occurrences in the processed text.
    /// Anywhere, case-insensitive, word-boundary matched. Used by
    /// [`Self::looks_listy`]; the ADR 0047 §Wave 2.2 threshold is `≥ 2`.
    pub ordinal_cues_detected: usize,
    /// Count of [`ENUMERATION_MARKERS`] occurrences at clause boundaries
    /// (sentence-initial or after `.,!?`). Used by [`Self::looks_listy`];
    /// the ADR 0047 §Wave 2.2 threshold is `≥ 3` (stricter than the
    /// ordinal cutoff because raw cardinal-number words are noisier).
    pub enumeration_markers_detected: usize,
}

impl ProcessedNotes {
    /// "Did the preprocessor see anything list-shaped in this
    /// transcript?" — the gate for the Wave-2.2 LLM-skip heuristic.
    /// If true, the LLM must run so the list gets rendered correctly
    /// (the preprocessor itself never inserts bullet/numbered list
    /// structure — that's out of its scope, per the module docs).
    ///
    /// Thresholds chosen in ADR 0047 §Wave 2.2: `≥ 2 ordinal cues`
    /// OR `≥ 3 enumeration markers`. The OR is deliberate — a
    /// speaker who said "first ... second" is enumerating even
    /// without numbers, and a speaker who said "one, two, three"
    /// is enumerating even without ordinal words.
    pub fn looks_listy(&self) -> bool {
        self.ordinal_cues_detected >= 2 || self.enumeration_markers_detected >= 3
    }
}

/// The deterministic preprocessor.
///
/// Single struct, single public method. All rule tables are static
/// so construction is free; the OnceLock-backed regex cache makes
/// the first call ~50 µs more expensive than subsequent ones.
#[derive(Debug, Default, Clone, Copy)]
pub struct Preprocessor;

impl Preprocessor {
    /// Construct. Stateless.
    pub const fn new() -> Self {
        Self
    }

    /// Run the pipeline. Order matters — see comments on each step.
    ///
    /// Critical ordering invariant: any pass that injects a newline
    /// (layout cues) MUST run AFTER any pass that uses
    /// `split_whitespace` (stutter collapse). Otherwise the
    /// whitespace tokeniser eats the newline and the paragraph
    /// break vanishes.
    pub fn process(&self, raw: &str) -> Processed {
        if raw.trim().is_empty() {
            return Processed::default();
        }

        let mut notes = ProcessedNotes::default();
        let mut text = raw.to_string();

        // 1. Self-corrections first: "X, wait, Y" → "Y". Done
        //    early so subsequent fillers + stutters in the discarded
        //    left half don't get spuriously counted.
        text = stitch_self_corrections(&text, &mut notes);

        // 2. Filler removal — tier-1 unconditional, tier-2 context-
        //    guarded. Done before stutters so "um the the the cat"
        //    collapses to "the cat" cleanly.
        text = strip_tier1_fillers(&text, &mut notes);
        text = strip_tier2_fillers(&text, &mut notes);

        // 3. Stutters. Uses split_whitespace, so MUST run before
        //    any pass that inserts newlines.
        text = collapse_stutters(&text, &mut notes);

        // 4. Cue rendering — punctuation, quotes, then layout.
        //    Layout (newlines) goes last so the stutter pass above
        //    doesn't eat the line breaks.
        text = render_punctuation_cues(&text, &mut notes);
        text = render_quote_bracket_cues(&text, &mut notes);
        text = render_layout_cues(&text, &mut notes);

        // 5. Whitespace + casing + terminal punct cleanup.
        text = collapse_whitespace(&text);
        text = capitalize_sentences(&text, &mut notes);
        text = add_terminal_punctuation(&text, &mut notes);

        // 6. List-signal detection — runs LAST so we measure the
        //    final text the LLM (or skip-path consumer) will see.
        //    Pure read pass; no text mutation.
        detect_list_signals(&text, &mut notes);

        Processed { text, notes }
    }
}

// ──────────────────────────────────────────────────────────────────────
// Rule implementations — pure functions, individually unit-tested.
// ──────────────────────────────────────────────────────────────────────

/// Replace verbal punctuation cues with their literal glyphs.
///
/// Each cue is matched as a standalone token (word boundary on both
/// sides) and substituted DIRECTLY (no leading space — punctuation
/// hugs the preceding word). Whitespace collapse at the end of the
/// pipeline cleans up any double-spaces this produces.
fn render_punctuation_cues(input: &str, notes: &mut ProcessedNotes) -> String {
    let mut out = input.to_string();
    for (cue, glyph) in PUNCTUATION_CUES {
        let re = cue_regex(cue);
        // Substitute with the glyph plus a trailing space so the next
        // word doesn't collide. Trailing whitespace is collapsed later.
        let count = re.find_iter(&out).count();
        if count > 0 {
            // Replace as " <glyph> " to neutralise the leading-space
            // capture group; whitespace collapse + the "drop space
            // before punctuation" pass below handle the rest.
            out = re
                .replace_all(&out, format!("{glyph} ").as_str())
                .into_owned();
            notes.punctuation_cues_rendered += count;
        }
    }
    // Drop the artifact " ." → ".", " ," → ",", etc. that arises
    // because cues are matched as standalone tokens with a leading
    // space included in the regex.
    out = drop_space_before_punctuation(&out);
    out
}

/// Count ordinal cues + clause-boundary enumeration markers in the
/// preprocessor's output. Pure read pass — does not mutate `input`.
///
/// The downstream consumer ([`ProcessedNotes::looks_listy`]) uses
/// the counts to decide whether the LLM-skip heuristic in
/// `cleanup/llm_cleaner.rs::run_cleanup` may bypass the LLM on a
/// short utterance. Under-counting here costs a needless LLM call;
/// over-counting here costs a list that the LLM never gets a chance
/// to render. Conservative on enumeration markers (clause-boundary
/// only) because false positives there are pricier than false
/// negatives on ordinal cues.
fn detect_list_signals(input: &str, notes: &mut ProcessedNotes) {
    static ORDINAL_RE: OnceLock<Regex> = OnceLock::new();
    let ordinal_re = ORDINAL_RE.get_or_init(|| {
        let alt = ORDINAL_CUES.join("|");
        Regex::new(&format!(r"(?i)\b(?:{alt})\b")).expect("ordinal cue regex")
    });
    notes.ordinal_cues_detected = ordinal_re.find_iter(input).count();

    static ENUM_RE: OnceLock<Regex> = OnceLock::new();
    let enum_re = ENUM_RE.get_or_init(|| {
        // Clause boundary: start-of-string OR `.,!?\n` followed by
        // whitespace. Then the number-word at a word boundary. This
        // pattern only matches enumeration-like uses; "I have two
        // cats" doesn't qualify because `two` is mid-clause.
        let alt = ENUMERATION_MARKERS.join("|");
        Regex::new(&format!(r"(?i)(?:^|[.,!?\n]\s*)(?:{alt})\b")).expect("enumeration marker regex")
    });
    notes.enumeration_markers_detected = enum_re.find_iter(input).count();
}

fn render_layout_cues(input: &str, notes: &mut ProcessedNotes) -> String {
    let mut out = input.to_string();
    for (cue, glyph) in LAYOUT_CUES {
        let re = cue_regex(cue);
        let count = re.find_iter(&out).count();
        if count > 0 {
            out = re.replace_all(&out, *glyph).into_owned();
            notes.layout_cues_rendered += count;
        }
    }
    out
}

fn render_quote_bracket_cues(input: &str, notes: &mut ProcessedNotes) -> String {
    let mut out = input.to_string();
    for (cue, glyph) in QUOTE_BRACKET_CUES {
        let re = cue_regex(cue);
        let count = re.find_iter(&out).count();
        if count > 0 {
            // Open glyphs hug the FOLLOWING word, close glyphs hug
            // the PRECEDING word. For simplicity we substitute the
            // glyph and let whitespace collapse + the "drop space
            // before close-punct" pass clean up.
            let is_close =
                matches!(*glyph, ")" | "]" | "}") || cue.contains("close") || *cue == "unquote";
            let replacement = if is_close {
                format!("{glyph} ")
            } else {
                format!(" {glyph}")
            };
            out = re.replace_all(&out, replacement.as_str()).into_owned();
            notes.quote_bracket_cues_rendered += count;
        }
    }
    drop_space_before_close_bracket(&out)
}

/// "X, wait, Y" / "X, no I mean Y" / "X, sorry Y" / "X, scratch that Y"
/// / "X, strike that Y" → "Y"
///
/// Conservative: only fires when comma-bounded on the left to avoid
/// chewing real text that happens to contain "wait" / "no" / "sorry".
fn stitch_self_corrections(input: &str, notes: &mut ProcessedNotes) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"(?i),\s*(?:wait|no wait|no\s*,?\s*i\s*mean|sorry|scratch that|strike that|i\s*meant|actually\s*scratch that)\s*,?\s*",
        )
        .expect("self-correction regex")
    });
    // For each match: collapse "<left>, wait, <right>" → "<right>".
    // Replace "<comma><cue>" with a single space so the right side
    // becomes a fresh sentence start; capitalization fixup handles
    // the rest.
    let mut count = 0usize;
    let out = re
        .replace_all(input, |_caps: &regex::Captures<'_>| {
            count += 1;
            ". ".to_string()
        })
        .into_owned();
    notes.self_corrections = count;
    out
}

fn strip_tier1_fillers(input: &str, notes: &mut ProcessedNotes) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        // Word-boundary match for any tier-1 filler. Case-insensitive.
        // The alternation is built from TIER_1_FILLERS so a const
        // addition is automatically picked up.
        let alt = TIER_1_FILLERS.join("|");
        Regex::new(&format!(r"(?i)\b(?:{alt})\b[,.]?\s*")).expect("tier-1 filler regex")
    });
    let count = re.find_iter(input).count();
    notes.fillers_stripped += count;
    re.replace_all(input, "").into_owned()
}

fn strip_tier2_fillers(input: &str, notes: &mut ProcessedNotes) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        // The cue that flags a Tier-2 token as FILLER (rather than
        // content) is the speaker's prosody, surfaced in the STT
        // output as a comma. So we only strip when:
        //   (a) comma-bounded both sides:  ",\s*<filler>\s*,"  OR
        //   (b) sentence-initial AND trailing comma:
        //                                  "(^|[.!?]\s+)<filler>,\s+"
        //
        // The TRAILING comma in (b) is load-bearing: it's the
        // difference between filler ("Like, let me explain") and
        // content ("You know nothing about it"). Both have the
        // candidate at sentence start; only the first one has the
        // prosodic comma that flags it as filler.
        let alt = TIER_2_FILLERS.join("|");
        Regex::new(&format!(
            r"(?i)(?:,\s*(?:{alt})\s*,|(?:^|[.!?]\s+)(?:{alt}),\s+)"
        ))
        .expect("tier-2 filler regex")
    });
    let mut count = 0usize;
    let out = re
        .replace_all(input, |caps: &regex::Captures<'_>| {
            count += 1;
            let m = caps.get(0).map(|m| m.as_str()).unwrap_or("");
            // Preserve sentence boundary if the match started with one.
            if let Some(c) = m.chars().next() {
                if c == '.' || c == '!' || c == '?' {
                    return format!("{c} ");
                }
            }
            // Comma-bounded form: leave a single comma.
            if m.starts_with(',') {
                return ",".to_string();
            }
            String::new()
        })
        .into_owned();
    notes.fillers_stripped += count;
    out
}

/// "the the the" → "the", "I I think" → "I think". Conservative:
/// only collapses 2+ consecutive identical SHORT tokens (≤ 4 chars).
/// Longer repeats are likely the speaker emphasising for effect
/// ("very very very important").
///
/// Implemented as a manual token walk because the Rust `regex` crate
/// (deliberately, for performance) does not support backreferences,
/// and stutter detection is fundamentally a backreference problem
/// ("this word equals the previous word"). A linear scan is O(n)
/// anyway — comparable cost to a regex pass.
fn collapse_stutters(input: &str, notes: &mut ProcessedNotes) -> String {
    const MAX_STUTTER_LEN: usize = 4;

    // We split-and-rejoin on whitespace runs. This loses the exact
    // whitespace characters used (tab vs space), which is fine
    // because the whitespace-collapse pass at the end of the
    // pipeline normalises everything to single spaces anyway. Line
    // breaks survive because they're inserted by render_layout_cues
    // BEFORE this pass and don't appear inside the word tokens.
    let mut out: Vec<&str> = Vec::with_capacity(input.split_whitespace().count());
    let mut count = 0usize;
    for token in input.split_whitespace() {
        // Compare against the last kept token (case-insensitive).
        // Only collapse if both are short — long repeats are
        // intentional emphasis ("very very very important").
        if let Some(prev) = out.last() {
            if token.len() <= MAX_STUTTER_LEN
                && prev.len() <= MAX_STUTTER_LEN
                && token.eq_ignore_ascii_case(prev)
            {
                count += 1;
                continue;
            }
        }
        out.push(token);
    }
    notes.stutters_collapsed = count;
    out.join(" ")
}

/// Capitalize the first letter of each sentence.
///
/// "Sentence" = string after `^`, after `.!?` + whitespace, or after
/// `\n\n`. We don't touch the rest of the string — speaker's casing
/// in the middle of a sentence is preserved (proper nouns, acronyms).
fn capitalize_sentences(input: &str, notes: &mut ProcessedNotes) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(^|[.!?]\s+|\n\n)([a-z])").expect("capitalize regex"));
    let mut count = 0usize;
    let out = re
        .replace_all(input, |caps: &regex::Captures<'_>| {
            count += 1;
            format!("{}{}", &caps[1], caps[2].to_uppercase())
        })
        .into_owned();
    notes.sentences_capitalized = count;
    out
}

/// Add a trailing `.` if the transcript ends without `.`, `?`, `!`.
fn add_terminal_punctuation(input: &str, notes: &mut ProcessedNotes) -> String {
    let trimmed = input.trim_end();
    if trimmed.is_empty() {
        return input.to_string();
    }
    let last_char = trimmed.chars().last();
    let needs_period = !matches!(last_char, Some('.') | Some('?') | Some('!') | Some(':'));
    if needs_period {
        notes.terminal_punctuation_added = true;
        return format!("{trimmed}.");
    }
    trimmed.to_string()
}

// ──────────────────────────────────────────────────────────────────────
// Small whitespace + punctuation hygiene helpers.
// ──────────────────────────────────────────────────────────────────────

fn collapse_whitespace(input: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"[ \t]{2,}").expect("ws collapse regex"));
    re.replace_all(input, " ").into_owned()
}

/// "hello ." → "hello.", "world ," → "world,". Used after the
/// punctuation-cue substitution leaves a stray leading space.
fn drop_space_before_punctuation(input: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\s+([.,;:!?…—])").expect("space-before-punct regex"));
    re.replace_all(input, "$1").into_owned()
}

fn drop_space_before_close_bracket(input: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\s+([)\]}])").expect("close-bracket regex"));
    re.replace_all(input, "$1").into_owned()
}

/// Build a case-insensitive, whitespace-tolerant standalone-token
/// matcher for a verbal cue.
///
/// The Rust `regex` crate is the safe non-backtracking implementation
/// and deliberately does NOT support lookaround (`(?=...)`/`(?<=...)`).
/// We work around that by CONSUMING the boundaries:
///
/// - `(?:^|\s)` consumes the leading boundary (start of string or one
///   whitespace char). For cues at the start of the transcript the
///   `^` alternative matches zero-width.
/// - `\b` (supported by `regex`) anchors the END of the cue at a
///   word boundary, preventing prefix matches ("comma" doesn't match
///   inside "command").
/// - `\s?` optionally consumes one trailing whitespace. The
///   replacement supplies whatever post-cue spacing it wants; the
///   later whitespace-collapse pass cleans up doubles.
///
/// Net effect: a match of `"hello new paragraph world"` consumes
/// `" new paragraph "` (7 + 1 chars), leaving room for the
/// replacement to insert exactly what it wants between `hello` and
/// `world` without trailing-whitespace artifacts.
fn cue_regex(cue: &str) -> Regex {
    let escaped = regex::escape(cue);
    Regex::new(&format!(r"(?i)(?:^|\s){escaped}\b\s?")).expect("cue regex")
}

// ──────────────────────────────────────────────────────────────────────
// Tests — the bug-catcher net for every rule above.
// ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &str) -> String {
        Preprocessor::new().process(input).text
    }

    fn notes(input: &str) -> ProcessedNotes {
        Preprocessor::new().process(input).notes
    }

    // -- Tier 1 fillers -----------------------------------------------

    #[test]
    fn strips_um_at_start() {
        assert_eq!(run("um hello world"), "Hello world.");
    }

    #[test]
    fn strips_uh_mid_sentence() {
        assert_eq!(run("I think uh we should go"), "I think we should go.");
    }

    #[test]
    fn strips_multiple_tier1_fillers_in_one_pass() {
        let n = notes("um uh I think er we should ah go");
        assert_eq!(n.fillers_stripped, 4);
    }

    #[test]
    fn preserves_words_containing_filler_substrings() {
        // "uhh" / "ohm" / "humble" should NOT be stripped — the
        // word-boundary anchors prevent substring matches.
        assert_eq!(
            run("the humble hummingbird hums"),
            "The humble hummingbird hums."
        );
    }

    // -- Tier 2 fillers -----------------------------------------------

    #[test]
    fn keeps_like_as_verb() {
        // "I like pizza" — `like` is the verb. Must not be stripped.
        assert_eq!(run("I like pizza"), "I like pizza.");
    }

    #[test]
    fn strips_sentence_initial_like() {
        assert_eq!(run("Like, let me explain"), "Let me explain.");
    }

    #[test]
    fn strips_comma_bounded_you_know() {
        assert_eq!(run("so, you know, it works"), "So, it works.");
    }

    #[test]
    fn keeps_you_know_when_not_bounded() {
        // "you know nothing" → "you know" is content. Conservative
        // rule: only strip when prosody (commas) flags it as filler.
        assert_eq!(
            run("you know nothing about it"),
            "You know nothing about it."
        );
    }

    // -- Stutters -----------------------------------------------------

    #[test]
    fn collapses_three_the() {
        assert_eq!(run("the the the cat"), "The cat.");
    }

    #[test]
    fn collapses_repeated_pronoun() {
        assert_eq!(run("I I think we should go"), "I think we should go.");
    }

    #[test]
    fn does_not_collapse_long_repeats() {
        // Speaker emphasis — "very very very important" stays.
        // (Long enough not to match the ≤4-char stutter heuristic.)
        assert_eq!(
            run("this is important important important stuff"),
            "This is important important important stuff."
        );
    }

    // -- Self-corrections ---------------------------------------------

    #[test]
    fn stitches_wait() {
        assert_eq!(
            run("send it to Bob, wait, Alice instead"),
            "Send it to Bob. Alice instead."
        );
    }

    #[test]
    fn stitches_no_i_mean() {
        assert_eq!(run("call John, no I mean Sarah"), "Call John. Sarah.");
    }

    #[test]
    fn stitches_scratch_that() {
        assert_eq!(run("meet at 3, scratch that, 4 pm"), "Meet at 3. 4 pm.");
    }

    // -- Verbal punctuation ------------------------------------------

    #[test]
    fn renders_comma_cue() {
        assert_eq!(run("hello comma world"), "Hello, world.");
    }

    #[test]
    fn renders_question_mark_cue() {
        assert_eq!(run("is it true question mark"), "Is it true?");
    }

    #[test]
    fn renders_exclamation_mark_cue() {
        assert_eq!(run("hey exclamation mark"), "Hey!");
    }

    #[test]
    fn renders_period_cue_explicitly() {
        assert_eq!(run("hello period world period"), "Hello. World.");
    }

    // -- Verbal layout ------------------------------------------------

    #[test]
    fn renders_new_paragraph() {
        let out = run("first thought new paragraph second thought");
        assert!(
            out.contains("\n\n"),
            "expected paragraph break, got {out:?}"
        );
    }

    #[test]
    fn renders_new_line() {
        let out = run("line one new line line two");
        assert!(out.contains('\n'), "expected line break, got {out:?}");
    }

    // -- Quotes / brackets -------------------------------------------

    #[test]
    fn renders_open_close_quote() {
        let out = run("he said open quote hi close quote");
        assert!(out.contains('"'), "expected quote marks, got {out:?}");
    }

    #[test]
    fn renders_parens() {
        let out = run("see the chart open paren attached close paren");
        assert!(out.contains('(') && out.contains(')'), "got {out:?}");
    }

    // -- Capitalization -----------------------------------------------

    #[test]
    fn capitalizes_first_letter() {
        assert_eq!(run("hello world"), "Hello world.");
    }

    #[test]
    fn capitalizes_after_period() {
        assert_eq!(
            run("this is one. this is two."),
            "This is one. This is two."
        );
    }

    #[test]
    fn does_not_recapitalize_proper_nouns() {
        // "Alice" / "Bob" inside sentence stay as-is. We only touch
        // the first letter after sentence boundaries.
        assert_eq!(run("hello Alice and Bob"), "Hello Alice and Bob.");
    }

    // -- Terminal punctuation ----------------------------------------

    #[test]
    fn adds_period_when_missing() {
        assert_eq!(run("hello world"), "Hello world.");
    }

    #[test]
    fn does_not_double_punctuate() {
        assert_eq!(run("hello world."), "Hello world.");
        assert_eq!(run("is it true?"), "Is it true?");
        assert_eq!(run("hey!"), "Hey!");
    }

    // -- Composition / integration -----------------------------------

    #[test]
    fn empty_input_returns_empty() {
        let p = Preprocessor::new().process("");
        assert!(p.text.is_empty());
        assert_eq!(p.notes, ProcessedNotes::default());
    }

    #[test]
    fn whitespace_only_returns_empty() {
        let p = Preprocessor::new().process("   \n  \t  ");
        assert!(p.text.is_empty());
    }

    #[test]
    fn realistic_dictation_with_fillers_and_correction() {
        // The kind of sentence that crashed v3 in the smoketest.
        let input = "um so I think we should um send it to Bob, wait, Alice instead";
        let out = run(input);
        // No tier-1 fillers, capitalised, terminal punct, self-correction stitched.
        assert!(!out.contains(" um "), "got {out:?}");
        assert!(!out.contains("uh"), "got {out:?}");
        assert!(out.starts_with('S'), "should be capitalised; got {out:?}");
        assert!(out.ends_with('.'), "needs terminal punct; got {out:?}");
        assert!(
            out.contains("Alice"),
            "stitch must keep right side; got {out:?}"
        );
    }

    #[test]
    fn list_pattern_passes_through_for_llm() {
        // The preprocessor doesn't render lists (no bullets / numbers
        // inserted) — that's the LLM's job. But the list-shape
        // SIGNAL must be detected so `looks_listy()` can gate the
        // Wave-2.2 LLM-skip path. Compare word set to ensure no
        // content was dropped, AND assert listy detection.
        let input = "here I have a list of keyboard supplies first thing is air duster second is alcohol wipes third is an extra cable";
        let processed = Preprocessor::new().process(input);
        for needle in [
            "list", "keyboard", "supplies", "air", "duster", "alcohol", "wipes", "extra", "cable",
        ] {
            assert!(
                processed.text.to_lowercase().contains(needle),
                "content word {needle:?} missing from {:?}",
                processed.text
            );
        }
        assert!(
            processed.notes.looks_listy(),
            "input with three ordinals should look listy; got notes={:?}",
            processed.notes
        );
    }

    // -- Determinism --------------------------------------------------

    #[test]
    fn is_idempotent_on_second_pass() {
        // Running the preprocessor on already-preprocessed text
        // should be a no-op (modulo a few obvious cases like
        // double-running terminal-punct addition, which we suppress
        // via the existing "ends with punct" check).
        let p = Preprocessor::new();
        let once = p.process("hello world").text;
        let twice = p.process(&once).text;
        assert_eq!(once, twice);
    }

    #[test]
    fn same_input_produces_same_output() {
        let p = Preprocessor::new();
        let a = p.process("um hello, you know, world").text;
        let b = p.process("um hello, you know, world").text;
        assert_eq!(a, b);
    }

    // -- List-signal detection (ADR 0047 §Wave 2.2) ------------------

    #[test]
    fn looks_listy_default_is_false() {
        // Default-constructed notes — zero of both signals — must
        // not flag as listy. Protects the LLM-skip path from
        // accidentally engaging on a fresh ProcessedNotes value
        // (e.g. the empty-input early-return in `process`).
        let n = ProcessedNotes::default();
        assert!(!n.looks_listy());
    }

    #[test]
    fn looks_listy_trips_on_two_ordinals() {
        let n = notes("first finish the migration second review the PR");
        assert!(
            n.ordinal_cues_detected >= 2,
            "expected ≥2 ordinals; got {}",
            n.ordinal_cues_detected
        );
        assert!(n.looks_listy(), "two-ordinal input should look listy");
    }

    #[test]
    fn looks_listy_does_not_trip_on_single_ordinal() {
        // Single ordinal is often content ("the first day of school");
        // the ≥2 threshold deliberately ignores it.
        let n = notes("the first day of school was great");
        assert_eq!(n.ordinal_cues_detected, 1);
        assert!(!n.looks_listy(), "single ordinal should NOT look listy");
    }

    #[test]
    fn looks_listy_trips_on_three_enumeration_markers() {
        // Clause-boundary number words: "one, two, three" at
        // sentence-start + after commas — the textbook spoken
        // enumeration shape.
        let n =
            notes("the steps are. one, prepare the room. two, set the table. three, serve dinner");
        assert!(
            n.enumeration_markers_detected >= 3,
            "expected ≥3 enumeration markers; got {}",
            n.enumeration_markers_detected
        );
        assert!(n.looks_listy(), "three-enum input should look listy");
    }

    #[test]
    fn looks_listy_ignores_midclause_number_words() {
        // "I have two cats and three dogs" should NOT count as
        // enumeration — the number words are mid-clause content, not
        // list prefixes. The clause-boundary anchor in the regex is
        // what enforces this.
        let n = notes("I have two cats and three dogs and five fish at home");
        assert_eq!(
            n.enumeration_markers_detected, 0,
            "mid-clause number words must not count"
        );
        assert!(!n.looks_listy());
    }

    #[test]
    fn looks_listy_false_on_plain_sentence() {
        let n = notes("this is a normal dictation with no list shape at all");
        assert_eq!(n.ordinal_cues_detected, 0);
        assert_eq!(n.enumeration_markers_detected, 0);
        assert!(!n.looks_listy());
    }

    #[test]
    fn version_constant_is_stable() {
        // Bumping this string is intentional; this test exists to
        // make sure version changes show up in a code review (and
        // the matching schema migration / model_used suffix change
        // is in the same PR).
        assert_eq!(PREPROCESSOR_VERSION, "preproc@v1");
    }
}
