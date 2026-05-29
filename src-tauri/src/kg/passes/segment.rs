//! Segment pass — splits one raw dictation into 0..N candidate
//! entry strings.
//!
//! Per PLAN §7.5 + §8.1: this is the riskiest pass (rambling memos
//! routinely contain 2-5 distinct items) and is weighted heavily by
//! the scorer accordingly. Empty array == junk bucket; that maps
//! directly onto the `AnswerKey::is_junk_no_entry_expected` flag.
//!
//! Graduated verbatim from the sandbox — import-path rewrites only.

use super::super::ollama::{GenerateOptions, OllamaDispatcher};
use super::{strip_json_envelope, PassError};

/// Run the segment pass for one dictation. Returns the (verbatim
/// or lightly stitched) text of each candidate entry. Empty vec is
/// the legitimate "junk" result.
///
/// `prompt_body` is the verbatim contents of `prompts/segment.md`
/// as loaded by `crate::kg::schema_loader::Schema` (ADR 0049
/// Move 1). The runtime prompt is `{prompt_body}{per_pass_context_suffix}`
/// and the suffix below is byte-identical to the pre-refactor form
/// (parity contract pinned by Wave 0.5.1 §parity-gate).
pub fn segment<D: OllamaDispatcher>(
    dispatcher: &D,
    model: &str,
    prompt_body: &str,
    dictation: &str,
    captured_iso: &str,
    options: &GenerateOptions,
) -> Result<Vec<String>, PassError> {
    let prompt =
        format!("{prompt_body}\n\nCONTEXT: captured at {captured_iso}.\nDICTATION:\n{dictation}\n");

    let raw = dispatcher.generate(model, &prompt, None, options)?;
    let candidate = strip_json_envelope(&raw);

    let parsed: Vec<String> = serde_json::from_str(candidate).map_err(|e| PassError::Parse {
        pass: "segment",
        error: e.to_string(),
        raw: raw.clone(),
    })?;

    // Light validation: drop empty / whitespace-only strings — the
    // model occasionally pads its output with stray `""` entries.
    Ok(parsed
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::super::super::ollama::testing::MockOllama;
    use super::*;

    fn opts() -> GenerateOptions {
        GenerateOptions {
            temperature: 0.2,
            seed: Some(42),
            num_ctx: 4096,
        }
    }

    const TEST_PROMPT: &str = "dummy segment prompt body";

    #[test]
    fn returns_three_entries_on_enumerated_multi_item() {
        let mock = MockOllama::new().default_response(
            r#"["finalize the budget by Thursday","ping Marcus about the contractor invoice","I had an idea about running the standup over video"]"#,
        );
        let r = segment(
            &mock,
            "test-model",
            TEST_PROMPT,
            "Okay so three things. One, finalize the budget by Thursday. Two, ping Marcus about the contractor invoice. And three, I had an idea — what if we ran the standup over video instead of in person.",
            "2026-06-14T08:00:00Z",
            &opts(),
        )
        .unwrap();
        assert_eq!(r.len(), 3);
        assert!(r[0].contains("finalize the budget"));
        assert!(r[1].contains("Marcus"));
        assert!(r[2].contains("video"));
    }

    #[test]
    fn returns_empty_for_junk() {
        let mock = MockOllama::new().default_response("[]");
        let r = segment(
            &mock,
            "test-model",
            TEST_PROMPT,
            "Uh hold on, I need to remember to... actually no, never mind.",
            "2026-06-14T08:00:00Z",
            &opts(),
        )
        .unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn does_not_split_on_internal_debate() {
        // Mock returns a single entry containing the OR-clause as proof
        // the test exercises the "don't split on internal debate" rule
        // (the prompt teaches it; the test pins that a single result
        // is honoured end-to-end without the parser fragmenting it).
        let mock = MockOllama::new().default_response(
            r#"["maybe rewrite the homepage hero, OR maybe just swap the photo"]"#,
        );
        let r = segment(
            &mock,
            "test-model",
            TEST_PROMPT,
            "Thinking about the homepage. Maybe rewrite the hero, or maybe just swap the photo.",
            "2026-06-14T08:00:00Z",
            &opts(),
        )
        .unwrap();
        assert_eq!(r.len(), 1);
        assert!(r[0].contains("OR maybe"));
    }

    #[test]
    fn tolerates_markdown_fenced_output() {
        let mock = MockOllama::new().default_response("```json\n[\"call dentist\"]\n```");
        let r = segment(
            &mock,
            "test-model",
            TEST_PROMPT,
            "Call the dentist.",
            "2026-06-14T08:00:00Z",
            &opts(),
        )
        .unwrap();
        assert_eq!(r, vec!["call dentist"]);
    }

    #[test]
    fn surfaces_parse_error_with_raw_output() {
        let mock = MockOllama::new().default_response("definitely not json");
        let err = segment(
            &mock,
            "test-model",
            TEST_PROMPT,
            "anything",
            "2026-06-14T08:00:00Z",
            &opts(),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("segment"), "missing pass tag: {msg}");
        assert!(msg.contains("definitely not json"), "missing raw: {msg}");
    }
}
