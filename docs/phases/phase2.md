# Phase 2 — Audio capture & STT

**Phase entry tag:** `phase-1-complete` (commit `565a8cf`)
**Phase exit tag:** `phase-2-complete` (target — adds Stt/Audio AppError variants, ships whisper-rs CUDA path, no schema changes)
**Planner:** planning-agent-8c2ca3
**Implementor:** code-puppy (active agent) with Helios as an on-demand tool-builder for the TTS fixture generator
**Estimated iterations:** 4–5

> Binding spec lives in PLAN-mockingbird-v2.md lines 1362–1376 (Phase 2 deliverables),
> §5 (Cargo deps incl. whisper-rs/ort/cpal/ringbuf/hound/criterion), §2.1 (cross-platform
> trait rule), and §3 (Layer 1+2 flow). This doc operationalizes them.

## Overview

Phase 2 turns the dormant Phase 1 shell into something that can listen. By the end, a developer can run `cargo run --bin stt_test -- path/to/file.wav` and see a transcript printed with GPU init logs, latency measurement, and CPU-fallback path. No hotkey yet (Phase 3); no UI (Phase 5); no cleanup LLM (Phase 4). Just: mic in (cpal) → VAD trim (Silero ONNX via `ort`) → Whisper transcribe (whisper-rs CUDA) → text out, all wrapped in cross-platform traits with `#[cfg(target_os = "windows")]` impls + macOS/Linux `todo!()` stubs per the standing rule. No migration 004 is needed; the `transcripts` table from migration 001 already accommodates Phase-2 writes (which the CLI harness performs against a temp DB for the integration tests).

## Pre-flight — ADRs authored in Wave 1

### ADR 0011 — whisper-rs CUDA feature flags + CPU fallback strategy

`whisper-rs = { version = "0.13", features = ["cuda"] }` on Windows. CUDA mandatory at **build time** (matches PLAN's RTX-2060 perf budget); CPU fallback exists at **runtime** — if `WhisperContext::new_with_params` fails with a CUDA backend error, retry with `use_gpu = false` and emit a `tracing::warn!` line. The build still requires cmake + nvcc on PATH; that's gated by `setup-dev.ps1` step 4 and `verify-environment.ps1`. Documenting here so future contributors don't reinvent the gate.

### ADR 0012 — ort runtime choice + Silero VAD model loading

Use `ort = "2"` with default features (the `download-binaries` feature bundles the ONNX Runtime DLLs at build time, no system install). Pin the minor version in `Cargo.toml` to avoid surprise upgrades during a wave. Silero is loaded from `models/silero_vad.onnx` (path resolved via the same model-path helper as Whisper) — NOT `include_bytes!`-embedded, because the binary stays slim and the SHA-256 check covers tamper-detection regardless.

### ADR 0013 — cpal + ringbuf audio capture design

PCM format: **16 kHz mono i16**. Frame size: **30 ms** (480 samples), aligned with Silero's preferred input window. Buffer: `ringbuf` SPSC sized for 30 s of audio (16000 × 30 × 2 ≈ 1 MB) — generous so a slow VAD drain never drops frames. Default-device-changed handler subscribes to `cpal::Host` events (Windows: `MMNotificationClient`); on change, stop+restart capture and log via `tracing::info!`. Sample-rate conversion (if device's native rate ≠ 16 kHz) uses `rubato` if needed — DEFERRED to Wave 2 implementation; if cpal can request 16 kHz directly on Windows we skip rubato entirely.

### ADR 0014 — Model storage path

**Runtime location:** `%LOCALAPPDATA%\Mockingbird\models\` — local-only, per-machine, can be large (~1.5 GB for whisper). Not `%APPDATA%` (roams; quota issues with big blobs). **Dev location:** `models/` at repo root (gitignored per `.gitignore`). **Resolution order:** (1) `MODEL_PATH` env var (dev override), (2) `<exe_dir>/models/` (portable install), (3) `%LOCALAPPDATA%\Mockingbird\models\`. `scripts/model-manifest.json` lists each model's name, expected size, SHA-256, and source URL. `scripts/download-models.ps1` is resumable (`Invoke-WebRequest -Resume`) and idempotent (skips if SHA-256 already matches).

## Phase 2 Cargo deps (incremental to Phase 1 manifest)

Add (or unlock, since Phase 1 deferred these per ADR 0004's commentary):

```toml
cpal = "0.15"
ringbuf = "0.4"
hound = "3"                                      # WAV I/O for CLI + tests
whisper-rs = { version = "0.13", features = ["cuda"] }
ort = "2"                                        # ONNX Runtime
criterion = "0.5"                                # dev — perf bench
```

`enigo` stays deferred to Phase 3 (injection). No `tauri-plugin-*` additions.

## AppError carry-forward

Wave 1 adds two new variants to `src-tauri/src/error.rs`:

```rust
/// Audio capture / VAD failures (cpal, ort, ringbuf overflow).
#[error("audio error: {0}")]
Audio(String),

/// Speech-to-text failures (whisper-rs init, transcribe, model load).
#[error("stt error: {0}")]
Stt(String),
```

Per the Phase 1 LESSONS pattern, modules add `#[from]` variants when they bring a new source error type; for `whisper-rs`/`ort`/`cpal` we wrap as `String` because their error types are noisy and cross-platform-divergent.

## Task waves

### Wave 1 — Decisions, deps, AppError, download script, module scaffolds (Iteration 1)

| bd-task title (prefix `Phase 2:`) | priority | files |
|-----------------------------------|----------|-------|
| ADR 0011 — whisper-rs CUDA + CPU fallback | 1 | `docs/adr/0011-whisper-rs-cuda-build.md` |
| ADR 0012 — ort runtime + Silero VAD model loading | 1 | `docs/adr/0012-ort-runtime.md` |
| ADR 0013 — cpal + ringbuf audio capture design | 1 | `docs/adr/0013-cpal-ringbuf-design.md` |
| ADR 0014 — Model storage path | 2 | `docs/adr/0014-model-storage-path.md` |
| Cargo deps + AppError Stt/Audio variants | 1 | `Cargo.toml`, `src-tauri/Cargo.toml`, `src-tauri/src/error.rs` |
| `scripts/download-models.ps1` + `scripts/model-manifest.json` | 1 | new files |
| `audio/` + `stt/` + `bin/stt_test.rs` scaffolds (todo!() bodies, cross-platform stubs) | 2 | `src-tauri/src/audio/{mod,capture,vad}.rs`, `src-tauri/src/stt/{mod,whisper,prompt_builder}.rs`, `src-tauri/src/bin/stt_test.rs` |

### Wave 2 — Audio capture (Iteration 2; depends on Wave 1)

| bd-task title (prefix `Phase 2:`) | priority |
|-----------------------------------|----------|
| `AudioCapture` trait + types in `audio/capture.rs` | 1 |
| cpal Windows capture impl + ring buffer | 1 |
| Default-device-changed handler | 2 |
| TTS audio fixture generator (Helios-built if absent) | 2 |
| Audio capture unit + integration tests | 1 |

### Wave 3 — VAD (Silero ONNX via ort) (Iteration 3)

| bd-task title (prefix `Phase 2:`) | priority |
|-----------------------------------|----------|
| `VoiceActivityDetector` trait + types in `audio/vad.rs` | 1 |
| Silero ONNX wrapper via `ort` crate | 1 |
| VAD trim helper (PCM in → speech-only PCM out, with hangover/lead-in) | 1 |
| VAD tests over fixture audio (speech / silence / mixed) | 1 |

### Wave 4 — STT + prompt builder (Iteration 4; the heaviest wave)

| bd-task title (prefix `Phase 2:`) | priority |
|-----------------------------------|----------|
| `SpeechToText` trait + `Transcript` type in `stt/mod.rs` | 1 |
| whisper-rs CUDA init + CPU fallback in `stt/whisper.rs` | 1 |
| `prompt_builder.rs` — 224-token initial_prompt assembly | 1 |
| Scoring: recency × frequency × app-match (pulls from `db/dictionary.rs`) | 2 |
| prompt_builder unit tests (token cap, ordering, app-match) | 1 |
| STT integration test (fixture WAV → expected transcript) | 1 |

### Wave 5 — CLI harness, bench, judges, seal (Iteration 5)

| bd-task title (prefix `Phase 2:`) | priority |
|-----------------------------------|----------|
| `bin/stt_test.rs` CLI harness (`cargo run --bin stt_test -- file.wav`) | 1 |
| criterion bench (`benches/stt_latency.rs`; perf budget < 1s for 10s on RTX 2060) | 1 |
| Judge cards (`stt-correct`, `cuda-verified`, `perf-stt`) under `docs/judges/phase-2/` | 1 |
| STATUS update + LESSONS retrospective + `phase-2-complete` seal | 1 |

## Cross-wave invariants

1. **File size hard limit: 600 lines** (project rule). `audio/capture.rs` and `stt/whisper.rs` will press against this; split into submodules before hitting 500.
2. **Test density target: ~10 tests per ~500 LoC.** Phase 1 hit ~100 tests / ~5,000 lines; Phase 2 targets ~40–50 new tests across ~2,500 new lines.
3. **Cross-platform traits from day one.** Every audio/STT module pairs a trait with `#[cfg(target_os = "windows")]` impl + macOS/Linux `todo!()` stubs. Even though v1 is Windows-only, the type system enforces the layer boundary.
4. **AppError carry-forward.** Wave 1 adds `Stt(String)` + `Audio(String)`. Modules wrap library errors at construction site (e.g. `WhisperError → AppError::Stt(e.to_string())`).
5. **No migration 004 in Phase 2.** STT output is in-memory; the CLI harness writes to a temp DB for tests. Real DB writes happen in Phase 3 (session row per hotkey press).
6. **`tracing` only** — no `println!` outside the `bin/stt_test.rs` harness (CLI tool exception).
7. **The brief pattern is the documented default.** Code-puppy writes `docs/phases/phase2-waveN-brief.md` at the end of wave N, with full context for wave N+1. Briefs target ~100% first-run test pass rates (Phase 1 retrospective evidence).

## Wave-specific brief expectations

For each wave transition (1→2, 2→3, 3→4, 4→5), code-puppy authors `docs/phases/phase2-waveN-brief.md` at end-of-wave-N before context clears. Each brief contains:

- Task list with bd IDs and file paths
- Full type definitions and function signatures for every public surface
- Specific test cases with inputs and expected outputs
- Known risks discovered during the prior wave
- Deviations from PLAN (justified)
- Exit checklist (cargo gate items, bd close commands, commit message template)

The Wave 1 brief (`docs/phases/phase2-wave1-brief.md`) is OPTIONAL — Wave 1 is small enough that this phase2.md doubles as its brief.

## Exit criteria

**PLAN line 1362 verbatim:**
> Hold a key in a CLI test → speak → see a transcript. CUDA path verified on RTX 2060.

**Operationalized:**
1. `cargo build --release` green with `whisper-rs/cuda` enabled (requires cmake + nvcc on PATH).
2. `cargo test --workspace` green (incl. ≥40 new tests across audio/vad/stt/prompt_builder).
3. `cargo clippy -- -D warnings` clean; `cargo fmt --check` clean.
4. `cargo run --bin stt_test -- tests/fixtures/audio/hello.wav` prints transcript + latency + `gpu_used: true`.
5. Logs show at least one line matching `CUDA` or `cuBLAS` during model load.
6. `cargo bench --bench stt_latency` reports < 1000 ms for a 10-second fixture on RTX 2060.
7. `git tag --list "phase-*"` includes `phase-2-complete`.
8. STATUS.md current phase = "Phase 3 (queued)".
9. AppError contains `Stt` + `Audio` variants; round-trip tests pass.

## Judges at phase exit

| Judge                          | Run? | Notes                                                  |
|--------------------------------|------|--------------------------------------------------------|
| `build-passes`                 | YES  | `cargo build --release` (with CUDA backend)            |
| `tests-pass`                   | YES  | `cargo test --workspace`                               |
| `lint-clean`                   | YES  | clippy + fmt                                           |
| `stt-correct` *(new)*          | YES  | CLI harness output matches fixture expected text within edit-distance tolerance |
| `cuda-verified` *(new)*        | YES  | greps logs for `CUDA`\|`cuBLAS` lines during model load |
| `perf-stt` *(new)*             | YES  | criterion JSON < 1000 ms for 10s fixture on RTX 2060   |
| `adr-recorded`                 | YES  | ADRs 0011–0014 present, Status=Accepted, schema match  |
| `plan-aligned`                 | YES  | deliverable checklist vs PLAN lines 1362–1376          |
| `status-updated`               | YES  | last-judge-run line present                            |
| `agents-md-present`            | passthrough |                                                 |
| `hook-config-valid`            | passthrough |                                                 |

Three NEW judge prompts to add (Wave 5 task): `stt-correct`, `cuda-verified`, `perf-stt`. Judge cards live in `docs/judges/phase-2/`.

## Risks (top 8)

1. **whisper-rs CUDA build fails without cmake/nvcc** → Dustin must install both before Wave 4; STATUS already flags. 5-attempt rule applies — if build keeps failing, escalate via STATUS, do not push to 10.
2. **cpal can't request 16 kHz mono on some default devices** → fallback path resamples via `rubato`. Decision deferred to Wave 2; flagged here.
3. **Silero ONNX model format drift** → pin a specific release in `scripts/model-manifest.json` SHA-256 column; ORT version pin (ADR 0012) covers runtime.
4. **Whisper hallucinates on silence** (per PLAN line 1752 — "non-negotiable") → VAD trim is the mitigation; Wave 3's `mixed` fixture test asserts hallucination absence.
5. **`ort` v2 API churn** → pin minor version; if API changes between waves, mode-lock in `Cargo.lock` (we already commit it).
6. **TTS fixture generator** — Helios may need to build a Windows TTS wrapper if `System.Speech.Synthesis` is the only option; Wave 2 task explicitly flags Helios delegation.
7. **CPU fallback path untested on dev machine** (Dustin has RTX 2060) → integration test gates `gpu_used=true`; CPU path covered by a `--force-cpu` CLI flag and a separate test with `#[cfg_attr(not(feature = "cuda-only-tests"), ignore)]`.
8. **Phase 1 LESSONS surprises that may recur:** `#[cfg(test)]` doesn't cross crate boundaries (already known), 4-min cold cargo check (whisper-rs CUDA build will be longer — 10+ min cold expected), `#![warn(missing_docs)]` will fight new modules (allow at module level per Phase 1 carry-forward).

## Out of scope (DEFER to later phases)

- Global hotkey + low-level keyboard hook → Phase 3
- Text injection (`SendInput` / clipboard paste) → Phase 3
- Cleanup LLM provider abstraction → Phase 4
- Recording UX (real recording window, audio meter) → Phase 5
- History viewer / data UI → Phase 6
- Session row writes to DB on real hotkey press → Phase 3 (Phase 2 writes only in tests / CLI harness)
- macOS/Linux audio + STT impls → Phase 9 (stubs only in Phase 2)

## Iteration plan

| Iteration | Scope                                                | Notes                                          |
|-----------|------------------------------------------------------|------------------------------------------------|
| 1         | Wave 1 (ADRs + deps + AppError + download + scaffolds) | No CUDA needed yet; can run on any dev machine. |
| 2         | Wave 2 (audio capture)                                | First wave requiring cpal; mic permission UX surfaces. |
| 3         | Wave 3 (VAD)                                          | First wave touching ort; ONNX Runtime DLL bundle test. |
| 4         | Wave 4 (STT + prompt builder)                         | **HARD BLOCKER if cmake/nvcc not installed.** Heaviest wave. |
| 5         | Wave 5 (CLI + bench + judges + seal)                  | Tag `phase-2-complete`. Buffer for any 5-attempt-rule escalation. |
