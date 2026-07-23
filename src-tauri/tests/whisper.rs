//! Cross-crate integration tests for `mockingbird_lib::stt::whisper::WhisperStt`.
//!
//! All tests skip gracefully when the Whisper model isn't on disk so
//! CI without GPU/large-model setup still passes.
//!
//! After the Wave 5 CUDA re-enable, the default backend is GPU. One
//! test (`cpu_fallback_construct_succeeds`) holds the CPU path as a
//! living canary so we notice if the fallback rots. The other tests
//! use the default (GPU-first) constructor — they ran ~19 CPU-minutes
//! on a 1 s sine input in pre-CUDA Wave 4; on GPU they finish in
//! sub-second. See `docs/judges/phase-2/cuda-verified.md`.

// macOS port: this whole integration test exercises the Windows-only `WhisperStt`
// (`#[cfg(target_os = "windows")]`). Gate the entire file to Windows until the
// cross-platform STT backend lands (Phase 3/4); compiles to an empty test bin
// on other targets.
#![cfg(target_os = "windows")]

use std::path::PathBuf;

use mockingbird_lib::stt::whisper::WhisperStt;
use mockingbird_lib::stt::{SpeechToText, TranscribeRequest};

fn fixtures_dir() -> PathBuf {
    let m = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    PathBuf::from(m).join("tests/fixtures/audio")
}

fn whisper_model_present() -> bool {
    if let Ok(p) = std::env::var("WHISPER_MODEL_PATH") {
        if std::path::Path::new(&p).is_file() {
            return true;
        }
    }
    if let Ok(d) = mockingbird_lib::stt::models_dir() {
        if d.join("whisper-large-v3-turbo-q5_0.bin").is_file() {
            return true;
        }
    }
    false
}

fn read_wav(path: &PathBuf) -> Vec<i16> {
    let mut reader = hound::WavReader::open(path).expect("open wav");
    reader.samples::<i16>().map(|s| s.unwrap()).collect()
}

/// CPU-fallback canary. Validates the ADR 0011 fallback path still
/// constructs without ever attempting GPU. Kept on CPU even after the
/// Wave-5 CUDA re-enable.
#[test]
fn cpu_fallback_construct_succeeds() {
    if !whisper_model_present() {
        eprintln!("SKIP: whisper model not on disk");
        return;
    }
    let stt = WhisperStt::new_with_options(true).expect("CPU construct");
    assert!(!stt.gpu_loaded(), "force_cpu must yield CPU backend");
}

#[test]
fn transcribe_silent_fixture_yields_short_output() {
    if !whisper_model_present() {
        return;
    }
    let audio = read_wav(&fixtures_dir().join("silent.wav"));
    let mut stt = WhisperStt::new().unwrap(); // GPU-first
    let tx = stt
        .transcribe(TranscribeRequest {
            audio: &audio,
            initial_prompt: None,
            force_cpu: false,
        })
        .unwrap();
    // 3 s of silence — Whisper's YouTube training makes it occasionally
    // emit short outro phrases like "Thank you." from silence. Assert
    // it didn't fabricate a long sentence.
    assert!(
        tx.text.len() < 100,
        "silent.wav produced unexpected long text: {:?}",
        tx.text
    );
    assert_eq!(tx.model_id, "whisper-large-v3-turbo-q5_0");
    assert!(tx.latency_ms > 0);
}

#[test]
fn transcribe_sine_does_not_panic() {
    if !whisper_model_present() {
        return;
    }
    let audio = read_wav(&fixtures_dir().join("sine_440.wav"));
    let mut stt = WhisperStt::new().unwrap();
    let tx = stt
        .transcribe(TranscribeRequest {
            audio: &audio,
            initial_prompt: None,
            force_cpu: false,
        })
        .unwrap();
    // 440 Hz isn't speech. Just verify the call returns a Transcript.
    // On GPU this is sub-second; on CPU it can loop for many minutes
    // (Whisper's non-speech iteration trap) so this test is GPU-only
    // in practice.
    assert_eq!(tx.model_id, "whisper-large-v3-turbo-q5_0");
    assert!(tx.latency_ms > 0);
}

#[test]
fn transcribe_accepts_initial_prompt_without_error() {
    if !whisper_model_present() {
        return;
    }
    let audio = read_wav(&fixtures_dir().join("silent.wav"));
    let mut stt = WhisperStt::new().unwrap();
    let _tx = stt
        .transcribe(TranscribeRequest {
            audio: &audio,
            initial_prompt: Some("Tauri, Rust, Mockingbird"),
            force_cpu: false,
        })
        .expect("transcribe with prompt");
}
