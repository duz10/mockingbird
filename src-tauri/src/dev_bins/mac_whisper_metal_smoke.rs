//! `mac_whisper_metal_smoke` — judge shim for `mac-p2-metal-transcript`
//! (mb-mac-v1.3.3). Thin wrapper over
//! [`mockingbird_lib::stt::judges_macos_v1::metal_transcript_probe`].
//!
//! Loads the GGUF whisper model with `--features metal`, transcribes a
//! known speech fixture (whisper.cpp's `jfk.wav`), and asserts:
//!   1. the transcript is NON-EMPTY, and
//!   2. the Metal backend actually ENGAGED (not a silent CPU fallback).
//!
//! Resolution order:
//!   - model:  argv[1] | $WHISPER_MODEL_PATH | $MODEL_PATH/<gguf> | <repo>/models/<gguf>
//!   - wav:    argv[2] | <repo>/src-tauri/tests/fixtures/audio/jfk.wav
//!
//! Run (via the Mac wrapper which exports MODEL_PATH + injects --features):
//!   scripts/dev/cargo-mac.sh run --release --example mac_whisper_metal_smoke
//!
//! Exit codes: 0 = pass · 1 = runtime/assert failure · 2 = wrong platform.

// Built (as a real probe) only on macOS WITH the metal feature, because
// it depends on `stt::judges_macos_v1`, which itself needs
// `whisper-rs/raw-api` (bundled into `metal`). Every other config gets a
// stub so `cargo build/clippy --all-targets` stays green without metal.
#[cfg(not(all(target_os = "macos", feature = "metal")))]
fn main() {
    eprintln!(
        "mac_whisper_metal_smoke requires macOS + `--features metal` \
         (use scripts/dev/cargo-mac.sh)"
    );
    std::process::exit(2);
}

#[cfg(all(target_os = "macos", feature = "metal"))]
fn main() {
    use std::path::PathBuf;

    use mockingbird_lib::stt::judges_macos_v1::{metal_transcript_probe, Backend};

    const GGUF: &str = "ggml-large-v3-turbo-q5_0.bin";

    // <repo>/src-tauri is CARGO_MANIFEST_DIR; <repo> is its parent.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap_or(&manifest_dir).to_path_buf();

    let mut args = std::env::args().skip(1);
    let model_arg = args.next();
    let wav_arg = args.next();

    let model_path = model_arg.map(PathBuf::from).unwrap_or_else(|| {
        if let Ok(p) = std::env::var("WHISPER_MODEL_PATH") {
            return PathBuf::from(p);
        }
        let dir = std::env::var("MODEL_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| repo_root.join("models"));
        dir.join(GGUF)
    });

    let wav_path = wav_arg.map(PathBuf::from).unwrap_or_else(|| {
        manifest_dir
            .join("tests")
            .join("fixtures")
            .join("audio")
            .join("jfk.wav")
    });

    println!("=== mac_whisper_metal_smoke (mac-p2-metal-transcript) ===");
    println!("model: {}", model_path.display());
    println!("wav:   {}", wav_path.display());
    println!();

    let report = match metal_transcript_probe(&model_path, &wav_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    println!("backend:   {}", report.backend);
    if let Some(ev) = &report.backend_evidence {
        println!("evidence:  {ev}");
    }
    println!("latency:   {} ms", report.latency_ms);
    println!("transcript:");
    println!("  {}", report.transcript);
    println!();

    let mut ok = true;
    if report.transcript.is_empty() {
        eprintln!("FAIL: transcript is empty");
        ok = false;
    }
    if report.backend != Backend::Metal {
        eprintln!(
            "FAIL: Metal backend did NOT engage (got {}). Silent CPU fallback \
             defeats the Phase 2 parity intent.",
            report.backend
        );
        eprintln!("---- captured ggml/whisper log ----");
        eprintln!("{}", report.log);
        ok = false;
    }

    if ok {
        println!("PASS: non-empty transcript via confirmed Metal backend.");
        std::process::exit(0);
    }
    std::process::exit(1);
}
