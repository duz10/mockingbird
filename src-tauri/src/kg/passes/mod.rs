//! Wave 1A — the four pipeline passes (PLAN §8.1, skipping the
//! Transcribe pass per ADR 0048 §G2).
//!
//! Graduated from `experimental/kg-validation/src/passes/` per
//! Wave 2 Task 6 (`mb-rtk2`). The only edits from the sandbox are
//! import-path rewrites (`crate::` → `super::`) and the
//! `PassError::Dispatcher` payload going from `anyhow::Error` to
//! `OllamaError` (binding parameter D3). Per-pass JSON parsing and
//! the date hard-gate (PLAN §8.4, LESSONS P9) are bit-identical.
//!
//! Each pass is a free function over an `OllamaDispatcher` so unit
//! tests can swap in a canned mock without touching network code.
//! Output JSON parsing lives inside each pass — the dispatcher trait
//! deliberately returns raw text so the per-pass parse error can
//! capture the offending model output for downstream artifact
//! inspection (see [`PassError::Parse`]).

pub mod classify;
pub mod extract;
pub mod extract_entities;
pub mod normalize;
pub mod segment;
pub mod tag_validator;

// The orchestrator (`super::pipeline`) consumes the verbs + the
// types it threads through; the other re-exports are here so the
// kg module's public surface (`kg::EntityType`) and Chunk 3's
// parity probe can both name them without spelling out the
// submodule paths. They look unused to `rustc` because no in-crate
// caller imports them yet — the parity probe lands in Chunk 3.
#[allow(unused_imports)]
pub use classify::{classify, Classification};
#[allow(unused_imports)]
pub use extract::{extract, Extraction, ProposedNewTag};
#[allow(unused_imports)]
pub use extract_entities::{extract_entities, EntityExtraction, EntityType, ExtractedEntity};
#[allow(unused_imports)]
pub use normalize::normalize_tags;
#[allow(unused_imports)]
pub use segment::segment;
#[allow(unused_imports)]
pub use tag_validator::{validate_tags, NewTagRequest, NewTagRequestSource, TagValidationResult};

use super::ollama::OllamaError;

/// Error type shared by all passes. The raw model output is captured
/// on `Parse` / `Validation` so the harness can write it to disk for
/// downstream inspection — Wave 3's scorer and the human reading the
/// run report both need to see what the model actually said when it
/// failed.
#[derive(Debug, thiserror::Error)]
pub enum PassError {
    #[error("dispatcher error: {0}")]
    Dispatcher(#[from] OllamaError),

    #[error("JSON parse failed in {pass} pass: {error}\nRaw output:\n{raw}")]
    Parse {
        pass: &'static str,
        error: String,
        raw: String,
    },

    #[error("schema validation failed in {pass} pass: {detail}\nRaw output:\n{raw}")]
    Validation {
        pass: &'static str,
        detail: String,
        raw: String,
    },
}

/// Strict-then-relaxed typed parse shared by every pass.
///
/// All four passes (segment / classify / extract / extract_entities)
/// hit Ollama, strip the markdown envelope, then `serde_json::from_str`
/// into a pass-specific type. The common failure mode pinned by
/// `mb-y390` (dictation #143) is that small local models switch to
/// Python-style **single-quoted** strings the moment the user content
/// contains literal double quotes -- e.g. emitting
/// `["I've ...", 'title is "Get a New Computer"', ...]` -- which strict
/// JSON rejects at the first `'`.
///
/// Strategy: strict parse first (the common case stays zero-overhead);
/// on failure, attempt one [`relax_pythonish_quotes`] rewrite + one
/// retry. A successful relaxed parse emits a `tracing::warn!` so the
/// recovery is observable; a failed relaxed parse propagates the
/// **original strict error** (clearer signal for triage than "the
/// relaxed candidate also didn't parse").
pub(crate) fn parse_pass_json<T: serde::de::DeserializeOwned>(
    raw: &str,
    candidate: &str,
    pass: &'static str,
) -> Result<T, PassError> {
    match serde_json::from_str::<T>(candidate) {
        Ok(v) => Ok(v),
        Err(strict_err) => {
            if let Some(relaxed) = relax_pythonish_quotes(candidate) {
                if let Ok(v) = serde_json::from_str::<T>(&relaxed) {
                    tracing::warn!(
                        target: "kg::passes",
                        pass,
                        "recovered from python-style single-quoted output via loose relaxation (mb-y390)"
                    );
                    return Ok(v);
                }
            }
            Err(PassError::Parse {
                pass,
                error: strict_err.to_string(),
                raw: raw.to_string(),
            })
        }
    }
}

/// Rewrite Python-style single-quoted strings inside a JSON-shaped
/// candidate into strict JSON double-quoted strings. Returns `None`
/// when the input is either:
///   - already strict JSON (no `'` present anywhere, so the relaxation
///     can't possibly help -- preserves the strict parse error verbatim
///     for triage), or
///   - structurally malformed in a way the walk can't safely repair
///     (string mode unclosed at end of input).
///
/// State machine (three modes):
///   - **Outside**: copying verbatim. `"` opens a strict-JSON string
///     (mode -> InDouble). `'` opens a Python single-quoted string
///     (mode -> InSingle) and emits `"` instead.
///   - **InDouble**: standard JSON string body. Backslash escapes copy
///     through (`\X` -> `\X`). Embedded `'` copies verbatim. `"` closes.
///   - **InSingle**: Python single-quoted string body. `'` closes
///     (emits `"`). Bare `"` becomes `\"` (JSON-escape required since
///     the string is now double-quoted). `\'` becomes literal `'`
///     (Python escape sequence; JSON doesn't need quote-escaping inside
///     a non-matching quote). Other `\X` escapes pass through.
///
/// The relaxer is deliberately scope-limited: it only fixes the
/// quote-style mismatch. It does NOT attempt to repair trailing
/// commas, unquoted keys, or any other JSON5-ish liberties -- if
/// the model emits those alongside single quotes the strict error
/// still surfaces.
pub(crate) fn relax_pythonish_quotes(candidate: &str) -> Option<String> {
    if !candidate.contains('\'') {
        return None;
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mode {
        Outside,
        InDouble,
        InSingle,
    }

    let mut out = String::with_capacity(candidate.len() + 16);
    let mut chars = candidate.chars().peekable();
    let mut mode = Mode::Outside;

    while let Some(c) = chars.next() {
        match (mode, c) {
            (Mode::Outside, '"') => {
                out.push('"');
                mode = Mode::InDouble;
            }
            (Mode::Outside, '\'') => {
                out.push('"');
                mode = Mode::InSingle;
            }
            (Mode::Outside, _) => out.push(c),

            (Mode::InDouble, '\\') => {
                out.push('\\');
                if let Some(nc) = chars.next() {
                    out.push(nc);
                }
            }
            (Mode::InDouble, '"') => {
                out.push('"');
                mode = Mode::Outside;
            }
            (Mode::InDouble, _) => out.push(c),

            (Mode::InSingle, '\\') => {
                // Python `\'` -> literal `'` (JSON doesn't need to
                // escape `'` inside a `".."` string). Any other
                // backslash-escape passes through verbatim -- `\n`,
                // `\t`, `\"`, `\\` are all valid in both Python and
                // JSON string bodies.
                match chars.peek() {
                    Some(&'\'') => {
                        chars.next();
                        out.push('\'');
                    }
                    Some(_) => {
                        out.push('\\');
                        out.push(chars.next().unwrap());
                    }
                    None => {
                        // Trailing backslash inside a string -- give
                        // up; the input is malformed.
                        return None;
                    }
                }
            }
            (Mode::InSingle, '\'') => {
                out.push('"');
                mode = Mode::Outside;
            }
            (Mode::InSingle, '"') => {
                // Bare `"` inside what was a single-quoted Python
                // string -- must be JSON-escaped now that the wrapper
                // is `".."`.
                out.push('\\');
                out.push('"');
            }
            (Mode::InSingle, _) => out.push(c),
        }
    }

    if mode == Mode::Outside {
        Some(out)
    } else {
        None
    }
}

/// Best-effort JSON extraction: strips ```` ```json ... ``` ```` code
/// fences and any prose before the first `{` or `[`. Small local
/// models chronically wrap their JSON in markdown despite explicit
/// instructions; this helper is the dam against that behaviour
/// leaking into every pass's parser. Returns the original text
/// unchanged if no obvious fence/prose pattern is found.
pub(crate) fn strip_json_envelope(raw: &str) -> &str {
    let trimmed = raw.trim();

    // Fenced block: take the inside of the first ``` ... ``` pair.
    if let Some(after_open) = trimmed.strip_prefix("```") {
        // Drop the optional language tag on the opening fence line.
        let body = match after_open.split_once('\n') {
            Some((_lang, rest)) => rest,
            None => after_open,
        };
        if let Some((inside, _)) = body.rsplit_once("```") {
            return inside.trim();
        }
    }

    // No fence — try to find the first JSON object/array opener.
    let first_brace = trimmed.find('{');
    let first_bracket = trimmed.find('[');
    let start = match (first_brace, first_bracket) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    match start {
        Some(0) => trimmed,
        Some(idx) => trimmed[idx..].trim(),
        None => trimmed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_passthrough_when_clean() {
        assert_eq!(strip_json_envelope(r#"["a","b"]"#), r#"["a","b"]"#);
        assert_eq!(strip_json_envelope(r#"{"k":1}"#), r#"{"k":1}"#);
    }

    #[test]
    fn envelope_strips_markdown_fence() {
        let raw = "```json\n[\"a\",\"b\"]\n```";
        assert_eq!(strip_json_envelope(raw), r#"["a","b"]"#);
    }

    #[test]
    fn envelope_strips_unlabelled_fence() {
        let raw = "```\n{\"k\":1}\n```";
        assert_eq!(strip_json_envelope(raw), r#"{"k":1}"#);
    }

    #[test]
    fn envelope_strips_leading_prose() {
        let raw = "Sure! Here is the JSON you asked for:\n[\"a\"]";
        assert_eq!(strip_json_envelope(raw), r#"["a"]"#);
    }

    // ----- relax_pythonish_quotes / parse_pass_json -----

    #[test]
    fn relax_returns_none_when_no_single_quote() {
        // Strict-JSON candidates short-circuit so the strict error
        // surfaces verbatim for triage.
        assert_eq!(relax_pythonish_quotes(r#"["a","b"]"#), None);
        assert_eq!(relax_pythonish_quotes(r#"{"k":1}"#), None);
        assert_eq!(relax_pythonish_quotes("definitely not json"), None);
    }

    #[test]
    fn relax_rewrites_simple_python_array() {
        let input = r#"['a','b']"#;
        assert_eq!(
            relax_pythonish_quotes(input).as_deref(),
            Some(r#"["a","b"]"#)
        );
        // Round-trip via serde_json to lock the contract end-to-end.
        let relaxed = relax_pythonish_quotes(input).unwrap();
        let parsed: Vec<String> = serde_json::from_str(&relaxed).unwrap();
        assert_eq!(parsed, vec!["a", "b"]);
    }

    #[test]
    fn relax_preserves_embedded_apostrophe_in_double_quoted_string() {
        // Mixed input: first element is strict JSON containing `'`,
        // second is Python-style. The relaxer must NOT touch the
        // apostrophe inside the strict element.
        let input = r#"["I've got one",'two']"#;
        let relaxed = relax_pythonish_quotes(input).unwrap();
        let parsed: Vec<String> = serde_json::from_str(&relaxed).unwrap();
        assert_eq!(parsed, vec!["I've got one", "two"]);
    }

    #[test]
    fn relax_escapes_bare_double_quote_inside_python_string() {
        // Python: 'title is "Get a New Computer"' must become JSON:
        // "title is \"Get a New Computer\"".
        let input = r#"['title is "Get a New Computer"']"#;
        let relaxed = relax_pythonish_quotes(input).unwrap();
        let parsed: Vec<String> = serde_json::from_str(&relaxed).unwrap();
        assert_eq!(parsed, vec![r#"title is "Get a New Computer""#]);
    }

    #[test]
    fn relax_reproduces_dictation_143_failure_shape() {
        // The literal raw output from mb-y390 / dictation #143's
        // 4-attempt deterministic segmenter failure. Strict
        // serde_json rejects this at column 46 (the `'` of
        // `'title is`). The relaxed rewrite must round-trip cleanly.
        let raw = r#"["I've got a test project I need to create", 'title is "Get a New Computer"', 'point is to get one that is good enough to run local large language models that are actually large enough to be useful']"#;

        // Strict-first MUST fail (the bug).
        assert!(
            serde_json::from_str::<Vec<String>>(raw).is_err(),
            "dictation #143 raw should fail strict JSON parse",
        );

        // Relaxed-second MUST succeed and yield three coherent strings.
        let relaxed = relax_pythonish_quotes(raw).expect("relaxer must apply");
        let parsed: Vec<String> =
            serde_json::from_str(&relaxed).expect("relaxed output must be strict JSON");
        assert_eq!(parsed.len(), 3);
        assert!(parsed[0].starts_with("I've got a test project"));
        assert_eq!(parsed[1], r#"title is "Get a New Computer""#);
        assert!(parsed[2].contains("local large language models"));
    }

    #[test]
    fn relax_handles_python_backslash_quote_escape() {
        // Python: 'it\'s fine' -> JSON: "it's fine".
        let input = r#"['it\'s fine']"#;
        let relaxed = relax_pythonish_quotes(input).unwrap();
        let parsed: Vec<String> = serde_json::from_str(&relaxed).unwrap();
        assert_eq!(parsed, vec!["it's fine"]);
    }

    #[test]
    fn relax_returns_none_on_unterminated_string() {
        // No closing quote -- relaxer bails so the strict error
        // remains the authoritative diagnostic.
        assert_eq!(relax_pythonish_quotes("['oops"), None);
    }

    #[test]
    fn parse_pass_json_passes_through_strict_json_unchanged() {
        let raw = r#"["a","b"]"#;
        let v: Vec<String> = parse_pass_json(raw, raw, "test").unwrap();
        assert_eq!(v, vec!["a", "b"]);
    }

    #[test]
    fn parse_pass_json_recovers_from_python_quotes() {
        let raw = r#"['a','b']"#;
        let v: Vec<String> = parse_pass_json(raw, raw, "test").unwrap();
        assert_eq!(v, vec!["a", "b"]);
    }

    #[test]
    fn parse_pass_json_propagates_strict_error_when_relaxation_cant_help() {
        // No `'` at all -- relaxer short-circuits with None; the
        // original strict error message + raw text propagate.
        let raw = "this is not json";
        let err = parse_pass_json::<Vec<String>>(raw, raw, "test").unwrap_err();
        match err {
            PassError::Parse {
                pass,
                error: _,
                raw: surfaced,
            } => {
                assert_eq!(pass, "test");
                assert_eq!(surfaced, raw);
            }
            other => panic!("expected PassError::Parse, got {other:?}"),
        }
    }
}
