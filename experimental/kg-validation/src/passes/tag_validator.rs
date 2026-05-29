//! Closed-vocabulary tag validator — Wave 0.5.3 / `mb-rzpd`.
//!
//! Takes raw extract output (`raw_topic_tags` + `proposed_new_tags`)
//! and splits it into two channels:
//!
//! 1. `validated_tags` — normalized tags from `raw_topic_tags` that
//!    survive vocabulary membership. These land on the Entry.
//! 2. `new_tag_requests` — every tag the model wanted but the
//!    vocabulary did not contain. Two sources, both logged:
//!    - **explicit**: the model used the `proposed_new_tags` JSON
//!      field as instructed.
//!    - **implicit**: the model emitted an out-of-vocab tag inside
//!      `raw_topic_tags`, forgetting the protocol. The validator is
//!      forgiving: out-of-vocab tags are dropped from the entry but
//!      logged as if they had been proposed (no rationale available).
//!
//! Per ADR 0049 §Move 3 / Wave 0.5.3, this is the load-bearing piece
//! that closes Gap 4 (tag-collapse). The vocabulary itself lives in
//! `SCHEMA.md` § "Canonical tag vocabulary"; the loader exposes it
//! via [`crate::schema_loader::Schema::canonical_tag_vocabulary`].
//!
//! The validator is pure Rust, no LLM, deterministic. Unit tests
//! cover the four cases that matter:
//! - Pure in-vocab → all pass through.
//! - Out-of-vocab in `raw_topic_tags` → dropped, logged implicit.
//! - Out-of-vocab in `proposed_new_tags` → never on entry, logged
//!   explicit.
//! - Normalization-first: `Kids` and `kids` normalize to `kid`, then
//!   match against `kid` in the vocab (the vocab is the
//!   post-normalization canonical form).

use std::collections::HashSet;

use serde::Serialize;

use crate::passes::extract::{Extraction, ProposedNewTag};
use crate::passes::normalize_tags;

/// Source of a new-tag-request. The distinction matters for the
/// run-end report: explicit requests are higher-signal (the model
/// reasoned about the gap), implicit ones are noise from the model
/// either forgetting the protocol or genuinely thinking an
/// out-of-vocab tag was valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NewTagRequestSource {
    /// Model used the `proposed_new_tags` JSON field per protocol.
    Explicit,
    /// Model emitted an out-of-vocab tag inside `raw_topic_tags`.
    Implicit,
}

/// One new-tag-request, post-normalization. The `tag` is the
/// post-normalize form so the run-end aggregator can dedupe by
/// canonical form even if the model emitted varying surface forms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewTagRequest {
    pub tag: String,
    pub rationale: String,
    pub source: NewTagRequestSource,
}

/// Result of validating one segment's extract output against the
/// closed vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagValidationResult {
    /// Normalized + in-vocab tags. Order preserved from the original
    /// raw_topic_tags (first-seen-wins after normalization, just like
    /// [`normalize_tags`]).
    pub validated_tags: Vec<String>,
    /// New-tag-requests, in the order they appeared (implicit ones
    /// from raw_topic_tags first, then explicit ones from
    /// proposed_new_tags).
    pub new_tag_requests: Vec<NewTagRequest>,
}

/// Validate one extract output against the closed vocabulary.
///
/// The vocabulary is taken as a slice of canonical strings (already
/// in post-normalize form per SCHEMA.md). The validator normalizes
/// raw_topic_tags via [`normalize_tags`] first, then checks
/// vocabulary membership — so `Kids` and `kid` both round-trip to
/// `kid` and match if the vocabulary contains `kid`.
pub fn validate_tags(extraction: &Extraction, vocabulary: &HashSet<String>) -> TagValidationResult {
    let normalized = normalize_tags(&extraction.raw_topic_tags);

    let mut validated_tags: Vec<String> = Vec::with_capacity(normalized.len());
    let mut new_tag_requests: Vec<NewTagRequest> = Vec::new();

    for tag in &normalized {
        if vocabulary.contains(tag) {
            validated_tags.push(tag.clone());
        } else {
            new_tag_requests.push(NewTagRequest {
                tag: tag.clone(),
                rationale: String::new(),
                source: NewTagRequestSource::Implicit,
            });
        }
    }

    if let Some(proposed) = extraction.proposed_new_tags.as_ref() {
        // Normalize each proposed tag too — the model emits them in
        // free form (could be capitalized, plural, etc.) but we want
        // the run-end aggregator to dedupe by canonical form.
        for ProposedNewTag { tag, rationale } in proposed {
            let normalized_tag = normalize_tags(std::slice::from_ref(tag));
            // normalize_tags drops empty strings; skip if the model
            // emitted whitespace-only or empty.
            let Some(canonical) = normalized_tag.into_iter().next() else {
                continue;
            };
            // If the proposed tag IS actually in vocab (model was
            // overcautious), promote it to validated rather than
            // logging a bogus request. This matches the kickoff's
            // "forgiving the model" disposition.
            if vocabulary.contains(&canonical) {
                if !validated_tags.iter().any(|t| t == &canonical) {
                    validated_tags.push(canonical);
                }
                continue;
            }
            new_tag_requests.push(NewTagRequest {
                tag: canonical,
                rationale: rationale.clone(),
                source: NewTagRequestSource::Explicit,
            });
        }
    }

    TagValidationResult {
        validated_tags,
        new_tag_requests,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocab(entries: &[&str]) -> HashSet<String> {
        entries.iter().map(|s| (*s).to_string()).collect()
    }

    fn extraction(raw: &[&str], proposed: Option<Vec<(&str, &str)>>) -> Extraction {
        Extraction {
            title: "t".to_string(),
            due_iso: None,
            raw_topic_tags: raw.iter().map(|s| (*s).to_string()).collect(),
            proposed_new_tags: proposed.map(|v| {
                v.into_iter()
                    .map(|(t, r)| ProposedNewTag {
                        tag: t.to_string(),
                        rationale: r.to_string(),
                    })
                    .collect()
            }),
        }
    }

    #[test]
    fn pure_in_vocab_passes_through() {
        let v = vocab(&["work", "budget"]);
        let r = validate_tags(&extraction(&["work", "budget"], None), &v);
        assert_eq!(r.validated_tags, vec!["work", "budget"]);
        assert!(r.new_tag_requests.is_empty());
    }

    #[test]
    fn out_of_vocab_raw_tag_is_dropped_and_logged_implicit() {
        let v = vocab(&["work"]);
        let r = validate_tags(&extraction(&["work", "q3-roadmap"], None), &v);
        assert_eq!(r.validated_tags, vec!["work"]);
        assert_eq!(r.new_tag_requests.len(), 1);
        assert_eq!(r.new_tag_requests[0].tag, "q3-roadmap");
        assert_eq!(r.new_tag_requests[0].source, NewTagRequestSource::Implicit);
        assert!(r.new_tag_requests[0].rationale.is_empty());
    }

    #[test]
    fn explicit_proposed_tag_never_lands_on_entry() {
        let v = vocab(&["work"]);
        let r = validate_tags(
            &extraction(
                &["work"],
                Some(vec![("q3-roadmap", "recurring planning concept")]),
            ),
            &v,
        );
        assert_eq!(r.validated_tags, vec!["work"]);
        assert_eq!(r.new_tag_requests.len(), 1);
        assert_eq!(r.new_tag_requests[0].tag, "q3-roadmap");
        assert_eq!(r.new_tag_requests[0].source, NewTagRequestSource::Explicit);
        assert_eq!(
            r.new_tag_requests[0].rationale,
            "recurring planning concept"
        );
    }

    #[test]
    fn normalization_runs_before_vocab_check() {
        // Vocabulary holds the post-normalize form `kid`. Model emits
        // `Kids` (capitalized, plural). Normalization → `kid`, then
        // vocab match → keep.
        let v = vocab(&["kid"]);
        let r = validate_tags(&extraction(&["Kids"], None), &v);
        assert_eq!(r.validated_tags, vec!["kid"]);
        assert!(r.new_tag_requests.is_empty());
    }

    #[test]
    fn overcautious_proposed_tag_already_in_vocab_is_promoted() {
        // Model puts `budget` in proposed_new_tags even though
        // `budget` is in vocab. Validator promotes it to validated
        // rather than logging a bogus new-tag-request.
        let v = vocab(&["budget"]);
        let r = validate_tags(
            &extraction(&[], Some(vec![("budget", "central topic")])),
            &v,
        );
        assert_eq!(r.validated_tags, vec!["budget"]);
        assert!(r.new_tag_requests.is_empty());
    }

    #[test]
    fn promoted_proposed_tag_does_not_duplicate_already_validated_tag() {
        // Model emits `budget` in BOTH raw_topic_tags AND
        // proposed_new_tags. Validator must not double-add.
        let v = vocab(&["budget"]);
        let r = validate_tags(&extraction(&["budget"], Some(vec![("budget", "huh")])), &v);
        assert_eq!(r.validated_tags, vec!["budget"]);
        assert!(r.new_tag_requests.is_empty());
    }

    #[test]
    fn mixed_case_implicit_and_explicit_preserves_order() {
        let v = vocab(&["a"]);
        let r = validate_tags(
            &extraction(
                &["a", "b", "c"],
                Some(vec![("d", "rationale-d"), ("e", "rationale-e")]),
            ),
            &v,
        );
        assert_eq!(r.validated_tags, vec!["a"]);
        // Order: implicit (b, c) first, then explicit (d, e).
        assert_eq!(r.new_tag_requests.len(), 4);
        let tags: Vec<&str> = r.new_tag_requests.iter().map(|n| n.tag.as_str()).collect();
        assert_eq!(tags, vec!["b", "c", "d", "e"]);
        assert_eq!(r.new_tag_requests[0].source, NewTagRequestSource::Implicit);
        assert_eq!(r.new_tag_requests[1].source, NewTagRequestSource::Implicit);
        assert_eq!(r.new_tag_requests[2].source, NewTagRequestSource::Explicit);
        assert_eq!(r.new_tag_requests[3].source, NewTagRequestSource::Explicit);
    }

    #[test]
    fn empty_proposed_new_tags_field_is_treated_as_none() {
        // Model emits `"proposed_new_tags": []` — explicitly empty.
        // Should behave identically to `proposed_new_tags = None`.
        let v = vocab(&["work"]);
        let r = validate_tags(&extraction(&["work"], Some(vec![])), &v);
        assert_eq!(r.validated_tags, vec!["work"]);
        assert!(r.new_tag_requests.is_empty());
    }

    #[test]
    fn whitespace_only_proposed_tag_is_skipped() {
        let v = vocab(&["work"]);
        let r = validate_tags(&extraction(&["work"], Some(vec![("   ", "rationale")])), &v);
        assert_eq!(r.validated_tags, vec!["work"]);
        assert!(r.new_tag_requests.is_empty());
    }
}
