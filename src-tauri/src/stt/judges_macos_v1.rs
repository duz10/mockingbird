//! macOS Metal STT judge probe (mb-mac-v1.3.3 / judge `mac-p2-metal-transcript`).
//!
//! Phase 2 of the macOS port swaps whisper-rs onto the `metal` feature
//! (Apple-Silicon GPU). This module is the in-tree probe that proves the
//! swap actually engaged the Metal backend — not a *silent* CPU fallback
//! that would still produce a transcript and thereby pass a naive
//! non-empty check while quietly defeating the parity intent.
//!
//! Why a fresh probe rather than reusing `WhisperStt`: that type is
//! `#[cfg(target_os = "windows")]` (the cross-platform STT backend is a
//! Phase 3/4 deliverable), and `tests/whisper.rs` was gated to Windows
//! during Phase 1 triage. So this probe drives `whisper-rs` directly,
//! mirroring `whisper.rs`'s call sequence, and adds the load-bearing
//! piece Phase 2 needs: backend confirmation.
//!
//! Backend confirmation strategy: whisper.cpp / ggml emit their backend
//! init lines (e.g. "ggml_metal_init: ...", "whisper_backend_init_gpu:
//! using Metal backend") through ggml's + whisper's C log callbacks. We
//! install our own capturing trampoline via the re-exported
//! `whisper_rs_sys::{ggml_log_set, whisper_log_set}`, run a transcribe,
//! then classify the captured log. A Metal init marker => GPU engaged;
//! its absence => CPU fallback (probe fails).
//!
//! This module is gated on macOS + the `metal` feature (its log-capture
//! backend confirmation needs `whisper-rs/raw-api`, which `metal` pulls
//! in); on other configs it compiles to nothing.
#![cfg(all(target_os = "macos", feature = "metal"))]

use std::ffi::CStr;
use std::os::raw::{c_char, c_void};
use std::path::Path;
use std::sync::{Mutex, Once};
use std::time::Instant;

use whisper_rs::{
    whisper_rs_sys, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
};

/// Captured ggml + whisper.cpp log text. The backend-init lines land
/// here during `WhisperContext` construction + the first `full()` pass.
static LOG_CAPTURE: Mutex<String> = Mutex::new(String::new());

/// Outcome of a single Metal transcript probe.
#[derive(Debug, Clone)]
pub struct MetalProbeReport {
    /// The transcript text (trimmed). Asserted non-empty by the judge.
    pub transcript: String,
    /// Classified backend: `"Metal"` (GPU engaged) or `"CPU"` (fallback).
    pub backend: Backend,
    /// The exact log line that proved the backend, if any.
    pub backend_evidence: Option<String>,
    /// End-to-end transcribe latency.
    pub latency_ms: u64,
    /// The full captured ggml/whisper log (for diagnostics / reporting).
    pub log: String,
}

/// Which compute backend whisper.cpp actually initialised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Metal (Apple-Silicon GPU) backend engaged.
    Metal,
    /// CPU backend — i.e. a (silent) fallback; the probe treats this as
    /// a parity failure for Phase 2.
    Cpu,
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Backend::Metal => write!(f, "Metal"),
            Backend::Cpu => write!(f, "CPU"),
        }
    }
}

/// SAFETY: ggml guarantees `text` is a valid, NUL-terminated C string for
/// the duration of the call. We only read it and append to a Mutex.
unsafe extern "C" fn capture_trampoline(
    _level: whisper_rs_sys::ggml_log_level,
    text: *const c_char,
    _user_data: *mut c_void,
) {
    if text.is_null() {
        return;
    }
    let s = CStr::from_ptr(text).to_string_lossy();
    if let Ok(mut buf) = LOG_CAPTURE.lock() {
        buf.push_str(&s);
    }
}

/// Install our capturing log callback on both ggml and whisper.cpp.
/// Idempotent — the C side keeps the last callback set, and `Once`
/// guarantees we don't thrash it.
fn install_capture() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        // SAFETY: process-wide one-time install of a `'static` callback.
        whisper_rs_sys::ggml_log_set(Some(capture_trampoline), std::ptr::null_mut());
        whisper_rs_sys::whisper_log_set(Some(capture_trampoline), std::ptr::null_mut());
    });
}

/// Classify the captured log into a backend + the evidence line.
///
/// Positive Metal markers (any one is sufficient):
///   - "using Metal backend"      (whisper_backend_init_gpu)
///   - "ggml_metal_init"          (Metal allocator init)
///   - a line mentioning both "metal" and "found device"
fn classify_backend(log: &str) -> (Backend, Option<String>) {
    for line in log.lines() {
        let ll = line.to_lowercase();
        let is_metal = ll.contains("using metal backend")
            || ll.contains("ggml_metal_init")
            || (ll.contains("metal") && ll.contains("found device"))
            || (ll.contains("metal") && ll.contains("device name"));
        if is_metal {
            return (Backend::Metal, Some(line.trim().to_string()));
        }
    }
    (Backend::Cpu, None)
}

/// Read a 16 kHz mono 16-bit WAV into i16 samples.
fn read_wav_16k_mono_i16(path: &Path) -> Result<Vec<i16>, String> {
    let mut reader =
        hound::WavReader::open(path).map_err(|e| format!("open wav {}: {e}", path.display()))?;
    let spec = reader.spec();
    if spec.sample_rate != 16_000 || spec.channels != 1 || spec.bits_per_sample != 16 {
        return Err(format!(
            "wav must be 16 kHz mono 16-bit; got {} Hz / {} ch / {} bps",
            spec.sample_rate, spec.channels, spec.bits_per_sample
        ));
    }
    reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("read wav samples: {e}"))
}

/// Load the GGUF model with GPU (Metal) requested, transcribe `wav_path`,
/// and report the transcript + the *confirmed* backend.
///
/// Returns `Err` only on hard failures (model missing, wav malformed,
/// whisper init/inference error). A successful return still requires the
/// caller (the judge shim) to assert `transcript` is non-empty and
/// `backend == Metal`.
pub fn metal_transcript_probe(
    model_path: &Path,
    wav_path: &Path,
) -> Result<MetalProbeReport, String> {
    install_capture();
    if let Ok(mut buf) = LOG_CAPTURE.lock() {
        buf.clear();
    }

    let model_str = model_path
        .to_str()
        .ok_or_else(|| "non-UTF8 model path".to_string())?;
    if !model_path.is_file() {
        return Err(format!(
            "whisper model not found at {} (run scripts/download-models.sh)",
            model_path.display()
        ));
    }

    // Request the GPU. With `--features metal` compiled in, this engages
    // the Metal backend; without it, whisper-rs treats use_gpu as a no-op
    // and we'd see a CPU log (which the classifier catches).
    let mut cparams = WhisperContextParameters::default();
    cparams.use_gpu(true);
    let ctx = WhisperContext::new_with_params(model_str, cparams)
        .map_err(|e| format!("WhisperContext init: {e}"))?;

    let audio = read_wav_16k_mono_i16(wav_path)?;
    let audio_f32: Vec<f32> = audio.iter().map(|&s| s as f32 / 32768.0).collect();

    let started = Instant::now();
    let mut state = ctx
        .create_state()
        .map_err(|e| format!("create_state: {e}"))?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_print_progress(false);
    params.set_print_special(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_language(Some("en"));

    state
        .full(params, &audio_f32)
        .map_err(|e| format!("whisper full: {e}"))?;

    let n = state.full_n_segments();
    let mut transcript = String::new();
    for i in 0..n {
        let seg = state
            .get_segment(i)
            .ok_or_else(|| format!("missing segment {i}"))?;
        let chunk = seg
            .to_str_lossy()
            .map_err(|e| format!("segment text {i}: {e}"))?;
        transcript.push_str(&chunk);
    }
    let latency_ms = started.elapsed().as_millis() as u64;

    let log = LOG_CAPTURE.lock().map(|b| b.clone()).unwrap_or_default();
    let (backend, backend_evidence) = classify_backend(&log);

    Ok(MetalProbeReport {
        transcript: transcript.trim().to_string(),
        backend,
        backend_evidence,
        latency_ms,
        log,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_detects_metal_backend() {
        let log = "whisper_backend_init_gpu: using Metal backend\nggml_metal_init: allocating\n";
        let (backend, ev) = classify_backend(log);
        assert_eq!(backend, Backend::Metal);
        assert!(ev.unwrap().to_lowercase().contains("metal"));
    }

    #[test]
    fn classify_detects_cpu_fallback() {
        let log = "whisper_backend_init: using CPU backend\nsystem_info: n_threads = 4\n";
        let (backend, ev) = classify_backend(log);
        assert_eq!(backend, Backend::Cpu);
        assert!(ev.is_none());
    }

    #[test]
    fn classify_found_device_line_counts_as_metal() {
        let log = "ggml_metal_init: found device: Apple M2 Pro\n";
        let (backend, _) = classify_backend(log);
        assert_eq!(backend, Backend::Metal);
    }
}
