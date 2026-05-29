//! Wave 2 — the four pipeline passes (spec §8.1, skipping the
//! Transcribe pass per ADR 0048 §G2).
//!
//! Each pass is a free function over an `OllamaDispatcher` so unit
//! tests can swap in a canned mock without touching network code.
//! Output JSON parsing lives inside each pass — the dispatcher trait
//! deliberately returns raw text so the per-pass parse error can
//! capture the offending model output for Wave 3 scoring (see
//! [`PassError::Parse`]).

pub mod classify;
pub mod extract;
pub mod normalize;
pub mod segment;
pub mod tag_validator;

pub use classify::{classify, Classification};
pub use extract::{extract, Extraction, ProposedNewTag};
pub use normalize::normalize_tags;
pub use segment::segment;
pub use tag_validator::{validate_tags, NewTagRequest, NewTagRequestSource, TagValidationResult};

/// Error type shared by all passes. The raw model output is captured
/// on `Parse` / `Validation` so the harness can write it to disk for
/// downstream inspection — Wave 3's scorer and the human reading the
/// run report both need to see what the model actually said when it
/// failed.
#[derive(Debug, thiserror::Error)]
pub enum PassError {
    #[error("dispatcher error: {0}")]
    Dispatcher(#[from] anyhow::Error),

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

/// Best-effort JSON extraction: strips ```` ```json ... ``` ```` code
/// fences and any prose before the first `{` or `[`. Small local models
/// chronically wrap their JSON in markdown despite explicit
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
}
