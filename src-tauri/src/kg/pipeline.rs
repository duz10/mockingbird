//! Per-dictation orchestrator: segment → classify+extract per
//! segment → normalize → assemble `Vec<Entry>`.
//!
//! Graduated from `experimental/kg-validation/src/harness/pipeline.rs`
//! per Wave 2 Task 7 (`mb-e5ib`).
//!
//! ## What changed at graduation
//!
//! - `artifact_dir: &Path` → `artifact_dir: Option<&Path>`. The
//!   production crate calls `run_pipeline` from the dictation loop
//!   where there's nowhere natural to dump per-pass JSON; the sandbox
//!   harness still wants them for the parity / score reports. Every
//!   `fs::*` call is now gated behind `if let Some(dir) = artifact_dir`.
//! - `println!` (none today, but the sandbox docstring mentioned them
//!   historically) → `tracing` per the project coding standards.
//!   The orchestrator stays mostly quiet — pass-level failures land
//!   in `per_pass_errors` for the caller to surface.
//! - `crate::` → `super::` import-path rewrites.
//!
//! ## Failure policy (verbatim from the sandbox)
//!
//! - A per-segment failure is recorded in [`PipelineResult::per_pass_errors`]
//!   and the remaining segments continue.
//! - A segment-pass failure aborts THIS dictation (no segments ==
//!   nothing to do).
//! - A classify/extract failure drops that one segment and lets the
//!   rest through.
//! - `per_pass_errors` records `"classify[2]"`-style stage tags so a
//!   caller can pin which segment failed.

use std::path::Path;

use serde::Serialize;

use super::ollama::{GenerateOptions, OllamaDispatcher};
use super::passes::{self, Classification, ExtractedEntity, Extraction, NewTagRequest, PassError};
use super::schema::{Entry, EntryType, Status};
use super::schema_loader::Schema;
use super::synonyms::SynonymMap;

/// Result of one dictation through the pipeline.
pub struct PipelineResult {
    /// One per surviving segment, in segment order.
    pub entries: Vec<Entry>,
    /// `(stage-tag, error)` where stage-tag is e.g. `"segment"` or
    /// `"classify[2]"` to pin which segment a per-segment failure
    /// belongs to.
    pub per_pass_errors: Vec<(String, PassError)>,
    /// Wave 0.5.3 / `mb-rzpd`: out-of-vocab tags the model wanted to
    /// apply. Paired with `segment_idx` so the run-end aggregator
    /// can attribute them back to a dictation + segment. Empty when
    /// the active schema is open-vocab.
    pub new_tag_requests: Vec<(usize, NewTagRequest)>,
    /// Phase 1B Chunk 3 (`mb-eke8`, ADR 0050) — per-segment entity
    /// outputs from the 5th pass (`extract_entities`). Threaded through
    /// for the KG filing worker's `apply_filed_outcome` consumer; the
    /// parity probe's `pipeline_result_to_value` manually builds the
    /// three-key JSON shape and is invisible to this additive field by
    /// construction (Chunk 2 LESSONS finding).
    ///
    /// Order is segment order; a segment that hit a `classify` or
    /// `extract` failure (and thus never assembled an `Entry`) is
    /// omitted here too — the pipeline `continue`s upstream of the
    /// entity pass. A segment whose `extract_entities` call itself
    /// failed appears here with `entities: Vec::new()` and the
    /// matching `extract_entities[i]` error in [`Self::per_pass_errors`].
    pub segment_entities: Vec<SegmentEntities>,
}

/// Per-segment slice of the `extract_entities` pass output. The
/// `segment_idx` is the 0-based pipeline segment ordinal (aligns with
/// `kg_entity_mentions.segment_idx` written by
/// `kg::store::apply_filed_outcome`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentEntities {
    pub segment_idx: usize,
    pub entities: Vec<ExtractedEntity>,
}

#[derive(Serialize)]
struct SegmentArtifact<'a> {
    raw_model_output: &'a str,
    parsed_segments: &'a [String],
}

#[derive(Serialize)]
struct ClassifyArtifact<'a> {
    raw_model_output: &'a str,
    parsed: Option<&'a Classification>,
    error: Option<String>,
    segment_text: &'a str,
}

#[derive(Serialize)]
struct ExtractArtifact<'a> {
    raw_model_output: &'a str,
    parsed: Option<&'a Extraction>,
    normalized_tags: Option<&'a [String]>,
    error: Option<String>,
    segment_text: &'a str,
}

/// Run the four-pass schema-driven KG pipeline on one dictation.
///
/// Returns a [`PipelineResult`] carrying the assembled entries, any
/// per-pass errors recorded along the way (with stage tags like
/// `"classify[2]"` pinning which segment failed), and any
/// out-of-vocab tags the model wanted to apply.
///
/// `artifact_dir = Some(_)` enables per-pass JSON dumps (used by the
/// sandbox harness + the Chunk 3 parity probe). Production callers
/// pass `None`.
///
/// ## Failure policy (verbatim from the sandbox)
///
/// - Segment-pass failure aborts THIS dictation (no segments ==
///   nothing to do).
/// - Classify- or extract-pass failure drops that one segment and
///   lets the rest through.
/// - Each failure is appended to `per_pass_errors` for the caller
///   to surface.
///
/// ## Signature note
///
/// Nine args is two over clippy's default cap. Every argument here
/// is a distinct runtime concern (dispatcher, schema, optional
/// synonym map, model id, dictation identity, content, captured
/// timestamp, sampling options, artifact destination) and bundling
/// them into a config struct would just be visual shuffling. The
/// function is an orchestrator; its signature is the contract.
#[allow(clippy::too_many_arguments)]
pub fn run_pipeline<D: OllamaDispatcher>(
    dispatcher: &D,
    schema: &Schema,
    synonym_map: Option<&SynonymMap>,
    model: &str,
    dictation_id: &str,
    dictation: &str,
    captured_iso: &str,
    options: &GenerateOptions,
    artifact_dir: Option<&Path>,
) -> PipelineResult {
    if let Some(dir) = artifact_dir {
        // Ignored on Err: artifact persistence is best-effort.
        let _ = std::fs::create_dir_all(dir);
    }

    let mut errors: Vec<(String, PassError)> = Vec::new();
    let mut new_tag_requests: Vec<(usize, NewTagRequest)> = Vec::new();
    let mut segment_entities: Vec<SegmentEntities> = Vec::new();

    // Resolve per-pass prompt bodies up front via the model-class
    // calibration profile (`mb-4xtd` / ADR 0049 Move 1). The dispatch
    // model is stable for the whole pipeline call, so one resolution
    // suffices. A missing prompt at this point is a schema-load bug,
    // not a per-segment failure.
    let segment_prompt = match schema.prompt_body("segment", model) {
        Ok(b) => b,
        Err(e) => {
            errors.push((
                format!("segment[{dictation_id}]"),
                PassError::Validation {
                    pass: "segment",
                    detail: format!("schema resolution: {e}"),
                    raw: String::new(),
                },
            ));
            return PipelineResult {
                entries: Vec::new(),
                per_pass_errors: errors,
                new_tag_requests,
                segment_entities,
            };
        }
    };
    let classify_prompt = match schema.prompt_body("classify", model) {
        Ok(b) => b,
        Err(e) => {
            errors.push((
                format!("classify[{dictation_id}]"),
                PassError::Validation {
                    pass: "classify",
                    detail: format!("schema resolution: {e}"),
                    raw: String::new(),
                },
            ));
            return PipelineResult {
                entries: Vec::new(),
                per_pass_errors: errors,
                new_tag_requests,
                segment_entities,
            };
        }
    };
    let extract_prompt = match schema.prompt_body("extract", model) {
        Ok(b) => b,
        Err(e) => {
            errors.push((
                format!("extract[{dictation_id}]"),
                PassError::Validation {
                    pass: "extract",
                    detail: format!("schema resolution: {e}"),
                    raw: String::new(),
                },
            ));
            return PipelineResult {
                entries: Vec::new(),
                per_pass_errors: errors,
                new_tag_requests,
                segment_entities,
            };
        }
    };
    let extract_entities_prompt = match schema.prompt_body("extract_entities", model) {
        Ok(b) => b,
        Err(e) => {
            errors.push((
                format!("extract_entities[{dictation_id}]"),
                PassError::Validation {
                    pass: "extract_entities",
                    detail: format!("schema resolution: {e}"),
                    raw: String::new(),
                },
            ));
            return PipelineResult {
                entries: Vec::new(),
                per_pass_errors: errors,
                new_tag_requests,
                segment_entities,
            };
        }
    };

    // ── Pass 1: segment ─────────────────────────────────
    let segments = match passes::segment(
        dispatcher,
        model,
        segment_prompt,
        dictation,
        captured_iso,
        options,
    ) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                target: "kg::pipeline",
                dictation_id = %dictation_id,
                error = %e,
                "segment pass failed; aborting dictation"
            );
            errors.push((format!("segment[{dictation_id}]"), e));
            return PipelineResult {
                entries: Vec::new(),
                per_pass_errors: errors,
                new_tag_requests,
                segment_entities,
            };
        }
    };

    if let Some(dir) = artifact_dir {
        let _ = write_json(
            &dir.join("segment.json"),
            &SegmentArtifact {
                raw_model_output: "<captured only on parse failure>",
                parsed_segments: &segments,
            },
        );
    }

    let mut entries: Vec<Entry> = Vec::new();

    for (idx, seg_text) in segments.iter().enumerate() {
        // ── Pass 2: classify ──────────────────────────────────────
        let classification =
            match passes::classify(dispatcher, model, classify_prompt, seg_text, options) {
                Ok(c) => c,
                Err(e) => {
                    let raw_for_artifact = raw_from_err(&e).unwrap_or("<unavailable>").to_string();
                    let msg = e.to_string();
                    errors.push((format!("classify[{idx}]"), e));
                    if let Some(dir) = artifact_dir {
                        let _ = write_json(
                            &dir.join(format!("classify-{idx}.json")),
                            &ClassifyArtifact {
                                raw_model_output: &raw_for_artifact,
                                parsed: None,
                                error: Some(msg),
                                segment_text: seg_text,
                            },
                        );
                    }
                    continue;
                }
            };
        if let Some(dir) = artifact_dir {
            let _ = write_json(
                &dir.join(format!("classify-{idx}.json")),
                &ClassifyArtifact {
                    raw_model_output: "<captured only on parse failure>",
                    parsed: Some(&classification),
                    error: None,
                    segment_text: seg_text,
                },
            );
        }

        // ── Pass 3: extract ───────────────────────────────────────
        let extraction = match passes::extract(
            dispatcher,
            model,
            extract_prompt,
            seg_text,
            &classification,
            captured_iso,
            options,
        ) {
            Ok(x) => x,
            Err(e) => {
                let raw_for_artifact = raw_from_err(&e).unwrap_or("<unavailable>").to_string();
                let msg = e.to_string();
                errors.push((format!("extract[{idx}]"), e));
                if let Some(dir) = artifact_dir {
                    let _ = write_json(
                        &dir.join(format!("extract-{idx}.json")),
                        &ExtractArtifact {
                            raw_model_output: &raw_for_artifact,
                            parsed: None,
                            normalized_tags: None,
                            error: Some(msg),
                            segment_text: seg_text,
                        },
                    );
                }
                continue;
            }
        };

        // ── Pass 4: normalize + (Wave 0.5.3) closed-vocab validate ─
        let (final_tags, new_requests_for_segment) = if schema.has_closed_tag_vocabulary() {
            let validation =
                passes::validate_tags(&extraction, schema.canonical_tag_vocabulary(), synonym_map);
            (validation.validated_tags, validation.new_tag_requests)
        } else {
            (
                passes::normalize_tags(&extraction.raw_topic_tags),
                Vec::new(),
            )
        };

        for req in &new_requests_for_segment {
            new_tag_requests.push((idx, req.clone()));
        }

        if let Some(dir) = artifact_dir {
            let _ = write_json(
                &dir.join(format!("extract-{idx}.json")),
                &ExtractArtifact {
                    raw_model_output: "<captured only on parse failure>",
                    parsed: Some(&extraction),
                    normalized_tags: Some(&final_tags),
                    error: None,
                    segment_text: seg_text,
                },
            );
        }

        // ── Pass 5: extract_entities ──────────────────────────────
        // Phase 1B Chunk 3 (`mb-eke8`, ADR 0050) — sits between
        // `extract` and normalize/assemble per ADR 0049 §6 pipeline
        // order. Failure here records `extract_entities[i]` to
        // `per_pass_errors` and proceeds with an empty entity vec for
        // this segment — the `Entry` is still assembled because
        // entries are valuable independent of entity provenance
        // (entity attribution is best-effort in 1B per ADR 0050).
        let entities = match passes::extract_entities(
            dispatcher,
            model,
            extract_entities_prompt,
            seg_text,
            options,
        ) {
            Ok(e) => e.entities,
            Err(e) => {
                tracing::warn!(
                    target: "kg::pipeline",
                    dictation_id = %dictation_id,
                    segment_idx = idx,
                    error = %e,
                    "extract_entities pass failed; segment kept with empty entity vec"
                );
                errors.push((format!("extract_entities[{idx}]"), e));
                Vec::new()
            }
        };
        segment_entities.push(SegmentEntities {
            segment_idx: idx,
            entities,
        });

        // ── Assemble Entry ────────────────────────────────────────
        let status = if classification.entry_type == EntryType::Task {
            Some(Status::Todo)
        } else {
            None
        };
        entries.push(Entry {
            title: extraction.title,
            category: classification.category,
            entry_type: classification.entry_type,
            status,
            topic_tags: final_tags,
            due_iso: extraction.due_iso,
            captured_iso: captured_iso.to_string(),
            body: seg_text.clone(),
        });
    }

    PipelineResult {
        entries,
        per_pass_errors: errors,
        new_tag_requests,
        segment_entities,
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, text)
}

fn raw_from_err(e: &PassError) -> Option<&str> {
    match e {
        PassError::Parse { raw, .. } | PassError::Validation { raw, .. } => Some(raw),
        PassError::Dispatcher(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::ollama::testing::MockOllama;
    use super::super::schema::{Category, EntryType};
    use super::*;

    fn test_schema() -> Schema {
        Schema::load_bundled().expect("load bundled schema")
    }

    #[test]
    fn end_to_end_with_mock_dispatcher() {
        let tmp = tempdir();
        // First-match-wins: extract rules MUST come before classify
        // rules, because the extract prompt also contains the segment
        // text. We disambiguate using the "CLASSIFICATION" marker
        // that only the extract prompt writes.
        // The extract_entities needle is registered FIRST because its
        // prompt body is the only one whose suffix overlaps with
        // classify (`SEGMENT:\n{seg}`). First-substring-match wins;
        // putting the unique extract_entities opener at the head of the
        // rule list disambiguates the two passes cleanly. Empty
        // entities are fine for this test — the assertion surface is
        // the entry shape, not entity provenance.
        let mock = MockOllama::new()
            .respond_when(
                "You extract specific named or concrete",
                r#"{"entities":[]}"#,
            )
            .respond_when(
                "DICTATION",
                r#"["call the daycare on Monday","ship the order before Friday"]"#,
            )
            .respond_when(
                "call the daycare on Monday\nCLASSIFICATION",
                r#"{"title":"Call daycare about spot","due_iso":"2026-06-15","raw_topic_tags":["Daycare","Kids"]}"#,
            )
            .respond_when(
                "ship the order before Friday\nCLASSIFICATION",
                r#"{"title":"Ship order to client","due_iso":"2026-06-19","raw_topic_tags":["orders","shipping"]}"#,
            )
            .respond_when(
                "SEGMENT:\ncall the daycare",
                r#"{"category":"personal","entry_type":"task"}"#,
            )
            .respond_when(
                "SEGMENT:\nship the order",
                r#"{"category":"professional","entry_type":"task"}"#,
            );

        let opts = GenerateOptions {
            temperature: 0.2,
            seed: Some(42),
            num_ctx: 4096,
        };
        let schema = test_schema();
        let result = run_pipeline(
            &mock,
            &schema,
            None,
            "test-model",
            "dict-1",
            "Two things. Call the daycare on Monday. Ship the order before Friday.",
            "2026-06-14T08:00:00Z",
            &opts,
            Some(tmp.path()),
        );

        assert!(
            result.per_pass_errors.is_empty(),
            "{:?}",
            result.per_pass_errors
        );
        assert_eq!(result.entries.len(), 2);

        // Chunk 3: extract_entities runs once per surviving segment;
        // the canned `{"entities":[]}` response yields two empty
        // SegmentEntities rows in segment order.
        assert_eq!(result.segment_entities.len(), 2);
        assert_eq!(result.segment_entities[0].segment_idx, 0);
        assert_eq!(result.segment_entities[1].segment_idx, 1);
        assert!(result.segment_entities[0].entities.is_empty());
        assert!(result.segment_entities[1].entities.is_empty());

        let e0 = &result.entries[0];
        assert_eq!(e0.category, Category::Personal);
        assert_eq!(e0.entry_type, EntryType::Task);
        assert_eq!(e0.status, Some(Status::Todo));
        assert_eq!(e0.due_iso.as_deref(), Some("2026-06-15"));
        // Normalize: "Daycare" → "daycare", "Kids" → "kid"
        assert_eq!(e0.topic_tags, vec!["daycare", "kid"]);
        assert_eq!(e0.captured_iso, "2026-06-14T08:00:00Z");

        let e1 = &result.entries[1];
        assert_eq!(e1.category, Category::Professional);

        // Artifacts written.
        assert!(tmp.path().join("segment.json").exists());
        assert!(tmp.path().join("classify-0.json").exists());
        assert!(tmp.path().join("classify-1.json").exists());
        assert!(tmp.path().join("extract-0.json").exists());
        assert!(tmp.path().join("extract-1.json").exists());
    }

    #[test]
    fn end_to_end_with_no_artifact_dir() {
        // The graduation API change: callers that don't want
        // per-pass JSON pass `None` and the orchestrator just
        // skips every fs::* call. Identical entry results.
        let mock = MockOllama::new()
            .respond_when(
                "You extract specific named or concrete",
                r#"{"entities":[]}"#,
            )
            .respond_when("DICTATION", r#"["call the daycare on Monday"]"#)
            .respond_when(
                "call the daycare on Monday\nCLASSIFICATION",
                r#"{"title":"Call daycare","due_iso":null,"raw_topic_tags":["daycare"]}"#,
            )
            .respond_when(
                "SEGMENT:\ncall the daycare",
                r#"{"category":"personal","entry_type":"task"}"#,
            );
        let schema = test_schema();
        let result = run_pipeline(
            &mock,
            &schema,
            None,
            "m",
            "dict-1",
            "Call the daycare on Monday.",
            "2026-06-14T08:00:00Z",
            &GenerateOptions::default(),
            None,
        );
        assert!(
            result.per_pass_errors.is_empty(),
            "{:?}",
            result.per_pass_errors
        );
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].topic_tags, vec!["daycare"]);
    }

    #[test]
    fn junk_returns_no_entries_no_errors() {
        let tmp = tempdir();
        let mock = MockOllama::new().respond_when("DICTATION", "[]");
        let schema = test_schema();
        let result = run_pipeline(
            &mock,
            &schema,
            None,
            "m",
            "junk-1",
            "Uh hold on, never mind.",
            "2026-06-14T08:00:00Z",
            &GenerateOptions::default(),
            Some(tmp.path()),
        );
        assert!(result.entries.is_empty());
        assert!(result.per_pass_errors.is_empty());
        assert!(tmp.path().join("segment.json").exists());
    }

    #[test]
    fn segment_failure_aborts_dictation() {
        let tmp = tempdir();
        let mock = MockOllama::new().default_response("not json");
        let schema = test_schema();
        let result = run_pipeline(
            &mock,
            &schema,
            None,
            "m",
            "bad-1",
            "anything",
            "2026-06-14T08:00:00Z",
            &GenerateOptions::default(),
            Some(tmp.path()),
        );
        assert!(result.entries.is_empty());
        assert_eq!(result.per_pass_errors.len(), 1);
        assert!(result.per_pass_errors[0].0.starts_with("segment["));
    }

    #[test]
    fn classify_failure_drops_only_that_segment() {
        let tmp = tempdir();
        let mock = MockOllama::new()
            // extract_entities needle first — its prompt suffix also
            // contains "SEGMENT:\n{seg}" but its body uniquely opens
            // with "You extract specific named or concrete". First-match
            // wins, so registering it ahead of the SEGMENT-keyed
            // classify needle is what keeps the disambiguation clean.
            .respond_when(
                "You extract specific named or concrete",
                r#"{"entities":[]}"#,
            )
            .respond_when("DICTATION", r#"["good segment","bad segment"]"#)
            // Extract for the good segment must be MATCHED FIRST —
            // it's the strictest needle (contains "CLASSIFICATION").
            // First-match-wins means a less-specific needle could
            // otherwise eat it.
            .respond_when(
                "good segment\nCLASSIFICATION",
                r#"{"title":"Good thing","due_iso":null,"raw_topic_tags":["a"]}"#,
            )
            .respond_when(
                "SEGMENT:\ngood segment",
                r#"{"category":"personal","entry_type":"task"}"#,
            )
            .respond_when("SEGMENT:\nbad segment", "not json");

        let schema = test_schema();
        let result = run_pipeline(
            &mock,
            &schema,
            None,
            "m",
            "mixed-1",
            "two segments",
            "2026-06-14T08:00:00Z",
            &GenerateOptions::default(),
            Some(tmp.path()),
        );

        assert_eq!(
            result.entries.len(),
            1,
            "good segment should still produce an entry"
        );
        assert_eq!(result.per_pass_errors.len(), 1);
        assert!(result.per_pass_errors[0].0.starts_with("classify["));
    }

    #[test]
    fn extract_entities_populates_segment_entities_per_segment() {
        // Chunk 3 (`mb-eke8`) — wire the 5th pass into the orchestrator.
        // One dictation → two segments → two entity rows wired through.
        let mock = MockOllama::new()
            .respond_when(
                "You extract specific named or concrete",
                r#"{"entities":[{"name":"madison","type":"person","aliases":[]}]}"#,
            )
            .respond_when(
                "DICTATION",
                r#"["segment one mentions madison","segment two also mentions madison"]"#,
            )
            .respond_when(
                "segment one mentions madison\nCLASSIFICATION",
                r#"{"title":"s1","due_iso":null,"raw_topic_tags":["a"]}"#,
            )
            .respond_when(
                "segment two also mentions madison\nCLASSIFICATION",
                r#"{"title":"s2","due_iso":null,"raw_topic_tags":["a"]}"#,
            )
            .respond_when(
                "SEGMENT:\nsegment one mentions madison",
                r#"{"category":"personal","entry_type":"task"}"#,
            )
            .respond_when(
                "SEGMENT:\nsegment two also mentions madison",
                r#"{"category":"personal","entry_type":"task"}"#,
            );
        let schema = test_schema();
        let result = run_pipeline(
            &mock,
            &schema,
            None,
            "qwen2.5:7b-instruct-q4_K_M",
            "ents-1",
            "two segments, both mention madison.",
            "2026-06-14T08:00:00Z",
            &GenerateOptions::default(),
            None,
        );
        assert!(
            result.per_pass_errors.is_empty(),
            "{:?}",
            result.per_pass_errors
        );
        assert_eq!(result.entries.len(), 2);
        assert_eq!(result.segment_entities.len(), 2);
        assert_eq!(result.segment_entities[0].entities.len(), 1);
        assert_eq!(result.segment_entities[0].entities[0].name, "madison");
        assert_eq!(result.segment_entities[1].entities.len(), 1);
        assert_eq!(result.segment_entities[1].entities[0].name, "madison");
    }

    #[test]
    fn extract_entities_failure_keeps_entry_with_empty_entities() {
        // Chunk 3 (`mb-eke8`) — failure isolation per ADR 0050:
        // entity attribution is best-effort in 1B; a parse failure on
        // the 5th pass records `extract_entities[i]` to per_pass_errors
        // but the segment's Entry still ships.
        let mock = MockOllama::new()
            .respond_when(
                "You extract specific named or concrete",
                "not json — entity pass should fail to parse",
            )
            .respond_when("DICTATION", r#"["only one segment here"]"#)
            .respond_when(
                "only one segment here\nCLASSIFICATION",
                r#"{"title":"hello","due_iso":null,"raw_topic_tags":["a"]}"#,
            )
            .respond_when(
                "SEGMENT:\nonly one segment here",
                r#"{"category":"personal","entry_type":"task"}"#,
            );
        let schema = test_schema();
        let result = run_pipeline(
            &mock,
            &schema,
            None,
            "qwen2.5:7b-instruct-q4_K_M",
            "ent-fail-1",
            "only one segment here.",
            "2026-06-14T08:00:00Z",
            &GenerateOptions::default(),
            None,
        );
        assert_eq!(result.entries.len(), 1, "entry must still ship");
        assert_eq!(result.segment_entities.len(), 1);
        assert!(result.segment_entities[0].entities.is_empty());
        assert_eq!(result.per_pass_errors.len(), 1);
        assert!(result.per_pass_errors[0].0.starts_with("extract_entities["));
    }

    // ── tiny tempdir helper so we don't pull in the `tempfile` crate
    fn tempdir() -> TempDir {
        let base = std::env::temp_dir();
        let unique = format!(
            "kg-pipeline-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let path = base.join(unique);
        std::fs::create_dir_all(&path).expect("create tempdir");
        TempDir { path }
    }

    struct TempDir {
        path: std::path::PathBuf,
    }
    impl TempDir {
        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
