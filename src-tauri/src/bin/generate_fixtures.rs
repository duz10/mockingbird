//! Audio fixture generator.
//!
//! Produces deterministic 16 kHz mono i16 WAV files under
//! `src-tauri/tests/fixtures/audio/` for use by Wave 2 integration
//! tests and Wave 3 VAD tests.
//!
//! TTS-rendered speech fixtures (`hello.wav`, `quick_brown_fox.wav`)
//! are NOT generated here — those are a Helios delegate task, per the
//! Wave 2 brief. This binary produces only synthetic fixtures:
//!   - `silent.wav`     — 3 s of zeros
//!   - `sine_440.wav`   — 1 s of a 440 Hz tone (A4)
//!   - `mixed.wav`      — 1 s silence + 1 s 440 Hz tone + 1 s silence
//!
//! Run: `cargo run --bin generate_fixtures`
//!
//! The output is checked into git (small: ~190 KB total) so CI
//! doesn't need to re-run this. Re-run only if the fixture spec
//! changes.

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};

const SAMPLE_RATE: u32 = 16_000;
const CHANNELS: u16 = 1;
const BITS_PER_SAMPLE: u16 = 16;

fn spec() -> WavSpec {
    WavSpec {
        channels: CHANNELS,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: BITS_PER_SAMPLE,
        sample_format: SampleFormat::Int,
    }
}

/// Sine tone, amplitude ~ half-scale to leave headroom.
fn sine_samples(freq_hz: f32, duration_secs: f32) -> Vec<i16> {
    let total = (SAMPLE_RATE as f32 * duration_secs) as usize;
    let amp = (i16::MAX / 3) as f32;
    (0..total)
        .map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            (amp * (TAU * freq_hz * t).sin()) as i16
        })
        .collect()
}

fn silent_samples(duration_secs: f32) -> Vec<i16> {
    let total = (SAMPLE_RATE as f32 * duration_secs) as usize;
    vec![0i16; total]
}

fn write_fixture(name: &str, samples: &[i16], out_dir: &Path) {
    let path = out_dir.join(name);
    let mut writer = WavWriter::create(&path, spec()).expect("create WavWriter");
    for &s in samples {
        writer.write_sample(s).expect("write_sample");
    }
    writer.finalize().expect("finalize");
    eprintln!("wrote {} ({} samples)", path.display(), samples.len());
}

fn main() {
    // Resolve `tests/fixtures/audio/` relative to the crate root. When
    // run via `cargo run --bin generate_fixtures`, CARGO_MANIFEST_DIR
    // points at `src-tauri/`.
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR (run via `cargo run`)");
    let out_dir = PathBuf::from(manifest_dir).join("tests/fixtures/audio");
    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    // 3 s of silence.
    write_fixture("silent.wav", &silent_samples(3.0), &out_dir);

    // 1 s of A4 (440 Hz).
    write_fixture("sine_440.wav", &sine_samples(440.0, 1.0), &out_dir);

    // 1 s silence + 1 s tone + 1 s silence — for VAD trim tests.
    let mut mixed = silent_samples(1.0);
    mixed.extend(sine_samples(440.0, 1.0));
    mixed.extend(silent_samples(1.0));
    write_fixture("mixed.wav", &mixed, &out_dir);

    eprintln!();
    eprintln!("Done. {} fixtures generated.", 3);
    eprintln!("TTS speech fixtures (hello.wav, quick_brown_fox.wav) are a");
    eprintln!("Helios delegate task — see docs/phases/phase2-wave2-brief.md.");
}
