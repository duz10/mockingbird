//! Cross-crate integration tests for `mockingbird_lib::audio::capture`.
//!
//! Exercises real cpal handles when a default input device is present.
//! Tests skip gracefully when no device is available so CI without a
//! mic still passes the suite.

use mockingbird_lib::audio::make_default_capture;
use std::path::PathBuf;
use std::time::Duration;

/// Locate `tests/fixtures/audio/` relative to the manifest dir.
fn fixtures_dir() -> PathBuf {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set under cargo");
    PathBuf::from(manifest_dir).join("tests/fixtures/audio")
}

#[test]
fn factory_returns_a_capture_on_windows() {
    #[cfg(target_os = "windows")]
    {
        let _ = make_default_capture().expect("Windows factory should construct");
    }
    #[cfg(not(target_os = "windows"))]
    {
        assert!(
            make_default_capture().is_err(),
            "non-Windows factory must return Err per Phase 9 stub policy"
        );
    }
}

#[test]
fn capture_format_constants_are_locked() {
    let cap = match make_default_capture() {
        Ok(c) => c,
        Err(_) => return, // CI without a device — skip
    };
    assert_eq!(cap.sample_rate(), 16_000);
    assert_eq!(cap.channels(), 1);
}

#[test]
fn start_then_drain_does_not_panic_when_device_present() {
    let mut cap = match make_default_capture() {
        Ok(c) => c,
        Err(_) => return,
    };
    if cap.start().is_err() {
        // Device exists but format unsupported — Phase 2 surfaces this
        // as an explicit Err. Wave-2 test path accepts the skip.
        return;
    }
    std::thread::sleep(Duration::from_millis(200));
    let mut buf = Vec::new();
    let n = cap.drain(&mut buf).unwrap();
    cap.stop().unwrap();
    // 200 ms at 16 kHz = 3200 samples. cpal frames + thread sched
    // jitter mean we may drain a bit more or less. Cap at a generous
    // upper bound and floor (anything in 0..=8000 is plausible).
    assert!(n <= 8000, "drained too many samples: {n}");
    assert_eq!(buf.len(), n);
}

#[test]
fn silent_fixture_parses_at_target_format() {
    let path = fixtures_dir().join("silent.wav");
    assert!(
        path.exists(),
        "fixture missing — run `cargo run --example generate_fixtures`"
    );
    let reader = hound::WavReader::open(&path).expect("open silent.wav");
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 16_000);
    assert_eq!(spec.channels, 1);
    assert_eq!(spec.bits_per_sample, 16);
    assert_eq!(spec.sample_format, hound::SampleFormat::Int);
}

#[test]
fn sine_fixture_has_expected_sample_count() {
    let path = fixtures_dir().join("sine_440.wav");
    let mut reader = hound::WavReader::open(&path).expect("open sine_440.wav");
    let n: usize = reader.samples::<i16>().count();
    // 1 second at 16 kHz mono = 16_000 samples.
    assert_eq!(n, 16_000, "expected 16k samples, got {n}");
}

#[test]
fn mixed_fixture_has_three_seconds() {
    let path = fixtures_dir().join("mixed.wav");
    let mut reader = hound::WavReader::open(&path).expect("open mixed.wav");
    let n: usize = reader.samples::<i16>().count();
    assert_eq!(n, 48_000, "expected 3s × 16kHz = 48k samples, got {n}");
}

#[test]
fn silent_fixture_is_actually_silent() {
    let path = fixtures_dir().join("silent.wav");
    let mut reader = hound::WavReader::open(&path).expect("open silent.wav");
    let any_nonzero = reader
        .samples::<i16>()
        .any(|s| s.map(|v| v != 0).unwrap_or(false));
    assert!(!any_nonzero, "silent.wav contains non-zero samples");
}

#[test]
fn sine_fixture_has_signal_energy() {
    let path = fixtures_dir().join("sine_440.wav");
    let mut reader = hound::WavReader::open(&path).expect("open sine_440.wav");
    let mut sum_abs: u64 = 0;
    for s in reader.samples::<i16>() {
        sum_abs += s.unwrap().unsigned_abs() as u64;
    }
    // For a half-amplitude sine, mean(|s|) ≈ (2/π) × amp ≈ 0.637 × amp.
    // amp = i16::MAX / 3 ≈ 10920. So mean(|s|) ≈ 6950 per sample.
    // We just want "much greater than zero" — set a conservative bar.
    let mean_abs = sum_abs / 16_000;
    assert!(
        mean_abs > 1000,
        "sine has no signal energy: mean(|s|) = {mean_abs}"
    );
}
