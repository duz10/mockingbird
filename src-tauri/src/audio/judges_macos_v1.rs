//! macOS ORT dylib discovery smoke (mb-mac-v1.3.2 / judge `mac-p2-ort-dylib`).
//!
//! Phase 2 needs to prove that `ort`'s `load-dynamic` discovery finds the
//! macOS `libonnxruntime.dylib` at runtime — the same dlopen path the
//! Phase 3/4 cross-platform Silero VAD will rely on. The live `SileroVad`
//! is still `#[cfg(target_os = "windows")]`, so this probe exercises the
//! load-bearing piece directly: build an `ort` Session from
//! `silero_vad.onnx`. A successful build means the dylib was located
//! (via `ORT_DYLIB_PATH`, wired by `scripts/dev/cargo-mac.sh`) and the
//! model graph loaded — i.e. no `dylib-not-found` panic.
//!
//! macOS-only; compiles to nothing elsewhere.
#![cfg(target_os = "macos")]

use std::path::Path;

/// Outcome of the ORT/VAD session smoke.
#[derive(Debug, Clone)]
pub struct OrtSmokeReport {
    /// The `ORT_DYLIB_PATH` the runtime resolved against, if set.
    pub dylib_path: Option<String>,
    /// The Silero model the session was built from.
    pub model_path: String,
}

/// Build an `ort` Session from `model_path` (Silero VAD ONNX), mirroring
/// the exact builder chain the Windows `SileroVad::from_path` uses.
///
/// Returns `Ok` iff the ONNX Runtime dylib was found + loaded and the
/// graph committed without panic. `Err` carries the failure reason
/// (missing model, dylib-not-found, malformed graph, ...).
pub fn ort_vad_session_smoke(model_path: &Path) -> Result<OrtSmokeReport, String> {
    use ort::session::builder::GraphOptimizationLevel;

    if !model_path.is_file() {
        return Err(format!(
            "silero_vad.onnx not found at {} (run scripts/download-models.sh)",
            model_path.display()
        ));
    }

    let dylib_path = std::env::var("ORT_DYLIB_PATH").ok();

    // Session construction is where `ort` lazily dlopen's the runtime;
    // a missing/incompatible dylib surfaces here.
    let _session = ort::session::Session::builder()
        .map_err(|e| format!("ort Session::builder (dylib load?): {e}"))?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| format!("set optimization level: {e}"))?
        .commit_from_file(model_path)
        .map_err(|e| format!("commit_from_file {}: {e}", model_path.display()))?;

    Ok(OrtSmokeReport {
        dylib_path,
        model_path: model_path.display().to_string(),
    })
}
