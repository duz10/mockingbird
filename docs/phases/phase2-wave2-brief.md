# Phase 2 Wave 2 — Implementation brief (audio capture)

> Wave 2 fills the `audio/capture.rs` scaffold with the real cpal +
> ringbuf body per ADR 0013. Wave 1 ships the trait shape and the
> `todo!()` body; Wave 2 makes microphone input work.
>
> Brief pattern is documented in Phase 1 LESSONS: 100% first-run test
> pass rates Waves 2-4. Same pattern here. **Treat as binding.**

## Tasks in scope (5 bd tasks from Wave 2)

| bd id    | Deliverable                                       | Approx. lines |
|----------|---------------------------------------------------|---------------|
| `mb-560` | `AudioCapture` trait refinement (already shipped Wave 1; verify shape) | 0 net (verify only) |
| `mb-mil` | cpal Windows capture impl + ring buffer body      | ~250 |
| `mb-nws` | Default-device-changed handler                    | ~80 |
| `mb-mqz` | TTS audio fixture generator (delegate to Helios if absent) | ~150 (script) |
| `mb-0rq` | Audio capture unit + integration tests            | ~200 |

Plus inevitable extras:
- `Cargo.toml`: maybe add `rubato` if device probing in Wave 2 shows
  Windows WASAPI won't pin to 16 kHz mono i16 (50/50 odds; budget it).
- `tests/audio_capture.rs`: new integration test file if the unit
  tests' scope can't cover the device-change scenario.

**Total budget:** ~700 lines net new + the fixtures (binary WAVs, not
counted).

## Cross-cutting decisions (binding from ADR 0013)

### 1. Target format is 16 kHz mono i16

The capture pipeline tries to request this format directly from cpal's
default input device. If the device pins to its native rate (commonly
48 kHz on USB headsets, 44.1 kHz on older mics), Wave 2 introduces
`rubato` for SINC resampling at the producer side. **Probe first, add
the dep only if needed.**

### 2. Ring buffer is `ringbuf::HeapRb<i16>` with 480_000-sample capacity

That's ~30 seconds at 16 kHz × 2 bytes/sample = ~960 KB. Just under
1 MB per ADR. SPSC — producer is the cpal callback thread, consumer
is whoever calls `CpalCapture::drain()`. Lock-free. The producer
writes whole frames (480 samples per 30 ms) atomically.

### 3. Overflow policy: drop-oldest with warn log

When the consumer falls behind enough that the ring is full, the
producer side advances the read cursor (discarding the oldest frame)
and emits `tracing::warn!(target: "audio", "ring buffer overflow,
dropped {n} samples")`. Phase 5 may add a flow-control signal but
Phase 2 picks raw availability.

### 4. Default-device-change handler

Use cpal's `Host::devices()` polling at 1 Hz (cheap; cpal doesn't
expose `MMNotificationClient` directly — that was an aspirational
note in ADR 0013). When the default input device's `name()` changes:
- Stop the current stream
- Build a new stream from the new default
- Log `tracing::info!(?old_device, ?new_device, "default input device
  changed; restarting capture")`
- Re-attach the same ring buffer producer (consumer is undisturbed)

If polling proves too coarse in production, Phase 5 can add a real
`IMMNotificationClient` via the `windows` crate. **Don't reach for
that in Wave 2** — keep the cpal abstraction.

### 5. `start()` is idempotent; `stop()` is idempotent

Double-start does nothing (returns Ok). Double-stop does nothing.
Tests pin both.

### 6. The cpal callback can't return errors — log + soldier on

cpal's data callback signature is `FnMut(&[T], &cpal::InputCallbackInfo)`
with no Result. On any internal failure (resample fail, ring overflow
counted), log via `tracing::warn!` and continue. The CALLER thread
detects systemic failure via empty drains, not callback returns.

### 7. `Send + 'static` everywhere callback-related

cpal requires data callbacks to be `Send + 'static`. The ring
buffer's producer half is `Send`. We move it into the callback via
`move ||`. Anything else captured must also be `Send` — clone or
`Arc<Mutex<_>>` if needed.

---

## Module 1: `src-tauri/src/audio/capture.rs` — Windows impl (~250 lines)

### Concrete shape

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Host, SampleFormat, Stream, StreamConfig};
use ringbuf::{traits::{Consumer, Producer, Split}, HeapRb};

use crate::error::{AppError, AppResult};
use super::AudioCapture;

const TARGET_SAMPLE_RATE: u32 = 16_000;
const TARGET_CHANNELS: u16 = 1;
const RING_CAPACITY: usize = 480_000; // ~30 s at 16 kHz

type SampleProducer = ringbuf::HeapProd<i16>;
type SampleConsumer = ringbuf::HeapCons<i16>;

#[cfg(target_os = "windows")]
pub struct CpalCapture {
    host: Host,
    /// Currently-active stream. None when stopped or pre-start.
    stream: Option<Stream>,
    /// Consumer half of the ring; producer half lives inside the cpal
    /// callback closure.
    consumer: SampleConsumer,
    /// Producer half held alongside the consumer so we can re-attach
    /// it to a new stream on device-change. Wrapped in Option so we
    /// can move it into the callback closure exactly once per build.
    producer_slot: Option<SampleProducer>,
    /// Set to false to signal the device-watcher thread (if any) to exit.
    watcher_alive: Arc<AtomicBool>,
    /// Name of the device we built `stream` against. Used to detect
    /// default-device changes.
    current_device_name: Option<String>,
}

#[cfg(target_os = "windows")]
impl CpalCapture {
    pub fn new() -> AppResult<Self> {
        let host = cpal::default_host();
        let rb = HeapRb::<i16>::new(RING_CAPACITY);
        let (producer, consumer) = rb.split();
        Ok(Self {
            host,
            stream: None,
            consumer,
            producer_slot: Some(producer),
            watcher_alive: Arc::new(AtomicBool::new(false)),
            current_device_name: None,
        })
    }

    /// Build a cpal Stream that pumps samples into `producer`. Returns
    /// the stream + the device's display name for change detection.
    fn build_stream(
        &self,
        device: &Device,
        mut producer: SampleProducer,
    ) -> AppResult<Stream> {
        let supported = device
            .default_input_config()
            .map_err(|e| AppError::Audio(format!("default_input_config: {e}")))?;

        // Wave 2 simplification: require the device's default config
        // to be a usable i16 stream. If it isn't, Wave 2 returns an
        // explicit error pointing at the rubato follow-up; we eat
        // that scope ONLY if the dev machine actually hits it.
        let sample_format = supported.sample_format();
        let config = StreamConfig {
            channels: supported.channels(),
            sample_rate: supported.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        let device_rate = config.sample_rate.0;
        let device_channels = config.channels;
        let needs_resample = device_rate != TARGET_SAMPLE_RATE
            || device_channels != TARGET_CHANNELS;
        if needs_resample {
            return Err(AppError::Audio(format!(
                "device provides {device_rate} Hz / {device_channels} ch; \
                 Wave 2 expects 16 kHz mono. Add rubato resampler — see ADR 0013."
            )));
        }
        if sample_format != SampleFormat::I16 {
            return Err(AppError::Audio(format!(
                "device sample format is {sample_format:?}; Wave 2 expects i16. \
                 Add conversion — see ADR 0013."
            )));
        }

        let err_cb = |e| tracing::warn!(target: "audio", error = %e, "cpal stream error");

        let stream = device
            .build_input_stream(
                &config,
                move |data: &[i16], _info: &cpal::InputCallbackInfo| {
                    let pushed = producer.push_slice(data);
                    if pushed < data.len() {
                        let dropped = data.len() - pushed;
                        tracing::warn!(target: "audio", dropped, "ring overflow");
                    }
                },
                err_cb,
                None,
            )
            .map_err(|e| AppError::Audio(format!("build_input_stream: {e}")))?;

        Ok(stream)
    }
}

#[cfg(target_os = "windows")]
impl AudioCapture for CpalCapture {
    fn start(&mut self) -> AppResult<()> {
        if self.stream.is_some() {
            return Ok(()); // idempotent
        }
        let device = self
            .host
            .default_input_device()
            .ok_or_else(|| AppError::Audio("no default input device".into()))?;
        let device_name = device.name().ok();

        // Move the producer into the stream callback. If start() runs
        // again after stop(), `producer_slot` is empty — we re-split
        // the consumer's drain count is unaffected.
        let producer = self
            .producer_slot
            .take()
            .ok_or_else(|| AppError::Audio("producer already moved (BUG)".into()))?;
        let stream = self.build_stream(&device, producer)?;
        stream
            .play()
            .map_err(|e| AppError::Audio(format!("stream play: {e}")))?;

        self.stream = Some(stream);
        self.current_device_name = device_name.clone();
        tracing::info!(target: "audio", ?device_name, "capture started");
        Ok(())
    }

    fn stop(&mut self) -> AppResult<()> {
        if let Some(stream) = self.stream.take() {
            drop(stream); // cpal stops + joins its thread on drop
            tracing::info!(target: "audio", "capture stopped");
        }
        // NOTE: producer is gone (moved into the dropped stream). For
        // restart support, Wave 2 must rebuild the ring AND re-split.
        // Simplest path: rebuild the entire CpalCapture on next start
        // if user wants restart. Document this as a known limitation;
        // Phase 5 may add a proper restart by holding the producer
        // outside the stream closure (Arc<Mutex<Option<SampleProducer>>>).
        Ok(())
    }

    fn drain(&mut self, buf: &mut Vec<i16>) -> AppResult<usize> {
        let before = buf.len();
        // ringbuf 0.4's HeapCons exposes pop_slice / pop_iter. We use
        // pop_iter to grow `buf` without preallocating capacity.
        for sample in self.consumer.pop_iter() {
            buf.push(sample);
        }
        Ok(buf.len() - before)
    }

    fn sample_rate(&self) -> u32 { TARGET_SAMPLE_RATE }
    fn channels(&self) -> u16 { TARGET_CHANNELS }
}
```

### Known limitation flagged in code

`stop()` then `start()` doesn't currently restart capture — the
producer is consumed by the prior stream. Phase 2 Wave 2 documents
this; Phase 5 fixes it (recording lifecycle owns restart semantics).
Tests assert that calling `start()` twice without an intervening
stop is a no-op, and that `stop()` then `start()` errors cleanly with
a known message — NOT a panic.

### Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Constructor doesn't touch the device — exercises cpal Host
    /// init only. Should always pass even on machines without a mic.
    #[test]
    fn new_does_not_open_a_device() {
        let _c = CpalCapture::new().unwrap();
    }

    #[test]
    fn sample_rate_and_channels_match_target() {
        let c = CpalCapture::new().unwrap();
        assert_eq!(c.sample_rate(), 16_000);
        assert_eq!(c.channels(), 1);
    }

    #[test]
    fn double_start_is_idempotent() {
        let mut c = CpalCapture::new().unwrap();
        if c.start().is_err() {
            eprintln!("skipping: no default input device on test runner");
            return;
        }
        c.start().unwrap(); // second call should Ok
        c.stop().unwrap();
    }

    /// CI runners may lack an input device. We accept either Ok (real
    /// hardware) or AppError::Audio (no device / unsupported format) —
    /// what we DON'T accept is a panic.
    #[test]
    fn start_does_not_panic_when_device_unavailable() {
        let mut c = CpalCapture::new().unwrap();
        let _ = c.start();
    }

    #[test]
    fn drain_on_empty_ring_returns_zero() {
        let mut c = CpalCapture::new().unwrap();
        let mut buf = Vec::new();
        assert_eq!(c.drain(&mut buf).unwrap(), 0);
        assert!(buf.is_empty());
    }
}
```

---

## Module 2: Default-device-changed handler (~80 lines)

Add a method `start_with_device_watcher()` (or extend `start()`) that
spawns a small thread polling `host.default_input_device().name()` at
1 Hz. On change:

```rust
fn spawn_device_watcher(
    host: Host,
    initial_name: Option<String>,
    on_change: impl Fn(String) + Send + 'static,
    alive: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        let mut current = initial_name;
        while alive.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let next = host.default_input_device().and_then(|d| d.name().ok());
            if next != current {
                tracing::info!(target: "audio", ?current, ?next, "default device changed");
                if let Some(name) = &next {
                    on_change(name.clone());
                }
                current = next;
            }
        }
    });
}
```

The `on_change` callback issues a rebuild. For Wave 2, accept the
limitation that rebuild doesn't preserve in-flight audio (clean cut).

### Risk flag

This polling approach is the brief's deviation from ADR 0013's
mention of `MMNotificationClient`. cpal does not expose that COM API
directly. **Document the deviation in the commit message and add a
LESSONS entry** so future contributors know why we polled.

### Tests

```rust
// Device-change is hard to unit-test without injecting a fake Host.
// For Wave 2, exercise spawn_device_watcher with a synthetic
// `on_change` closure and assert it fires when we manually change
// the "current_name" tracking. Use a Condvar + Mutex<Option<String>>
// to deterministically signal the callback fired.
```

---

## Module 3: TTS audio fixture generator (~150 lines, PowerShell)

Path: `scripts/generate-audio-fixtures.ps1`

Generate `tests/fixtures/audio/*.wav` for test inputs. Windows ships
`System.Speech.Synthesis` which writes WAV files directly:

```powershell
Add-Type -AssemblyName System.Speech
$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
$synth.SetOutputToWaveFile($outPath)
$synth.Speak("hello world")
$synth.Dispose()
```

Then convert to 16 kHz mono i16 via the `hound` CLI (or a tiny
PowerShell wrapper). Fixtures to generate:

- `hello.wav` — "hello world"
- `quick_brown_fox.wav` — pangram for a longer test
- `silent.wav` — 3 seconds of silence (write zeros via hound, not TTS)
- `mixed.wav` — 1 s silence + "test" + 1 s silence (for VAD)

**If `System.Speech.Synthesis` proves unreliable** (no voices
installed, locale issues): delegate to Helios to build a fixture
generator. PLAN line 1366 explicitly anticipates this.

### Tests

Fixtures are checked into git LFS or `tests/fixtures/` directly
(decide based on size — 3 s of 16 kHz mono i16 is ~96 KB, totally
fine to commit raw). Integration tests in `tests/audio_capture.rs`
load and assert structural properties (sample rate, channel count,
length); actual transcription happens in Wave 4.

---

## Module 4: Audio capture integration tests (~200 lines)

`tests/audio_capture.rs` (new cross-crate integration test).

```rust
//! Integration tests for audio capture. Exercises real cpal handles
//! when a device is available; skips gracefully on CI without mics.

use mockingbird_lib::audio::{make_default_capture, AudioCapture};
use std::time::Duration;

#[test]
fn factory_returns_a_capture_on_windows() {
    #[cfg(target_os = "windows")]
    {
        let _ = make_default_capture().expect("Windows should construct");
    }
    #[cfg(not(target_os = "windows"))]
    {
        assert!(make_default_capture().is_err());
    }
}

#[test]
fn start_then_drain_collects_samples_when_device_present() {
    let mut cap = match make_default_capture() {
        Ok(c) => c,
        Err(_) => return, // no device; CI-friendly
    };
    if cap.start().is_err() {
        return; // device exists but format not supported; CI-friendly
    }
    // Give cpal ~200 ms to push some frames.
    std::thread::sleep(Duration::from_millis(200));
    let mut buf = Vec::new();
    let n = cap.drain(&mut buf).unwrap();
    cap.stop().unwrap();
    // We can't assert n > 0 reliably (silent mic in CI) but we CAN
    // assert no panic + drain returned a sane count <= 200 ms worth.
    let max_samples_200ms = 16_000 / 5 + 100; // 3300 samples
    assert!(n <= max_samples_200ms, "drained too many samples: {n}");
}
```

Plus 1-2 fixture-WAV roundtrip tests using `hound` to confirm the
fixture generator's output parses correctly. These are independent of
cpal — they just exercise the WAV format expectations the rest of
Phase 2 assumes.

---

## Wave 2 exit checklist

- [ ] `cargo check --workspace` green
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] `cargo test --workspace` green (target +~10 tests over Wave-1 = ~114 total)
- [ ] `cargo fmt --check` clean
- [ ] `bd close mb-560 mb-mil mb-nws mb-mqz mb-0rq`
- [ ] STATUS.md: Wave 2 ✅, Waves 3-5 queued
- [ ] LESSONS.md: any non-obvious finding (resampler scope, device-watcher polling deviation)
- [ ] Commit: `feat(phase-2-wave-2): cpal audio capture + ring buffer + device watcher + fixtures`
- [ ] End of iteration: write `docs/phases/phase2-wave3-brief.md` for VAD
- [ ] **DO NOT** add `whisper-rs` to Cargo.toml — Wave 4

## Known risks

1. **Device might not pin to 16 kHz mono i16.** ~50% odds on modern
   USB headsets (often 48 kHz). The Wave-2 code errors out explicitly
   if so. If you hit this on the dev machine, scope-creep ONE thing:
   add `rubato` to Cargo.toml, write a `Resampler` helper, route the
   cpal callback through it. Document the deviation in the commit.
2. **cpal's `default_input_device()` may return None on Windows under
   audio service issues.** The test path handles this; production
   code should surface a useful error message.
3. **`producer` move-into-closure means restart is broken.** Flagged
   above. Tests assert behavior, Phase 5 fixes properly.
4. **`System.Speech.Synthesis` may produce no audio on Server SKUs.**
   Fallback: a tiny C# `Microsoft.CognitiveServices.Speech`-free
   generator, OR delegate to Helios for a Python TTS wrapper.
5. **CI without a mic** — every test that calls `start()` must accept
   a graceful Err and not fail the suite. Pattern: `if c.start().is_err() { return; }`.
6. **5-attempt rule** on cpal API friction. cpal 0.15 docs are sparse;
   if a build call signature doesn't match this brief after 5 tries,
   escalate via LESSONS.md and ask Dustin.

## Out of scope for Wave 2

- VAD (Wave 3)
- STT (Wave 4)
- Real `MMNotificationClient` integration (Phase 5)
- Audio metering for UI (Phase 5)
- Restart-after-stop bug fix (Phase 5)
- Resampler implementation IF device pins to 16 kHz mono (defer
  rubato dep until needed)

## Wave-3 brief preview

End of Wave 2, write `docs/phases/phase2-wave3-brief.md`. Wave 3 adds
`ort = "2"` and fills `audio/vad.rs::SileroVad`. Estimated lines: ~250
across the impl + a `vad_trim` helper that consumes captured frames
and produces speech-only PCM. The brief should specify the exact ort
API calls used (the v2 API is non-trivial) and the model SHA-256 once
`download-models.ps1` runs.
