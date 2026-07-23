//! Whisper inference latency benchmark.
//!
//! Times `WhisperStt::transcribe` over the synthetic `sine_440.wav`
//! fixture. Not about Whisper accuracy (a sine tone isn't speech) —
//! purely measures the model + WAV + state-creation round-trip for
//! regression detection.
//!
//! Run:
//!   cargo bench --bench whisper_latency
//!
//! Skips gracefully when the Whisper model isn't on disk.

// macOS port: PathBuf + black_box are consumed only by the Windows-gated
// bench body below; gate them to match (Phase 3/4 wires the cross-platform STT).
#[cfg(target_os = "windows")]
use std::path::PathBuf;

#[cfg(target_os = "windows")]
use criterion::black_box;
use criterion::{criterion_group, criterion_main, Criterion};

#[cfg(target_os = "windows")]
use mockingbird_lib::stt::whisper::WhisperStt;
#[cfg(target_os = "windows")]
use mockingbird_lib::stt::{SpeechToText, TranscribeRequest};

#[cfg(target_os = "windows")]
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio")
}

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
fn bench_transcribe_sine(c: &mut Criterion) {
    let path = fixtures_dir().join("sine_440.wav");
    if !path.exists() {
        eprintln!("SKIP: sine_440.wav missing; run `cargo run --example generate_fixtures`");
        return;
    }
    if !whisper_model_present() {
        eprintln!("SKIP: whisper model not on disk");
        return;
    }

    let mut reader = hound::WavReader::open(&path).expect("open sine_440.wav");
    let audio: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();

    // Force CPU for deterministic measurement.
    let mut stt = WhisperStt::new_with_options(true).expect("init whisper");

    c.bench_function("whisper_latency_1s_sine_cpu", |b| {
        b.iter(|| {
            let req = TranscribeRequest {
                audio: black_box(&audio),
                initial_prompt: None,
                force_cpu: true,
            };
            let _ = stt.transcribe(req).expect("transcribe");
        })
    });
}

#[cfg(not(target_os = "windows"))]
fn bench_transcribe_sine(_: &mut Criterion) {
    eprintln!("SKIP: STT is Windows-only in Phase 2");
}

criterion_group!(benches, bench_transcribe_sine);
criterion_main!(benches);
