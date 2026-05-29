//! Per-dictation orchestrator: segment → classify+extract per
//! segment → normalize → assemble `Vec<Entry>`. Persists raw + parsed
//! intermediates so the Wave 3 scorer (and a curious human) can see
//! exactly what each model said at each step.
//!
//! Failure policy: a per-segment failure is recorded in
//! [`PipelineResult::per_pass_errors`] and the remaining segments
//! continue. A segment-pass failure aborts THIS dictation (no
//! segments == nothing to do); a classify/extract failure drops
//! that one segment and lets the rest through.

use std::path::Path;

use serde::Serialize;

use crate::ollama::{GenerateOptions, OllamaDispatcher};
use crate::passes::{self, Classification, Extraction, NewTagRequest, PassError};
use crate::schema::{Entry, EntryType, Status};
use crate::schema_loader::Schema;

pub struct PipelineResult {
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

// Eight args is one over clippy's default cap. Every argument here
// is a distinct runtime concern (dispatcher, schema, model id,
// dictation identity, content, captured timestamp, sampling options,
// artifact destination) and bundling them into a config struct
// would just be visual shuffling. The function is an orchestrator;
// its signature is the contract.
#[allow(clippy::too_many_arguments)]
pub fn run_pipeline<D: OllamaDispatcher>(
    dispatcher: &D,
    schema: &Schema,
    model: &str,
    dictation_id: &str,
    dictation: &str,
    captured_iso: &str,
    options: &GenerateOptions,
    artifact_dir: &Path,
) -> PipelineResult {
    std::fs::create_dir_all(artifact_dir).ok();

    let mut errors: Vec<(String, PassError)> = Vec::new();
    let mut new_tag_requests: Vec<(usize, NewTagRequest)> = Vec::new();

    // Resolve per-pass prompt bodies up front via the model-class
    // calibration profile (`mb-4xtd` / ADR 0049 Move 1). The dispatch
    // model is stable for the whole pipeline call, so one resolution
    // suffices. A missing prompt at this point is a schema-load bug,
    // not a per-segment failure — the runner already verified the
    // file existed at startup; we treat the unwrap-equivalent as
    // fail-fast via the explicit error.
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
            errors.push((format!("segment[{dictation_id}]"), e));
            return PipelineResult {
                entries: Vec::new(),
                per_pass_errors: errors,
                new_tag_requests,
            };
        }
    };

    // Persist segment-stage artifact (rich form so Wave 3 can see
    // both raw and parsed; we don't have direct raw access here —
    // the parse already succeeded, so reconstruct a JSON view).
    let _ = write_json(
        &artifact_dir.join("segment.json"),
        &SegmentArtifact {
            raw_model_output: "<captured only on parse failure>",
            parsed_segments: &segments,
        },
    );

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
                    let _ = write_json(
                        &artifact_dir.join(format!("classify-{idx}.json")),
                        &ClassifyArtifact {
                            raw_model_output: &raw_for_artifact,
                            parsed: None,
                            error: Some(msg),
                            segment_text: seg_text,
                        },
                    );
                    continue;
                }
            };
        let _ = write_json(
            &artifact_dir.join(format!("classify-{idx}.json")),
            &ClassifyArtifact {
                raw_model_output: "<captured only on parse failure>",
                parsed: Some(&classification),
                error: None,
                segment_text: seg_text,
            },
        );

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
                let _ = write_json(
                    &artifact_dir.join(format!("extract-{idx}.json")),
                    &ExtractArtifact {
                        raw_model_output: &raw_for_artifact,
                        parsed: None,
                        normalized_tags: None,
                        error: Some(msg),
                        segment_text: seg_text,
                    },
                );
                continue;
            }
        };

        // ── Pass 4: normalize + (Wave 0.5.3) closed-vocab validate ─
        //
        // Two-mode behaviour by schema state:
        //  - open vocab (no canonical list in SCHEMA.md): legacy
        //    behaviour — normalize and use as-is. No new-tag-requests.
        //  - closed vocab (Wave 0.5.3+): normalize, then split into
        //    in-vocab (→ Entry.topic_tags) and out-of-vocab (→
        //    new_tag_requests, surfaced to the runner).
        let (final_tags, new_requests_for_segment) = if schema.has_closed_tag_vocabulary() {
            let validation = passes::validate_tags(&extraction, schema.canonical_tag_vocabulary());
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

        let _ = write_json(
            &artifact_dir.join(format!("extract-{idx}.json")),
            &ExtractArtifact {
                raw_model_output: "<captured only on parse failure>",
                parsed: Some(&extraction),
                normalized_tags: Some(&final_tags),
                error: None,
                segment_text: seg_text,
            },
        );

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
    use super::*;
    use crate::ollama::testing::MockOllama;
    use crate::schema::{Category, EntryType};

    fn test_schema() -> Schema {
        Schema::load_default().expect("load default sandbox schema")
    }

    #[test]
    fn end_to_end_with_mock_dispatcher() {
        let tmp = tempdir();
        // First-match-wins: extract rules MUST come before classify
        // rules, because the extract prompt also contains the
        // segment text. We disambiguate using the "CLASSIFICATION"
        // marker that only the extract prompt writes.
        let mock = MockOllama::new()
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
            "test-model",
            "dict-1",
            "Two things. Call the daycare on Monday. Ship the order before Friday.",
            "2026-06-14T08:00:00Z",
            &opts,
            tmp.path(),
        );

        assert!(
            result.per_pass_errors.is_empty(),
            "{:?}",
            result.per_pass_errors
        );
        assert_eq!(result.entries.len(), 2);

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
    fn junk_returns_no_entries_no_errors() {
        let tmp = tempdir();
        let mock = MockOllama::new().respond_when("DICTATION", "[]");
        let schema = test_schema();
        let result = run_pipeline(
            &mock,
            &schema,
            "m",
            "junk-1",
            "Uh hold on, never mind.",
            "2026-06-14T08:00:00Z",
            &GenerateOptions::default(),
            tmp.path(),
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
            "m",
            "bad-1",
            "anything",
            "2026-06-14T08:00:00Z",
            &GenerateOptions::default(),
            tmp.path(),
        );
        assert!(result.entries.is_empty());
        assert_eq!(result.per_pass_errors.len(), 1);
        assert!(result.per_pass_errors[0].0.starts_with("segment["));
    }

    #[test]
    fn classify_failure_drops_only_that_segment() {
        let tmp = tempdir();
        // MockOllama matches by prompt substring; needles are
        // distinct slices of each pass's prompt template so the
        // dispatcher can tell classify-good from classify-bad from
        // extract-good. The segment text appears in each pass's
        // prompt, so we anchor with the unique pass-marker prefix
        // ("SEGMENT:\n" / "CLASSIFICATION") that each pass writes.
        let mock = MockOllama::new()
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
            "m",
            "mixed-1",
            "two segments",
            "2026-06-14T08:00:00Z",
            &GenerateOptions::default(),
            tmp.path(),
        );

        assert_eq!(
            result.entries.len(),
            1,
            "good segment should still produce an entry"
        );
        assert_eq!(result.per_pass_errors.len(), 1);
        assert!(result.per_pass_errors[0].0.starts_with("classify["));
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
