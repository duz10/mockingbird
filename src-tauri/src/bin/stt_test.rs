//! `stt_test` — CLI harness for the Phase 2 STT pipeline.
//!
//! Loads a WAV (16 kHz mono i16), optionally trims silence via Silero
//! VAD, runs Whisper, and prints either pretty or JSON output.
//!
//! Usage:
//!   cargo run --bin stt_test -- path/to/audio.wav
//!   cargo run --bin stt_test -- path/to/audio.wav --force-cpu
//!   cargo run --bin stt_test -- path/to/audio.wav --json
//!   cargo run --bin stt_test -- path/to/audio.wav --no-vad
//!   cargo run --bin stt_test -- path/to/audio.wav --prompt "Rust, Tauri, cargo"
//!   cargo run --bin stt_test -- path/to/audio.wav --model-path C:\path\to\ggml-large-v3-turbo-q5_0.bin
//!
//! Exit codes:
//!   0 — success
//!   1 — runtime failure (WAV read, model init, transcribe, etc.)
//!   2 — usage / argument error

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("stt_test is Windows-only in Phase 2");
    std::process::exit(2);
}

#[cfg(target_os = "windows")]
fn main() {
    use std::path::PathBuf;
    use std::process::ExitCode;

    use mockingbird_lib::audio::vad::make_default_vad;
    use mockingbird_lib::audio::vad_trim::{vad_trim, TrimConfig};
    use mockingbird_lib::stt::whisper::WhisperStt;
    use mockingbird_lib::stt::{SpeechToText, TranscribeRequest};

    let opts = match Options::parse() {
        Ok(o) => o,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };

    // Optional model-path override.
    if let Some(p) = &opts.model_path {
        std::env::set_var("WHISPER_MODEL_PATH", p);
    }

    let exit = (|| -> Result<ExitCode, String> {
        let audio = read_wav_i16_16k_mono(&opts.wav)?;

        let trimmed = if opts.no_vad {
            audio
        } else {
            match make_default_vad() {
                Ok(mut v) => {
                    let cfg = TrimConfig::default();
                    vad_trim(&audio, v.as_mut(), &cfg).map_err(|e| format!("vad_trim: {e}"))?
                }
                Err(e) => {
                    eprintln!("warning: VAD unavailable ({e}); skipping trim");
                    audio
                }
            }
        };

        let mut stt = WhisperStt::new_with_options(opts.force_cpu)
            .map_err(|e| format!("whisper init: {e}"))?;
        let req = TranscribeRequest {
            audio: &trimmed,
            initial_prompt: opts.prompt.as_deref(),
            force_cpu: opts.force_cpu,
        };
        let tx = stt
            .transcribe(req)
            .map_err(|e| format!("transcribe: {e}"))?;

        if opts.json {
            // Tiny hand-rolled JSON — no serde_json dep needed in this binary.
            println!(
                "{{\"text\":{},\"gpu_used\":{},\"latency_ms\":{},\"model_id\":{},\"input_samples\":{},\"trimmed_samples\":{}}}",
                json_string(&tx.text),
                tx.gpu_used,
                tx.latency_ms,
                json_string(&tx.model_id),
                opts.wav.metadata().map(|m| m.len()).unwrap_or(0),
                trimmed.len()
            );
        } else {
            println!("=== stt_test ===");
            println!("input:     {}", opts.wav.display());
            println!("samples:   {} (after VAD trim)", trimmed.len());
            println!(
                "backend:   {}",
                if tx.gpu_used { "GPU (CUDA)" } else { "CPU" }
            );
            println!("latency:   {} ms", tx.latency_ms);
            println!("model:     {}", tx.model_id);
            println!();
            println!("transcript:");
            println!("  {}", tx.text);
        }
        Ok(ExitCode::SUCCESS)
    })();

    match exit {
        Ok(c) => std::process::exit(if c == ExitCode::SUCCESS { 0 } else { 1 }),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }

    // ----- helpers --------------------------------------------------

    struct Options {
        wav: PathBuf,
        force_cpu: bool,
        json: bool,
        no_vad: bool,
        prompt: Option<String>,
        model_path: Option<PathBuf>,
    }

    impl Options {
        fn parse() -> Result<Self, String> {
            let mut args = std::env::args().skip(1).peekable();
            let mut wav: Option<PathBuf> = None;
            let mut force_cpu = false;
            let mut json = false;
            let mut no_vad = false;
            let mut prompt: Option<String> = None;
            let mut model_path: Option<PathBuf> = None;
            while let Some(a) = args.next() {
                match a.as_str() {
                    "--force-cpu" => force_cpu = true,
                    "--json" => json = true,
                    "--no-vad" => no_vad = true,
                    "--prompt" => prompt = args.next(),
                    "--model-path" => model_path = args.next().map(PathBuf::from),
                    "-h" | "--help" => {
                        return Err(USAGE.into());
                    }
                    other if !other.starts_with('-') && wav.is_none() => {
                        wav = Some(PathBuf::from(other));
                    }
                    other => return Err(format!("unrecognised arg: {other}\n\n{USAGE}")),
                }
            }
            Ok(Self {
                wav: wav.ok_or_else(|| format!("missing <wav> argument\n\n{USAGE}"))?,
                force_cpu,
                json,
                no_vad,
                prompt,
                model_path,
            })
        }
    }

    const USAGE: &str = "usage: stt_test <path-to-wav> \
                        [--force-cpu] [--json] [--no-vad] \
                        [--prompt TEXT] [--model-path PATH]";

    fn read_wav_i16_16k_mono(path: &PathBuf) -> Result<Vec<i16>, String> {
        let mut reader = hound::WavReader::open(path)
            .map_err(|e| format!("open wav {}: {e}", path.display()))?;
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

    fn json_string(s: &str) -> String {
        // Minimal RFC-8259 string escape; sufficient for transcript output.
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }
}
