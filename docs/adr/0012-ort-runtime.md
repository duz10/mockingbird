# ADR-0012: ort runtime for ONNX models (Silero VAD)

- **Status:** Accepted
- **Date:** 2026-05-16
- **Deciders:** Dustin (project lead), code-puppy (implementor), planning-agent

## Context

Phase 2 needs voice-activity detection (VAD) on the input audio to
trim silence before passing to Whisper. Silero VAD is the de-facto
choice for client-side VAD: small (~1.8 MB), fast (~1 ms per 30 ms
frame on CPU), accurate. It ships as an ONNX model.

We need a Rust ONNX Runtime. The two viable options are:

- **`ort`** (formerly `onnxruntime-rs`) — actively maintained, wraps
  Microsoft's ONNX Runtime C library. Supports CPU and GPU
  (CUDA/DirectML).
- **`tract`** — pure-Rust ONNX runtime. No external DLLs but smaller
  op coverage; Silero loads but slower.

## Decision

Use `ort = "2"` with default features. The `download-binaries`
feature (default) bundles the ONNX Runtime DLLs into the build
artifact — no system install required. We pin the minor version in
`Cargo.toml` (`ort = "2"`) to avoid surprise upgrades during a wave;
the major-version lock matches `tracing`/`tokio` convention in this
project.

Silero is loaded from disk (`<models_dir>/silero_vad.onnx`), NOT
`include_bytes!`-embedded. Rationale: the binary stays slim
(~1.8 MB is small but adds up across multiple models), and the
SHA-256 verification in `download-models.ps1` covers tamper
detection regardless. The model path resolves via the same helper
as Whisper (ADR 0014).

## Consequences

- **Positive:** maintained crate, GPU acceleration possible later
  (Phase 9+ for DirectML on consumer GPUs), ONNX Runtime DLLs
  bundled means no MSI-installer dance for VAD.
- **Negative:** ~30 MB DLL footprint in the release artifact;
  `ort`'s API has churned across versions (we pin minor to mitigate).
- **Neutral:** if Silero ever ships an updated model with new ops
  that `ort` v2 doesn't support, we revisit — but PLAN expects
  stability through Phase 2.

## Alternatives considered

- **`tract`:** pure-Rust, no DLLs, but slower (~3× for Silero based
  on `tract` benchmarks). Rejected; the speed matters at the
  per-frame level.
- **`onnxruntime` (the older crate):** unmaintained since 2023.
  Rejected.
- **Custom CNN port:** not realistic for a 4-hour wave. Rejected.

## Cross-references

- PLAN line 1363 (Silero ONNX via `ort` crate, real model file)
- `docs/phases/phase2.md` Wave 3
- `scripts/model-manifest.json` (Silero entry, SHA-256 pinned)
- ADR 0014 (model storage path)
