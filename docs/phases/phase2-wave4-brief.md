# Phase 2 Wave 4 — Implementation brief (whisper-rs + prompt builder + CLI)

> Wave 4 fills the `stt/whisper.rs` scaffold with a real
> whisper-rs-backed transcriber, wires the 224-token prompt builder,
> finishes the `stt_test` CLI harness end-to-end, and adds the TTS
> speech fixtures deferred from Wave 2.
>
> **⚠️ HARD TOOLCHAIN PRECONDITIONS.** Before this wave begins:
> 1. **cmake** ≥ 3.20 on PATH (`scoop install cmake` works)
> 2. **CUDA Toolkit** 12.x on PATH including `nvcc.exe`
>    (full installer from developer.nvidia.com, not just runtime)
> 3. **Visual Studio 2022 Build Tools** with C++ workload —
>    whisper-rs needs C++17 and ggml's CUDA kernels are MSVC-2022
>    only. VS 2019 BT (currently on path) may work for CPU-only
>    builds but CUDA support requires 2022.
>
> Verify with `scripts/verify-environment.ps1` (NEW in this wave —
> spec'd below). All three checks must pass before `cargo build`.

## Tasks in scope (7 bd tasks)

| bd id    | Deliverable                                          | Approx. lines |
|----------|------------------------------------------------------|---------------|
| `mb-bbc` | `SpeechToText` trait + `Transcript` shape verification | 0 net (verify Wave-1) |
| `mb-jq6` | `WhisperStt` impl via whisper-rs                     | ~280 |
| `mb-tpc` | 224-token prompt builder                             | ~150 |
| `mb-prl` | CUDA build wiring + runtime CPU fallback             | ~80 |
| `mb-1z6` | `stt_test` CLI end-to-end (Wave-1 has scaffold only) | ~200 |
| `mb-9q7` | STT unit + integration tests + criterion bench       | ~250 |
| `mb-mqz` | TTS speech fixtures (Wave 2 deferral; Helios delegate) | ~150 (script) |

Plus inevitable extras:
- `scripts/verify-environment.ps1` — preflight tool detection
- `Cargo.toml`: add `whisper-rs = { version = "0.13", features = ["cuda"] }` + `criterion` workspace
- `src-tauri/benches/whisper_latency.rs` — first criterion bench

**Total budget:** ~1,100 lines net new.

## Cross-cutting decisions (binding)

### 1. `whisper-rs = "0.13"` with `cuda` feature

Latest stable at time of writing. The `cuda` feature triggers
ggml's CUDA build via cmake. Without `cuda`, falls back to CPU
SIMD (still fast on 16-core hosts; ~3-5x slower than RTX 30/40-class GPU).

### 2. Runtime fallback: try GPU first, retry on CPU

Per ADR 0011. `WhisperStt::new()` calls `WhisperContext::new_with_params`
with `use_gpu = true`. If that errors (driver missing, OOM, ggml
init failure), retry with `use_gpu = false` and log the downgrade.
A `--force-cpu` CLI flag bypasses the GPU attempt.

### 3. Model: `whisper-large-v3-turbo-q5_0.bin`

Per Wave 1 manifest. ~547 MB after q5_0 quantization. Stored in
`models_dir()/whisper-large-v3-turbo-q5_0.bin`. Wave 1 already added
the manifest entry — Wave 4 just downloads via `download-models.ps1`
(SHA-256 currently `TBD-pin-when-downloaded`; verify on first fetch).

### 4. Prompt token cap: 224 (locked by `PROMPT_TOKEN_CAP`)

Defined in Wave 1's `stt/prompt_builder.rs`. The cap is enforced by
**truncating from the end of the dictionary section first**,
preserving any system prefix. Wave 4 makes the truncation real (Wave
1 has a stub).

### 5. Output type — `Transcript` (locked Wave 1)

```rust
pub struct Transcript {
    pub raw_text: String,
    pub segments: Vec<Segment>,
    pub model_id: String,           // "whisper-large-v3-turbo-q5_0"
    pub language: Option<String>,    // BCP-47, "en" expected
    pub duration_ms: u64,           // wall-clock inference time
    pub prompt_used: Option<String>, // for provenance
}
pub struct Segment {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub confidence: Option<f32>,
}
```

**Provenance is total** — the `Transcript` carries enough to
reconstruct the inference. Phase 4 will persist these via the
transcripts repo.

### 6. `force_cpu: bool` flag on WhisperStt

Constructor takes it. Tests use `force_cpu: true` to avoid
GPU-availability flakes in CI / dev-without-GPU. Production reads it
from settings (Phase 4) or CLI flag.

### 7. CLI harness shape (extends Wave-1 scaffold)

```
stt_test --wav <path> [--language en] [--prompt <text>] [--force-cpu]
         [--model-path <path>]
```

Reads WAV via hound, sends to WhisperStt, prints JSON-serialized
Transcript to stdout. Exit code 0 on success, non-zero on any
failure with error printed to stderr.

### 8. Criterion bench scope

One bench: `whisper_latency_on_sine_440`. Times a single
`WhisperStt::transcribe` call over the 1-second `sine_440.wav`. Not
about Whisper accuracy (a sine tone isn't speech) — just measures
the model + ort + I/O round-trip latency for performance regression
detection. Threshold: < 2 seconds per inference on CPU, < 500 ms on
GPU.

---

## Module 1: `src-tauri/src/stt/whisper.rs` — Windows impl (~280 lines)

### Concrete shape

```rust
#![allow(missing_docs)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
};

use crate::error::{AppError, AppResult};
use super::{Segment, SpeechToText, Transcript};

const MODEL_ID: &str = "whisper-large-v3-turbo-q5_0";
const SAMPLE_RATE: i32 = 16_000;

#[cfg(target_os = "windows")]
pub struct WhisperStt {
    ctx: WhisperContext,
    model_path: PathBuf,
    using_gpu: bool,
}

#[cfg(target_os = "windows")]
impl WhisperStt {
    /// Construct with GPU-first, CPU-fallback semantics.
    pub fn new(force_cpu: bool) -> AppResult<Self> {
        let model_path = locate_model()?;
        Self::from_path(&model_path, force_cpu)
    }

    pub fn from_path(path: &Path, force_cpu: bool) -> AppResult<Self> {
        // First attempt: GPU on if not forced off.
        if !force_cpu {
            let mut params = WhisperContextParameters::default();
            params.use_gpu = true;
            match WhisperContext::new_with_params(
                path.to_str().ok_or_else(|| AppError::Stt("non-UTF8 model path".into()))?,
                params,
            ) {
                Ok(ctx) => {
                    tracing::info!(target: "stt", "Whisper loaded with GPU");
                    return Ok(Self { ctx, model_path: path.to_path_buf(), using_gpu: true });
                }
                Err(e) => {
                    tracing::warn!(target: "stt", error = %e, "GPU init failed; falling back to CPU");
                }
            }
        }

        // CPU fallback (or forced).
        let mut params = WhisperContextParameters::default();
        params.use_gpu = false;
        let ctx = WhisperContext::new_with_params(
            path.to_str().ok_or_else(|| AppError::Stt("non-UTF8 model path".into()))?,
            params,
        )
        .map_err(|e| AppError::Stt(format!("Whisper CPU init: {e}")))?;
        tracing::info!(target: "stt", "Whisper loaded with CPU");
        Ok(Self { ctx, model_path: path.to_path_buf(), using_gpu: false })
    }

    pub fn using_gpu(&self) -> bool { self.using_gpu }
}

#[cfg(target_os = "windows")]
impl SpeechToText for WhisperStt {
    fn transcribe(
        &mut self,
        audio_pcm_i16_16k_mono: &[i16],
        language_hint: Option<&str>,
        prompt: Option<&str>,
    ) -> AppResult<Transcript> {
        let started = Instant::now();

        // whisper-rs expects f32 in [-1.0, 1.0].
        let audio_f32: Vec<f32> = audio_pcm_i16_16k_mono
            .iter()
            .map(|&s| s as f32 / 32768.0)
            .collect();

        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| AppError::Stt(format!("create_state: {e}")))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_print_progress(false);
        params.set_print_special(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        if let Some(lang) = language_hint {
            params.set_language(Some(lang));
        }
        if let Some(p) = prompt {
            params.set_initial_prompt(p);
        }

        state
            .full(params, &audio_f32)
            .map_err(|e| AppError::Stt(format!("whisper full: {e}")))?;

        let n_segments = state
            .full_n_segments()
            .map_err(|e| AppError::Stt(format!("n_segments: {e}")))?;

        let mut segments = Vec::with_capacity(n_segments as usize);
        let mut raw_text = String::new();
        for i in 0..n_segments {
            let text = state
                .full_get_segment_text(i)
                .map_err(|e| AppError::Stt(format!("segment text {i}: {e}")))?;
            let t0 = state
                .full_get_segment_t0(i)
                .map_err(|e| AppError::Stt(format!("segment t0 {i}: {e}")))?;
            let t1 = state
                .full_get_segment_t1(i)
                .map_err(|e| AppError::Stt(format!("segment t1 {i}: {e}")))?;
            // whisper-rs returns timestamps in centiseconds (×10 ms).
            let start_ms = (t0 as u64) * 10;
            let end_ms = (t1 as u64) * 10;

            raw_text.push_str(&text);
            segments.push(Segment {
                text: text.trim().to_string(),
                start_ms,
                end_ms,
                confidence: None, // whisper-rs 0.13 doesn't expose per-segment scores
            });
        }

        Ok(Transcript {
            raw_text: raw_text.trim().to_string(),
            segments,
            model_id: MODEL_ID.into(),
            language: language_hint.map(String::from),
            duration_ms: started.elapsed().as_millis() as u64,
            prompt_used: prompt.map(String::from),
        })
    }
}

#[cfg(target_os = "windows")]
fn locate_model() -> AppResult<PathBuf> {
    let dir = crate::stt::models_dir()?;
    let candidate = dir.join("whisper-large-v3-turbo-q5_0.bin");
    if !candidate.is_file() {
        return Err(AppError::Stt(format!(
            "Whisper model not found at {} — run `scripts/download-models.ps1`",
            candidate.display()
        )));
    }
    Ok(candidate)
}
```

### Tests (in-file unit tests)

```rust
#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    fn whisper_available() -> bool {
        locate_model().is_ok()
    }

    #[test]
    fn force_cpu_constructs_without_gpu() {
        if !whisper_available() {
            eprintln!("SKIP: Whisper model not on disk");
            return;
        }
        let stt = WhisperStt::new(true).expect("CPU construct");
        assert!(!stt.using_gpu());
    }

    #[test]
    fn transcribe_silent_audio_returns_empty_or_neutral() {
        if !whisper_available() {
            return;
        }
        let mut stt = WhisperStt::new(true).unwrap();
        let silent = vec![0i16; 16_000]; // 1 s
        let tx = stt.transcribe(&silent, Some("en"), None).unwrap();
        // Whisper may emit a special token (e.g. "[BLANK_AUDIO]") for
        // pure silence. We just assert it didn't fabricate a long
        // English sentence.
        assert!(tx.raw_text.len() < 50, "silent audio produced: {}", tx.raw_text);
        assert_eq!(tx.model_id, MODEL_ID);
        assert_eq!(tx.language.as_deref(), Some("en"));
    }

    #[test]
    fn transcribe_writes_duration() {
        if !whisper_available() {
            return;
        }
        let mut stt = WhisperStt::new(true).unwrap();
        let silent = vec![0i16; 16_000];
        let tx = stt.transcribe(&silent, None, None).unwrap();
        assert!(tx.duration_ms > 0, "duration_ms is zero");
    }
}
```

---

## Module 2: `src-tauri/src/stt/prompt_builder.rs` — fill in the body (~150 lines)

Wave 1 ships `build_prompt(_: &PromptInputs) -> Option<String>` with
a stub `None` return. Wave 4 implements:

```rust
pub struct PromptInputs<'a> {
    pub system_prefix: Option<&'a str>,
    pub dictionary_terms: &'a [&'a str],
    pub recent_examples: &'a [&'a str],
}

pub fn build_prompt(inputs: &PromptInputs) -> Option<String> {
    // Layout:
    //   {system_prefix}\nDictionary: {term1}, {term2}, ...\nExamples: ...
    //
    // Truncation order when over PROMPT_TOKEN_CAP (224):
    //   1. Drop recent_examples from oldest to newest
    //   2. Then drop dictionary_terms from end-of-list
    //   3. Keep system_prefix intact (caller's responsibility)
    //
    // Token counting: whisper's tokenizer isn't available; we
    // approximate with characters/4 (industry rule of thumb).
    //
    // Returns None if even system_prefix alone exceeds the cap
    // (caller must shorten it).
}
```

**Test specs:**
- `build_prompt_returns_none_for_empty_input` ✅ (Wave 1 already wrote)
- `build_prompt_truncates_dictionary_from_end_when_over_cap`
- `build_prompt_drops_examples_before_dictionary`
- `build_prompt_returns_none_when_system_prefix_alone_too_long`
- `prompt_token_cap_is_224` ✅ (Wave 1 already wrote)
- `build_prompt_preserves_term_order_until_truncation`

### Token estimator

Conservative: `(text.chars().count() as f32 / 3.5).ceil() as usize`.
The 3.5 char/token average is pessimistic vs the 4.0 industry default —
errs on the side of under-stuffing the prompt.

---

## Module 3: `scripts/verify-environment.ps1` (~80 lines)

```powershell
<#
.SYNOPSIS
    Preflight check for Phase 2 Wave 4 toolchain requirements.

.DESCRIPTION
    whisper-rs with the `cuda` feature needs cmake + nvcc + a
    sufficiently modern MSVC. This script enumerates the tools and
    fails loudly if any are missing, with installation hints.

    Run before `cargo build` whenever the toolchain might have changed.
#>

[CmdletBinding()]
param(
    [switch]$AllowMissingCuda  # for CPU-only builds
)

$ErrorActionPreference = 'Stop'
$problems = @()

# cmake
$cmake = Get-Command cmake -ErrorAction SilentlyContinue
if (-not $cmake) {
    $problems += @{
        Tool = 'cmake'
        Fix  = 'scoop install cmake  # or download from cmake.org'
    }
} else {
    $cmakeVersion = (cmake --version | Select-String -Pattern 'version ([\d.]+)').Matches.Groups[1].Value
    Write-Host "✓ cmake $cmakeVersion at $($cmake.Source)" -ForegroundColor Green
}

# nvcc (CUDA Toolkit)
$nvcc = Get-Command nvcc -ErrorAction SilentlyContinue
if (-not $nvcc) {
    if ($AllowMissingCuda) {
        Write-Host "⚠ nvcc not found — CPU-only build (use --force-cpu at runtime)" -ForegroundColor Yellow
    } else {
        $problems += @{
            Tool = 'nvcc (CUDA Toolkit)'
            Fix  = 'Install CUDA Toolkit 12.x from https://developer.nvidia.com/cuda-downloads'
        }
    }
} else {
    $nvccVersion = (nvcc --version | Select-String -Pattern 'release ([\d.]+)').Matches.Groups[1].Value
    Write-Host "✓ nvcc $nvccVersion at $($nvcc.Source)" -ForegroundColor Green
}

# MSVC version
$cl = Get-Command cl.exe -ErrorAction SilentlyContinue
if ($cl) {
    $clOutput = & cl.exe 2>&1 | Select-Object -First 1
    Write-Host "ℹ MSVC: $clOutput" -ForegroundColor Cyan
    if ($clOutput -match '19\.2[0-9]\.') {
        Write-Host "⚠ Detected MSVC 19.2x (VS 2019). whisper-rs cuda may need MSVC 19.3x (VS 2022)" -ForegroundColor Yellow
    }
}

if ($problems.Count -gt 0) {
    Write-Host ""
    Write-Host "❌ Missing tools:" -ForegroundColor Red
    foreach ($p in $problems) {
        Write-Host "   $($p.Tool):  $($p.Fix)" -ForegroundColor Red
    }
    exit 1
}

Write-Host ""
Write-Host "✅ All preconditions satisfied for Phase 2 Wave 4." -ForegroundColor Green
```

---

## Module 4: `src-tauri/src/bin/stt_test.rs` — end-to-end CLI (~200 lines)

Wave 1 ships scaffold (arg parsing only). Wave 4 makes it actually
load WAVs, run inference, print JSON output.

Use `clap` (already a workspace dep) for arg parsing. Use `serde_json`
(already in scope) for output serialization. Add `#[derive(Serialize)]`
to `Transcript` + `Segment` in `stt/mod.rs`.

```rust
//! Wave 4 binary: load a WAV, run Whisper, print JSON transcript.
//!
//! Usage:
//!   cargo run --bin stt_test -- --wav path.wav --language en --force-cpu
//!
//! Exit 0 on success; non-zero with stderr on any failure.

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("stt_test is Windows-only in Phase 2");
    std::process::exit(2);
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    use clap::Parser;
    use mockingbird_lib::stt::{whisper::WhisperStt, SpeechToText};

    #[derive(Parser, Debug)]
    struct Args {
        #[arg(long)]
        wav: std::path::PathBuf,
        #[arg(long)]
        language: Option<String>,
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long, default_value_t = false)]
        force_cpu: bool,
        #[arg(long)]
        model_path: Option<std::path::PathBuf>,
    }

    let args = Args::parse();

    let mut reader = match hound::WavReader::open(&args.wav) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("open wav: {e}");
            return std::process::ExitCode::from(1);
        }
    };

    let spec = reader.spec();
    if spec.sample_rate != 16_000 || spec.channels != 1 || spec.bits_per_sample != 16 {
        eprintln!(
            "wav must be 16 kHz mono 16-bit; got {} Hz / {} ch / {} bps",
            spec.sample_rate, spec.channels, spec.bits_per_sample
        );
        return std::process::ExitCode::from(1);
    }

    let audio: Vec<i16> = match reader.samples::<i16>().collect::<Result<_, _>>() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("read wav samples: {e}");
            return std::process::ExitCode::from(1);
        }
    };

    let stt_result = if let Some(p) = args.model_path {
        WhisperStt::from_path(&p, args.force_cpu)
    } else {
        WhisperStt::new(args.force_cpu)
    };

    let mut stt = match stt_result {
        Ok(s) => s,
        Err(e) => {
            eprintln!("whisper init: {e}");
            return std::process::ExitCode::from(1);
        }
    };

    let transcript = match stt.transcribe(
        &audio,
        args.language.as_deref(),
        args.prompt.as_deref(),
    ) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("transcribe: {e}");
            return std::process::ExitCode::from(1);
        }
    };

    println!("{}", serde_json::to_string_pretty(&transcript).unwrap());
    std::process::ExitCode::SUCCESS
}
```

---

## Module 5: TTS speech fixtures (mb-mqz, deferred from Wave 2)

`scripts/generate-tts-fixtures.ps1` — uses `System.Speech.Synthesis`
(built into Windows) to render:
- `hello.wav` — "hello world"
- `quick_brown_fox.wav` — "The quick brown fox jumps over the lazy dog"

Then converts to 16 kHz mono i16 via... hmm, no SoX/ffmpeg on the
dev machine. **Two paths:**

1. **Helios delegate** — invoke `helios` to write a Rust binary
   `bin/render_speech_fixtures.rs` that uses the Windows `tts` crate
   or a pure-Rust SAPI binding to generate 16 kHz output directly.
2. **PowerShell + on-the-fly resample** — `System.Speech` writes
   22.05 kHz; we resample to 16 kHz in `hound` via a Rust helper.

**Decision for Wave 4:** Try Helios first. If the `tts` crate isn't
viable on Windows SAPI, fall back to Rust resampling via `rubato`
(adding it as the resampler dep we deferred in Wave 2).

```rust
// Helios target: src-tauri/src/bin/render_speech_fixtures.rs
// Generates 2 WAVs in tests/fixtures/audio/ matching Wave-2's
// hound spec (16 kHz mono i16). Idempotent: skip if file exists.
```

Add 2 integration tests in `tests/whisper.rs` that:
- Load `hello.wav`, transcribe, assert `tx.raw_text.to_lowercase().contains("hello")`
- Load `quick_brown_fox.wav`, transcribe, assert `tx.raw_text.contains("fox")`

These tests guard the production pipeline.

---

## Module 6: Criterion bench (~80 lines)

`src-tauri/benches/whisper_latency.rs`:

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use mockingbird_lib::stt::{whisper::WhisperStt, SpeechToText};
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio")
}

fn bench_transcribe_sine(c: &mut Criterion) {
    let path = fixtures_dir().join("sine_440.wav");
    if !path.exists() {
        eprintln!("SKIP: sine_440.wav missing; run generate_fixtures");
        return;
    }
    let mut reader = hound::WavReader::open(&path).unwrap();
    let audio: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();

    // Force CPU for deterministic measurement (GPU thermal/clock
    // variance dwarfs the inference cost on short clips).
    let mut stt = match WhisperStt::new(true) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("SKIP: whisper model not loaded");
            return;
        }
    };

    c.bench_function("whisper_latency_1s_sine_cpu", |b| {
        b.iter(|| {
            let _ = stt.transcribe(black_box(&audio), Some("en"), None).unwrap();
        })
    });
}

criterion_group!(benches, bench_transcribe_sine);
criterion_main!(benches);
```

Add `bench = false` to the existing `[lib]` section and a new
`[[bench]]` to `src-tauri/Cargo.toml`:

```toml
[[bench]]
name = "whisper_latency"
harness = false
```

---

## Wave 4 exit checklist

- [ ] `pwsh scripts/verify-environment.ps1` passes (cmake + nvcc + MSVC)
- [ ] `pwsh scripts/download-models.ps1` fetches whisper model; SHA-256 pinned in manifest after first run
- [ ] `cargo check --workspace` green
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] `cargo test --workspace` green (target ~145 tests)
- [ ] `cargo fmt --check` clean
- [ ] `cargo run --bin stt_test -- --wav tests/fixtures/audio/hello.wav --force-cpu --language en` prints JSON with "hello" in raw_text
- [ ] `cargo bench --bench whisper_latency` runs and produces a baseline number
- [ ] `bd close mb-bbc mb-jq6 mb-tpc mb-prl mb-1z6 mb-9q7 mb-mqz`
- [ ] STATUS.md: Wave 4 ✅, Wave 5 queued
- [ ] LESSONS.md: any whisper-rs build/runtime discoveries
- [ ] Commit: `feat(phase-2-wave-4): whisper-rs CUDA+CPU + prompt builder + stt_test CLI + bench`
- [ ] End-of-iteration: write Wave 5 brief

## Known risks

1. **VS 2019 BT may not link whisper-rs CUDA.** If `cargo build`
   fails with similar `__std_*` symbols, you must install VS 2022
   BuildTools. ESCALATE — don't push past 5 attempts.
2. **CUDA OOM at startup.** Large-v3-turbo on small VRAM cards
   (< 6 GB) may OOM during ggml init. The CPU fallback catches this.
3. **whisper-rs API drift.** v0.13 is target; if a newer version is
   on crates.io, the function signatures (`FullParams::set_*`,
   `state.full_get_segment_t0`) may have moved. Read the docs first.
4. **Token estimator imprecision.** The 3.5 chars/token rule is an
   approximation; some prompts will under-fill, others may
   accidentally exceed Whisper's hard 224-token limit (Whisper will
   silently truncate). Verify with the longest-realistic prompt fixture
   in a dedicated test.
5. **Criterion bench machine variance.** Don't fail CI on bench
   regressions; record numbers as info-only. Phase 5 establishes
   regression thresholds with multiple data points.
6. **TTS fixtures via System.Speech may not be 16 kHz.** SAPI's
   default voices commonly output 22.05 kHz or 16 kHz depending on
   voice. If resampling is needed, Helios builds the rubato wrapper.
7. **whisper-rs 0.13 may emit `[BLANK_AUDIO]` or similar tokens** for
   silent input. Tests over `silent.wav` assert `raw_text.len() < 50`
   rather than equality with empty string.

## Out of scope for Wave 4

- Streaming transcription (Phase 5; Wave 4 batches the whole audio)
- Speaker diarization (Phase 6 stretch)
- Per-segment confidence scores (whisper-rs 0.13 doesn't expose)
- Custom model fine-tuning (Phase 9)
- Settings UI integration for `force_cpu` (Phase 5)
- Production DLL bundling for onnxruntime.dll (Phase 4 Tauri integration)

## Wave-5 brief preview

End of Wave 4, write `docs/phases/phase2-wave5-brief.md`. Wave 5
is the seal wave for Phase 2:
- Hook up captureAudio → vad_trim → transcribe end-to-end pipeline
- Write 3 new judges (e.g., `whisper-fallback-tested`, `vad-config-locked`, `stt-uses-prompt-builder`)
- Seal commit + `phase-2-complete` tag
- Final retrospective in `docs/LESSONS.md` mirroring Phase 1's structure
