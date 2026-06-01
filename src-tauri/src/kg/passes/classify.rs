//! Classify pass — assigns PLAN §7.2 Layer 1 + Layer 2 to one segment.
//!
//! Graduated verbatim from the sandbox under Wave 2 Task 6
//! (`mb-rtk2`) — import-path rewrites only.

use serde::{Deserialize, Serialize};

use super::super::ollama::{GenerateOptions, OllamaDispatcher};
use super::super::schema::{Category, EntryType};
use super::{parse_pass_json, strip_json_envelope, PassError};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Classification {
    pub category: Category,
    pub entry_type: EntryType,
}

/// `prompt_body` is the verbatim contents of `prompts/classify.md`
/// as loaded by `crate::kg::schema_loader::Schema`. The runtime prompt
/// is byte-identical to the pre-graduation form.
pub fn classify<D: OllamaDispatcher>(
    dispatcher: &D,
    model: &str,
    prompt_body: &str,
    segment: &str,
    options: &GenerateOptions,
) -> Result<Classification, PassError> {
    let prompt = format!("{prompt_body}\n\nSEGMENT:\n{segment}\n");
    let raw = dispatcher.generate(model, &prompt, None, options)?;
    let candidate = strip_json_envelope(&raw);
    let parsed: Classification = parse_pass_json(&raw, candidate, "classify")?;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::super::super::ollama::testing::MockOllama;
    use super::*;

    fn opts() -> GenerateOptions {
        GenerateOptions::default()
    }

    const TEST_PROMPT: &str = "dummy classify prompt body";

    #[test]
    fn parses_personal_task() {
        let mock =
            MockOllama::new().default_response(r#"{"category":"personal","entry_type":"task"}"#);
        let r = classify(&mock, "m", TEST_PROMPT, "Pick up Tyler at 5", &opts()).unwrap();
        assert_eq!(r.category, Category::Personal);
        assert_eq!(r.entry_type, EntryType::Task);
    }

    #[test]
    fn parses_objective_idea() {
        let mock =
            MockOllama::new().default_response(r#"{"category":"objective","entry_type":"idea"}"#);
        let r = classify(
            &mock,
            "m",
            TEST_PROMPT,
            "I want to get back into running",
            &opts(),
        )
        .unwrap();
        assert_eq!(r.category, Category::Objective);
        assert_eq!(r.entry_type, EntryType::Idea);
    }

    #[test]
    fn rejects_unknown_variant_with_raw_in_error() {
        let mock = MockOllama::new().default_response(r#"{"category":"work","entry_type":"task"}"#);
        let err = classify(&mock, "m", TEST_PROMPT, "anything", &opts()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("classify"));
        assert!(msg.contains(r#""work""#));
    }
}
