//! Classify pass — assigns spec §7.2 Layer 1 + Layer 2 to one
//! segment.

use serde::{Deserialize, Serialize};

use crate::ollama::{GenerateOptions, OllamaDispatcher};
use crate::passes::{strip_json_envelope, PassError};
use crate::schema::{Category, EntryType};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Classification {
    pub category: Category,
    pub entry_type: EntryType,
}

/// `prompt_body` is the verbatim contents of `prompts/classify.md`
/// as loaded by `crate::schema_loader::Schema` (ADR 0049 Move 1).
/// The runtime prompt is byte-identical to the pre-refactor form.
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
    let parsed: Classification = serde_json::from_str(candidate).map_err(|e| PassError::Parse {
        pass: "classify",
        error: e.to_string(),
        raw: raw.clone(),
    })?;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ollama::testing::MockOllama;

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
