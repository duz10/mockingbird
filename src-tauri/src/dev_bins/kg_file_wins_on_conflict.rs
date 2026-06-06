//! `kg_file_wins_on_conflict` -- J2 (Wave 1E.9 / `mb-kazi`).
//!
//! Thin shim around
//! `mockingbird_lib::vault::judges_phase_1e::run_file_wins_on_conflict_probe`.
//!
//! Invocation:
//!
//! ```text
//! powershell -File scripts\cargo-with-cuda.ps1 run --release --bin kg_file_wins_on_conflict
//! ```
//!
//! Exit `0` on green, `1` on any assertion failure.

fn main() {
    std::process::exit(mockingbird_lib::vault::judges_phase_1e::run_file_wins_on_conflict_probe());
}
