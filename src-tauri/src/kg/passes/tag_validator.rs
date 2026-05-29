//! Closed-vocabulary tag validator — Wave 0.5.3 / `mb-rzpd` / `mb-e10v`.
//!
//! Graduated from the sandbox under Wave 2 Task 6 (`mb-rtk2`) —
//! import-path rewrites only. The pipeline order:
//!
//! ```text
//! model output → normalize → synonym-collapse (if map provided) → vocab check → keep/drop → emit
//! ```
//!
//! is load-bearing for the iter-3 fix that restored the open-vocab
//! tag-collapse baseline (LESSONS P11 in the sandbox writeup).
//! Touching the order here regresses Wave 0.5.3 acceptance.
//!
//! Two output channels:
//!
//! 1. `validated_tags` — canonicalized tags from `raw_topic_tags`
//!    that survive vocabulary membership. These land on the Entry.
//! 2. `new_tag_requests` — every tag the model wanted but the
//!    vocabulary did not contain (after canonicalization). Two
//!    sources, both logged:
//!    - **explicit**: model used the `proposed_new_tags` JSON field
//!      as instructed.
//!    - **implicit**: model emitted an out-of-vocab tag inside
//!      `raw_topic_tags`, forgetting the protocol. The validator is
//!      forgiving: out-of-vocab tags are dropped from the entry but
//!      logged as if they had been proposed (no rationale available).
//!
//! The validator is pure Rust, no LLM, deterministic.

use std::collections::HashSet;

use serde::Serialize;

use super::super::synonyms::SynonymMap;
use super::extract::{Extraction, ProposedNewTag};
use super::normalize::normalize_tags;

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

/// One new-tag-request, post-canonicalization. The `tag` is the
/// canonical form so the run-end aggregator can dedupe even if the
/// model emitted varying surface forms.
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
    /// Canonical + in-vocab tags. Order preserved from the original
    /// `raw_topic_tags` (first-seen-wins after canonicalization).
    pub validated_tags: Vec<String>,
    /// New-tag-requests, in the order they appeared (implicit ones
    /// from `raw_topic_tags` first, then explicit ones from
    /// `proposed_new_tags`).
    pub new_tag_requests: Vec<NewTagRequest>,
}

fn canonicalize_ordered(tags: &[String], synonym_map: Option<&SynonymMap>) -> Vec<String> {
    match synonym_map {
        Some(map) => map.canonicalize_ordered(tags),
        None => normalize_tags(tags),
    }
}

fn canonicalize_one(tag: &str, synonym_map: Option<&SynonymMap>) -> String {
    match synonym_map {
        Some(map) => map.canonicalize(tag),
        None => normalize_tags(std::slice::from_ref(&tag.to_string()))
            .into_iter()
            .next()
            .unwrap_or_default(),
    }
}

/// Validate one extract output against the closed vocabulary.
///
/// The vocabulary is taken as a set of canonical strings (already in
/// post-canonicalize form per SCHEMA.md). The validator canonicalizes
/// `raw_topic_tags` first (normalize → synonym-collapse), then checks
/// vocabulary membership — so `automobile-repair` → `car-repair`
/// (assuming the map links them) → matches `car-repair` in vocab.
///
/// When `synonym_map = None` the validator falls back to plain
/// normalization (legacy open-vocab callers; tests).
pub fn validate_tags(
    extraction: &Extraction,
    vocabulary: &HashSet<String>,
    synonym_map: Option<&SynonymMap>,
) -> TagValidationResult {
    let canonical_raw = canonicalize_ordered(&extraction.raw_topic_tags, synonym_map);

    let mut validated_tags: Vec<String> = Vec::with_capacity(canonical_raw.len());
    let mut new_tag_requests: Vec<NewTagRequest> = Vec::new();

    for tag in &canonical_raw {
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
        for ProposedNewTag { tag, rationale } in proposed {
            let canonical = canonicalize_one(tag, synonym_map);
            if canonical.is_empty() {
                continue;
            }
            // If the proposed tag IS actually in vocab (model was
            // overcautious OR submitted a synonym that maps to an
            // in-vocab canonical), promote it to validated rather
            // than logging a bogus request.
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
    use super::super::super::synonyms::SynonymMap;
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

    fn map_from_json(s: &str) -> SynonymMap {
        let p = std::env::temp_dir().join(format!(
            "tag-validator-syn-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&p, s).unwrap();
        let m = SynonymMap::load(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        m
    }

    fn tiny_map() -> SynonymMap {
        map_from_json(
            r#"{
              "version": "test-validator-v0",
              "schema_version": "synonym-map-v1",
              "synonyms": [
                {"canonical": "car-repair", "variants": ["automobile-repair", "auto-maintenance", "vehicle-repair"], "rationale": "", "source": "test"},
                {"canonical": "meeting",    "variants": ["standup", "1on1"], "rationale": "", "source": "test"},
                {"canonical": "kid",        "variants": [], "rationale": "", "source": "test"},
                {"canonical": "budget",     "variants": ["budgeting"], "rationale": "", "source": "test"}
              ]
            }"#,
        )
    }

    // ── Legacy (no synonym map) cases ──

    #[test]
    fn pure_in_vocab_passes_through_no_map() {
        let v = vocab(&["work", "budget"]);
        let r = validate_tags(&extraction(&["work", "budget"], None), &v, None);
        assert_eq!(r.validated_tags, vec!["work", "budget"]);
        assert!(r.new_tag_requests.is_empty());
    }

    #[test]
    fn out_of_vocab_raw_tag_is_dropped_and_logged_implicit_no_map() {
        let v = vocab(&["work"]);
        let r = validate_tags(&extraction(&["work", "q3-roadmap"], None), &v, None);
        assert_eq!(r.validated_tags, vec!["work"]);
        assert_eq!(r.new_tag_requests.len(), 1);
        assert_eq!(r.new_tag_requests[0].tag, "q3-roadmap");
        assert_eq!(r.new_tag_requests[0].source, NewTagRequestSource::Implicit);
        assert!(r.new_tag_requests[0].rationale.is_empty());
    }

    #[test]
    fn explicit_proposed_tag_never_lands_on_entry_no_map() {
        let v = vocab(&["work"]);
        let r = validate_tags(
            &extraction(
                &["work"],
                Some(vec![("q3-roadmap", "recurring planning concept")]),
            ),
            &v,
            None,
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
    fn normalization_runs_before_vocab_check_no_map() {
        let v = vocab(&["kid"]);
        let r = validate_tags(&extraction(&["Kids"], None), &v, None);
        assert_eq!(r.validated_tags, vec!["kid"]);
        assert!(r.new_tag_requests.is_empty());
    }

    #[test]
    fn overcautious_proposed_tag_already_in_vocab_is_promoted_no_map() {
        let v = vocab(&["budget"]);
        let r = validate_tags(
            &extraction(&[], Some(vec![("budget", "central topic")])),
            &v,
            None,
        );
        assert_eq!(r.validated_tags, vec!["budget"]);
        assert!(r.new_tag_requests.is_empty());
    }

    #[test]
    fn promoted_proposed_tag_does_not_duplicate_already_validated_tag_no_map() {
        let v = vocab(&["budget"]);
        let r = validate_tags(
            &extraction(&["budget"], Some(vec![("budget", "huh")])),
            &v,
            None,
        );
        assert_eq!(r.validated_tags, vec!["budget"]);
        assert!(r.new_tag_requests.is_empty());
    }

    #[test]
    fn mixed_case_implicit_and_explicit_preserves_order_no_map() {
        let v = vocab(&["a"]);
        let r = validate_tags(
            &extraction(
                &["a", "b", "c"],
                Some(vec![("d", "rationale-d"), ("e", "rationale-e")]),
            ),
            &v,
            None,
        );
        assert_eq!(r.validated_tags, vec!["a"]);
        assert_eq!(r.new_tag_requests.len(), 4);
        let tags: Vec<&str> = r.new_tag_requests.iter().map(|n| n.tag.as_str()).collect();
        assert_eq!(tags, vec!["b", "c", "d", "e"]);
        assert_eq!(r.new_tag_requests[0].source, NewTagRequestSource::Implicit);
        assert_eq!(r.new_tag_requests[1].source, NewTagRequestSource::Implicit);
        assert_eq!(r.new_tag_requests[2].source, NewTagRequestSource::Explicit);
        assert_eq!(r.new_tag_requests[3].source, NewTagRequestSource::Explicit);
    }

    #[test]
    fn empty_proposed_new_tags_field_is_treated_as_none_no_map() {
        let v = vocab(&["work"]);
        let r = validate_tags(&extraction(&["work"], Some(vec![])), &v, None);
        assert_eq!(r.validated_tags, vec!["work"]);
        assert!(r.new_tag_requests.is_empty());
    }

    #[test]
    fn whitespace_only_proposed_tag_is_skipped_no_map() {
        let v = vocab(&["work"]);
        let r = validate_tags(
            &extraction(&["work"], Some(vec![("   ", "rationale")])),
            &v,
            None,
        );
        assert_eq!(r.validated_tags, vec!["work"]);
        assert!(r.new_tag_requests.is_empty());
    }

    // ── Wave 0.5.3 iter 3 / `mb-e10v` cases — synonym-map in-band ──

    #[test]
    fn synonym_variant_in_raw_canonicalizes_into_vocab_kept_silently() {
        let m = tiny_map();
        let v = vocab(&["car-repair"]);
        let r = validate_tags(&extraction(&["automobile-repair"], None), &v, Some(&m));
        assert_eq!(r.validated_tags, vec!["car-repair"]);
        assert!(
            r.new_tag_requests.is_empty(),
            "synonym-collapsed in-vocab variant must not produce a new-tag-request"
        );
    }

    #[test]
    fn synonym_variant_in_proposed_canonicalizes_into_vocab_downgrades_request() {
        let m = tiny_map();
        let v = vocab(&["car-repair"]);
        let r = validate_tags(
            &extraction(&[], Some(vec![("automobile-repair", "model proposed it")])),
            &v,
            Some(&m),
        );
        assert_eq!(r.validated_tags, vec!["car-repair"]);
        assert!(r.new_tag_requests.is_empty());
    }

    #[test]
    fn genuinely_novel_tag_with_no_synonym_logged_as_implicit_request() {
        let m = tiny_map();
        let v = vocab(&["car-repair"]);
        let r = validate_tags(&extraction(&["bicycle-tune-up"], None), &v, Some(&m));
        assert!(r.validated_tags.is_empty());
        assert_eq!(r.new_tag_requests.len(), 1);
        assert_eq!(r.new_tag_requests[0].tag, "bicycle-tune-up");
        assert_eq!(r.new_tag_requests[0].source, NewTagRequestSource::Implicit);
    }

    #[test]
    fn two_variants_collapsing_to_same_canonical_dedupe_to_one_tag() {
        let m = tiny_map();
        let v = vocab(&["car-repair"]);
        let r = validate_tags(
            &extraction(&["automobile-repair", "vehicle-repair"], None),
            &v,
            Some(&m),
        );
        assert_eq!(r.validated_tags, vec!["car-repair"]);
        assert!(r.new_tag_requests.is_empty());
    }

    #[test]
    fn mixed_synonyms_canonicals_and_unknowns_preserves_order() {
        let m = tiny_map();
        let v = vocab(&["meeting", "kid"]);
        let r = validate_tags(
            &extraction(&["standup", "kid", "bicycle-tune-up"], None),
            &v,
            Some(&m),
        );
        assert_eq!(r.validated_tags, vec!["meeting", "kid"]);
        assert_eq!(r.new_tag_requests.len(), 1);
        assert_eq!(r.new_tag_requests[0].tag, "bicycle-tune-up");
        assert_eq!(r.new_tag_requests[0].source, NewTagRequestSource::Implicit);
    }

    #[test]
    fn out_of_vocab_explicit_request_with_rationale_survives() {
        let m = tiny_map();
        let v = vocab(&["car-repair"]);
        let r = validate_tags(
            &extraction(
                &["car-repair"],
                Some(vec![(
                    "bicycle-tune-up",
                    "recurring across last 4 dictations",
                )]),
            ),
            &v,
            Some(&m),
        );
        assert_eq!(r.validated_tags, vec!["car-repair"]);
        assert_eq!(r.new_tag_requests.len(), 1);
        assert_eq!(r.new_tag_requests[0].tag, "bicycle-tune-up");
        assert_eq!(r.new_tag_requests[0].source, NewTagRequestSource::Explicit);
        assert_eq!(
            r.new_tag_requests[0].rationale,
            "recurring across last 4 dictations"
        );
    }

    #[test]
    fn variant_followed_by_canonical_in_raw_dedupes() {
        let m = tiny_map();
        let v = vocab(&["car-repair"]);
        let r = validate_tags(
            &extraction(&["automobile-repair", "car-repair"], None),
            &v,
            Some(&m),
        );
        assert_eq!(r.validated_tags, vec!["car-repair"]);
        assert!(r.new_tag_requests.is_empty());
    }
}
