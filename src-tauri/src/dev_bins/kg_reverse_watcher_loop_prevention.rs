//! `kg_reverse_watcher_loop_prevention` -- J1 (Wave 1E.9 / `mb-kazi`).
//!
//! Thin shim around
//! `mockingbird_lib::vault::judges_phase_1e::run_reverse_watcher_loop_prevention_probe`.
//! The probe implementation lives inside the `vault::judges_phase_1e`
//! module so it can reach `vault::watcher_reconcile::reconcile_entry_file`
//! (sibling-module access; no public-surface widening).
//!
//! Invocation (the wrapper sets the CUDA + MSVC env that all release
//! binaries in this workspace need):
//!
//! ```text
//! powershell -File scripts\cargo-with-cuda.ps1 run --release --bin kg_reverse_watcher_loop_prevention
//! ```
//!
//! Exit `0` on green, `1` on any assertion failure.

fn main() {
    std::process::exit(
        mockingbird_lib::vault::judges_phase_1e::run_reverse_watcher_loop_prevention_probe(),
    );
}
