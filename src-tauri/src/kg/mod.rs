//! Knowledge-graph subsystem — schema-driven structured-entry pipeline.
//!
//! Sibling of `dictation::`, `meetings::`, `activity::`, `vault::`,
//! `inbox::`. Chartered by **ADR 0049** (Phase 0.5 + v1 architectural
//! pivot) and the **PHASE-0-5-REPORT.md** §6 commitments + §7 wave plan.
//!
//! ## Status
//!
//! **Phase 1A Wave 2 (this commit, epic `mb-2mc9`).** The graduated
//! subset from `experimental/kg-validation/src/` now lives here. The
//! sandbox stays alive as the v1.1+ regression rig (binding parameter D5);
//! it is the source of the parity fixture Chunk 3 will probe against.
//!
//! ## Public surface (binding parameter D6)
//!
//! Only two functions + the schema types are `pub` from this module:
//! every consumer outside `kg::` should reach for these and nothing
//! else.
//!
//! ```ignore
//! use mockingbird_lib::kg::{run_pipeline, PipelineResult};
//! use mockingbird_lib::kg::{Entry, Category, EntryType, EntityType, Status, AnswerKey};
//! ```
//!
//! The orchestrator, the pass internals, the dispatcher trait, the
//! schema loader, the synonym map, and the embeddings dispatcher are
//! all `pub(crate)` so we can land follow-up wiring in this crate
//! without re-litigating the public boundary.
//!
//! ## Asset bundling (binding parameter D2)
//!
//! `SCHEMA.md` + per-pass prompt bodies are baked into the binary via
//! `include_str!` at compile time. At runtime the `MOCKINGBIRD_KG_SCHEMA_DIR`
//! env var overrides the bundled set — useful for prompt tuning without
//! rebuilds. Source-of-truth selection is **either / or, never merge**;
//! the loader emits a `tracing::info!` line at startup naming which
//! source won (see `schema_loader::Schema::load_default`).
//!
//! ## What stays in the sandbox (binding parameter D5)
//!
//! `experimental/kg-validation/` remains the v1.1+ regression rig and
//! parity-fixture source. Per ADR 0049 §"Sandbox isolation" the sandbox
//! is **not** deleted on graduation. Specifically NOT graduating:
//! `judges/`, `scoring/`, `wiggum/`, exemplars, `harness/runner.rs`, the
//! six standalone binaries, `corpus/`, `runs/`, `judge-calibration/`.
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

pub(crate) mod ollama;
pub(crate) mod passes;
pub(crate) mod pipeline;
pub(crate) mod schema;
pub(crate) mod schema_loader;
pub(crate) mod synonyms;

// Embeddings dispatcher graduated per binding parameter A1 — preserved
// for entity disambiguation (NOT classification). Wired but not consumed
// in 1A; the `dead_code` allow is the honest signal that the trait is
// here intentionally without a current caller (a follow-up wave wires
// it into the entity-extraction path).
#[allow(dead_code)]
pub(crate) mod embeddings;

// D6 public surface — orchestrator entry point + schema types.
pub use passes::EntityType;
pub use pipeline::{run_pipeline, PipelineResult};
pub use schema::{AnswerKey, Category, Entry, EntryType, Status};

// Smoke test for the public surface — confirms the wiring compiles
// and `run_pipeline` is callable via a `MockOllama`. This is NOT the
// parity probe (that lands in Chunk 3).
#[cfg(test)]
mod smoke;
