//! `kg_parity` — Phase 1A graduation gate (`mb-2mc9` / `mb-qdgn`).
//!
//! Thin shim around [`mockingbird_lib::kg::run_parity_probe`]. The
//! probe implementation lives inside the `kg` module so it has direct
//! access to `pub(crate)` items like the `OllamaDispatcher` trait + the
//! `Schema` loader without widening the kg public surface beyond the
//! one new function this binary calls.
//!
//! Invocation (the wrapper sets the CUDA + MSVC env that all release
//! binaries in this workspace need):
//!
//! ```text
//! powershell -File scripts\cargo-with-cuda.ps1 run --release --bin kg_parity
//! ```
//!
//! Exit `0` on `32/32` parity, `1` on any divergence or fixture
//! load failure. Failure output goes to stderr.

fn main() {
    std::process::exit(mockingbird_lib::kg::run_parity_probe());
}
