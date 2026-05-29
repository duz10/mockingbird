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
}
