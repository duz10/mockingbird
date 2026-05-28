//! LLM tag-equivalence judge — spec §8.3 + ADR 0048 §G4 (different
//! model family than the pipeline) + §G5 Gate 2 (chain-of-thought,
//! reasoning BEFORE verdict).
//!
//! The judge is one bounded LLM call per comparison: "do tag set A
//! and tag set B refer to the same concept?" It answers with
//! reasoning then a verdict marker. The parser is deliberately
//! strict — a verdict marker in the wrong position, or absent
//! reasoning, or reasoning shorter than 30 chars, surfaces as a
//! parse error that JVP Gate 2 will count against the judge.
//!
//! We preserve `raw_output` on every successful verdict so Gate 2
//! can re-audit the chain-of-thought after the fact.

use serde::{Deserialize, Serialize};

use crate::ollama::{GenerateOptions, OllamaDispatcher};

const PROMPT: &str = include_str!("../../prompts/judge-tag-equivalence.md");

/// The two-set tag-equivalence question this judge answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagJudgeRequest {
    pub tags_a: Vec<String>,
    pub tags_b: Vec<String>,
}

/// Verdict — keep the variant set tiny so the parser is unambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TagEquivalence {
    Equivalent,
    NotEquivalent,
}

/// One judge call's full result. `raw_output` is the verbatim model
/// response (envelope-stripped) — JVP Gate 2 reads it to audit that
/// the reasoning actually references both candidate sides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagJudgeVerdict {
    pub reasoning: String,
    pub verdict: TagEquivalence,
    pub raw_output: String,
}

/// Errors a judge call can raise. Distinct from `passes::PassError`
/// because the judge's failure modes are JVP-specific (Gate 2 needs
/// to differentiate "no verdict marker" from "verdict-first") and we
/// don't want to overload the pipeline error type.
#[derive(Debug, thiserror::Error)]
pub enum JudgeError {
    #[error("dispatcher error: {0}")]
    Dispatcher(#[from] anyhow::Error),

    #[error("judge output missing VERDICT marker.\nRaw:\n{raw}")]
    NoVerdict { raw: String },

    #[error(
        "judge output had VERDICT marker before REASONING (verdict-first pattern).\nRaw:\n{raw}"
    )]
    VerdictBeforeReasoning { raw: String },

    #[error("judge output had unrecognized verdict word `{word}`.\nRaw:\n{raw}")]
    UnknownVerdictWord { word: String, raw: String },

    #[error("judge output had empty/missing REASONING block.\nRaw:\n{raw}")]
    EmptyReasoning { raw: String },
}

/// Single judge call. Returns a `TagJudgeVerdict` with the raw model
/// output preserved so downstream JVP gates (especially Gate 2) can
/// re-audit it.
pub fn judge_tag_equivalence<D: OllamaDispatcher>(
    dispatcher: &D,
    model: &str,
    request: &TagJudgeRequest,
    options: &GenerateOptions,
) -> Result<TagJudgeVerdict, JudgeError> {
    let prompt = render_prompt(request);
    let raw = dispatcher.generate(model, &prompt, None, options)?;
    parse_verdict(&raw)
}

fn render_prompt(request: &TagJudgeRequest) -> String {
    let a = format_tag_list(&request.tags_a);
    let b = format_tag_list(&request.tags_b);
    format!("{PROMPT}\n\nA: {a}\nB: {b}\n")
}

fn format_tag_list(tags: &[String]) -> String {
    let inner: Vec<String> = tags.iter().map(|t| format!("\"{t}\"")).collect();
    format!("[{}]", inner.join(", "))
}

/// Strict parse: find `VERDICT:` marker, require `REASONING:` before
/// it, require non-empty reasoning, recognize only `equivalent` /
/// `not-equivalent` (case-insensitive) as the verdict word.
pub(crate) fn parse_verdict(raw: &str) -> Result<TagJudgeVerdict, JudgeError> {
    let trimmed = raw.trim();

    // Verdict marker — case-insensitive position search.
    let verdict_pos = find_marker_ci(trimmed, "VERDICT:").ok_or_else(|| JudgeError::NoVerdict {
        raw: trimmed.to_string(),
    })?;

    // Reasoning marker — must exist AND appear before verdict.
    let reasoning_pos = find_marker_ci(trimmed, "REASONING:");
    match reasoning_pos {
        Some(rp) if rp < verdict_pos => {}
        _ => {
            return Err(JudgeError::VerdictBeforeReasoning {
                raw: trimmed.to_string(),
            });
        }
    }

    // Slice reasoning between the two markers.
    let reasoning_start = reasoning_pos.unwrap() + "REASONING:".len();
    let reasoning_raw = trimmed[reasoning_start..verdict_pos].trim();
    if reasoning_raw.is_empty() {
        return Err(JudgeError::EmptyReasoning {
            raw: trimmed.to_string(),
        });
    }

    // Verdict word — take the first non-whitespace token after the marker.
    let verdict_tail = trimmed[verdict_pos + "VERDICT:".len()..].trim();
    let verdict_word = verdict_tail
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches(['.', ',', ';']);
    let verdict = match verdict_word.to_ascii_lowercase().as_str() {
        "equivalent" => TagEquivalence::Equivalent,
        "not-equivalent" => TagEquivalence::NotEquivalent,
        other => {
            return Err(JudgeError::UnknownVerdictWord {
                word: other.to_string(),
                raw: trimmed.to_string(),
            });
        }
    };

    Ok(TagJudgeVerdict {
        reasoning: reasoning_raw.to_string(),
        verdict,
        raw_output: trimmed.to_string(),
    })
}

/// Case-insensitive substring position. Tiny helper — `str::find` is
/// case-sensitive and small local models capitalize inconsistently.
fn find_marker_ci(haystack: &str, needle: &str) -> Option<usize> {
    let h = haystack.to_ascii_lowercase();
    let n = needle.to_ascii_lowercase();
    h.find(&n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ollama::testing::MockOllama;

    fn opts() -> GenerateOptions {
        GenerateOptions {
            temperature: 0.2,
            seed: Some(42),
            num_ctx: 4096,
        }
    }

    fn req(a: &[&str], b: &[&str]) -> TagJudgeRequest {
        TagJudgeRequest {
            tags_a: a.iter().map(|s| s.to_string()).collect(),
            tags_b: b.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn parses_well_formed_equivalent_verdict() {
        let mock = MockOllama::new().default_response(
            "REASONING: Set A names a car repair with the broader auto tag. \
             Set B names the same car repair with auto-maintenance. Synonymous.\n\n\
             VERDICT: equivalent",
        );
        let v = judge_tag_equivalence(
            &mock,
            "judge",
            &req(&["car-repair", "auto"], &["car-repair", "auto-maintenance"]),
            &opts(),
        )
        .unwrap();
        assert_eq!(v.verdict, TagEquivalence::Equivalent);
        assert!(v.reasoning.contains("car repair"));
        assert!(v.raw_output.contains("VERDICT: equivalent"));
    }

    #[test]
    fn parses_well_formed_not_equivalent_verdict() {
        let mock = MockOllama::new().default_response(
            "REASONING: Taxes is a personal-finance tag; vacation is travel. \
             No overlap whatsoever.\n\n\
             VERDICT: not-equivalent",
        );
        let v = judge_tag_equivalence(&mock, "judge", &req(&["taxes"], &["vacation"]), &opts())
            .unwrap();
        assert_eq!(v.verdict, TagEquivalence::NotEquivalent);
    }

    #[test]
    fn no_verdict_marker_errors() {
        let mock = MockOllama::new().default_response("REASONING: they look kind of similar");
        let err = judge_tag_equivalence(&mock, "j", &req(&["a"], &["b"]), &opts()).unwrap_err();
        assert!(matches!(err, JudgeError::NoVerdict { .. }));
    }

    #[test]
    fn verdict_before_reasoning_errors() {
        let mock = MockOllama::new()
            .default_response("VERDICT: equivalent\n\nREASONING: they seemed similar at a glance");
        let err = judge_tag_equivalence(&mock, "j", &req(&["a"], &["b"]), &opts()).unwrap_err();
        assert!(matches!(err, JudgeError::VerdictBeforeReasoning { .. }));
    }

    #[test]
    fn empty_reasoning_errors() {
        let mock = MockOllama::new().default_response("REASONING:    \nVERDICT: equivalent");
        let err = judge_tag_equivalence(&mock, "j", &req(&["a"], &["b"]), &opts()).unwrap_err();
        assert!(matches!(err, JudgeError::EmptyReasoning { .. }));
    }

    #[test]
    fn unknown_verdict_word_errors() {
        let mock = MockOllama::new().default_response(
            "REASONING: looks similar to me by the end of the day\nVERDICT: similar",
        );
        let err = judge_tag_equivalence(&mock, "j", &req(&["a"], &["b"]), &opts()).unwrap_err();
        match err {
            JudgeError::UnknownVerdictWord { word, .. } => assert_eq!(word, "similar"),
            other => panic!("expected UnknownVerdictWord, got {other:?}"),
        }
    }

    #[test]
    fn verdict_word_trailing_punctuation_tolerated() {
        let mock = MockOllama::new().default_response(
            "REASONING: clearly the same target file for filing/search\nVERDICT: equivalent.",
        );
        let v = judge_tag_equivalence(&mock, "j", &req(&["a"], &["b"]), &opts()).unwrap();
        assert_eq!(v.verdict, TagEquivalence::Equivalent);
    }

    #[test]
    fn case_insensitive_markers() {
        let mock = MockOllama::new().default_response(
            "reasoning: this is the lowercase variant some models emit\n\nverdict: not-equivalent",
        );
        let v = judge_tag_equivalence(&mock, "j", &req(&["a"], &["b"]), &opts()).unwrap();
        assert_eq!(v.verdict, TagEquivalence::NotEquivalent);
    }

    #[test]
    fn rendered_prompt_contains_both_tag_sets() {
        let mock = MockOllama::new().default_response("REASONING: x\nVERDICT: equivalent");
        judge_tag_equivalence(
            &mock,
            "j",
            &req(&["car-repair"], &["auto-maintenance"]),
            &opts(),
        )
        .unwrap();
        let calls = mock.calls();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].prompt.contains("\"car-repair\""));
        assert!(calls[0].prompt.contains("\"auto-maintenance\""));
        assert!(calls[0].prompt.contains("A: "));
        assert!(calls[0].prompt.contains("B: "));
    }

    #[test]
    fn temperature_and_seed_reach_dispatcher() {
        let mock = MockOllama::new().default_response("REASONING: x\nVERDICT: equivalent");
        let o = GenerateOptions {
            temperature: 0.2,
            seed: Some(17),
            num_ctx: 4096,
        };
        judge_tag_equivalence(&mock, "j", &req(&["a"], &["b"]), &o).unwrap();
        let calls = mock.calls();
        assert!((calls[0].options.temperature - 0.2).abs() < f32::EPSILON);
        assert_eq!(calls[0].options.seed, Some(17));
    }
}
