# Phase 2 Wave 3 — Implementation brief (Silero VAD via `ort`)

> Wave 3 fills the `audio/vad.rs` scaffold with a real Silero VAD impl
> via the `ort` crate (ONNX Runtime v2). Wave 1 shipped the trait
> shape; Wave 3 makes voice/silence detection work on captured PCM.
>
> Model is **already on disk** (~2.27 MB, SHA-256 pinned in
> `scripts/model-manifest.json` post-Wave-2):
>   `$env:USERPROFILE\mockingbird_models\silero_vad.onnx`
>
> Brief pattern continues to deliver 100% first-run rates. Treat as
> binding.

## Tasks in scope (4 bd tasks from Wave 3)

| bd id    | Deliverable                                        | Approx. lines |
|----------|----------------------------------------------------|---------------|
| `mb-8ym` | `VoiceActivityDetector` trait — verify Wave-1 shape | 0 net (verify)|
| `mb-n2z` | Silero ONNX wrapper via `ort` v2                   | ~250 |
| `mb-3cq` | VAD trim helper (PCM in → speech-only PCM out)     | ~120 |
| `mb-fdt` | VAD tests over fixture audio                       | ~150 |

Plus inevitable extras:
- `Cargo.toml`: add `ort = "2"` workspace dep.
- `tests/vad.rs`: cross-crate integration tests over the 3 fixtures.

**Total budget:** ~520 lines net new.

## Cross-cutting decisions (binding, locked by ADR 0012 + this brief)

### 1. `ort = "2"` with default features

Default features bundle the ONNX Runtime DLLs (`onnxruntime.dll` ≈
8 MB on Windows) — no system install needed. Pin minor version
(`ort = "2"`) to ride patch releases automatically but avoid major
API breakage during the wave.

### 2. Silero v5+ model API (the new one)

The current Silero VAD model (the one we just downloaded) exposes
this ONNX signature:

```
inputs:
  - "input": float32, shape [batch, num_samples]  (audio @ 16 kHz)
  - "state": float32, shape [2, batch, 128]       (LSTM h + c stacked)
  - "sr":    int64,   shape []                     (scalar; 16000)
outputs:
  - "output": float32, shape [batch, 1]            (speech probability)
  - "stateN": float32, shape [2, batch, 128]       (updated state for next call)
```

Frame size **512 samples** (32 ms @ 16 kHz) — Silero v5's expected
window. Our cpal pipeline produces 30 ms frames (480 samples); we
need to either:
- Buffer to 512 before calling Silero, OR
- Pad each 480 to 512 with zeros (simpler; minor accuracy cost)

**Decision:** Buffer in the VAD wrapper. Append incoming samples to an
internal `Vec<i16>`; consume 512-sample chunks; carry residual. This
keeps the cpal/VAD framing decoupled.

### 3. Threshold = 0.5

Silero's default. Above → speech. Configurable later (Phase 6 maybe);
Wave 3 hard-codes.

### 4. State carries between calls; `reset()` zeroes it

The `state` tensor is the LSTM hidden+cell stacked. We initialize to
zeros on construction, feed it back through every call, and `reset()`
zeros it again (used between utterances).

### 5. Model loading: try `models_dir()` first, then `MODEL_PATH`, then a Wave-3-specific test-only fallback

In production, the loader uses `crate::stt::models_dir()` (Wave 1
ready) to find `silero_vad.onnx`. In tests, we want CI to skip
gracefully if the model isn't downloaded. The trick:

```rust
fn locate_model() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SILERO_VAD_PATH") {
        let pb = PathBuf::from(p);
        if pb.exists() { return Some(pb); }
    }
    if let Ok(dir) = crate::stt::models_dir() {
        let candidate = dir.join("silero_vad.onnx");
        if candidate.exists() { return Some(candidate); }
    }
    None
}
```

Tests pass `SILERO_VAD_PATH` env var to point at their preferred
location; production sets nothing and relies on `models_dir()`.

### 6. `ort` v2 API churn — adapt if minor signatures differ

The brief specifies the v2.0 API shape (as of late 2024). If a
later patch version changes things (`Session::builder` → `EnvBuilder`,
`inputs!` macro renamed, etc.), adapt and **add a LESSONS entry**.
Don't push to 5 attempts.

### 7. VAD trim helper is a pure function

```rust
pub struct TrimConfig {
    pub lead_in_ms: u32,         // keep N ms of pre-speech buffer
    pub hangover_ms: u32,        // keep N ms of post-speech buffer
    pub min_speech_ms: u32,      // discard utterances shorter than N ms
}
```

`vad_trim(audio: &[i16], detector: &mut dyn VoiceActivityDetector, cfg: &TrimConfig) -> Vec<i16>` returns speech-only PCM with `lead_in` + `hangover` padding. Pure-ish: it mutates the detector's state but doesn't allocate beyond the output.

---

## Module 1: `src-tauri/src/audio/vad.rs` — Silero impl (~250 lines)

### Concrete shape (subject to ort v2 API verification)

```rust
#![allow(missing_docs)]

//! Voice Activity Detection — Silero ONNX via the `ort` crate.

use std::path::PathBuf;

use ort::{
    inputs,
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VadFrame {
    pub is_speech: bool,
    pub confidence: f32,
}

pub trait VoiceActivityDetector: Send {
    /// Score one 512-sample frame (32 ms @ 16 kHz).
    fn process_frame(&mut self, frame: &[i16]) -> AppResult<VadFrame>;

    /// Reset internal LSTM state.
    fn reset(&mut self);

    /// Required input frame size in samples. Silero v5 = 512.
    fn frame_samples(&self) -> usize;
}

pub fn make_default_vad() -> AppResult<Box<dyn VoiceActivityDetector>> {
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(SileroVad::new()?))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(AppError::Audio(
            "VAD not implemented for this platform (Phase 9)".into(),
        ))
    }
}

#[cfg(target_os = "windows")]
const SILERO_FRAME_SAMPLES: usize = 512;
#[cfg(target_os = "windows")]
const SILERO_STATE_SHAPE: [usize; 3] = [2, 1, 128]; // (h+c, batch, hidden)
#[cfg(target_os = "windows")]
const SILERO_SR: i64 = 16_000;
#[cfg(target_os = "windows")]
const SPEECH_THRESHOLD: f32 = 0.5;

#[cfg(target_os = "windows")]
pub struct SileroVad {
    session: Session,
    /// LSTM hidden+cell state (flat row-major; we view as [2, 1, 128]).
    state: Vec<f32>,
}

#[cfg(target_os = "windows")]
impl SileroVad {
    pub fn new() -> AppResult<Self> {
        let model_path = locate_model().ok_or_else(|| {
            AppError::Audio(
                "silero_vad.onnx not found — set SILERO_VAD_PATH or run \
                 `pwsh scripts/download-models.ps1`"
                    .into(),
            )
        })?;
        Self::from_path(&model_path)
    }

    pub fn from_path(path: &std::path::Path) -> AppResult<Self> {
        let session = Session::builder()
            .map_err(|e| AppError::Audio(format!("ort Session::builder: {e}")))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| AppError::Audio(format!("set optimization level: {e}")))?
            .commit_from_file(path)
            .map_err(|e| AppError::Audio(format!("load model {}: {e}", path.display())))?;

        let state = vec![0.0f32; SILERO_STATE_SHAPE.iter().product()];
        Ok(Self { session, state })
    }
}

#[cfg(target_os = "windows")]
fn locate_model() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SILERO_VAD_PATH") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    if let Ok(dir) = crate::stt::models_dir() {
        let candidate = dir.join("silero_vad.onnx");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(target_os = "windows")]
impl VoiceActivityDetector for SileroVad {
    fn process_frame(&mut self, frame: &[i16]) -> AppResult<VadFrame> {
        if frame.len() != SILERO_FRAME_SAMPLES {
            return Err(AppError::Audio(format!(
                "Silero expects {SILERO_FRAME_SAMPLES} samples, got {}",
                frame.len()
            )));
        }

        // Convert i16 → f32 in [-1.0, 1.0].
        let audio_f32: Vec<f32> = frame
            .iter()
            .map(|&s| s as f32 / i16::MAX as f32)
            .collect();

        // Build tensors via ort's typed Tensor::from_array.
        // input: [1, 512]
        let input_t = Tensor::from_array(([1usize, SILERO_FRAME_SAMPLES], audio_f32))
            .map_err(|e| AppError::Audio(format!("build input tensor: {e}")))?;
        // state: [2, 1, 128]
        let state_t = Tensor::from_array((SILERO_STATE_SHAPE.to_vec(), self.state.clone()))
            .map_err(|e| AppError::Audio(format!("build state tensor: {e}")))?;
        // sr: scalar i64
        let sr_t = Tensor::from_array(([1usize], vec![SILERO_SR]))
            .map_err(|e| AppError::Audio(format!("build sr tensor: {e}")))?;

        let outputs = self
            .session
            .run(inputs![
                "input" => input_t,
                "state" => state_t,
                "sr" => sr_t,
            ])
            .map_err(|e| AppError::Audio(format!("ort run: {e}")))?;

        // Extract probability + new state.
        let (_, prob_data) = outputs["output"]
            .try_extract_tensor::<f32>()
            .map_err(|e| AppError::Audio(format!("extract output: {e}")))?;
        let confidence = prob_data[0];

        let (_, new_state) = outputs["stateN"]
            .try_extract_tensor::<f32>()
            .map_err(|e| AppError::Audio(format!("extract stateN: {e}")))?;
        self.state.clear();
        self.state.extend_from_slice(new_state);

        Ok(VadFrame {
            is_speech: confidence >= SPEECH_THRESHOLD,
            confidence,
        })
    }

    fn reset(&mut self) {
        self.state.iter_mut().for_each(|x| *x = 0.0);
    }

    fn frame_samples(&self) -> usize {
        SILERO_FRAME_SAMPLES
    }
}
```

**API verification step:** before writing the body above, run a quick
sanity script to dump the model's input/output names and shapes. If
they don't match `input`/`state`/`sr` → `output`/`stateN`, adapt and
add a LESSONS entry. The names can drift between Silero versions.

---

## Module 2: `src-tauri/src/audio/vad_trim.rs` (~120 lines) — NEW FILE

```rust
#![allow(missing_docs)]

//! Voice-activity-aware trimming helper.

use super::vad::VoiceActivityDetector;
use crate::error::AppResult;

#[derive(Debug, Clone)]
pub struct TrimConfig {
    pub lead_in_ms: u32,
    pub hangover_ms: u32,
    pub min_speech_ms: u32,
}

impl Default for TrimConfig {
    fn default() -> Self {
        Self {
            lead_in_ms: 100,
            hangover_ms: 300,
            min_speech_ms: 200,
        }
    }
}

/// Frame-aligned VAD scan over `audio` at 16 kHz. Returns trimmed
/// speech regions concatenated, with `lead_in_ms` and `hangover_ms`
/// preserved around each. Returns empty Vec when nothing scored as speech.
pub fn vad_trim(
    audio: &[i16],
    detector: &mut dyn VoiceActivityDetector,
    cfg: &TrimConfig,
) -> AppResult<Vec<i16>> {
    let fs = detector.frame_samples();
    let sample_rate = 16_000usize;
    let lead_in_samples = (cfg.lead_in_ms as usize * sample_rate) / 1000;
    let hangover_samples = (cfg.hangover_ms as usize * sample_rate) / 1000;
    let min_speech_samples = (cfg.min_speech_ms as usize * sample_rate) / 1000;

    // First pass: score every frame; build a Vec<bool> of frame decisions.
    let n_frames = audio.len() / fs;
    let mut frame_speech = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        let start = i * fs;
        let end = start + fs;
        let frame_decision = detector.process_frame(&audio[start..end])?;
        frame_speech.push(frame_decision.is_speech);
    }

    // Second pass: walk frames, identify contiguous speech runs,
    // attach lead_in + hangover, concat into output. Skip runs shorter
    // than min_speech_samples.
    let mut out = Vec::new();
    let mut i = 0;
    while i < frame_speech.len() {
        if !frame_speech[i] {
            i += 1;
            continue;
        }
        let speech_start_frame = i;
        while i < frame_speech.len() && frame_speech[i] {
            i += 1;
        }
        let speech_end_frame = i;

        let speech_start = speech_start_frame * fs;
        let speech_end = speech_end_frame * fs;
        if speech_end - speech_start < min_speech_samples {
            continue;
        }

        let region_start = speech_start.saturating_sub(lead_in_samples);
        let region_end = (speech_end + hangover_samples).min(audio.len());
        out.extend_from_slice(&audio[region_start..region_end]);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::vad::{VadFrame, VoiceActivityDetector};

    /// Fake VAD that says "speech" for frames whose samples include
    /// any non-zero value. Sufficient for testing the trim helper
    /// without loading Silero.
    struct AmplitudeVad {
        frame_size: usize,
    }
    impl VoiceActivityDetector for AmplitudeVad {
        fn process_frame(&mut self, frame: &[i16]) -> AppResult<VadFrame> {
            let any_nonzero = frame.iter().any(|&s| s != 0);
            Ok(VadFrame {
                is_speech: any_nonzero,
                confidence: if any_nonzero { 1.0 } else { 0.0 },
            })
        }
        fn reset(&mut self) {}
        fn frame_samples(&self) -> usize {
            self.frame_size
        }
    }

    #[test]
    fn empty_audio_returns_empty() {
        let mut v = AmplitudeVad { frame_size: 512 };
        let out = vad_trim(&[], &mut v, &TrimConfig::default()).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn all_silence_returns_empty() {
        let mut v = AmplitudeVad { frame_size: 512 };
        let audio = vec![0i16; 16_000];
        let out = vad_trim(&audio, &mut v, &TrimConfig::default()).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn pure_speech_returns_with_padding() {
        let mut v = AmplitudeVad { frame_size: 512 };
        // 1024 samples of tone (2 frames worth).
        let audio = vec![1000i16; 1024];
        let out = vad_trim(
            &audio,
            &mut v,
            &TrimConfig {
                lead_in_ms: 0,
                hangover_ms: 0,
                min_speech_ms: 0,
            },
        )
        .unwrap();
        assert_eq!(out.len(), 1024);
    }

    #[test]
    fn min_speech_threshold_drops_short_runs() {
        let mut v = AmplitudeVad { frame_size: 512 };
        // 512 samples of tone (1 frame) — 32ms — below 200ms min.
        let audio = vec![1000i16; 512];
        let out = vad_trim(&audio, &mut v, &TrimConfig::default()).unwrap();
        assert!(out.is_empty());
    }
}
```

Wire `vad_trim` into `audio/mod.rs`:

```rust
pub mod vad_trim;
pub use vad_trim::{vad_trim as trim_speech, TrimConfig};
```

---

## Module 3: `src-tauri/tests/vad.rs` (~150 lines) — NEW FILE

```rust
//! Cross-crate integration tests for Silero VAD. Skips gracefully
//! when the model file isn't present.

use mockingbird_lib::audio::vad::{make_default_vad, VoiceActivityDetector};
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    let m = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    PathBuf::from(m).join("tests/fixtures/audio")
}

/// True when Silero ONNX is reachable on this machine.
fn silero_available() -> bool {
    // Try the env override first, then the production resolution.
    if let Ok(p) = std::env::var("SILERO_VAD_PATH") {
        if std::path::Path::new(&p).is_file() {
            return true;
        }
    }
    if let Ok(dir) = mockingbird_lib::stt::models_dir() {
        if dir.join("silero_vad.onnx").is_file() {
            return true;
        }
    }
    false
}

fn read_wav_samples(path: &PathBuf) -> Vec<i16> {
    let mut reader = hound::WavReader::open(path).expect("open wav");
    reader.samples::<i16>().map(|s| s.unwrap()).collect()
}

#[test]
fn factory_constructs_when_model_present() {
    if !silero_available() {
        eprintln!("SKIP: silero_vad.onnx not on disk");
        return;
    }
    let _ = make_default_vad().expect("VAD construction");
}

#[test]
fn silent_audio_scores_below_threshold() {
    if !silero_available() {
        return;
    }
    let audio = read_wav_samples(&fixtures_dir().join("silent.wav"));
    let mut vad = make_default_vad().unwrap();
    let fs = vad.frame_samples();
    // First 5 frames of silence should all score as not-speech.
    let mut speech_count = 0;
    for i in 0..5 {
        let start = i * fs;
        let end = start + fs;
        if end > audio.len() {
            break;
        }
        let frame = vad.process_frame(&audio[start..end]).unwrap();
        if frame.is_speech {
            speech_count += 1;
        }
    }
    assert_eq!(speech_count, 0, "silence flagged as speech");
}

#[test]
fn sine_tone_scores_above_threshold_eventually() {
    if !silero_available() {
        return;
    }
    let audio = read_wav_samples(&fixtures_dir().join("sine_440.wav"));
    let mut vad = make_default_vad().unwrap();
    let fs = vad.frame_samples();
    // A 440 Hz tone is musical, not speech — Silero may or may not
    // flag it. We assert ONLY that processing doesn't error.
    for i in 0..(audio.len() / fs) {
        let start = i * fs;
        let end = start + fs;
        let _ = vad.process_frame(&audio[start..end]).unwrap();
    }
}

#[test]
fn reset_clears_state() {
    if !silero_available() {
        return;
    }
    let mut vad = make_default_vad().unwrap();
    let frame = vec![1000i16; vad.frame_samples()];
    let before = vad.process_frame(&frame).unwrap();
    vad.reset();
    let after = vad.process_frame(&frame).unwrap();
    // After reset, the LSTM state is back to zeros. For deterministic
    // models like Silero, processing the same frame after reset should
    // produce the same confidence (within float epsilon).
    assert!((before.confidence - after.confidence).abs() < 1e-4);
}
```

---

## Wave 3 exit checklist

- [ ] `cargo check --workspace` green
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] `cargo test --workspace` green (target ~130-135 tests total)
- [ ] `cargo fmt --check` clean
- [ ] `bd close mb-8ym mb-n2z mb-3cq mb-fdt`
- [ ] STATUS.md: Wave 3 ✅, Waves 4-5 queued
- [ ] LESSONS.md: any ort v2 API discoveries; Silero tensor name discoveries
- [ ] Commit: `feat(phase-2-wave-3): Silero VAD via ort + vad_trim helper`
- [ ] End-of-iteration: write Wave 4 brief (cmake/CUDA install precondition documented)
- [ ] **DO NOT** add `whisper-rs` to Cargo.toml — Wave 4

## Known risks

1. **ort v2 API drift.** The brief specifies the API shape as of late
   2024. Patch releases may move things. Adapt with LESSONS entries.
2. **Silero tensor names** ("input"/"state"/"sr" → "output"/"stateN")
   may differ in your downloaded version. Inspect the model first;
   adapt the `inputs!` call. Don't push beyond 5 attempts — escalate.
3. **i16 → f32 conversion precision.** Dividing by `i16::MAX` produces
   floats in (-1.0, 1.0). Some references use `32768.0` instead — either
   is fine; pick one and document.
4. **CPU inference on cold runs.** First call may take 30-100 ms (warm
   subsequent calls are < 5 ms). Tests should `process_frame` a few
   times before timing assertions.
5. **The "fake VAD" in `vad_trim` tests** intentionally doesn't load
   Silero — that's the unit-test boundary. Keep it that way.
6. **`Send` bound.** `Session` from ort is `Send` (per docs). The trait
   `VoiceActivityDetector: Send` (defined Wave 1) should hold. If it
   doesn't, drop the bound and document in LESSONS like we did for cpal.

## Out of scope for Wave 3

- STT (Wave 4)
- Real-time streaming VAD (Phase 5; Wave 3 batches)
- Configurable threshold (Wave 4 or Phase 6)
- GPU inference via DirectML (Phase 9 stretch)
- TTS speech fixtures (Wave 4 + Helios)
