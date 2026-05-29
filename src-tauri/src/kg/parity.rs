//! KG parity probe — bit-identical re-run gate for Phase 1A graduation.
//!
//! Loads the Wave 0.5.4 seed-42 fixture + its MockOllama canned-response
//! sidecar, drives `run_pipeline` + the standalone `extract_entities`
//! pass through a fixture-scripted [`OllamaDispatcher`], and asserts the
//! production output matches the sandbox's sealed run at the
//! `serde_json::Value` structural level. Charter: Phase 1A Wave 3 brief,
//! ADR 0049, epic `mb-2mc9`, sub-bead `mb-qdgn`.
//!
//! ## Why a probe and not a `#[test]`?
//!
//! LESSONS PINNED P2: `cargo test --release` exits with
//! `STATUS_ENTRYPOINT_NOT_FOUND` on this box. The app binary launches
//! fine — only the test runner is affected. This probe lives in a
//! `[[bin]]` (see `src-tauri/src/bin/kg_parity.rs`) so it sidesteps the
//! test-runner bug entirely.
//!
//! ## Why drive `OllamaDispatcher` only (R2 mitigation)
//!
//! The Chunk 2 graduated [`super::ollama::OllamaClient`] is
//! `#[allow(dead_code)]` and intentionally unwired in Phase 1A. Per the
//! Wave 3 brief's R2 risk flag, this probe MUST consume canned responses
//! exclusively through the [`OllamaDispatcher`] trait. The
//! [`FixtureDispatcher`] below is the sole production trait consumer
//! Phase 1A ships.
//!
//! ## Comparison strategy (`serde_json::Value` structural equality)
//!
//! The Wave 3 brief recommended byte-identical JSON, but Python
//! (`aggregate_fixture.py`) and Rust (`serde_json`) emit semantically
//! identical JSON in non-byte-identical ways (key ordering inside
//! objects, whitespace, number formatting). The brief's permission to
//! "hand-roll the diff with serde_json Value walking" is taken: we parse
//! both sides into [`serde_json::Value`] and compare structurally. Two
//! `Value`s compare equal iff the underlying JSON is semantically equal
//! regardless of serialization order — the right contract for a parity
//! gate. On mismatch the probe emits both sides as pretty JSON so a
//! human can diff them.
//!
//! ## Pass dispatch
//!
//! [`FixtureDispatcher`] disambiguates which pipeline pass is asking by
//! checking the resolved-prompt prefix. Each pass's `prompt_body` (loaded
//! via `Schema::prompt_body(pass, model)`) is unique content; the
//! runtime prompt is always `{prompt_body}\n\n{per_pass_suffix}`, so
//! `prompt.starts_with(prompt_body)` uniquely identifies the pass.
//! The dispatcher then returns the next canned response from the matching
//! per-dictation, per-pass array (segment + extract_entities are
//! single-string; classify + extract are arrays indexed by segment).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Deserialize;
use serde_json::{json, Value};

use super::ollama::{GenerateOptions, OllamaDispatcher, OllamaError};
use super::passes::{extract_entities, ExtractedEntity};
use super::pipeline::{run_pipeline, PipelineResult};
use super::schema_loader::Schema;

/// Resolved-at-runtime model id the Wave 0.5.4 fixture was captured
/// against. Drives schema-profile resolution to `mid-confident`.
const PROBE_MODEL: &str = "qwen2.5:7b-instruct-q4_K_M";

/// Deterministic `captured_iso` baked into every fixture row.
const PROBE_CAPTURED_ISO: &str = "2026-06-14T08:00:00Z";

/// Fixture file paths, resolved relative to `CARGO_MANIFEST_DIR`
/// (= `src-tauri/`) so the probe binary works from any CWD.
const EXPECTED_FIXTURE_REL: &str = "../docs/knowledge-graph/parity/wave-0.5.4-seed-42.json";
const CANNED_RESPONSES_REL: &str =
    "../docs/knowledge-graph/parity/wave-0.5.4-seed-42-canned-responses.json";

/// CRLF / `include_str!` byte-stability hint baked into every mismatch
/// message per Chunk 2 R1.
const CRLF_HINT: &str = "\n\
    Hint: if you see this on a fresh checkout with no production-side\n\
    code changes, suspect CRLF line endings in `src-tauri/src/kg/assets/`.\n\
    Confirm with: git check-attr eol -- src-tauri/src/kg/assets/SCHEMA.md\n\
    (.gitattributes should pin `*.md eol=lf`).";

// ────────────────────────────────────────────────────────────────────
// Fixture types — narrow `Deserialize` shells over the fixture JSON,
// kept private to this module. We do NOT graduate these into the public
// `kg::` surface (per D6) because the fixture is parity-probe internal.
// ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ExpectedFixture {
    dictations: Vec<ExpectedDictation>,
}

#[derive(Deserialize)]
struct ExpectedDictation {
    dictation_id: String,
    dictation_text: String,
    pipeline_result: Value,
    /// `None` when the sandbox entity probe did not record an artifact
    /// for this id (Chunk 1 captured the aggregate where one exists).
    entities: Option<Value>,
}

#[derive(Deserialize)]
struct CannedResponses {
    per_dictation: HashMap<String, DictationCanned>,
}

#[derive(Deserialize, Clone)]
struct DictationCanned {
    /// JSON-string array of segments. Single string because segment runs
    /// once per dictation.
    segment: Option<String>,
    /// One JSON-string per segment, ordered by `segment_idx`.
    classify: Vec<Option<String>>,
    /// Same shape as `classify`.
    extract: Vec<Option<String>>,
    /// Per-dictation aggregate (see `parity/README.md` §3 option 1).
    extract_entities: Option<String>,
}

// ────────────────────────────────────────────────────────────────────
// FixtureDispatcher — the OllamaDispatcher impl that returns canned
// responses driven by prompt-prefix pass detection + per-pass call
// counters.
// ────────────────────────────────────────────────────────────────────

struct PassPrompts {
    segment: String,
    classify: String,
    extract: String,
    extract_entities: String,
}

#[derive(Default)]
struct PassCounters {
    classify: usize,
    extract: usize,
}

struct FixtureDispatcher {
    /// One PROBE_MODEL → resolved-prompt-body table, captured at probe
    /// startup. Used as match prefixes against incoming runtime prompts.
    prompts: PassPrompts,
    /// Per-dictation canned responses (reset per dictation).
    canned: DictationCanned,
    /// Per-pass call counters (segment + extract_entities are stateless
    /// since they're either single-call or return the same aggregate).
    counters: Mutex<PassCounters>,
    /// Owning dictation id — included in error messages so a probe
    /// failure points at the offending fixture row.
    dictation_id: String,
}

impl OllamaDispatcher for FixtureDispatcher {
    fn generate(
        &self,
        _model: &str,
        prompt: &str,
        _system: Option<&str>,
        _options: &GenerateOptions,
    ) -> Result<String, OllamaError> {
        // Order matters: extract_entities and classify both have
        // `SEGMENT:\n...` in the *suffix*, but the prompt BODIES are
        // disjoint and uniquely prefix-match. Check the more-specific
        // pass bodies first to avoid surprise if a future prompt
        // refactor introduces shared boilerplate.
        if prompt.starts_with(&self.prompts.extract_entities) {
            return self.canned.extract_entities.clone().ok_or_else(|| {
                OllamaError::Mock(format!(
                    "[{}] extract_entities called but fixture has no canned response",
                    self.dictation_id
                ))
            });
        }
        if prompt.starts_with(&self.prompts.extract) {
            let mut c = self.counters.lock().unwrap();
            let idx = c.extract;
            c.extract += 1;
            return self
                .canned
                .extract
                .get(idx)
                .cloned()
                .flatten()
                .ok_or_else(|| {
                    OllamaError::Mock(format!(
                        "[{}] extract call #{idx} has no canned response (have {})",
                        self.dictation_id,
                        self.canned.extract.len()
                    ))
                });
        }
        if prompt.starts_with(&self.prompts.classify) {
            let mut c = self.counters.lock().unwrap();
            let idx = c.classify;
            c.classify += 1;
            return self
                .canned
                .classify
                .get(idx)
                .cloned()
                .flatten()
                .ok_or_else(|| {
                    OllamaError::Mock(format!(
                        "[{}] classify call #{idx} has no canned response (have {})",
                        self.dictation_id,
                        self.canned.classify.len()
                    ))
                });
        }
        if prompt.starts_with(&self.prompts.segment) {
            return self.canned.segment.clone().ok_or_else(|| {
                OllamaError::Mock(format!(
                    "[{}] segment called but fixture has no canned response",
                    self.dictation_id
                ))
            });
        }

        let head: String = prompt.chars().take(200).collect();
        Err(OllamaError::Mock(format!(
            "[{}] unmatched prompt prefix — no pass body prefix-matched:\n{head}",
            self.dictation_id
        )))
    }
}

// ────────────────────────────────────────────────────────────────────
// Public entry point
// ────────────────────────────────────────────────────────────────────

/// Run the parity probe. Returns the process exit code: `0` on green,
/// `1` on any divergence or fixture-load failure. Stdout carries the
/// per-fixture status line stream; stderr carries diff dumps on failure.
///
/// The `[[bin]]` wrapper at `src-tauri/src/bin/kg_parity.rs` is a
/// 3-line shim that just calls this and exits.
pub fn run_parity_probe() -> i32 {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let expected_path = manifest_dir.join(EXPECTED_FIXTURE_REL);
    let canned_path = manifest_dir.join(CANNED_RESPONSES_REL);

    println!("🐶 KG parity probe — Wave 0.5.4 seed-42 graduation gate (mb-2mc9 / mb-qdgn)");
    println!("    expected:  {}", expected_path.display());
    println!("    canned:    {}", canned_path.display());

    let expected = match load_expected(&expected_path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("FATAL: failed to load expected fixture: {e}");
            return 1;
        }
    };
    let canned = match load_canned(&canned_path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("FATAL: failed to load canned-responses sidecar: {e}");
            return 1;
        }
    };

    let schema = match Schema::load_default() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("FATAL: schema_loader could not load default schema: {e}");
            return 1;
        }
    };

    let prompts = match resolve_prompts(&schema) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("FATAL: failed to resolve per-pass prompt bodies: {e}");
            return 1;
        }
    };

    let options = GenerateOptions {
        temperature: 0.2,
        seed: Some(42),
        num_ctx: 4096,
    };

    let total = expected.dictations.len();
    println!("    schema_source: {:?}", schema.source);
    println!("    fixtures:  {total}");
    println!();

    let mut passed = 0usize;
    let mut failures: Vec<DictationFailure> = Vec::new();

    for dict in &expected.dictations {
        let canned_for_dict = match canned.per_dictation.get(&dict.dictation_id) {
            Some(c) => c.clone(),
            None => {
                failures.push(DictationFailure {
                    dictation_id: dict.dictation_id.clone(),
                    reason: format!(
                        "canned-responses sidecar has no row for dictation_id `{}`",
                        dict.dictation_id
                    ),
                    diff: None,
                });
                continue;
            }
        };

        match run_one_dictation(dict, canned_for_dict, &schema, &prompts, &options) {
            Ok(()) => {
                passed += 1;
                println!("  ✓ {}", dict.dictation_id);
            }
            Err(failure) => {
                println!("  ✗ {}", dict.dictation_id);
                failures.push(failure);
            }
        }
    }

    println!();
    if failures.is_empty() {
        println!("✅ PARITY GREEN: {passed}/{total}");
        0
    } else {
        eprintln!(
            "❌ PARITY FAILED: {}/{total} passed, {} failed",
            passed,
            failures.len()
        );
        eprintln!();
        // Dump only the FIRST failure (per brief) — the rest are noise
        // once root cause is known. The summary list below lets a human
        // see the failure surface.
        let first = &failures[0];
        eprintln!("──── first failure: {} ────", first.dictation_id);
        eprintln!("{}", first.reason);
        if let Some(diff) = &first.diff {
            eprintln!();
            eprintln!("{diff}");
        }
        eprintln!("{CRLF_HINT}");
        eprintln!();
        eprintln!("All failing fixtures:");
        for f in &failures {
            eprintln!("  ✗ {} — {}", f.dictation_id, first_line(&f.reason));
        }
        1
    }
}

// ────────────────────────────────────────────────────────────────────
// Per-dictation runner
// ────────────────────────────────────────────────────────────────────

struct DictationFailure {
    dictation_id: String,
    reason: String,
    diff: Option<String>,
}

fn run_one_dictation(
    dict: &ExpectedDictation,
    canned: DictationCanned,
    schema: &Schema,
    prompts: &PassPrompts,
    options: &GenerateOptions,
) -> Result<(), DictationFailure> {
    let dispatcher = FixtureDispatcher {
        prompts: clone_prompts(prompts),
        canned: canned.clone(),
        counters: Mutex::new(PassCounters::default()),
        dictation_id: dict.dictation_id.clone(),
    };

    // ── 4-pass pipeline ──────────────────────────────────────────
    let result = run_pipeline(
        &dispatcher,
        schema,
        None, // no synonym map per D6
        PROBE_MODEL,
        &dict.dictation_id,
        &dict.dictation_text,
        PROBE_CAPTURED_ISO,
        options,
        None, // no artifact dir; probe asserts in-memory
    );

    let actual_pipeline = pipeline_result_to_value(&result).map_err(|e| DictationFailure {
        dictation_id: dict.dictation_id.clone(),
        reason: format!("failed to serialize PipelineResult to JSON: {e}"),
        diff: None,
    })?;

    if actual_pipeline != dict.pipeline_result {
        return Err(DictationFailure {
            dictation_id: dict.dictation_id.clone(),
            reason: "pipeline_result diverged from fixture".to_string(),
            diff: Some(format_value_diff(&dict.pipeline_result, &actual_pipeline)),
        });
    }

    // ── 5th pass (extract_entities) — per-segment, dedupe to aggregate
    // per README §3 option 1 ─────────────────────────────────────
    let segments = segments_from_canned(&canned).map_err(|e| DictationFailure {
        dictation_id: dict.dictation_id.clone(),
        reason: format!("could not derive segments from canned responses: {e}"),
        diff: None,
    })?;

    let entity_prompt_body = schema
        .prompt_body("extract_entities", PROBE_MODEL)
        .map_err(|e| DictationFailure {
            dictation_id: dict.dictation_id.clone(),
            reason: format!("schema could not resolve extract_entities prompt: {e}"),
            diff: None,
        })?;

    let actual_entities = if dict.entities.is_some() {
        let mut aggregate: Vec<ExtractedEntity> = Vec::new();
        for seg in &segments {
            let res = extract_entities(&dispatcher, PROBE_MODEL, entity_prompt_body, seg, options)
                .map_err(|e| DictationFailure {
                    dictation_id: dict.dictation_id.clone(),
                    reason: format!("extract_entities pass failed: {e}"),
                    diff: None,
                })?;
            aggregate.extend(res.entities);
        }
        Some(aggregate_entities_to_value(&aggregate, segments.len()))
    } else {
        None
    };

    match (&actual_entities, &dict.entities) {
        (Some(actual), Some(expected)) => {
            // Set-level equality on the `entities` array (per `parity/README.md`
            // §3 option 1 — order-insensitive). The `segment_count` and
            // `segment_failures` fields ride along in the surrounding object
            // and ARE asserted byte-equal because the fixture writes them as
            // scalars.
            if !entity_objects_set_equal(actual, expected) {
                return Err(DictationFailure {
                    dictation_id: dict.dictation_id.clone(),
                    reason: "entities aggregate diverged from fixture".to_string(),
                    diff: Some(format_value_diff(expected, actual)),
                });
            }
        }
        (None, None) => { /* both empty — pass */ }
        _ => {
            return Err(DictationFailure {
                dictation_id: dict.dictation_id.clone(),
                reason: "entities presence mismatch (fixture has aggregate but probe produced none, or vice-versa)"
                    .to_string(),
                diff: None,
            });
        }
    }

    Ok(())
}

// ────────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────────

fn load_expected(path: &Path) -> Result<ExpectedFixture, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str::<ExpectedFixture>(&text)
        .map_err(|e| format!("parse {}: {e}", path.display()))
}

fn load_canned(path: &Path) -> Result<CannedResponses, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str::<CannedResponses>(&text)
        .map_err(|e| format!("parse {}: {e}", path.display()))
}

fn resolve_prompts(schema: &Schema) -> Result<PassPrompts, String> {
    Ok(PassPrompts {
        segment: schema
            .prompt_body("segment", PROBE_MODEL)
            .map_err(|e| format!("segment prompt: {e}"))?
            .to_string(),
        classify: schema
            .prompt_body("classify", PROBE_MODEL)
            .map_err(|e| format!("classify prompt: {e}"))?
            .to_string(),
        extract: schema
            .prompt_body("extract", PROBE_MODEL)
            .map_err(|e| format!("extract prompt: {e}"))?
            .to_string(),
        extract_entities: schema
            .prompt_body("extract_entities", PROBE_MODEL)
            .map_err(|e| format!("extract_entities prompt: {e}"))?
            .to_string(),
    })
}

fn clone_prompts(p: &PassPrompts) -> PassPrompts {
    PassPrompts {
        segment: p.segment.clone(),
        classify: p.classify.clone(),
        extract: p.extract.clone(),
        extract_entities: p.extract_entities.clone(),
    }
}

/// Re-parse the canned `segment` JSON into the same `Vec<String>` the
/// production `segment` pass would have returned. Used by the entity
/// step to drive `extract_entities` once per segment.
fn segments_from_canned(c: &DictationCanned) -> Result<Vec<String>, String> {
    let raw = c
        .segment
        .as_deref()
        .ok_or_else(|| "canned `segment` is null".to_string())?;
    let parsed: Vec<String> = serde_json::from_str(raw)
        .map_err(|e| format!("canned `segment` is not a JSON string array: {e}"))?;
    Ok(parsed
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// Render the production [`PipelineResult`] in the JSON shape the
/// fixture captured — `entries[]` + `per_pass_errors[]` (with
/// `(stage_tag, message_string)` tuples) + `new_tag_requests[]` (with
/// `(segment_idx, NewTagRequest)` tuples). `serde_json::to_value`
/// handles the Entry serde shape (status / due_iso skip-when-None) and
/// the `NewTagRequest` derive.
fn pipeline_result_to_value(result: &PipelineResult) -> Result<Value, serde_json::Error> {
    let entries = serde_json::to_value(&result.entries)?;
    let per_pass_errors: Vec<Value> = result
        .per_pass_errors
        .iter()
        .map(|(stage, err)| json!([stage, err.to_string()]))
        .collect();
    let new_tag_requests: Vec<Value> = result
        .new_tag_requests
        .iter()
        .map(|(idx, req)| -> Result<Value, serde_json::Error> {
            Ok(json!([idx, serde_json::to_value(req)?]))
        })
        .collect::<Result<_, _>>()?;
    Ok(json!({
        "entries": entries,
        "per_pass_errors": per_pass_errors,
        "new_tag_requests": new_tag_requests,
    }))
}

fn aggregate_entities_to_value(aggregate: &[ExtractedEntity], segment_count: usize) -> Value {
    let mut dedup: Vec<&ExtractedEntity> = Vec::with_capacity(aggregate.len());
    'outer: for ent in aggregate {
        for prior in &dedup {
            if prior.name == ent.name && prior.entity_type == ent.entity_type {
                continue 'outer;
            }
        }
        dedup.push(ent);
    }
    let entities: Vec<Value> = dedup
        .into_iter()
        .map(|e| {
            json!({
                "aliases": e.aliases,
                "name": e.name,
                "type": e.entity_type.as_str(),
            })
        })
        .collect();
    json!({
        "entities": entities,
        "segment_count": segment_count,
        "segment_failures": Vec::<Value>::new(),
    })
}

/// Order-insensitive comparison of the `entities` array nested inside
/// the actual/expected aggregate Values. `segment_count` +
/// `segment_failures` ride along as ordinary scalar equality.
fn entity_objects_set_equal(actual: &Value, expected: &Value) -> bool {
    let (Some(a_arr), Some(e_arr)) = (
        actual.get("entities").and_then(Value::as_array),
        expected.get("entities").and_then(Value::as_array),
    ) else {
        return actual == expected;
    };
    if a_arr.len() != e_arr.len() {
        return false;
    }
    for ent in a_arr {
        if !e_arr.iter().any(|other| other == ent) {
            return false;
        }
    }
    if actual.get("segment_count") != expected.get("segment_count") {
        return false;
    }
    if actual.get("segment_failures") != expected.get("segment_failures") {
        return false;
    }
    true
}

fn format_value_diff(expected: &Value, actual: &Value) -> String {
    let exp_pretty =
        serde_json::to_string_pretty(expected).unwrap_or_else(|e| format!("<format error: {e}>"));
    let act_pretty =
        serde_json::to_string_pretty(actual).unwrap_or_else(|e| format!("<format error: {e}>"));
    format!("── expected (fixture) ──\n{exp_pretty}\n\n── actual (production) ──\n{act_pretty}")
}

fn first_line(s: &str) -> &str {
    s.split_once('\n').map(|(a, _)| a).unwrap_or(s)
}

#[cfg(test)]
mod tests {
    //! These are pure-data unit tests of the probe's helper layer.
    //! The probe ITSELF runs as a binary (see module docstring + bin
    //! shim) because of LESSONS P2; we don't `#[test]` the end-to-end
    //! 32-fixture run here.

    use super::*;
    use crate::kg::passes::EntityType;

    #[test]
    fn first_line_strips_after_first_newline() {
        assert_eq!(first_line("alpha\nbeta\ngamma"), "alpha");
        assert_eq!(first_line("noeol"), "noeol");
    }

    #[test]
    fn entity_objects_set_equal_ignores_array_order() {
        let a = json!({
            "entities": [
                {"aliases": [], "name": "a", "type": "person"},
                {"aliases": [], "name": "b", "type": "object"},
            ],
            "segment_count": 2,
            "segment_failures": [],
        });
        let b = json!({
            "entities": [
                {"aliases": [], "name": "b", "type": "object"},
                {"aliases": [], "name": "a", "type": "person"},
            ],
            "segment_count": 2,
            "segment_failures": [],
        });
        assert!(entity_objects_set_equal(&a, &b));
    }

    #[test]
    fn entity_objects_set_equal_catches_segment_count_drift() {
        let a = json!({
            "entities": [],
            "segment_count": 2,
            "segment_failures": [],
        });
        let b = json!({
            "entities": [],
            "segment_count": 3,
            "segment_failures": [],
        });
        assert!(!entity_objects_set_equal(&a, &b));
    }

    #[test]
    fn aggregate_entities_to_value_dedupes_repeated_pairs() {
        let aggregate = vec![
            ExtractedEntity {
                name: "becca".to_string(),
                entity_type: EntityType::Person,
                aliases: vec![],
            },
            ExtractedEntity {
                name: "becca".to_string(),
                entity_type: EntityType::Person,
                aliases: vec![],
            },
            ExtractedEntity {
                name: "becca".to_string(),
                entity_type: EntityType::Project,
                aliases: vec![],
            },
        ];
        let v = aggregate_entities_to_value(&aggregate, 3);
        let entities = v.get("entities").unwrap().as_array().unwrap();
        assert_eq!(entities.len(), 2);
        assert_eq!(v.get("segment_count").unwrap().as_u64(), Some(3));
    }

    #[test]
    fn segments_from_canned_parses_array() {
        let canned = DictationCanned {
            segment: Some(r#"["a","b","c"]"#.to_string()),
            classify: vec![],
            extract: vec![],
            extract_entities: None,
        };
        let segs = segments_from_canned(&canned).unwrap();
        assert_eq!(segs, vec!["a", "b", "c"]);
    }

    #[test]
    fn segments_from_canned_strips_whitespace_and_empties() {
        let canned = DictationCanned {
            segment: Some(r#"["  hello ","","world"]"#.to_string()),
            classify: vec![],
            extract: vec![],
            extract_entities: None,
        };
        let segs = segments_from_canned(&canned).unwrap();
        assert_eq!(segs, vec!["hello", "world"]);
    }
}
