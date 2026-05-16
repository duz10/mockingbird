# ADR-0013: cpal + ringbuf audio capture design

- **Status:** Accepted
- **Date:** 2026-05-16
- **Deciders:** Dustin (project lead), code-puppy (implementor), planning-agent

## Context

Phase 2 needs cross-platform audio capture from the system's default
input device. cpal (Cross-Platform Audio Library) is the standard
Rust crate for this: pure Rust + thin OS wrappers (WASAPI on
Windows, CoreAudio on macOS, ALSA on Linux).

The capture thread runs in a tight callback loop (~10 ms granularity)
and must hand audio off to the consumer thread (VAD + STT) without
blocking. A lock-free ring buffer is the standard pattern; we use
the `ringbuf` crate (SPSC, lock-free, well-tested).

Open questions: target PCM format, frame size, buffer size,
default-device-changed handling.

## Decision

**PCM format:** 16 kHz mono i16. Matches Whisper's training input
exactly; Silero VAD's preferred input is also 16 kHz; saves any
runtime resampling on the happy path.

**Frame size:** 30 ms (480 samples at 16 kHz). Aligns with Silero's
preferred window. Also a natural chunk for streaming UI feedback in
Phase 5.

**Ring buffer size:** SPSC, 1 MB capacity (≈ 30 s of audio at the
target format). Generous so a slow VAD/STT drain never drops frames
even during heavy GC pauses or initial CUDA warmup. Drop policy on
overflow: log a `tracing::warn!` and discard the oldest frame
(producer-priority).

**Sample-rate conversion:** if cpal's default device doesn't support
16 kHz mono i16 directly (most modern devices do, but USB headsets
sometimes pin to 48 kHz), we resample via `rubato`. **DEFERRED to
Wave 2 implementation** — if Windows WASAPI on the dev machine
honors a 16 kHz `SampleFormat::I16` request, we skip `rubato`
entirely. Wave 2 brief makes the call after probing the actual
device.

**Default-device-changed handler:** Windows-specific via
`MMNotificationClient` (subscribed through cpal's `Host` event
stream). On change: stop the current capture stream, log
`tracing::info!(?old_device, ?new_device, "default input device
changed; restarting capture")`, start a new stream against the new
default. macOS / Linux impls stay `todo!()` per the cross-platform
rule.

## Consequences

- **Positive:** standard pattern; lock-free; zero-copy producer side;
  format matches downstream consumers, eliminating resampling on the
  happy path; generous buffer survives transient stalls.
- **Negative:** 1 MB RAM cost is non-trivial on memory-constrained
  systems (acceptable for a desktop app). USB headset users may hit
  the resample path more often; we eat the perf cost.
- **Neutral:** macOS/Linux capture is `todo!()` for now; Phase 9
  finishes them. The trait shape locks the contract.

## Alternatives considered

- **`portaudio-rs`:** older crate, fewer maintainers, requires C
  library install. Rejected.
- **Direct WASAPI via the `windows` crate:** more control, more code,
  no cross-platform abstraction. Rejected for v1; revisit if cpal
  ever blocks us.
- **Larger buffer (5 s):** doesn't help; the consumer is fast enough.
- **Smaller buffer (1 s):** risks overflow during CUDA init. Rejected.
- **Blocking channels (`crossbeam`):** simpler API but the audio
  callback can't afford to block. Rejected.

## Cross-references

- PLAN line 1362 (cpal, 16 kHz mono, ring buffer, default-device-
  changed handler)
- PLAN line 185 (cross-platform abstraction rule)
- `docs/phases/phase2.md` Wave 2
- `Cargo.toml` workspace deps (cpal = "0.15", ringbuf = "0.4")
- ADR 0012 (ort — adjacent decision; both rely on file-on-disk
  models)
