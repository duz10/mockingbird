//! Phase 1E judges (Wave 1E.9 / `mb-kazi`).
//!
//! Four deterministic probes that mechanically enforce ADR 0053's
//! "Obsidian as source of truth" invariants. All four are binary
//! probes (`src/bin/kg_*.rs`) rather than `#[test]`s, for the
//! LESSONS PINNED P2 reason: `cargo test --release` is blocked on
//! this box, and the gates need to actually run end-to-end at
//! seal time.
//!
//! Each probe exposes a `run_*_probe() -> i32` entry point that
//! the matching `src/bin/` shim hands to `std::process::exit`.
//! Probes return `0` on green (all assertions held) and `1` on
//! any failure, with the failing assertion printed to stderr.
//!
//! Module layout mirrors the existing `kg_parity` / `kg_source_gate`
//! / `kg_graph_off` precedent: implementation lives in the library
//! (so the probe can reach `pub(crate)` helpers without widening
//! anyone's public surface), and the `bin/` shim is a one-liner
//! that calls `std::process::exit(run_*_probe())`.
//!
//! Spec: `docs/phases/phase-1e.md` §"Wave 1E.9" + ADR 0053
//! §"Acceptance gates".
//!
//! | Judge | Probe fn | Asserts |
//! |---|---|---|
//! | J1 | [`run_reverse_watcher_loop_prevention_probe`] | Own writes (hash-matching) return `LoopPrevented`; external edits return `Reconciled`. |
//! | J2 | [`run_file_wins_on_conflict_probe`] | External edit -> DB mention rows mirror the FILE's tags/entities, not the DB's previous state. |
//! | J3 | [`run_subtree_bootstrap_idempotent_probe`] | 4 cells (missing / empty / populated / partial); back-to-back bootstrap calls converge to the same on-disk shape. |
//! | J4 | [`run_serializer_golden_roundtrip_probe`] | `parse_entry(serialize_entry(e))` -> `e`; serialize of each shipped golden fixture is byte-identical to disk. |
//!
//! All four are deterministic + LLM-free. Per AGENTS.md "Judges --
//! when": "a single one-off judge for a single bead is fine when
//! the invariant is narrow". These invariants are binary
//! (row counts, byte equality, enum discriminant) -- an LLM-graded
//! judge would only add noise.

pub mod bootstrap_idempotent;
pub mod file_wins;
pub mod loop_prevention;
pub mod serializer_golden;

pub use bootstrap_idempotent::run_subtree_bootstrap_idempotent_probe;
pub use file_wins::run_file_wins_on_conflict_probe;
pub use loop_prevention::run_reverse_watcher_loop_prevention_probe;
pub use serializer_golden::run_serializer_golden_roundtrip_probe;
