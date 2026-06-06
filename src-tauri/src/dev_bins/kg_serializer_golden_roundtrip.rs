//! `kg_serializer_golden_roundtrip` -- J4 (Wave 1E.9 / `mb-kazi`).
//!
//! Thin shim around
//! `mockingbird_lib::vault::judges_phase_1e::run_serializer_golden_roundtrip_probe`.
//!
//! Invocation:
//!
//! ```text
//! powershell -File scripts\cargo-with-cuda.ps1 run --release --bin kg_serializer_golden_roundtrip
//! ```
//!
//! Exit `0` on green, `1` on any assertion failure.

fn main() {
    std::process::exit(
        mockingbird_lib::vault::judges_phase_1e::run_serializer_golden_roundtrip_probe(),
    );
}
