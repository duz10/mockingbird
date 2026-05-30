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
// Phase 1B Chunk 2 (`mb-geds`, ADR 0050) -- KG persistence half. Owns
// the five tables created by migration 024, the two concept-page
// VIEWs, and the filing queue's FIFO state machine. `enqueue_for_filing`
// is the dictation hook's call site (Chunk 4); the worker (Chunk 3)
// composes the rest.
pub(crate) mod store;
pub(crate) mod synonyms;
// Phase 1B Chunk 3 (`mb-eke8`, ADR 0050) — filing worker thread.
// Drains `kg_filing_queue` FIFO, runs the 5-pass pipeline, commits via
// `store::apply_filed_outcome` + `queue::mark_done` in a single txn.
// Spawned at boot iff `KgGraphEnabled = true` (read-once Decision C).
// Module is `pub(crate)`; the only thing `lib.rs::run()` reaches for
// is `worker::KgFilingRuntime::spawn` so the spawn site stays narrow.
pub(crate) mod worker;

// Phase 1A Wave 3 (`mb-qdgn`) — bit-identical re-run gate against the
// Wave 0.5.4 seed-42 fixture. Lives inside the kg module so it can
// drive `OllamaDispatcher` + `Schema` + `run_pipeline` without
// widening the D6 public surface beyond one new function
// (`run_parity_probe`, re-exported below). Consumed by the
// `[[bin]] kg_parity` shim at `src/bin/kg_parity.rs`.
pub(crate) mod parity;

// Embeddings dispatcher graduated per binding parameter A1 — preserved
// for entity disambiguation (NOT classification). Wired but not consumed
// in 1A; the `dead_code` allow is the honest signal that the trait is
// here intentionally without a current caller (a follow-up wave wires
// it into the entity-extraction path).
#[allow(dead_code)]
pub(crate) mod embeddings;

// D6 public surface — orchestrator entry point + schema types.
pub use passes::EntityType;
pub use pipeline::{run_pipeline, PassTimings, PipelineResult};
pub use schema::{AnswerKey, Category, Entry, EntryType, Status};

// Phase 1B Chunk 2 — only the dictation-hook call site is `pub`. The
// worker-side `apply_filed_outcome` + `SegmentOutput` are crate-internal
// (see `kg::store` module docs for the rationale)— the worker is a
// sibling module in `kg::` so it composes the lower-level pieces
// without widening the published API.
pub use store::enqueue_for_filing;

// Wave 3 (`mb-qdgn`) — narrow public re-export for the `kg_parity`
// binary shim. Returns a process exit code; everything else (the
// FixtureDispatcher, fixture types, schema-driven prompt resolution)
// stays `pub(crate)` per D6.
//
// Phase 1B Chunk 5 (`mb-k17a`, ADR 0050 §D8 gate 1) adds
// [`run_parity_probe_persist`] as a second narrow surface for the
// same shim — the `--persist` mode extends the fixture round-trip
// through the store layer + migration 024 triggers.
pub use parity::{run_parity_probe, run_parity_probe_persist};

// Phase 1B Chunk 5 (`mb-k17a`, ADR 0050 §D8 gate 2) — graph-off
// invariant probe. Lives in `kg::` so it can call the dictation-tail
// helper directly and exercise the Chunk 4 outcome gate end-to-end
// against a tempfile-backed SQLite. Consumed by
// `src/bin/kg_graph_off_invariant.rs`.
pub(crate) mod graph_off_invariant;
pub use graph_off_invariant::run_graph_off_invariant_probe;

// Phase 1C.0 (`mb-plz9`, ADR 0051) — filing-pipeline latency bench.
// Drives real Ollama against five representative parity fixtures +
// emits CSV-on-stdout for the empirical baseline doc. Consumed by
// `src/bin/kg_latency_bench.rs`. Discharges `mb-b3jy` (the
// ADR 0049 §6 ~1 min latency-budget verification).
pub(crate) mod latency_bench;
pub use latency_bench::run_latency_bench;

// Phase 1D Wave 1D.2 (`mb-j00j`, ADR 0052) -- KG dashboard data
// assembly. Pure-Rust composition of the read-only dashboard payload
// powering `/knowledge-graph`; the IPC layer
// (`commands::kg::kg_dashboard_snapshot`) is a thin wrapper. Lives
// in `kg::` (rather than `kg::store::`) because it composes existing
// store helpers across `entities` / `queue` / `search` rather than
// owning any new table; mirrors the `kg::latency_bench` precedent.
pub(crate) mod dashboard;

// Phase 1D Wave 1D.3 (`mb-0gt6`, ADR 0052) -- text-note ingest. The
// KG-screen text input fires `kg_ingest_text_note(text)`; this module
// is the persistence + enqueue half. Sibling of
// `crate::dictation::ingest` (the file-import path); both write to
// `sessions` + `transcripts` with full provenance, distinguished by
// `capture_kind`. See the module docstring for the divergence note
// from ADR 0052 §D3's original synthetic-entry-id sketch.
pub(crate) mod ingest_text;
pub use ingest_text::ingest_text_note;

// Smoke test for the public surface — confirms the wiring compiles
// and `run_pipeline` is callable via a `MockOllama`. This is NOT the
// parity probe (that lands in Chunk 3).
#[cfg(test)]
mod smoke;
