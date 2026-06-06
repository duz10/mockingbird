//! `kg_graph_off_invariant` — ADR 0050 §D8 gate 2 (`mb-k17a`).
//!
//! Thin shim around `mockingbird_lib::kg::run_graph_off_invariant_probe`.
//! The probe implementation lives inside the `kg` module so it can
//! reach `crate::dictation::try_enqueue_for_kg_filing` (the Chunk 4
//! free-fn helper) without widening any public surface.
//!
//! Invocation (the wrapper sets the CUDA + MSVC env that all release
//! binaries in this workspace need):
//!
//! ```text
//! powershell -File scripts\cargo-with-cuda.ps1 run --release --bin kg_graph_off_invariant
//! ```
//!
//! Exit `0` on green (all eight `InjectionOutcome` variants leave
//! every `kg_*` table empty under `KgGraphEnabled=false`, plus the
//! positive-control flip), `1` on any assertion failure. Failure
//! output goes to stderr.

fn main() {
    std::process::exit(mockingbird_lib::kg::run_graph_off_invariant_probe());
}
