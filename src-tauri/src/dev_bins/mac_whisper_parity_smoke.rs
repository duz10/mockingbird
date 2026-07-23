//! `mac_whisper_parity_smoke` -- judge shim for `mac-v1-parity-whisper-metal`
//! (mb-mac-v1.6.2, Phase 5). Thin wrapper over
//! [`mockingbird_lib::stt::parity_macos_v1::run_parity_eval`].
//!
//! Runs the PRODUCTION Metal-backed `WhisperStt` over the reference
//! corpus (`src-tauri/eval/stt_reference_transcripts.json`), computes
//! WER + CER per clip against ground truth, and asserts:
//!   1. the Metal backend actually ENGAGED (not a silent CPU fallback), and
//!   2. aggregate WER <= budget.wer_max AND aggregate CER <= budget.cer_max.
//!
//! ## Budget rationale
//!
//! Parity premise: macOS Metal and Windows CUDA run the same whisper.cpp
//! engine over the same `whisper-large-v3-turbo-q5_0` GGUF, so their
//! output should be near-identical. large-v3-turbo is a strong model;
//! on a clean clip like `jfk.wav` it is expected to hit WER 0 (an exact
//! transcript). The budget (default WER <= 0.10, CER <= 0.05, in the
//! corpus JSON) leaves a few % of slack for q5 quantization plus
//! Metal-vs-CUDA floating-point drift -- generous enough not to be
//! flaky, tight enough that a real quality regression fails it.
//!
//! ## Corpus
//!
//! Only `jfk.wav` is real speech (whisper.cpp's canonical sample). The
//! other committed fixtures are synthetic non-speech and carry no
//! ground-truth transcript, so they are not part of the WER/CER
//! acceptance. See the corpus JSON's `_doc` for how to widen it.
//!
//! ## Mac-vs-Windows-CUDA
//!
//! This scores Metal-vs-ground-truth (the primary parity acceptance). A
//! stricter Metal-vs-CUDA-golden comparison would need CUDA reference
//! transcripts generated on the Windows box; none exist in-repo. The
//! final report flags exactly what to generate there if that tighter
//! check is wanted for v1.
//!
//! Resolution order:
//!   - model:  argv[1] | $WHISPER_MODEL_PATH | $MODEL_PATH/<gguf> | <repo>/models/<gguf>
//!   - corpus: argv[2] | <repo>/src-tauri/eval/stt_reference_transcripts.json
//!   - wavs:   <corpus dir>/../tests/fixtures/audio/<wav> (per-clip `wav`)
//!
//! Run (via the Mac wrapper which exports MODEL_PATH + injects --features):
//!   scripts/dev/cargo-mac.sh run --release --example mac_whisper_parity_smoke
//!
//! Exit codes: 0 = pass · 1 = runtime/assert failure · 2 = wrong platform.

// Built as a real probe only on macOS WITH metal (its driver depends on
// `stt::judges_macos_v1`, which needs `whisper-rs/raw-api` from `metal`).
// Every other config gets a stub so `--all-targets` stays green without metal.
#[cfg(not(all(target_os = "macos", feature = "metal")))]
fn main() {
    eprintln!(
        "mac_whisper_parity_smoke requires macOS + `--features metal` \
         (use scripts/dev/cargo-mac.sh)"
    );
    std::process::exit(2);
}

#[cfg(all(target_os = "macos", feature = "metal"))]
fn main() {
    use std::path::PathBuf;

    use mockingbird_lib::stt::parity_macos_v1::{run_parity_eval, ClipSpec};

    const GGUF: &str = "whisper-large-v3-turbo-q5_0.bin";

    // <repo>/src-tauri is CARGO_MANIFEST_DIR; <repo> is its parent.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap_or(&manifest_dir).to_path_buf();

    let mut args = std::env::args().skip(1);
    let model_arg = args.next();
    let corpus_arg = args.next();

    let model_path = model_arg.map(PathBuf::from).unwrap_or_else(|| {
        if let Ok(p) = std::env::var("WHISPER_MODEL_PATH") {
            return PathBuf::from(p);
        }
        let dir = std::env::var("MODEL_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| repo_root.join("models"));
        dir.join(GGUF)
    });

    let corpus_path = corpus_arg.map(PathBuf::from).unwrap_or_else(|| {
        manifest_dir
            .join("eval")
            .join("stt_reference_transcripts.json")
    });

    println!("=== mac_whisper_parity_smoke (mac-v1-parity-whisper-metal) ===");
    println!("model:  {}", model_path.display());
    println!("corpus: {}", corpus_path.display());
    println!();

    // --- Parse the corpus JSON (budget + clips) --------------------------
    let raw = match std::fs::read_to_string(&corpus_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: read corpus {}: {e}", corpus_path.display());
            std::process::exit(1);
        }
    };
    let json: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: parse corpus JSON: {e}");
            std::process::exit(1);
        }
    };

    let wer_max = json["budget"]["wer_max"].as_f64().unwrap_or(0.10);
    let cer_max = json["budget"]["cer_max"].as_f64().unwrap_or(0.05);

    // WAVs live under src-tauri/tests/fixtures/audio/ relative to the crate.
    let audio_dir = manifest_dir.join("tests").join("fixtures").join("audio");

    let mut clips: Vec<ClipSpec> = Vec::new();
    match json["clips"].as_array() {
        Some(arr) => {
            for c in arr {
                let wav = match c["wav"].as_str() {
                    Some(w) => w,
                    None => {
                        eprintln!("error: corpus clip missing `wav`");
                        std::process::exit(1);
                    }
                };
                let reference = c["reference"].as_str().unwrap_or("").to_string();
                clips.push(ClipSpec {
                    name: wav.to_string(),
                    wav_path: audio_dir.join(wav),
                    reference,
                });
            }
        }
        None => {
            eprintln!("error: corpus JSON has no `clips` array");
            std::process::exit(1);
        }
    }

    if clips.is_empty() {
        eprintln!("error: corpus has zero clips");
        std::process::exit(1);
    }

    println!("budget: WER <= {wer_max:.3}  CER <= {cer_max:.3}");
    println!("clips:  {}", clips.len());
    println!();

    // --- Run the production Metal eval -----------------------------------
    let report = match run_parity_eval(&model_path, &clips) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    println!("backend: {}", report.backend);
    if let Some(ev) = &report.backend_evidence {
        println!("evidence: {ev}");
    }
    println!();

    for c in &report.clips {
        println!("--- {} ---", c.name);
        println!("  reference:  {}", c.reference);
        println!("  hypothesis: {}", c.hypothesis);
        println!(
            "  WER: {:.4} ({} word edits / {} ref words)",
            c.wer, c.word_counts.distance, c.word_counts.reference_len
        );
        println!(
            "  CER: {:.4} ({} char edits / {} ref chars)",
            c.cer, c.char_counts.distance, c.char_counts.reference_len
        );
        println!("  latency: {} ms", c.latency_ms);
        println!();
    }

    println!(
        "AGGREGATE: WER {:.4}  CER {:.4}  (micro-averaged)",
        report.aggregate_wer, report.aggregate_cer
    );
    println!();

    // --- Verdict ----------------------------------------------------------
    let mut ok = true;
    if !report.metal_engaged {
        eprintln!(
            "FAIL: Metal backend did NOT engage (got {}). A silent CPU \
             fallback defeats the parity intent.",
            report.backend
        );
        ok = false;
    }
    if report.aggregate_wer > wer_max {
        eprintln!(
            "FAIL: aggregate WER {:.4} exceeds budget {:.4}",
            report.aggregate_wer, wer_max
        );
        ok = false;
    }
    if report.aggregate_cer > cer_max {
        eprintln!(
            "FAIL: aggregate CER {:.4} exceeds budget {:.4}",
            report.aggregate_cer, cer_max
        );
        ok = false;
    }

    if ok {
        println!(
            "PASS: Metal STT meets the parity bar (WER {:.4} <= {:.3}, \
             CER {:.4} <= {:.3}) on a confirmed Metal backend.",
            report.aggregate_wer, wer_max, report.aggregate_cer, cer_max
        );
        std::process::exit(0);
    }
    std::process::exit(1);
}
