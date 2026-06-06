//! `kg_latency_bench` — Phase 1C.0 (`mb-plz9`, ADR 0051).
//!
//! Thin shim around `mockingbird_lib::kg::run_latency_bench`. Drives
//! [`run_pipeline`] + [`apply_filed_outcome`] against a real local
//! Ollama daemon over five representative parity-fixture dictations
//! and prints per-fixture wall-clock timings as CSV on stdout, with
//! a summary block (mean / p50 / p95 / max for `total_pipeline_ms`)
//! at the end.
//!
//! Exit codes:
//!
//! - `0` — all fixtures ran cleanly.
//! - `1` — fixture parse / schema load / DB seeding / etc. failure.
//!   Details on stderr.
//! - `2` — Ollama daemon unreachable. Stdout carries a single
//!   `# ollama-unreachable` line; stderr carries the hint.
//!
//! Invocation (the wrapper sets the CUDA + MSVC env all release
//! binaries in this workspace need):
//!
//! ```text
//! powershell -File scripts\cargo-with-cuda.ps1 run --release --bin kg_latency_bench \
//!   > docs\knowledge-graph\phase-1c-latency-baseline-raw.csv
//! ```
//!
//! [`run_pipeline`]: mockingbird_lib::kg::run_pipeline

fn main() {
    let code = mockingbird_lib::kg::run_latency_bench();
    std::process::exit(code);
}
