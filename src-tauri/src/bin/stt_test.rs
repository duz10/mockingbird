//! `stt_test` — CLI harness for the Phase 2 STT pipeline.
//!
//! Wave 1 ships the scaffold (parses args, prints a placeholder).
//! Wave 5 wires the full pipeline: load WAV → VAD trim → Whisper →
//! print transcript + latency + `gpu_used`.
//!
//! Usage (Wave 5):
//!   cargo run --bin stt_test -- path/to/audio.wav
//!   cargo run --bin stt_test -- path/to/audio.wav --force-cpu

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: stt_test <path-to-wav> [--force-cpu]");
        std::process::exit(2);
    }
    let path = &args[1];
    let force_cpu = args.iter().any(|a| a == "--force-cpu");

    eprintln!("stt_test scaffold (Phase 2 Wave 1)");
    eprintln!("  input: {path}");
    eprintln!("  force_cpu: {force_cpu}");
    eprintln!();
    eprintln!("Wave 5 wires the real pipeline:");
    eprintln!("  1. hound: load WAV → i16 samples");
    eprintln!("  2. audio::vad: trim silence");
    eprintln!("  3. stt::whisper: transcribe");
    eprintln!("  4. print transcript + latency + gpu_used");
    std::process::exit(0);
}
