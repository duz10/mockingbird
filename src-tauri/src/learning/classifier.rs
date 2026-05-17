//! LLM-driven classification of a `(before_text, after_text)` correction.
//!
//! Categories (binding):
//!
//! | Category          | Meaning                                              | Action by promoter |
//! |-------------------|------------------------------------------------------|--------------------|
//! | `new_vocab`       | User changed a misspelled proper noun / jargon term | Insert into `dictionary` |
//! | `style_change`    | User reworded for tone, flow, or formality          | Insert (raw → final) into `style_examples` |
//! | `mistranscription`| Whisper just heard the wrong word                   | No-op (already corrected; no learning needed) |
//! | `noise`           | Junk correction (typo of a typo, etc.) — ignore     | No-op |
//!
//! The classifier uses the same [`CleanupProvider`] trait as Phase 4
//! — no new infrastructure. The prompt + parse is deterministic for
//! a given provider output, so a unit test with [`StubClassifier`]
//! covers the entire decision tree.
//!
//! ## Why an enum (not a free-form string)
//!
//! Type-checked at compile time + makes the `promoter` dispatch a
//! `match` rather than a string comparison. The `as_str` mapping is
//! the single source of truth — the SQL `corrections.classification`
//! column carries the same canonical strings.

use crate::cleanup::provider::{CleanupProvider, CleanupRequest};
use crate::error::AppResult;
use crate::learning::corrections::Correction;

/// One of four classification outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    /// User fixed a proper-noun / jargon spelling.
    NewVocab,
    /// User changed the wording for style / tone.
    StyleChange,
    /// Whisper misheard; the cleaned text was wrong; the correction
    /// is correct but we have nothing to learn (no pattern to extract).
    Mistranscription,
    /// Garbage correction; do not act on it.
    Noise,
}

impl Classification {
    /// Canonical string for the `corrections.classification` column.
    pub fn as_str(self) -> &'static str {
        match self {
            Classification::NewVocab => "new_vocab",
            Classification::StyleChange => "style_change",
            Classification::Mistranscription => "mistranscription",
            Classification::Noise => "noise",
        }
    }

    /// Parse from the canonical string. Unknown → `Noise`
    /// (conservative — never act on something we don't understand).
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "new_vocab" => Classification::NewVocab,
            "style_change" => Classification::StyleChange,
            "mistranscription" => Classification::Mistranscription,
            _ => Classification::Noise,
        }
    }
}

/// Classify a single correction via an LLM provider.
pub trait Classifier: Send {
    /// Return the classification for the given correction. Errors
    /// propagate to the runner which records them in `learning_runs.notes`.
    fn classify(&mut self, correction: &Correction) -> AppResult<Classification>;
}

/// LLM-backed classifier. Wraps any [`CleanupProvider`] and shapes
/// the prompt + parses the response.
pub struct LlmClassifier {
    provider: Box<dyn CleanupProvider>,
    model_id: String,
}

impl LlmClassifier {
    /// Construct.
    pub fn new(provider: Box<dyn CleanupProvider>, model_id: String) -> Self {
        Self { provider, model_id }
    }
}

/// The classification prompt template. Filled with `{BEFORE}` and
/// `{AFTER}` per correction. Frozen by ADR 0008 once Phase 8 ships.
const CLASSIFY_PROMPT: &str = "\
You are a learning-loop classifier for Mockingbird, a voice dictation
app. A user just corrected something Mockingbird typed.

The text we typed:
<<<
{BEFORE}
>>>

The text the user changed it to:
<<<
{AFTER}
>>>

Classify the correction with EXACTLY ONE of these labels (lowercase,
no punctuation, no explanation):

- new_vocab        — they fixed a misspelled proper noun, brand, or
                     domain-specific term that's likely to recur
- style_change     — they reworded for tone, flow, or formality
                     without changing the literal meaning
- mistranscription — the speech-to-text just heard wrong; no pattern
                     to extract
- noise            — meaningless edit, typo of a typo, or unclear

Output: the label only.
";

impl Classifier for LlmClassifier {
    fn classify(&mut self, c: &Correction) -> AppResult<Classification> {
        let prompt = CLASSIFY_PROMPT
            .replace("{BEFORE}", &c.before_text)
            .replace("{AFTER}", &c.after_text);
        let result = self.provider.cleanup(CleanupRequest {
            prompt: &prompt,
            raw_transcript: &c.before_text,
            model_id: &self.model_id,
            temperature: 0.0,
            max_tokens: 16,
            mode_slug: "classify",
        })?;
        Ok(Classification::parse(&result.text))
    }
}

// --------------------------------------------------------------------
// Deterministic classifier for tests + the `mb-learning-eval` judge.
// --------------------------------------------------------------------

/// Test-only classifier with a hand-tuned heuristic.
///
/// Rules (in order):
///
/// 1. If `before` and `after` differ in fewer than 2 characters but
///    both are single words → `Mistranscription`.
/// 2. If `before` and `after` are single words AND `after` has a
///    capital letter or non-ASCII char → `NewVocab` (looks like a
///    proper noun).
/// 3. If `after.len() > before.len() * 1.3` OR `after.len() < before.len() / 1.3`
///    → `StyleChange`.
/// 4. Otherwise → `Noise`.
///
/// Good enough to exercise the runner's dispatch logic + the eval
/// pass without standing up Ollama in CI.
pub struct HeuristicClassifier;

impl Classifier for HeuristicClassifier {
    fn classify(&mut self, c: &Correction) -> AppResult<Classification> {
        let before = c.before_text.trim();
        let after = c.after_text.trim();
        let before_words = before.split_whitespace().count();
        let after_words = after.split_whitespace().count();

        if before_words == 1 && after_words == 1 {
            // Rule 1 (HIGHER PRIORITY): proper-noun shape wins over
            // edit-distance, because "mockingbird" → "Mockingbird"
            // looks like a 1-char edit but is semantically vocab.
            // Trigger: after has an uppercase letter or non-ASCII char
            // that does NOT appear at the same position in before.
            let after_chars: Vec<char> = after.chars().collect();
            let before_chars: Vec<char> = before.chars().collect();
            let added_caps = after_chars.iter().enumerate().any(|(i, &c)| {
                (c.is_uppercase() || !c.is_ascii()) && before_chars.get(i).copied() != Some(c)
            });
            if added_caps {
                return Ok(Classification::NewVocab);
            }
            // Rule 2: tiny lowercase-only edit → mistranscription.
            let dist = char_distance(before, after);
            if dist > 0 && dist < 2 {
                return Ok(Classification::Mistranscription);
            }
        }

        // Rule 3: length ratio drift => style change.
        let before_len = before.len() as f64;
        let after_len = after.len() as f64;
        if before_len > 0.0 && (after_len > before_len * 1.3 || after_len * 1.3 < before_len) {
            return Ok(Classification::StyleChange);
        }

        Ok(Classification::Noise)
    }
}

/// Minimal char-edit count (NOT full Levenshtein; just same-position
/// mismatches up to the shorter length + the suffix-length delta).
/// Good enough for the heuristic.
fn char_distance(a: &str, b: &str) -> usize {
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let common = ac.len().min(bc.len());
    let mut diff = 0;
    for i in 0..common {
        if ac[i] != bc[i] {
            diff += 1;
        }
    }
    diff + ac.len().abs_diff(bc.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn correction(before: &str, after: &str) -> Correction {
        Correction {
            id: 1,
            session_id: 1,
            before_text: before.into(),
            after_text: after.into(),
            detection_method: "manual".into(),
            classification: None,
            classified_at: None,
            created_at: "2026".into(),
        }
    }

    #[test]
    fn classification_str_round_trips() {
        for v in [
            Classification::NewVocab,
            Classification::StyleChange,
            Classification::Mistranscription,
            Classification::Noise,
        ] {
            assert_eq!(Classification::parse(v.as_str()), v);
        }
    }

    #[test]
    fn classification_unknown_string_falls_to_noise() {
        assert_eq!(Classification::parse("garbage"), Classification::Noise);
        assert_eq!(Classification::parse(""), Classification::Noise);
        // Whitespace + case-normalisation works, but the underscore
        // is part of the canonical form — a space-separated variant
        // does NOT parse.
        assert_eq!(
            Classification::parse("  NEW_VOCAB \n"),
            Classification::NewVocab
        );
        assert_eq!(Classification::parse("New Vocab"), Classification::Noise);
    }

    #[test]
    fn heuristic_proper_noun_is_new_vocab() {
        let mut c = HeuristicClassifier;
        let got = c
            .classify(&correction("mockingbird", "Mockingbird"))
            .unwrap();
        assert_eq!(got, Classification::NewVocab);
    }

    #[test]
    fn heuristic_one_letter_typo_is_mistranscription() {
        let mut c = HeuristicClassifier;
        let got = c.classify(&correction("kubectl", "kubectk")).unwrap();
        assert_eq!(got, Classification::Mistranscription);
    }

    #[test]
    fn heuristic_long_reword_is_style_change() {
        let mut c = HeuristicClassifier;
        let got = c
            .classify(&correction(
                "hi",
                "Hello there my dear friend, I hope you are well",
            ))
            .unwrap();
        assert_eq!(got, Classification::StyleChange);
    }

    #[test]
    fn heuristic_similar_length_unclear_is_noise() {
        let mut c = HeuristicClassifier;
        let got = c
            .classify(&correction("hello world", "hello world!"))
            .unwrap();
        assert_eq!(got, Classification::Noise);
    }

    /// LlmClassifier wraps a provider. We don't have a recording stub
    /// for classifier-shaped output, so this test confirms the
    /// provider's response gets parsed correctly via a custom stub.
    struct FixedProvider {
        response: &'static str,
    }
    impl CleanupProvider for FixedProvider {
        fn cleanup(
            &self,
            _req: CleanupRequest<'_>,
        ) -> AppResult<crate::cleanup::provider::CleanupResult> {
            Ok(crate::cleanup::provider::CleanupResult {
                text: self.response.into(),
                model_used: "fixed".into(),
                latency_ms: 0,
                input_tokens: None,
                output_tokens: None,
            })
        }
        fn provider_name(&self) -> &'static str {
            "fixed"
        }
        fn supports_model(&self, _: &str) -> bool {
            true
        }
    }

    #[test]
    fn llm_classifier_parses_provider_response() {
        let c = correction("hello", "Hi");
        let mut clf = LlmClassifier::new(
            Box::new(FixedProvider {
                response: "style_change",
            }),
            "m".into(),
        );
        assert_eq!(clf.classify(&c).unwrap(), Classification::StyleChange);
    }

    #[test]
    fn llm_classifier_handles_trailing_whitespace() {
        let c = correction("hi", "Hi");
        let mut clf = LlmClassifier::new(
            Box::new(FixedProvider {
                response: "  new_vocab\n",
            }),
            "m".into(),
        );
        assert_eq!(clf.classify(&c).unwrap(), Classification::NewVocab);
    }
}
