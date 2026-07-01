//! `mac_meeting_mic_smoke` — judge shim for
//! `mac-p4a-mic-meeting-roundtrip` (mb-mac-v1.5.1). Thin wrapper over
//! [`mockingbird_lib::meetings::judges_macos_v1::mic_meeting_roundtrip_probe`].
//!
//! Proves Phase 4a: a MIC-ONLY meeting flows end to end on macOS
//! through the real pipeline — the un-gated mic backend constructs,
//! the source probe reports system-audio UNavailable (4b gate intact),
//! a test-double mic (jfk.wav) is captured + chunked (WAV + CRC32),
//! transcribed by the production Whisper on a CONFIRMED Metal backend,
//! formatted deterministically, persisted, and read back as a
//! `Complete`, `source = mic` meeting with non-empty prose.
//!
//! Resolution order:
//!   - model:  argv[1] | $WHISPER_MODEL_PATH | $MODEL_PATH/<gguf> | <repo>/models/<gguf>
//!   - wav:    argv[2] | <repo>/src-tauri/tests/fixtures/audio/jfk.wav
//!
//! Run (via the Mac wrapper which exports MODEL_PATH + ORT_DYLIB_PATH
//! and injects --features metal):
//!   scripts/dev/cargo-mac.sh run --release --example mac_meeting_mic_smoke
//!
//! Exit codes: 0 = pass · 1 = runtime/assert failure · 2 = wrong platform.

// Built (as a real probe) only on macOS WITH the metal feature — it
// depends on `meetings::judges_macos_v1`, which transcribes via Whisper
// on Metal. Every other config gets a stub so `cargo build/clippy
// --all-targets` stays green without metal.
#[cfg(not(all(target_os = "macos", feature = "metal")))]
fn main() {
    eprintln!(
        "mac_meeting_mic_smoke requires macOS + `--features metal` \
         (use scripts/dev/cargo-mac.sh)"
    );
    std::process::exit(2);
}

#[cfg(all(target_os = "macos", feature = "metal"))]
fn main() {
    use std::io::Write;
    use std::path::PathBuf;

    use mockingbird_lib::meetings::judges_macos_v1::mic_meeting_roundtrip_probe;
    use mockingbird_lib::meetings::{MeetingSource, MeetingStatus};

    // Whisper's `ort`-free here, but keep the `_exit` convention the
    // other mac judges use (flush our own I/O, skip buggy teardown).
    fn clean_exit(code: i32) -> ! {
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        // SAFETY: deliberate fast process exit; all our work + I/O is done.
        unsafe { libc::_exit(code) }
    }

    // The real model file shipped in <repo>/models.
    const GGUF: &str = "whisper-large-v3-turbo-q5_0.bin";

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

    println!("=== mac_meeting_mic_smoke (mac-p4a-mic-meeting-roundtrip) ===");
    println!("model: {}", model_path.display());
    println!("wav:   {}", wav_path.display());
    println!();

    let report = match mic_meeting_roundtrip_probe(&model_path, &wav_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            clean_exit(1);
        }
    };

    println!(
        "mic backend constructs: {}",
        report.mic_backend_constructs_ok
    );
    println!("probe mic_available:    {}", report.probe_mic_available);
    println!("probe system_available: {}", report.probe_system_available);
    println!("chunk wav files:        {}", report.chunk_wav_count);
    println!("mic segments:           {}", report.mic_segments);
    println!("gpu_used (Metal):       {}", report.gpu_used);
    println!("persisted rowid:        {}", report.persisted_rowid);
    println!(
        "persisted status:       {}",
        report.persisted_status.as_db_str()
    );
    println!(
        "persisted source:       {}",
        report.persisted_source.as_db_str()
    );
    println!(
        "formatted mic (DB len): {}",
        report.persisted_formatted_mic_len
    );
    println!("formatted mic prose:");
    println!("  {}", report.formatted_mic);
    println!();

    let mut ok = true;
    if !report.mic_backend_constructs_ok {
        eprintln!("FAIL: mic backend (make_default_capture) did not construct on macOS");
        ok = false;
    }
    if report.probe_system_available {
        eprintln!(
            "FAIL: probe reports system_available = true on macOS; system/loopback \
             capture is Phase 4b (ScreenCaptureKit) and must stay unavailable in 4a"
        );
        ok = false;
    }
    if report.chunk_wav_count == 0 {
        eprintln!("FAIL: no chunk WAV files were written by the chunker");
        ok = false;
    }
    if report.mic_segments == 0 {
        eprintln!("FAIL: LongFormStt produced zero mic segments (CRC/transcribe path)");
        ok = false;
    }
    if report.formatted_mic.is_empty() {
        eprintln!("FAIL: formatted mic prose is empty");
        ok = false;
    }
    if !report.gpu_used {
        eprintln!(
            "FAIL: mic chunk did not report gpu_used = true — silent CPU fallback \
             defeats the Metal-STT intent"
        );
        ok = false;
    }
    if report.persisted_status != MeetingStatus::Complete {
        eprintln!(
            "FAIL: persisted status is {} (want complete)",
            report.persisted_status.as_db_str()
        );
        ok = false;
    }
    if report.persisted_source != MeetingSource::Mic {
        eprintln!(
            "FAIL: persisted source is {} (want mic)",
            report.persisted_source.as_db_str()
        );
        ok = false;
    }
    if report.persisted_formatted_mic_len == 0 {
        eprintln!("FAIL: read-back meeting has empty formatted mic transcript");
        ok = false;
    }

    if ok {
        println!(
            "PASS: mic-only meeting captured → chunked (WAV+CRC32) → transcribed via \
             confirmed Metal → formatted → persisted → read back as a complete mic meeting."
        );
        clean_exit(0);
    }
    clean_exit(1);
}
