//! `mac_dictation_backends_smoke` — judge shim for
//! `mac-p3-dictation-backends-ungated` (mb-mac-v1.4.7a). Thin wrapper
//! over [`mockingbird_lib::dictation::judges_macos_v1::ungate_backends_probe`].
//!
//! Proves the `.4.7a` un-gating: the audio-capture, VAD, and STT
//! `make_default_*` factories all return `Ok` on macOS, and the
//! production `WhisperStt` transcribes whisper.cpp's `jfk.wav` to a
//! non-empty transcript via a CONFIRMED Metal backend.
//!
//! Resolution order:
//!   - model:  argv[1] | $WHISPER_MODEL_PATH | $MODEL_PATH/<gguf> | <repo>/models/<gguf>
//!   - wav:    argv[2] | <repo>/src-tauri/tests/fixtures/audio/jfk.wav
//!
//! Run (via the Mac wrapper which exports MODEL_PATH + ORT_DYLIB_PATH
//! and injects --features metal):
//!   scripts/dev/cargo-mac.sh run --release --example mac_dictation_backends_smoke
//!
//! Exit codes: 0 = pass · 1 = runtime/assert failure · 2 = wrong platform.

// Built (as a real probe) only on macOS WITH the metal feature, because
// it depends on `dictation::judges_macos_v1`, which confirms the Metal
// backend via `whisper-rs/raw-api` (bundled into `metal`). Every other
// config gets a stub so `cargo build/clippy --all-targets` stays green
// without metal.
#[cfg(not(all(target_os = "macos", feature = "metal")))]
fn main() {
    eprintln!(
        "mac_dictation_backends_smoke requires macOS + `--features metal` \
         (use scripts/dev/cargo-mac.sh)"
    );
    std::process::exit(2);
}

#[cfg(all(target_os = "macos", feature = "metal"))]
fn main() {
    use std::io::Write;
    use std::path::PathBuf;

    // After onnxruntime (the VAD `ort` session) is loaded, its global
    // teardown aborts during the libc `exit()` static-destructor chain
    // ("mutex lock failed") — which would mask our true status with
    // SIGABRT (134). `_exit` flushes nothing and runs no atexit handlers,
    // so we flush our own streams first, then skip the buggy teardown.
    fn clean_exit(code: i32) -> ! {
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        // SAFETY: deliberate fast process exit to dodge onnxruntime's
        // broken global-destructor teardown. All our work + I/O is done.
        unsafe { libc::_exit(code) }
    }

    use mockingbird_lib::dictation::judges_macos_v1::ungate_backends_probe;
    use mockingbird_lib::stt::judges_macos_v1::Backend;

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

    println!("=== mac_dictation_backends_smoke (mac-p3-dictation-backends-ungated) ===");
    println!("model: {}", model_path.display());
    println!("wav:   {}", wav_path.display());
    if let Ok(p) = std::env::var("ORT_DYLIB_PATH") {
        println!("ort:   {p}");
    }
    println!();

    let report = match ungate_backends_probe(&model_path, &wav_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            clean_exit(1);
        }
    };

    println!(
        "capture factory: {}",
        if report.capture_ok { "Ok" } else { "Err" }
    );
    println!(
        "vad factory:     {}",
        if report.vad_ok { "Ok" } else { "Err" }
    );
    println!(
        "stt factory:     {}",
        if report.stt_ok { "Ok" } else { "Err" }
    );
    println!("backend:         {}", report.backend);
    if let Some(ev) = &report.backend_evidence {
        println!("evidence:        {ev}");
    }
    println!("gpu_used flag:   {}", report.gpu_used);
    println!("latency:         {} ms", report.latency_ms);
    println!("transcript:");
    println!("  {}", report.transcript);
    println!();

    let mut ok = true;
    if !(report.capture_ok && report.vad_ok && report.stt_ok) {
        eprintln!("FAIL: one or more make_default_* factories did not return Ok");
        ok = false;
    }
    if report.transcript.is_empty() {
        eprintln!("FAIL: transcript is empty");
        ok = false;
    }
    if report.backend != Backend::Metal {
        eprintln!(
            "FAIL: Metal backend did NOT engage (got {}). Silent CPU fallback \
             defeats the .4.7a GPU-STT intent.",
            report.backend
        );
        eprintln!("---- captured ggml/whisper log ----");
        eprintln!("{}", report.log);
        ok = false;
    }

    if ok {
        println!(
            "PASS: capture/VAD/STT factories Ok + non-empty transcript via confirmed Metal backend."
        );
        clean_exit(0);
    }
    clean_exit(1);
}
