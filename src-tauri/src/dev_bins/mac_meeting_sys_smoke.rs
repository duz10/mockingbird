//! `mac_meeting_sys_smoke` — judge shim for
//! `mac-p4b-sys-meeting-roundtrip` (mb-mac-v1.5.3, ADR 0068). Thin
//! wrapper over
//! [`mockingbird_lib::meetings::judges_macos_v1::sys_meeting_roundtrip_probe`].
//!
//! Proves Phase 4b: a SYSTEM-audio meeting flows end to end on macOS
//! through the real pipeline — a doubled system source (jfk.wav replayed
//! as the system channel) is captured + chunked (WAV + CRC32) with
//! `Channel::Sys` attribution, transcribed by the production Whisper on a
//! CONFIRMED Metal backend, formatted deterministically, persisted as a
//! `source = system` meeting, and read back with non-empty sys prose —
//! PLUS a construction + start/stop lifecycle smoke of the REAL
//! `SckSysCapture` (the ScreenCaptureKit backend).
//!
//! Real vs doubled vs deferred:
//!   - REAL: TwinStreamCapture + chunker + LongFormStt(Metal) + formatter
//!     + persist + read-back, and the real SckSysCapture lifecycle smoke.
//!   - DOUBLED: the physical system-audio device (jfk.wav replay).
//!   - DEFERRED-TO-USER: live SCK capture of real system audio — the
//!     grant-gated `mac-p4b-sys-meeting-e2e` (grant Screen Recording →
//!     play audio → start a System/Both meeting → confirm a Channel::Sys
//!     transcript).
//!
//! Resolution order:
//!   - model:  argv[1] | $WHISPER_MODEL_PATH | $MODEL_PATH/<gguf> | <repo>/models/<gguf>
//!   - wav:    argv[2] | <repo>/src-tauri/tests/fixtures/audio/jfk.wav
//!
//! Run (via the Mac wrapper which exports MODEL_PATH + ORT_DYLIB_PATH
//! and injects --features metal):
//!   scripts/dev/cargo-mac.sh run --release --example mac_meeting_sys_smoke
//!
//! Exit codes: 0 = pass · 1 = runtime/assert failure · 2 = wrong platform.

// Built (as a real probe) only on macOS WITH the metal feature — it
// depends on `meetings::judges_macos_v1`, which transcribes via Whisper
// on Metal. Every other config gets a stub so `cargo build/clippy
// --all-targets` stays green without metal.
#[cfg(not(all(target_os = "macos", feature = "metal")))]
fn main() {
    eprintln!(
        "mac_meeting_sys_smoke requires macOS + `--features metal` \
         (use scripts/dev/cargo-mac.sh)"
    );
    std::process::exit(2);
}

#[cfg(all(target_os = "macos", feature = "metal"))]
fn main() {
    use std::io::Write;
    use std::path::PathBuf;

    use mockingbird_lib::meetings::judges_macos_v1::sys_meeting_roundtrip_probe;
    use mockingbird_lib::meetings::{MeetingSource, MeetingStatus};

    fn clean_exit(code: i32) -> ! {
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        // SAFETY: deliberate fast process exit; all our work + I/O is done.
        unsafe { libc::_exit(code) }
    }

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

    println!("=== mac_meeting_sys_smoke (mac-p4b-sys-meeting-roundtrip) ===");
    println!("model: {}", model_path.display());
    println!("wav:   {}", wav_path.display());
    println!();

    let report = match sys_meeting_roundtrip_probe(&model_path, &wav_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            clean_exit(1);
        }
    };

    println!(
        "SckSysCapture constructs: {}",
        report.sck_capture_constructs_ok
    );
    println!(
        "SckSysCapture start/stop: {} (no panic)",
        report.sck_start_stop_no_panic
    );
    println!(
        "probe system_available:   {} (grant-dependent, informational)",
        report.probe_system_available
    );
    println!("chunk wav files:          {}", report.chunk_wav_count);
    println!("sys segments:             {}", report.sys_segments);
    println!("gpu_used (Metal):         {}", report.gpu_used);
    println!("persisted rowid:          {}", report.persisted_rowid);
    println!(
        "persisted status:         {}",
        report.persisted_status.as_db_str()
    );
    println!(
        "persisted source:         {}",
        report.persisted_source.as_db_str()
    );
    println!(
        "formatted sys (DB len):   {}",
        report.persisted_formatted_sys_len
    );
    println!("formatted sys prose:");
    println!("  {}", report.formatted_sys);
    println!();

    let mut ok = true;
    if !report.sck_capture_constructs_ok {
        eprintln!("FAIL: real SckSysCapture did not construct on macOS");
        ok = false;
    }
    if !report.sck_start_stop_no_panic {
        eprintln!("FAIL: SckSysCapture start/stop lifecycle panicked");
        ok = false;
    }
    if report.chunk_wav_count == 0 {
        eprintln!("FAIL: no chunk WAV files were written by the chunker");
        ok = false;
    }
    if report.sys_segments == 0 {
        eprintln!("FAIL: LongFormStt produced zero sys segments (CRC/transcribe path)");
        ok = false;
    }
    if report.formatted_sys.is_empty() {
        eprintln!("FAIL: formatted sys prose is empty");
        ok = false;
    }
    if !report.gpu_used {
        eprintln!(
            "FAIL: sys chunk did not report gpu_used = true — silent CPU fallback \
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
    if report.persisted_source != MeetingSource::System {
        eprintln!(
            "FAIL: persisted source is {} (want system)",
            report.persisted_source.as_db_str()
        );
        ok = false;
    }
    if report.persisted_formatted_sys_len == 0 {
        eprintln!("FAIL: read-back meeting has empty formatted sys transcript");
        ok = false;
    }

    if ok {
        println!(
            "PASS: system-audio meeting captured → chunked (WAV+CRC32, Channel::Sys) → \
             transcribed via confirmed Metal → formatted → persisted → read back as a \
             complete system meeting; real SckSysCapture construct + start/stop smoke clean."
        );
        clean_exit(0);
    }
    clean_exit(1);
}
