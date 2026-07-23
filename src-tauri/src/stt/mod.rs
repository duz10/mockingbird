#![allow(missing_docs)] // Trait + factory + helper; method-level docs are the API.

//! Speech-to-text — Whisper via whisper-rs.
//!
//! Wave 1 ships the trait + the `models_dir()` resolver (binding for
//! Wave 3 VAD too). Wave 4 fills in whisper-rs CUDA + CPU fallback
//! per ADR 0011.

pub mod initial_prompt;
pub mod prompt_builder;
pub mod whisper;

// macOS port Phase 2 (mb-mac-v1.3.3): Metal-backend transcript judge probe.
// Gated on `macos` AND `metal` because the backend-confirmation log
// capture uses `whisper_rs::whisper_rs_sys`, which is only re-exported
// under `whisper-rs/raw-api` (pulled in by our `metal` feature). A plain
// (no-metal) build must NOT try to compile it.
#[cfg(all(target_os = "macos", feature = "metal"))]
pub mod judges_macos_v1;

// macOS port Phase 5 (mb-mac-v1.6.2): Metal STT parity eval + WER/CER
// metric (judge `mac-v1-parity-whisper-metal`). The WER/CER metric is
// pure text math and stays UN-gated so it compiles + unit-tests on every
// target; only the `run_parity_eval` driver inside is `macos + metal`.
pub mod parity_macos_v1;

use std::path::PathBuf;

// AppError is used unconditionally: `models_dir()` constructs it on every
// platform, and `make_default_stt`'s Linux arm does too.
use crate::error::AppError;
use crate::error::AppResult;

/// One STT pass output.
#[derive(Debug, Clone)]
pub struct Transcript {
    pub text: String,
    /// Whether GPU (CUDA) was used. Logged + asserted by the
    /// Wave-5 `cuda-verified` judge.
    pub gpu_used: bool,
    /// End-to-end latency for this transcribe call.
    pub latency_ms: u64,
    /// Which Whisper model produced this. Recorded in
    /// `transcripts.model_used`.
    pub model_id: String,
}

/// Transcription request.
#[derive(Debug, Clone)]
pub struct TranscribeRequest<'a> {
    /// 16 kHz mono i16 PCM, already VAD-trimmed.
    pub audio: &'a [i16],
    /// Optional 224-token `initial_prompt` from
    /// [`prompt_builder::build_prompt`]. Whisper's prompt cap.
    pub initial_prompt: Option<&'a str>,
    /// Force CPU even if CUDA is available (CLI / test path).
    pub force_cpu: bool,
}

/// One Whisper segment with timing in milliseconds (ADR 0030).
///
/// Whisper.cpp emits per-segment timestamps in centiseconds; the trait
/// surfaces them in milliseconds for ergonomic alignment with the rest
/// of the meeting pipeline (which is millisecond-typed end-to-end).
///
/// `Serialize` + `Deserialize` added in Phase MC Wave 4 so the meeting
/// `persist_meeting` path can JSON-encode the raw_segments stage row
/// (`meeting_transcripts.text` for stage='raw_segments' is the
/// `serde_json::to_string` of a `Vec<SttSegment>`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SttSegment {
    pub text: String,
    pub t0_ms: u32,
    pub t1_ms: u32,
}

/// Multi-segment STT pass output (ADR 0030).
///
/// Returned by [`SpeechToText::transcribe_segments`]. Carries the same
/// top-line shape as [`Transcript`] plus a vector of timed segments
/// (sorted by `t0_ms`, monotone non-decreasing).
#[derive(Debug, Clone)]
pub struct TranscriptWithSegments {
    pub text: String,
    pub segments: Vec<SttSegment>,
    pub gpu_used: bool,
    pub latency_ms: u64,
    pub model_id: String,
}

pub trait SpeechToText: Send {
    fn transcribe(&mut self, req: TranscribeRequest<'_>) -> AppResult<Transcript>;

    /// ADR 0030. Returns segments alongside the top-line text.
    ///
    /// Default impl falls back to a single-segment wrap of the
    /// existing `transcribe` output — keeps the trait extension
    /// non-breaking for any future external implementor (cloud-STT
    /// layer, mocked test STT) without forcing them to author a real
    /// segment walker. [`whisper::WhisperStt`] overrides this with a
    /// proper walk of whisper.cpp's per-segment timestamps.
    fn transcribe_segments(
        &mut self,
        req: TranscribeRequest<'_>,
    ) -> AppResult<TranscriptWithSegments> {
        let t = self.transcribe(req)?;
        let single = SttSegment {
            text: t.text.clone(),
            t0_ms: 0,
            // Wrap the whole utterance as a single segment spanning
            // [0, latency_ms). The default impl is a fallback only;
            // callers that need accurate per-Whisper-segment timing
            // should target an impl that overrides this method.
            t1_ms: t.latency_ms as u32,
        };
        Ok(TranscriptWithSegments {
            text: t.text,
            segments: vec![single],
            gpu_used: t.gpu_used,
            latency_ms: t.latency_ms,
            model_id: t.model_id,
        })
    }
}

/// Construct the platform-default STT impl.
pub fn make_default_stt() -> AppResult<Box<dyn SpeechToText>> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        Ok(Box::new(whisper::WhisperStt::new()?))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err(AppError::Stt(
            "STT not implemented for this platform (Phase 9 Linux)".into(),
        ))
    }
}

/// Resolve the directory containing on-disk ML model files.
///
/// Every model consumer funnels through here -- Whisper
/// (`stt::whisper::locate_whisper_model`), the Silero VAD
/// (`audio::vad::locate_model`), and the ORT dylib
/// (`dictation::runtime::ensure_ort_dylib_set`) -- so fixing discovery
/// here fixes all three at once (mb-3cr).
///
/// Resolution order (per ADR 0014, extended for the packaged macOS
/// `.app` in mb-3cr):
///   1. `MODEL_PATH` env var (absolute path; dev override -- wins on
///      every platform so `scripts/dev/cargo-mac.sh` keeps working).
///   2. `<exe_dir>/models/` (portable install; also next to a dev bin).
///   3. macOS `.app` Resources: `<exe_dir>/../Resources/models/`. The
///      bundled `.app` ships the GGUF + silero + libonnxruntime.dylib
///      here (see `src-tauri/tauri.macos.conf.json`), and has NONE of
///      the dev env vars -- this is the branch that unbricks the
///      packaged app.
///   4. `<app-data>/models/` (user drop-in / first-run copy target;
///      same app-data root Tauri uses -- see `resolve_app_data_dir`).
///   5. `%LOCALAPPDATA%\Mockingbird\models\` (release default; Windows)
///   6. `%USERPROFILE%\mockingbird_models\` (Phase 2 dev convention; Windows)
///
/// Returns `Err(AppError::Stt)` -- listing every path it tried, so the
/// failure is LOUD and actionable instead of a silent dead runtime --
/// if none resolve. Caller is responsible for verifying the directory
/// actually contains the model it wants; this only locates the directory.
pub fn models_dir() -> AppResult<PathBuf> {
    // 1. Explicit override (dev; highest priority on every platform).
    if let Ok(p) = std::env::var("MODEL_PATH") {
        return Ok(PathBuf::from(p));
    }

    // Accumulate the candidates we probe so a miss can report them all.
    let mut tried: Vec<PathBuf> = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            // 2. Portable install / dev bin sibling.
            let candidate = exe_dir.join("models");
            if candidate.is_dir() {
                return Ok(candidate);
            }
            tried.push(candidate);

            // 3. macOS packaged `.app`: models bundled under
            //    `Mockingbird.app/Contents/Resources/models/`. The exe
            //    lives at `Contents/MacOS/<bin>`, so Resources is a
            //    sibling of MacOS one directory up.
            #[cfg(target_os = "macos")]
            if let Some(contents_dir) = exe_dir.parent() {
                let candidate = contents_dir.join("Resources").join("models");
                if candidate.is_dir() {
                    return Ok(candidate);
                }
                tried.push(candidate);
            }
        }
    }

    // 4. User app-data models dir -- reuse the exact root Tauri resolves
    //    so this stays in lock-step with the rest of the app. Gated to
    //    macOS: this is the packaged-app drop-in path (mb-3cr), and
    //    Windows keeps its established LOCALAPPDATA/USERPROFILE order
    //    below byte-for-byte.
    #[cfg(target_os = "macos")]
    if let Ok(app_data) = crate::resolve_app_data_dir() {
        let candidate = app_data.join("models");
        if candidate.is_dir() {
            return Ok(candidate);
        }
        tried.push(candidate);
    }

    // 5. Release default (Windows).
    #[cfg(target_os = "windows")]
    if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
        let path = PathBuf::from(localappdata)
            .join("Mockingbird")
            .join("models");
        if path.is_dir() {
            return Ok(path);
        }
        tried.push(path);
    }
    // 6. Phase 2 dev convention: models downloaded to
    //    `%USERPROFILE%\mockingbird_models\` via `scripts/download-models.ps1`.
    #[cfg(target_os = "windows")]
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let path = PathBuf::from(profile).join("mockingbird_models");
        if path.is_dir() {
            return Ok(path);
        }
        tried.push(path);
    }

    let tried_list = tried
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    tracing::error!(
        tried = %tried_list,
        "could not resolve models directory -- STT/VAD cannot start"
    );
    Err(AppError::Stt(format!(
        "could not resolve models directory. Set MODEL_PATH, bundle models into \
         the app Resources, or place them in <app-data>/models/. Tried: [{tried_list}]"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_dir_honors_model_path_env() {
        let prev = std::env::var("MODEL_PATH").ok();
        // SAFETY: tests run single-threaded for env mutation via
        // `cargo test -- --test-threads=1` if you have other env-touching
        // tests. Phase 1 didn't have any; this is currently safe.
        std::env::set_var("MODEL_PATH", "C:\\custom\\models");
        let result = models_dir().unwrap();
        assert_eq!(result, PathBuf::from("C:\\custom\\models"));
        // Restore prior state.
        match prev {
            Some(v) => std::env::set_var("MODEL_PATH", v),
            None => std::env::remove_var("MODEL_PATH"),
        }
    }
}
