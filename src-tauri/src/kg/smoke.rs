//! Public-surface smoke test for `kg::` — confirms the wiring
//! compiles and `run_pipeline` is callable via the D6 surface
//! using a `MockOllama`.
//!
//! **This is NOT the parity probe.** Chunk 3 (mb-2mc9 epic) lands
//! `kg_parity` against `docs/knowledge-graph/parity/wave-0.5.4-seed-42.json`.
//! This file only proves that the public re-exports in `kg::mod.rs`
//! (D6) are sufficient for an in-crate caller to drive the pipeline
//! end-to-end with mocked I/O.

use super::ollama::testing::MockOllama;
use super::ollama::GenerateOptions;
use super::schema_loader::Schema;
use super::{run_pipeline, AnswerKey, Category, EntityType, Entry, EntryType, Status};

#[test]
fn public_surface_runs_pipeline_with_mock_dispatcher() {
    // Two canned responses, exactly per the kickoff brief:
    //   1. segment → one segment
    //   2. classify + extract for that segment (extract rule first;
    //      first-match-wins, and the extract prompt contains the
    //      segment text but also the CLASSIFICATION marker).
    let mock = MockOllama::new()
        .respond_when("DICTATION", r#"["call the dentist on Friday"]"#)
        .respond_when(
            "call the dentist on Friday\nCLASSIFICATION",
            r#"{"title":"Call the dentist","due_iso":"2026-06-19","raw_topic_tags":["dentist"]}"#,
        )
        .respond_when(
            "SEGMENT:\ncall the dentist on Friday",
            r#"{"category":"personal","entry_type":"task"}"#,
        );

    let schema = Schema::load_bundled().expect("bundled schema loads");
    let result = run_pipeline(
        &mock,
        &schema,
        None,
        "qwen2.5:3b-instruct-q4_K_M",
        "smoke-1",
        "Call the dentist on Friday.",
        "2026-06-14T08:00:00Z",
        &GenerateOptions::default(),
        None, // No artifact dir — exercises the D7 Option<&Path> path too.
    );

    assert!(
        result.per_pass_errors.is_empty(),
        "smoke pipeline produced errors: {:?}",
        result.per_pass_errors
    );
    assert_eq!(result.entries.len(), 1, "expected exactly one entry");

    let entry: &Entry = &result.entries[0];
    assert_eq!(entry.title, "Call the dentist");
    assert_eq!(entry.category, Category::Personal);
    assert_eq!(entry.entry_type, EntryType::Task);
    assert_eq!(entry.status, Some(Status::Todo));
    assert_eq!(entry.due_iso.as_deref(), Some("2026-06-19"));
    assert_eq!(entry.topic_tags, vec!["dentist"]);
}

/// Belt-and-braces compile-time check: the re-exported D6 types
/// can be NAMED through `super::` (i.e. the `pub use` lines in
/// `kg::mod.rs` are wired correctly). If a future refactor
/// accidentally demotes one of these to `pub(crate)`, this stops
/// compiling.
#[test]
fn d6_public_types_are_nameable() {
    // The function bodies don't matter — the assertion is that the
    // type names resolve via the public `use super::*` re-exports.
    fn _entry(_: Entry) {}
    fn _category(_: Category) {}
    fn _entry_type(_: EntryType) {}
    fn _entity_type(_: EntityType) {}
    fn _status(_: Status) {}
    fn _answer_key(_: AnswerKey) {}
}
