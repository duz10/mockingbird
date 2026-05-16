//! Cross-crate integration tests for Silero VAD over fixture audio.
//!
//! These tests need BOTH:
//!   - `silero_vad.onnx` discoverable (env `SILERO_VAD_PATH` or via
//!     `models_dir()`), AND
//!   - The ONNX Runtime DLL on PATH or via env `ORT_DYLIB_PATH`
//!     (matched against the version ort-rs expects — currently 1.22.x).
//!
//! Without either, every test below short-circuits to a graceful skip
//! via [`silero_runtime_available`].

use mockingbird_lib::audio::vad::make_default_vad;
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    let m = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    PathBuf::from(m).join("tests/fixtures/audio")
}

/// True when both model + runtime are reachable.
fn silero_runtime_available() -> bool {
    // Model
    let model_ok = std::env::var("SILERO_VAD_PATH")
        .ok()
        .map(|p| std::path::Path::new(&p).is_file())
        .unwrap_or(false)
        || mockingbird_lib::stt::models_dir()
            .map(|d| d.join("silero_vad.onnx").is_file())
            .unwrap_or(false);
    if !model_ok {
        return false;
    }
    // Runtime — quick smoke: try to build a default VAD. If ORT_DYLIB_PATH
    // is wrong or the DLL is missing, construction either panics or
    // errors. Use `catch_unwind` so the test framework keeps marching.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(make_default_vad))
        .map(|r| r.is_ok())
        .unwrap_or(false)
}

fn read_wav_samples(path: &PathBuf) -> Vec<i16> {
    let mut reader = hound::WavReader::open(path).expect("open wav");
    reader.samples::<i16>().map(|s| s.unwrap()).collect()
}

#[test]
fn factory_constructs_when_runtime_available() {
    if !silero_runtime_available() {
        eprintln!("SKIP: Silero model + ORT runtime not configured");
        return;
    }
    let _ = make_default_vad().expect("VAD construction");
}

#[test]
fn silent_fixture_scores_no_speech() {
    if !silero_runtime_available() {
        return;
    }
    let audio = read_wav_samples(&fixtures_dir().join("silent.wav"));
    let mut vad = make_default_vad().unwrap();
    let fs = vad.frame_samples();
    let mut speech_frames = 0;
    let mut total_frames = 0;
    for i in 0..(audio.len() / fs) {
        let start = i * fs;
        let end = start + fs;
        let f = vad.process_frame(&audio[start..end]).unwrap();
        total_frames += 1;
        if f.is_speech {
            speech_frames += 1;
        }
    }
    assert!(
        total_frames > 30,
        "silent.wav too short: {total_frames} frames"
    );
    assert_eq!(
        speech_frames, 0,
        "Silero flagged {speech_frames}/{total_frames} silence frames as speech"
    );
}

#[test]
fn mixed_fixture_processes_all_frames_without_error() {
    if !silero_runtime_available() {
        return;
    }
    let audio = read_wav_samples(&fixtures_dir().join("mixed.wav"));
    let mut vad = make_default_vad().unwrap();
    let fs = vad.frame_samples();
    // Just exercise the full pipeline over the whole file — Silero may
    // classify the 440 Hz tone as speech or not (musical, not speech),
    // so we don't assert on the count, only that no frame errors.
    for i in 0..(audio.len() / fs) {
        let start = i * fs;
        let end = start + fs;
        let _ = vad.process_frame(&audio[start..end]).unwrap();
    }
}

#[test]
fn vad_trim_over_mixed_fixture_produces_some_audio_or_none() {
    use mockingbird_lib::audio::vad_trim::{vad_trim, TrimConfig};
    if !silero_runtime_available() {
        return;
    }
    let audio = read_wav_samples(&fixtures_dir().join("mixed.wav"));
    let mut vad = make_default_vad().unwrap();
    let out = vad_trim(&audio, vad.as_mut(), &TrimConfig::default()).unwrap();
    // Either Silero flagged the tone region as speech (out non-empty
    // and bounded by audio length) OR it didn't (out empty). Both are
    // valid — we just verify the pipeline is sound.
    assert!(
        out.len() <= audio.len() + 16_000,
        "trim output exceeds input + 1s padding: {} vs {}",
        out.len(),
        audio.len()
    );
}
