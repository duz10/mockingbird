//! macOS mic-only meeting-capture roundtrip judge
//! (mb-mac-v1.5.1 / judge `mac-p4a-mic-meeting-roundtrip`).
//!
//! Phase 4a un-gates the meeting mic-capture builder + source probe +
//! the `lib.rs` meeting-spawn block from Windows-only to
//! `any(windows, macos)`. System-audio (loopback) capture stays
//! Windows-only until macOS ScreenCaptureKit lands (Phase 4b). This
//! probe is the in-tree proof that a **mic-only** meeting flows end to
//! end on macOS through the REAL pipeline:
//!
//!   1. The un-gated mic backend constructs on macOS
//!      ([`crate::audio::make_default_capture`] — the exact
//!      `CpalCapture` that the private `build_mic_capture` wraps —
//!      construction only, so no Microphone grant is needed here).
//!   2. [`crate::meetings::capture::probe_sources`] reports
//!      `system_available == false` (4b gate intact); `mic_available`
//!      is reported (device-dependent, not hard-asserted in CI).
//!   3. A mic-only [`TwinStreamCapture`] (mic builder `Some`, sys
//!      builder `None` — the `source.needs_mic()` without
//!      `needs_system()` path) captures a fixed audio buffer
//!      (whisper.cpp's `jfk.wav` via a test-double mic source), the
//!      chunker rolls a chunk WAV stamped with a CRC32, and the REAL
//!      [`LongFormStt`] driver reads it back, **verifies the WAV +
//!      CRC32** (mismatch → hard error), and transcribes it via the
//!      production Whisper on a CONFIRMED Metal backend.
//!   4. The stitched segments are formatted by the deterministic
//!      [`format`] pass and [`persist_meeting`]'d into a real
//!      in-memory DB, then read back via [`load_meeting_detail`] to
//!      assert a `Complete`, `source = mic` row with non-empty
//!      formatted mic prose.
//!
//! ### Real vs doubled (honest boundary)
//!
//!   REAL: the un-gated mic backend construction, the source probe,
//!   the full `TwinStreamCapture` coordinator + chunker (real WAV
//!   write + CRC32), the `LongFormStt` driver + its CRC verification,
//!   the production `make_default_stt` → `WhisperStt` on Metal, the
//!   deterministic formatter, `persist_meeting`, and the repo read.
//!
//!   DOUBLED: only the physical microphone. A live CoreAudio device +
//!   Microphone TCC grant is a CI-hostile dependency, so the sanctioned
//!   test-double mic (`WavMicCapture`) replays `jfk.wav` through the
//!   otherwise-real capture coordinator. The live-mic end-to-end
//!   (Meetings page → Start → speak → Stop) stays the human-gated
//!   `mac-p4a-mic-meeting-e2e`.
//!
//! Gated on macOS + the `metal` feature (the Whisper transcription
//! needs it, and it reuses `stt::judges_macos_v1`'s Metal-backend
//! log classifier + wav reader — DRY); compiles to nothing on every
//! other config so the default cross-platform build stays green.
#![cfg(all(target_os = "macos", feature = "metal"))]

use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::audio::AudioCapture;
use crate::error::AppResult;
use crate::meetings::capture::{probe_sources, CaptureBuilder, MeetingSource, TwinStreamCapture};
use crate::meetings::chunker::ChunkerConfig;
use crate::meetings::filler_words::FILLERS;
use crate::meetings::formatter::{format, FormatOpts};
use crate::meetings::long_form_stt::{LongFormConfig, LongFormOutput, LongFormStt};
use crate::meetings::persist::{persist_meeting, MeetingPersistRequest, MeetingStatus};
use crate::meetings::repo::load_meeting_detail;

/// Test-double microphone. Replays a fixed i16 (16 kHz mono) buffer in
/// a single drain, then reports EOF. Stands in for the CoreAudio
/// `CpalCapture` so the roundtrip needs no live device or Microphone
/// grant — every OTHER stage in the pipeline is the real one.
struct WavMicCapture {
    samples: Vec<i16>,
    pos: usize,
}

impl WavMicCapture {
    fn new(samples: Vec<i16>) -> Self {
        Self { samples, pos: 0 }
    }
}

impl AudioCapture for WavMicCapture {
    fn start(&mut self) -> AppResult<()> {
        Ok(())
    }
    fn stop(&mut self) -> AppResult<()> {
        Ok(())
    }
    fn drain(&mut self, buf: &mut Vec<i16>) -> AppResult<usize> {
        if self.pos >= self.samples.len() {
            return Ok(0);
        }
        // Yield the whole remaining buffer in one drain so the trailing
        // chunk the chunker flushes on stop is lossless (jfk.wav is
        // ~11 s < the 30 s chunk window, so no chunk rolls mid-stream;
        // `finalize()` emits exactly one trailing chunk).
        let n = self.samples.len() - self.pos;
        buf.extend_from_slice(&self.samples[self.pos..]);
        self.pos = self.samples.len();
        Ok(n)
    }
    fn sample_rate(&self) -> u32 {
        16_000
    }
    fn channels(&self) -> u16 {
        1
    }
}

/// Outcome of the Phase 4a mic-only meeting roundtrip probe.
#[derive(Debug, Clone)]
pub struct MicMeetingReport {
    /// The un-gated mic backend (`make_default_capture` → the same
    /// `CpalCapture` `build_mic_capture` wraps) constructed `Ok` on
    /// macOS. Construction only — no `start()`, no Microphone grant.
    pub mic_backend_constructs_ok: bool,
    /// `probe_sources().mic_available` (device-dependent; reported).
    pub probe_mic_available: bool,
    /// `probe_sources().system_available` — MUST be `false` on macOS
    /// in 4a (system/loopback capture is 4b/ScreenCaptureKit).
    pub probe_system_available: bool,
    /// Number of chunk WAV files the chunker wrote to disk.
    pub chunk_wav_count: usize,
    /// Stitched mic-channel segment count from the real `LongFormStt`
    /// (proves CRC verification passed + Whisper produced segments).
    pub mic_segments: usize,
    /// Whether at least one mic chunk reported `gpu_used = true`
    /// (Metal backend confirmation).
    pub gpu_used: bool,
    /// The deterministic formatter's mic prose (trimmed).
    pub formatted_mic: String,
    /// Rowid returned by `persist_meeting`.
    pub persisted_rowid: i64,
    /// The persisted status read back via the repo (want `complete`).
    pub persisted_status: MeetingStatus,
    /// The persisted source read back via the repo (want `mic`).
    pub persisted_source: MeetingSource,
    /// Length of the formatted-mic prose read back from the DB.
    pub persisted_formatted_mic_len: usize,
}

/// Drive a mic-only meeting end to end through the real pipeline.
///
/// `model_path` pins the GGUF whisper model (the probe sets
/// `WHISPER_MODEL_PATH` so `make_default_stt`'s production locator
/// finds it). `wav_path` is the fixed input audio (whisper.cpp
/// `jfk.wav`). Returns `Err` on any hard failure; a successful return
/// still requires the caller (the shim) to assert the invariants.
pub fn mic_meeting_roundtrip_probe(
    model_path: &Path,
    wav_path: &Path,
) -> Result<MicMeetingReport, String> {
    // 1. Prove the un-gated mic backend constructs on macOS. This is
    //    the same CpalCapture the private `build_mic_capture` wraps;
    //    construction is permission-free (no device open / start()).
    let _mic_backend =
        crate::audio::make_default_capture().map_err(|e| format!("make_default_capture: {e}"))?;
    let mic_backend_constructs_ok = true;

    // 2. Source probe — system MUST be unavailable (4b gate intact).
    let probe = probe_sources().map_err(|e| format!("probe_sources: {e}"))?;

    // 3. Pin the whisper model for the production STT locator.
    if !model_path.is_file() {
        return Err(format!(
            "whisper model not found at {} (run scripts/download-models.sh)",
            model_path.display()
        ));
    }
    // SAFETY: probe runs single-threaded before any STT thread reads
    // WHISPER_MODEL_PATH; this routes make_default_stt at the model.
    unsafe {
        std::env::set_var("WHISPER_MODEL_PATH", model_path);
    }

    let samples = crate::stt::judges_macos_v1::read_wav_16k_mono_i16(wav_path)?;

    // Isolated per-run chunk dir under the OS temp dir.
    let chunk_dir = std::env::temp_dir().join(format!("mb-p4a-mic-{}", std::process::id()));
    std::fs::create_dir_all(&chunk_dir)
        .map_err(|e| format!("create chunk dir {chunk_dir:?}: {e}"))?;

    let uuid = "p4a-mic-roundtrip".to_string();

    // 4. Mic-only TwinStreamCapture: mic builder Some, sys builder None
    //    — the `source.needs_mic()` without `needs_system()` path.
    let mic_builder: CaptureBuilder = {
        let s = samples.clone();
        Box::new(move || Ok(Box::new(WavMicCapture::new(s)) as Box<dyn AudioCapture>))
    };
    let mut capture = TwinStreamCapture::start_with(
        uuid.clone(),
        chunk_dir.clone(),
        ChunkerConfig::default(),
        Some(mic_builder),
        None,
    )
    .map_err(|e| format!("TwinStreamCapture::start_with: {e}"))?;
    let chunk_rx = capture
        .take_chunk_rx()
        .ok_or_else(|| "chunk_rx already taken".to_string())?;

    // 5. Real long-form Whisper(Metal) driver on a worker thread — it
    //    reads each chunk WAV, verifies the CRC32, and transcribes.
    let lf = thread::Builder::new()
        .name("p4a-long-form".into())
        .spawn(move || -> AppResult<LongFormOutput> {
            let mut stt = crate::stt::make_default_stt()?;
            let driver = LongFormStt::new(
                stt.as_mut(),
                chunk_rx,
                |_p| { /* no overlay in the judge */ },
                LongFormConfig::default(),
            );
            driver.run()
        })
        .map_err(|e| format!("spawn long-form: {e}"))?;

    // Let the owner poll drain the synthetic buffer into the chunker,
    // then stop — flushing the trailing chunk + disconnecting the rx.
    thread::sleep(Duration::from_millis(250));
    capture.stop().map_err(|e| format!("capture.stop(): {e}"))?;

    let output = lf
        .join()
        .map_err(|e| format!("long-form thread panicked: {e:?}"))?
        .map_err(|e| format!("long-form driver: {e}"))?;

    // Count the chunk WAVs the chunker wrote.
    let chunk_wav_count = std::fs::read_dir(&chunk_dir)
        .map_err(|e| format!("read chunk dir: {e}"))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("wav"))
        .count();

    // 6. Deterministic format (canonical pass — no LLM).
    let formatted_mic = format(&output.mic_segments, &FILLERS, &FormatOpts::default())
        .map_err(|e| format!("format: {e}"))?;

    // 7. Persist into a real in-memory DB (migration 011 tables), then
    //    read the row back through the repo.
    let database =
        crate::db::Database::open_in_memory().map_err(|e| format!("open_in_memory: {e}"))?;
    let req = MeetingPersistRequest {
        uuid: uuid.clone(),
        title: crate::meetings::title::derive_meeting_title(None, Some(&formatted_mic), None),
        started_at: "2026-07-01T00:00:00Z".to_string(),
        ended_at: "2026-07-01T00:00:11Z".to_string(),
        status: MeetingStatus::Complete,
        error_message: None,
        source: MeetingSource::Mic,
        total_duration_ms: 11_000,
        mic_duration_ms: Some(11_000),
        sys_duration_ms: None,
        hotkey_pressed: "cc".to_string(),
        audio_blob_path: Some(chunk_dir.display().to_string()),
        whisper_model_id: "whisper-large-v3-turbo-q5_0".to_string(),
        formatter_version: "mc-v1".to_string(),
        chunk_count_mic: Some(chunk_wav_count as u32),
        chunk_count_sys: None,
        stt_latency_ms: None,
        formatter_latency_ms: None,
        formatted_mic: Some(formatted_mic.clone()),
        formatted_sys: None,
        formatted_merged: None,
        segments_mic: Some(output.mic_segments.clone()),
        segments_sys: None,
    };
    let persisted_rowid =
        persist_meeting(&database.conn, &req).map_err(|e| format!("persist_meeting: {e}"))?;

    let detail = load_meeting_detail(&database.conn, &uuid)
        .map_err(|e| format!("load_meeting_detail: {e}"))?
        .ok_or_else(|| "persisted meeting not found on read-back".to_string())?;

    // Best-effort cleanup of the scratch chunk dir.
    let _ = std::fs::remove_dir_all(&chunk_dir);

    Ok(MicMeetingReport {
        mic_backend_constructs_ok,
        probe_mic_available: probe.mic_available,
        probe_system_available: probe.system_available,
        chunk_wav_count,
        mic_segments: output.mic_segments.len(),
        gpu_used: output.mic_gpu_used,
        formatted_mic: formatted_mic.trim().to_string(),
        persisted_rowid,
        persisted_status: detail.status,
        persisted_source: detail.source,
        persisted_formatted_mic_len: detail.formatted_mic.as_deref().unwrap_or("").len(),
    })
}
