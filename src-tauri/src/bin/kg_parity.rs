//! `kg_parity` — Phase 1A graduation gate (`mb-2mc9` / `mb-qdgn`) +
//! Phase 1B Chunk 5 store-layer round-trip gate (`mb-k17a` / ADR 0050
//! §D8 gate 1).
//!
//! Thin shim around the two `pub` entry points the `kg` module exposes:
//!
//! - Default (no flags) — `mockingbird_lib::kg::run_parity_probe`.
//!   32 fixtures, semantic JSON equality vs the Wave 0.5.4 seed-42
//!   sealed run.
//! - `--persist` — `mockingbird_lib::kg::run_parity_probe_persist`.
//!   Same 32 fixtures, plus per-fixture round-trip through the
//!   `kg::store::*` layer against a tempfile-backed SQLite with all
//!   24 migrations applied + `PRAGMA foreign_keys = ON`. Asserts row
//!   counts, idempotency of `apply_filed_outcome`, and that migration
//!   024's `BEFORE UPDATE` immutability triggers actually fire.
//!
//! Both modes return `0` on green, `1` on any divergence or
//! fixture-load failure. Failure output goes to stderr.
//!
//! Invocation (the wrapper sets the CUDA + MSVC env that all release
//! binaries in this workspace need):
//!
//! ```text
//! powershell -File scripts\cargo-with-cuda.ps1 run --release --bin kg_parity
//! powershell -File scripts\cargo-with-cuda.ps1 run --release --bin kg_parity -- --persist
//! ```

fn main() {
    let persist = std::env::args().skip(1).any(|a| a == "--persist");
    let code = if persist {
        mockingbird_lib::kg::run_parity_probe_persist()
    } else {
        mockingbird_lib::kg::run_parity_probe()
    };
    std::process::exit(code);
}
