# ADR-0011: whisper-rs CUDA build with runtime CPU fallback

- **Status:** Accepted
- **Date:** 2026-05-16
- **Deciders:** Dustin (project lead), code-puppy (implementor), planning-agent

## Context

Phase 2 needs Whisper inference fast enough to meet the PLAN line 1372
perf budget (< 1 s for 10 s of audio on RTX 2060). CUDA-accelerated
inference via `whisper.cpp`'s GPU backend is the only realistic path
to that budget on the target hardware.

`whisper-rs` (0.13) exposes a `cuda` feature flag that links against
the CUDA-enabled `whisper.cpp` build. Enabling it at build time
requires `cmake` + the CUDA Toolkit's `nvcc` on PATH.

At runtime, the same binary must keep working on machines without a
CUDA GPU (laptops, CI runners). `whisper-rs` supports per-context
`use_gpu = false` to force CPU execution; the same crate build can
serve both paths.

## Decision

**Build time:** `whisper-rs = { version = "0.13", features = ["cuda"] }`
on Windows. CUDA is mandatory at build time on dev/release machines.
`scripts/setup-dev.ps1` and `scripts/verify-environment.ps1` gate the
required toolchain (cmake + nvcc on PATH).

**Runtime:** the STT context first attempts `use_gpu = true`. If
`WhisperContext::new_with_params` returns a CUDA-backend error (model
load failure, no compatible GPU detected, OOM at init), we retry with
`use_gpu = false` and emit a `tracing::warn!(target: "stt", "CUDA init
failed, falling back to CPU: {err}")` line. Subsequent transcribe
calls on that context stay on CPU.

The CLI harness (`bin/stt_test.rs`) gets a `--force-cpu` flag so we
can exercise the CPU path on any dev machine for tests.

## Consequences

- **Positive:** matches the perf budget on the target hardware; same
  shipped binary works on CPU-only machines (slower); single feature
  flag keeps `Cargo.toml` simple.
- **Negative:** `cargo build` fails on any machine without cmake +
  nvcc on PATH. New contributors must run `setup-dev.ps1` before
  their first build. Cold compile of `whisper.cpp` is ~5–10 min the
  first time (acceptable; cached afterward).
- **Neutral:** CPU fallback path is the only way to test on
  GPU-absent CI; we accept a slower CI test run for that integration
  test specifically.

## Alternatives considered

- **CPU-only build:** misses perf budget by ~10×. Rejected.
- **Two binaries (cuda / cpu):** doubles release surface, complicates
  installer, doesn't help dev contributors who'd still need cmake for
  the CUDA build. Rejected.
- **`hound` + `candle` Whisper port:** less mature, no CUDA story on
  Windows at the time of this decision. Rejected.

## Cross-references

- PLAN line 1364 (whisper-rs CUDA build, GPU init logged, CPU
  fallback path)
- PLAN line 1372 (perf budget: < 1s for 10s audio on RTX 2060)
- `docs/phases/phase2.md` Wave 4
- `scripts/setup-dev.ps1`, `scripts/verify-environment.ps1`
- ADR 0014 (model storage path — affects where whisper finds the
  `.bin` model)
