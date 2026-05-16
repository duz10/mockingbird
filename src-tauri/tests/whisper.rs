//! Cross-crate integration tests for `mockingbird_lib::stt::whisper::WhisperStt`.
//!
//! All tests skip gracefully when the Whisper model isn't on disk so
//! CI without GPU/large-model setup still passes.

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

#[test]
fn cpu_construct_succeeds_when_model_present() {
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
    let mut stt = WhisperStt::new_with_options(true).unwrap();
    let tx = stt
        .transcribe(TranscribeRequest {
            audio: &audio,
            initial_prompt: None,
            force_cpu: true,
        })
        .unwrap();
    // 3 s of pure silence — Whisper may emit "[BLANK_AUDIO]" or empty.
    assert!(
        tx.text.len() < 100,
        "silent.wav produced unexpected long text: {:?}",
        tx.text
    );
    assert_eq!(tx.model_id, "whisper-large-v3-turbo-q5_0");
    assert!(tx.latency_ms > 0);
    assert!(!tx.gpu_used);
}

#[test]
fn transcribe_sine_does_not_panic() {
    if !whisper_model_present() {
        return;
    }
    let audio = read_wav(&fixtures_dir().join("sine_440.wav"));
    let mut stt = WhisperStt::new_with_options(true).unwrap();
    let tx = stt
        .transcribe(TranscribeRequest {
            audio: &audio,
            initial_prompt: None,
            force_cpu: true,
        })
        .unwrap();
    // A 440 Hz tone isn't speech — Whisper may hallucinate something
    // short or output a special token. Just verify the call succeeds
    // and produces a Transcript with model_id and non-zero latency.
    assert_eq!(tx.model_id, "whisper-large-v3-turbo-q5_0");
    assert!(tx.latency_ms > 0);
}

#[test]
fn transcribe_accepts_initial_prompt_without_error() {
    if !whisper_model_present() {
        return;
    }
    let audio = read_wav(&fixtures_dir().join("silent.wav"));
    let mut stt = WhisperStt::new_with_options(true).unwrap();
    let _tx = stt
        .transcribe(TranscribeRequest {
            audio: &audio,
            initial_prompt: Some("Tauri, Rust, Mockingbird"),
            force_cpu: true,
        })
        .expect("transcribe with prompt");
}
