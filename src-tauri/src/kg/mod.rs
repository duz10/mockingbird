//! Knowledge-graph subsystem — schema-driven structured-entry pipeline.
//!
//! Sibling of `dictation::`, `meetings::`, `activity::`, `vault::`,
//! `inbox::`. Chartered by **ADR 0049** (Phase 0.5 + v1 architectural
//! pivot) and the **PHASE-0-5-REPORT.md** §6 commitments + §7 wave plan.
//!
//! ## Where we are right now
//!
//! **Phase 1A Wave 1 (this commit, mb-2mc9 / mb-ep3c) is scaffold only.**
//! The module body is intentionally empty. Chunk 2 graduates the library
//! subset from `experimental/kg-validation/src/` into this tree; Chunk 3
//! lands the `kg_parity` probe consuming `docs/knowledge-graph/parity/`.
//!
//! See `docs/knowledge-graph/phase-1a-brief.md` for the full graduation
//! scope, binding decisions (D1–D6), parity gate procedure, and seal
//! criteria.
//!
//! ## What graduates (Chunks 2 + 3 — DO NOT add here yet)
//!
//! - `schema` — `Entry` / `Category` / `EntryType` / `EntityType` / `Status` / `AnswerKey` types.
//! - `schema_loader` — SCHEMA.md + per-pass prompt parser (`include_str!` bundled
//!   with `MOCKINGBIRD_KG_SCHEMA_DIR` env override per D2).
//! - `passes::{segment, classify, extract, extract_entities, normalize, validate_tags}`.
//! - `ollama` — `OllamaDispatcher` trait + `ureq::Agent`-based impl (D1) plus a
//!   test-only `MockOllama`.
//! - `synonyms` — shared `SynonymMap`.
//! - `embeddings` — local-only embedding helpers (currently unused by `run_pipeline`
//!   but graduating per binding parameter 5).
//! - `pipeline` — `run_pipeline(&D, ...) -> PipelineResult`. The public surface of
//!   `kg::` exports `pipeline::{run_pipeline, PipelineResult}` and the schema types (D6).
//!
//! ## What stays in the sandbox (binding parameter 5)
//!
//! `experimental/kg-validation/` remains the v1.1+ regression rig and
//! parity-fixture source. Per ADR 0049 §"Sandbox isolation" the sandbox
//! is **not** deleted on graduation. Specifically NOT graduating:
//! `judges/`, `scoring/`, `wiggum/`, exemplars, `harness/runner.rs`, the
//! six standalone binaries, `corpus/`, `runs/`, `judge-calibration/`.
//!
//! ## Public surface (D6)
//!
//! Once Chunk 2 lands:
//! ```ignore
//! pub use pipeline::{run_pipeline, PipelineResult};
//! pub use schema::{Entry, Category, EntryType, EntityType, Status, AnswerKey};
//! // everything else: pub(crate)
//! ```
//!
//! ## NOT in scope (Phase 1B+ and beyond)
//!
//! - DB tables / migrations (Phase 1B).
//! - Retrieval UX, Tauri command wiring (Phase 1C).
//! - Backfill over existing transcripts (Phase 1D).
//! - v1 beta tag + acceptance criteria (Phase 1E).
//!
//! Phase 1A produces a **callable library that nobody calls yet**.
//! Wiring into the dictation loop is a downstream consumer concern.

// Intentionally empty — children graduate in Chunk 2 (mb-2mc9).
//
// Placeholder declarations are NOT added here ahead of Chunk 2; rustc
// would complain about missing files and the diff would have to bounce
// twice. The single source of truth for the to-be-graduated children
// is the docstring above + phase-1a-brief.md §"Scope: what graduates".
