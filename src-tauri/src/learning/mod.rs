//! Phase 8 — learning loop.
//!
//! Nightly job that detects user corrections and improves the system
//! without human intervention. Rolls back on regression. Per PLAN §10.
//!
//! ## Pipeline
//!
//! ```text
//!   corrections (last 7 d) ─▶ classifier ─▶ {new_vocab, style_change,
//!                                            mistranscription, noise}
//!         │                                       │
//!         │           new_vocab ──────────────────┼──▶ dictionary
//!         │                                       │
//!         │           style_change ───────────────┼──▶ style_examples
//!         │                                       │
//!         ▼                                       ▼
//!   eval(replay last 24 h sessions, compare cleaned-vs-corrected pair-wise)
//!         │
//!         ▼
//!   correction_rate_after <= correction_rate_before ?
//!         │
//!         ├── yes ─▶ COMMIT; insert learning_runs row
//!         └── no  ─▶ ROLLBACK via audit::rollback_table_to_timestamp;
//!                    insert learning_runs row with rolled_back=1
//! ```
//!
//! ## Sub-modules
//!
//! - [`corrections`] — repository for the `corrections` table.
//! - [`runs`] — repository for the `learning_runs` table.
//! - [`classifier`] — LLM-driven correction classification (uses the
//!   same `CleanupProvider` trait Phase 4 introduced — no new
//!   infrastructure).
//! - [`promoter`] — applies the classifier output: dictionary inserts,
//!   style-example inserts + per-mode pruning to ~50.
//! - [`eval`] — replays sessions, computes correction-rate metric.
//! - [`runner`] — orchestrates the full pipeline in one transaction
//!   with automatic rollback on regression.
//! - [`scheduler`] — wraps `schtasks.exe` for Windows install/uninstall.
//!
//! ## Why classification is LLM-driven (not heuristic)
//!
//! The classifier must distinguish "new word the user always wants
//! spelled this way" (→ dictionary) from "the user prefers a more
//! formal tone here" (→ style example) from "the LLM hallucinated
//! something" (→ noise; ignore). Heuristic rules brittle; cheap local
//! LLM with a fixed prompt is the right call. Stays within the
//! local-only constraint.
//!
//! ## Tests
//!
//! Every sub-module has unit tests over the pure logic. `runner` has
//! an end-to-end test with a simulated 50-correction dataset that
//! exercises the full classify → promote → eval → commit path; a
//! synthetic regression test triggers the rollback path.

pub mod classifier;
pub mod corrections;
pub mod eval;
pub mod promoter;
pub mod runner;
pub mod runs;
pub mod scheduler;
