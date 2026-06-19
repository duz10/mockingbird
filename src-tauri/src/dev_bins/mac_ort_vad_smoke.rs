//! `mac_ort_vad_smoke` — judge shim for `mac-p2-ort-dylib` (mb-mac-v1.3.2).
//!
//! Confirms `ort`'s `load-dynamic` discovery finds the macOS
//! `libonnxruntime.dylib` at runtime by building a real Silero VAD ONNX
//! session. Thin wrapper over
//! [`mockingbird_lib::audio::judges_macos_v1::ort_vad_session_smoke`].
//!
//! Resolution: argv[1] | $SILERO_VAD_PATH | $MODEL_PATH/silero_vad.onnx |
//!             <repo>/models/silero_vad.onnx
//!
//! Run (via the Mac wrapper, which exports MODEL_PATH + ORT_DYLIB_PATH):
//!   scripts/dev/cargo-mac.sh run --release --example mac_ort_vad_smoke
//!
//! Exit codes: 0 = ORT loaded + session built · 1 = failure · 2 = wrong OS.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("mac_ort_vad_smoke is macOS-only (Phase 2 ORT dylib probe)");
    std::process::exit(2);
}

// Terminate immediately without running C++ atexit / static destructors.
// ONNX Runtime registers a global Environment destructor that, on macOS,
// throws `mutex lock failed` while the C++ runtime is mid-teardown,
// aborting (SIGABRT / exit 134) AFTER our work succeeded. Bypassing
// atexit with `_exit` gives the judge a deterministic exit code. No new
// crate dep -- `_exit` is in libSystem, always linked.
#[cfg(target_os = "macos")]
extern "C" {
    fn _exit(code: i32) -> !;
}

#[cfg(target_os = "macos")]
fn main() {
    use std::io::Write;
    use std::path::PathBuf;

    use mockingbird_lib::audio::judges_macos_v1::ort_vad_session_smoke;

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap_or(&manifest_dir).to_path_buf();

    let model_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if let Ok(p) = std::env::var("SILERO_VAD_PATH") {
                return PathBuf::from(p);
            }
            let dir = std::env::var("MODEL_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| repo_root.join("models"));
            dir.join("silero_vad.onnx")
        });

    println!("=== mac_ort_vad_smoke (mac-p2-ort-dylib) ===");
    println!(
        "ORT_DYLIB_PATH: {}",
        std::env::var("ORT_DYLIB_PATH").unwrap_or_else(|_| "(unset)".into())
    );
    println!("model: {}", model_path.display());
    println!();

    let code = match ort_vad_session_smoke(&model_path) {
        Ok(report) => {
            println!(
                "PASS: ORT runtime loaded + Silero session built (dylib: {}).",
                report
                    .dylib_path
                    .as_deref()
                    .unwrap_or("resolved by ort default search")
            );
            0
        }
        Err(e) => {
            eprintln!("FAIL: {e}");
            1
        }
    };
    // Flush before the hard exit; `_exit` skips the libc stdio flush.
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    // SAFETY: terminate now, skipping ORT's abort-prone static destructors.
    unsafe { _exit(code) }
}
