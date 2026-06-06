//! `kg_source_gate_invariant` — ADR 0052 §"Acceptance gates" J1
//! (Wave 1D.6, `mb-q2p1`).
//!
//! Thin shim around
//! `mockingbird_lib::kg::run_source_gate_invariant_probe`. The probe
//! implementation lives inside the `kg` module so it can reach
//! `crate::dictation::try_enqueue_for_kg_filing` (the pub(crate)
//! 3-gate cascade) AND `crate::kg::ingest_text::ingest_text_note`
//! (the text-note ingest path) without widening any public surface.
//!
//! Invocation (the wrapper sets the CUDA + MSVC env that all release
//! binaries in this workspace need):
//!
//! ```text
//! powershell -File scripts\cargo-with-cuda.ps1 run --release --bin kg_source_gate_invariant
//! ```
//!
//! Exit `0` on green (all 6 corpus cells of the 3 capture_kinds ×
//! 2 toggle states matrix match expected enqueue counts), `1` on
//! any breach. Failure output goes to stderr.

fn main() {
    std::process::exit(mockingbird_lib::kg::run_source_gate_invariant_probe());
}
