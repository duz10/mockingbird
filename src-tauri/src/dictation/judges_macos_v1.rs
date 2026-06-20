//! macOS dictation-backend un-gating judge
//! (mb-mac-v1.4.7a / judge `mac-p3-dictation-backends-ungated`).
//!
//! Phase 3 `.4.7a` widens the audio-capture, VAD, and STT `#[cfg]`
//! gates from Windows-only to `any(windows, macos)`, so the three
//! `make_default_*` factories construct real backends on macOS. This
//! probe is the in-tree proof of that un-gating:
//!
//!   1. [`crate::audio::make_default_capture`] returns `Ok` (a real
//!      `CpalCapture` over the CoreAudio mic — constructed only, never
//!      `start()`ed, so no Microphone grant is needed here).
//!   2. [`crate::audio::vad::make_default_vad`] returns `Ok` (a real
//!      Silero `ort` session — exercises the macOS `libonnxruntime.dylib`
//!      discovery wired into `ensure_ort_dylib_set`).
//!   3. [`crate::stt::make_default_stt`] returns `Ok`, AND the PRODUCTION
//!      [`crate::stt::whisper::WhisperStt`] it builds transcribes
//!      whisper.cpp's `jfk.wav` to a NON-EMPTY transcript via a
//!      CONFIRMED Metal backend (not a silent CPU fallback).
//!
//! The Metal confirmation reuses [`crate::stt::judges_macos_v1`]'s
//! ggml/whisper log-capture trampoline + classifier (DRY) rather than
//! re-implementing backend detection. Unlike `metal_transcript_probe`
//! (which drives a raw `WhisperContext`), this probe goes through the
//! real `make_default_stt` -> `WhisperStt::new` production path — the
//! one the dictation runtime will spawn in `.4.7c`.
//!
//! Gated on macOS + the `metal` feature (the backend confirmation needs
//! `whisper-rs/raw-api`, which `metal` pulls in); compiles to nothing
//! on every other config so the default cross-platform build stays green.
#![cfg(all(target_os = "macos", feature = "metal"))]

use std::path::Path;

use crate::audio::vad::{VadFrame, VoiceActivityDetector};
use crate::audio::AudioCapture;
use crate::dictation::runtime::{DictationRuntime, OrchestratorDeps, OrchestratorDepsFn};
use crate::error::AppResult;
use crate::stt::judges_macos_v1::Backend;
use crate::stt::{SpeechToText, TranscribeRequest, Transcript};

/// Outcome of the `.4.7a` dictation-backend un-gate probe.
#[derive(Debug, Clone)]
pub struct UngateReport {
    /// `make_default_capture()` returned `Ok`.
    pub capture_ok: bool,
    /// `make_default_vad()` returned `Ok`.
    pub vad_ok: bool,
    /// `make_default_stt()` returned `Ok`.
    pub stt_ok: bool,
    /// Trimmed transcript of `jfk.wav` through the production WhisperStt.
    pub transcript: String,
    /// Whether the production `Transcript.gpu_used` flag came back true.
    pub gpu_used: bool,
    /// Classified backend from the captured ggml/whisper log.
    pub backend: Backend,
    /// The exact log line that proved the backend, if any.
    pub backend_evidence: Option<String>,
    /// End-to-end transcribe latency (ms).
    pub latency_ms: u64,
    /// Full captured ggml/whisper log (diagnostics).
    pub log: String,
}

/// Construct each dictation backend via its `make_default_*` factory and
/// transcribe `wav_path` through the production STT wrapper.
///
/// `model_path` pins the GGUF whisper model (the probe sets
/// `WHISPER_MODEL_PATH` so the production `WhisperStt::new` locator finds
/// it). Returns `Err` on any hard failure (factory `Err`, model/wav
/// missing, inference error). A successful return still requires the
/// caller (the judge shim) to assert `transcript` is non-empty and
/// `backend == Metal`.
pub fn ungate_backends_probe(model_path: &Path, wav_path: &Path) -> Result<UngateReport, String> {
    // 1. Audio capture factory. Construction only — opening the device
    //    (start()) needs a Microphone grant and is out of scope for the
    //    construct-and-assert-Ok contract.
    let _capture =
        crate::audio::make_default_capture().map_err(|e| format!("make_default_capture: {e}"))?;
    let capture_ok = true;

    // 2. VAD factory — builds a real Silero ort session (dylib discovery).
    let _vad =
        crate::audio::vad::make_default_vad().map_err(|e| format!("make_default_vad: {e}"))?;
    let vad_ok = true;

    // 3. STT factory + production WhisperStt over jfk.wav, with Metal
    //    confirmation. Begin capturing the backend-init log BEFORE the
    //    factory builds the WhisperContext.
    crate::stt::judges_macos_v1::begin_backend_capture();

    if !model_path.is_file() {
        return Err(format!(
            "whisper model not found at {} (run scripts/download-models.sh)",
            model_path.display()
        ));
    }
    // SAFETY: probe runs single-threaded at startup; no other thread is
    // reading WHISPER_MODEL_PATH yet. This routes make_default_stt's
    // production locator at the pinned model.
    std::env::set_var("WHISPER_MODEL_PATH", model_path);

    let mut stt = crate::stt::make_default_stt().map_err(|e| format!("make_default_stt: {e}"))?;
    let stt_ok = true;

    let audio = crate::stt::judges_macos_v1::read_wav_16k_mono_i16(wav_path)?;
    let req = TranscribeRequest {
        audio: &audio,
        initial_prompt: None,
        force_cpu: false,
    };
    let transcript = stt
        .transcribe(req)
        .map_err(|e| format!("WhisperStt::transcribe: {e}"))?;

    let (backend, backend_evidence, log) = crate::stt::judges_macos_v1::classify_captured_backend();

    Ok(UngateReport {
        capture_ok,
        vad_ok,
        stt_ok,
        transcript: transcript.text.trim().to_string(),
        gpu_used: transcript.gpu_used,
        backend,
        backend_evidence,
        latency_ms: transcript.latency_ms,
        log,
    })
}

// --------------------------------------------------------------------
// `.4.7c` — DictationRuntime spawn + teardown judge
// (judge `mac-p3-dictation-runtime-spawn`, ADR 0063)
// --------------------------------------------------------------------
//
// Probes that `DictationRuntime` SPAWNS and TEARS DOWN cleanly on macOS.
// What is REAL vs DOUBLED (honest boundary, ADR 0063):
//
//   REAL: `spawn_with_deps` (channels + threads + struct construction),
//   the hotkey listener (`make_default_listener` → `MacKeyboardHook`:
//   real `install()` + Drop teardown — degrades gracefully to inert
//   without Input Monitoring, so it needs no TCC grant in CI), the
//   injector / window-context / secure-guard factories (permission-free
//   to construct), the cleaner (real `PassthroughCleaner` fallback), and
//   a real in-memory DB + config + vault.
//
//   DOUBLED: audio / VAD / STT — a real mic, ORT load, and Metal model
//   are device/permission/heavy deps the spawn/teardown contract must
//   NOT depend on. The doubles below stand in for them.
//
// The probe asserts spawn returns `Ok` (threads start, no panic) and
// `drop(runtime)` completes promptly without hang or panic. It does NOT
// assert the dictation thread joins (it is detached by design — the
// orchestrator holds a `hotkey_tx` clone, so its channel cascade
// completes at process exit; see ADR 0063). Real PTT / CGEventTap / mic
// capture stay the human-gated `mac-p3-dictation-e2e`.

/// Silent mic double — never yields samples. Stands in for `CpalCapture`.
struct SilentCapture;

impl AudioCapture for SilentCapture {
    fn start(&mut self) -> AppResult<()> {
        Ok(())
    }
    fn stop(&mut self) -> AppResult<()> {
        Ok(())
    }
    fn drain(&mut self, _buf: &mut Vec<i16>) -> AppResult<usize> {
        Ok(0)
    }
    fn sample_rate(&self) -> u32 {
        16_000
    }
    fn channels(&self) -> u16 {
        1
    }
}

/// VAD double — always "no speech". Stands in for the ORT `SileroVad`
/// (and dodges the onnxruntime teardown-abort entirely, since no `ort`
/// session is ever loaded).
struct NoVad;

impl VoiceActivityDetector for NoVad {
    fn process_frame(&mut self, _frame: &[i16]) -> AppResult<VadFrame> {
        Ok(VadFrame {
            is_speech: false,
            confidence: 0.0,
        })
    }
    fn reset(&mut self) {}
    fn frame_samples(&self) -> usize {
        512
    }
}

/// STT double — empty transcript. Stands in for `WhisperStt` (no Metal
/// model load).
struct SilentStt;

impl SpeechToText for SilentStt {
    fn transcribe(&mut self, _req: TranscribeRequest<'_>) -> AppResult<Transcript> {
        Ok(Transcript {
            text: String::new(),
            gpu_used: false,
            latency_ms: 0,
            model_id: "test-double".to_string(),
        })
    }
}

/// Outcome of the `.4.7c` spawn + teardown probe.
#[derive(Debug, Clone)]
pub struct TeardownReport {
    /// `DictationRuntime::spawn_with_deps` returned `Ok` (threads
    /// started, no panic).
    pub spawn_ok: bool,
    /// Wall-clock time for `drop(runtime)` to return. A hung teardown
    /// would block here (the shim wraps this in a timeout watchdog).
    pub teardown_ms: u64,
}

/// Build a real `DictationRuntime` with DOUBLED device backends, spawn
/// it, then drop it and time the teardown. Returns `Err` on any hard
/// failure (db/config/vault/spawn).
///
/// NOTE: keep this on a worker thread in the shim and bound it with a
/// timeout so a (hypothetical) hung teardown surfaces as a failure
/// rather than an indefinite hang.
pub fn spawn_teardown_probe() -> Result<TeardownReport, String> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    // Real in-memory DB + Normal-mode config + vault runtime. All
    // construct-only — no disk I/O happens during the probe.
    let database =
        crate::db::Database::open_in_memory().map_err(|e| format!("open_in_memory: {e}"))?;
    let config = crate::dictation::runtime::default_normal_config(&database.conn)
        .map_err(|e| format!("default_normal_config: {e}"))?;
    let conn = Arc::new(Mutex::new(database.conn));
    let vault = Arc::new(
        crate::vault::export_job::VaultRuntime::new(&conn)
            .map_err(|e| format!("VaultRuntime::new: {e}"))?,
    );

    // Doubling dep builder: doubled audio/vad/stt; REAL
    // injector/window-context/secure-guard (permission-free to build).
    let deps_fn: OrchestratorDepsFn = Box::new(|| {
        Ok(OrchestratorDeps {
            audio: Box::new(SilentCapture),
            vad: Box::new(NoVad),
            stt: Box::new(SilentStt),
            injector: crate::injection::make_default_injector()?,
            window_ctx: crate::window_context::make_default_context()?,
            secure_guard: crate::injection::secure_guard::make_default_guard(),
        })
    });

    // REAL spawn: builds the real channels, installs the REAL
    // MacKeyboardHook listener, spawns the state-driver + dictation
    // threads.
    let runtime = DictationRuntime::spawn_with_deps(
        Arc::clone(&conn),
        config,
        HashMap::new(),
        vault,
        deps_fn,
    )
    .map_err(|e| format!("spawn_with_deps: {e}"))?;
    let spawn_ok = true;

    // Let the spawned threads reach steady state (dictation thread
    // builds deps + enters its select! loop; the listener installs).
    std::thread::sleep(Duration::from_millis(150));

    // Trigger shutdown. Dropping the runtime drives the REAL listener
    // teardown (CFRunLoopStop + tap-thread join on macOS) and detaches
    // the dictation thread. This MUST return promptly.
    let start = Instant::now();
    drop(runtime);
    let teardown_ms = start.elapsed().as_millis() as u64;

    Ok(TeardownReport {
        spawn_ok,
        teardown_ms,
    })
}
