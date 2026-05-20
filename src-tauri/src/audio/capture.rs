#![allow(missing_docs)] // Brief documents the API; submodule fields are internal.

//! cpal-backed audio capture on Windows.
//!
//! See ADR 0013 (16 kHz mono i16, 30 ms frames, 1 MB SPSC ringbuf)
//! and `docs/phases/phase2-wave2-brief.md` for the design.
//!
//! Layout:
//!   - [`CpalCapture`] is the live capture handle (cpal stream + ring
//!     consumer + watcher).
//!   - The cpal callback owns the ring producer; we ferry it via an
//!     `Option<SampleProducer>` slot so `start()` can move it in.
//!   - Device-change is polled at 1 Hz by a watcher thread; the
//!     watcher merely logs and flips an `Arc<AtomicBool>` flag the
//!     consumer can read via [`CpalCapture::device_changed`]. Wave 2
//!     does not auto-restart on change (clean restart support is a
//!     Phase 5 concern).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

#[cfg(target_os = "windows")]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(target_os = "windows")]
use cpal::{Device, Host, SampleFormat, Stream, StreamConfig};
#[cfg(target_os = "windows")]
use ringbuf::traits::{Consumer, Split};
#[cfg(target_os = "windows")]
use ringbuf::HeapRb;

#[cfg(target_os = "windows")]
use crate::error::AppError;
use crate::error::AppResult;

/// Target audio format. Locked in by ADR 0013.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;
/// Target channel count. Locked in by ADR 0013.
pub const TARGET_CHANNELS: u16 = 1;
/// Ring buffer capacity. Sized to match the hotkey state machine's
/// `max_session = 300 s` (see `hotkey/state.rs`) so the buffer can
/// hold an entire max-length recording without overwriting samples.
///
/// Math: 300 s × 16 000 Hz × 2 bytes/sample (i16 mono) ≈ 9.6 MB.
/// Cheap RAM; eliminates the silent-truncation footgun where
/// recordings > 30 s would lose audio off the front (or back,
/// depending on which end the producer hits first).
///
/// Pre-Wave-2 value was 480_000 (= 30 s), which matched Whisper's
/// single-window inference budget but NOT the hotkey ceiling. 2026-
/// 05-17 user smoketest in formal mode (1 m 27 s recording) produced
/// a raw transcript covering only the first ~28 s; the rest fell off
/// the buffer's edge before STT could consume them. See bead
/// `audio-streaming-chunked-whisper` for the proper long-term fix
/// (streaming Whisper inference with overlapping chunks); this
/// bump is the interim that keeps the user productive in the
/// meantime.
pub const RING_CAPACITY: usize = 4_800_000;
/// Device-watcher poll interval.
pub const DEVICE_POLL_INTERVAL: Duration = Duration::from_secs(1);

#[cfg(target_os = "windows")]
pub(super) type SampleProducer = <HeapRb<i16> as Split>::Prod;
#[cfg(target_os = "windows")]
type SampleConsumer = <HeapRb<i16> as Split>::Cons;

/// Which endpoint a [`CpalCapture`] grabs at `start()`.
///
/// Phase 2 (dictation) only ever needs `Input`. Phase MC (ADR 0031)
/// adds `Loopback`, which targets the default RENDER endpoint and
/// relies on cpal 0.15's WASAPI backend transparently setting
/// `AUDCLNT_STREAMFLAGS_LOOPBACK` when `build_input_stream` is
/// invoked on a device with `data_flow() == eRender`.
#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeviceSource {
    Input,
    Loopback,
}

#[cfg(target_os = "windows")]
impl DeviceSource {
    fn resolve(self, host: &Host) -> Option<Device> {
        match self {
            DeviceSource::Input => host.default_input_device(),
            DeviceSource::Loopback => host.default_output_device(),
        }
    }

    fn label(self) -> &'static str {
        match self {
            DeviceSource::Input => "input",
            DeviceSource::Loopback => "loopback",
        }
    }
}

#[cfg(target_os = "windows")]
pub struct CpalCapture {
    host: Host,
    /// Which endpoint to grab on `start()`. Set at construction; never
    /// mutated after.
    source: DeviceSource,
    /// Currently-active stream. None when stopped or pre-start.
    stream: Option<Stream>,
    /// Consumer half of the *current* ring.
    ///
    /// Rebuilt on every `start()` (along with a fresh producer that
    /// moves into the cpal callback). The old consumer is dropped at
    /// reassign, which also drops any stale samples from the prior
    /// session — no need to drain manually.
    consumer: SampleConsumer,
    /// Name of the device the active stream is built against. None
    /// before first `start()`.
    current_device_name: Option<String>,
    /// Watcher thread handle (Some when watcher is running).
    watcher_handle: Option<JoinHandle<()>>,
    /// Set to false to ask the watcher to exit.
    watcher_alive: Arc<AtomicBool>,
    /// Set by the watcher when it detects the default input device
    /// changed. Consumer can poll via [`device_changed`] +
    /// [`take_device_changed`].
    device_changed: Arc<AtomicBool>,
}

#[cfg(target_os = "windows")]
impl CpalCapture {
    pub fn new() -> AppResult<Self> {
        Self::with_source(DeviceSource::Input)
    }

    /// Phase MC (ADR 0031). Construct a capture handle that, on
    /// `start()`, opens the default RENDER endpoint in loopback mode.
    ///
    /// Implementation note: cpal 0.15's WASAPI backend transparently
    /// enables loopback recording when `build_input_stream` is invoked
    /// on a device whose `data_flow() == eRender`. No explicit flag
    /// plumbing is required at the cpal API level. See ADR 0031 for
    /// the cpal source-line evidence.
    pub fn new_loopback() -> AppResult<Self> {
        Self::with_source(DeviceSource::Loopback)
    }

    fn with_source(source: DeviceSource) -> AppResult<Self> {
        let host = cpal::default_host();
        // Pre-start consumer: tied to an orphan ring that has no
        // producer. drain() on this returns 0 cleanly. Replaced on
        // first start().
        let rb = HeapRb::<i16>::new(RING_CAPACITY);
        let (_orphan_producer, consumer) = rb.split();
        Ok(Self {
            host,
            source,
            stream: None,
            consumer,
            current_device_name: None,
            watcher_handle: None,
            watcher_alive: Arc::new(AtomicBool::new(false)),
            device_changed: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Returns true if the device watcher has observed a default-device
    /// change since the last call to [`take_device_changed`].
    pub fn device_changed(&self) -> bool {
        self.device_changed.load(Ordering::Relaxed)
    }

    /// Clear the device-changed flag and return whether it was set.
    pub fn take_device_changed(&self) -> bool {
        self.device_changed.swap(false, Ordering::Relaxed)
    }

    /// Build the cpal stream for `device`, moving `producer` into the
    /// data callback. Returns the live stream.
    ///
    /// Accepts ANY device-native config and resamples / downmixes /
    /// converts on the fly via [`super::resampler::AudioPipeline`].
    /// The downstream consumer always sees `TARGET_SAMPLE_RATE` mono
    /// `i16`, regardless of what the device exposed.
    fn build_stream(device: &Device, producer: SampleProducer) -> AppResult<Stream> {
        let supported = device
            .default_input_config()
            .map_err(|e| AppError::Audio(format!("default_input_config: {e}")))?;

        let sample_format = supported.sample_format();
        let config = StreamConfig {
            channels: supported.channels(),
            sample_rate: supported.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        let device_rate = config.sample_rate.0;
        let device_channels = config.channels;

        let pipeline = super::resampler::AudioPipeline::new(device_rate, device_channels)?;

        let err_cb = |e| tracing::warn!(target: "audio", error = %e, "cpal stream error");

        // Branch on cpal sample format. The callback closure captures
        // both the pipeline and the producer; cpal type-checks `T` by
        // the closure signature, so each arm needs its own typed call.
        let stream = match sample_format {
            SampleFormat::I16 => {
                Self::build_i16_stream(device, &config, pipeline, producer, err_cb)?
            }
            SampleFormat::F32 => {
                Self::build_f32_stream(device, &config, pipeline, producer, err_cb)?
            }
            other => {
                return Err(AppError::Audio(format!(
                    "unsupported cpal sample format {other:?}; supported: i16, f32"
                )));
            }
        };

        Ok(stream)
    }

    fn build_i16_stream(
        device: &Device,
        config: &StreamConfig,
        mut pipeline: super::resampler::AudioPipeline,
        mut producer: SampleProducer,
        err_cb: impl FnMut(cpal::StreamError) + Send + 'static,
    ) -> AppResult<Stream> {
        device
            .build_input_stream(
                config,
                move |data: &[i16], _info: &cpal::InputCallbackInfo| {
                    pipeline.process_i16(data, &mut producer);
                },
                err_cb,
                None,
            )
            .map_err(|e| AppError::Audio(format!("build_input_stream<i16>: {e}")))
    }

    fn build_f32_stream(
        device: &Device,
        config: &StreamConfig,
        mut pipeline: super::resampler::AudioPipeline,
        mut producer: SampleProducer,
        err_cb: impl FnMut(cpal::StreamError) + Send + 'static,
    ) -> AppResult<Stream> {
        device
            .build_input_stream(
                config,
                move |data: &[f32], _info: &cpal::InputCallbackInfo| {
                    pipeline.process_f32(data, &mut producer);
                },
                err_cb,
                None,
            )
            .map_err(|e| AppError::Audio(format!("build_input_stream<f32>: {e}")))
    }

    /// Spawn the device-watcher thread. Runs until `watcher_alive`
    /// flips false (which `stop()` does).
    ///
    /// `cpal::Host` isn't `Clone`, so the watcher thread calls
    /// `cpal::default_host()` itself — Host is effectively a
    /// per-platform singleton (cheap to re-resolve).
    fn spawn_watcher(&self, initial_name: Option<String>) -> JoinHandle<()> {
        let alive = self.watcher_alive.clone();
        let changed = self.device_changed.clone();
        let source = self.source;
        std::thread::spawn(move || {
            let host = cpal::default_host();
            let mut current = initial_name;
            while alive.load(Ordering::Relaxed) {
                std::thread::sleep(DEVICE_POLL_INTERVAL);
                if !alive.load(Ordering::Relaxed) {
                    break;
                }
                let next: Option<String> =
                    source.resolve(&host).and_then(|d: Device| d.name().ok());
                if next != current {
                    tracing::info!(
                        target: "audio",
                        kind = source.label(),
                        old = ?current,
                        new = ?next,
                        "default device changed"
                    );
                    changed.store(true, Ordering::Relaxed);
                    current = next;
                }
            }
        })
    }
}

#[cfg(target_os = "windows")]
impl super::AudioCapture for CpalCapture {
    fn start(&mut self) -> AppResult<()> {
        if self.stream.is_some() {
            return Ok(());
        }
        let device = self
            .source
            .resolve(&self.host)
            .ok_or_else(|| AppError::Audio(format!("no default {} device", self.source.label())))?;
        let device_name = device.name().ok();

        // Fresh ring per session — supports restart-after-stop, which
        // Phase 3 needs for hold-after-hold dictation. The previous
        // consumer (and any stale samples) is dropped on the next
        // line; the new producer is moved into the cpal callback by
        // `build_stream`.
        let rb = HeapRb::<i16>::new(RING_CAPACITY);
        let (producer, consumer) = rb.split();
        self.consumer = consumer;

        let stream = Self::build_stream(&device, producer)?;
        stream
            .play()
            .map_err(|e| AppError::Audio(format!("stream play: {e}")))?;

        self.current_device_name = device_name.clone();
        self.stream = Some(stream);

        self.watcher_alive.store(true, Ordering::Relaxed);
        self.device_changed.store(false, Ordering::Relaxed);
        self.watcher_handle = Some(self.spawn_watcher(device_name.clone()));

        tracing::info!(
            target: "audio",
            kind = self.source.label(),
            ?device_name,
            "capture started"
        );
        Ok(())
    }

    fn stop(&mut self) -> AppResult<()> {
        // Signal watcher to exit first so it doesn't observe a torn-down
        // stream during shutdown.
        self.watcher_alive.store(false, Ordering::Relaxed);
        if let Some(h) = self.watcher_handle.take() {
            // Best-effort join; ignore poison.
            let _ = h.join();
        }

        if let Some(stream) = self.stream.take() {
            drop(stream);
            tracing::info!(target: "audio", "capture stopped");
        }
        Ok(())
    }

    fn drain(&mut self, buf: &mut Vec<i16>) -> AppResult<usize> {
        let before = buf.len();
        // ringbuf 0.4 `pop_iter` returns an iterator over consumed samples.
        for sample in self.consumer.pop_iter() {
            buf.push(sample);
        }
        Ok(buf.len() - before)
    }

    fn sample_rate(&self) -> u32 {
        TARGET_SAMPLE_RATE
    }
    fn channels(&self) -> u16 {
        TARGET_CHANNELS
    }
}

#[cfg(target_os = "windows")]
impl Drop for CpalCapture {
    fn drop(&mut self) {
        // Ensure the watcher thread exits even if stop() wasn't called.
        self.watcher_alive.store(false, Ordering::Relaxed);
        if let Some(h) = self.watcher_handle.take() {
            let _ = h.join();
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use crate::audio::AudioCapture;

    /// Constructor must succeed without touching any device.
    #[test]
    fn new_does_not_open_a_device() {
        let _c = CpalCapture::new().unwrap();
    }

    #[test]
    fn sample_rate_and_channels_match_target() {
        let c = CpalCapture::new().unwrap();
        assert_eq!(c.sample_rate(), TARGET_SAMPLE_RATE);
        assert_eq!(c.channels(), TARGET_CHANNELS);
    }

    #[test]
    fn drain_on_empty_ring_returns_zero() {
        let mut c = CpalCapture::new().unwrap();
        let mut buf = Vec::new();
        assert_eq!(c.drain(&mut buf).unwrap(), 0);
        assert!(buf.is_empty());
    }

    /// CI runners may lack a default input device. We accept Ok
    /// (real hardware) OR AppError::Audio (no device / format
    /// mismatch). We do NOT accept a panic.
    #[test]
    fn start_does_not_panic_when_device_absent_or_unsupported() {
        let mut c = CpalCapture::new().unwrap();
        let _ = c.start();
        let _ = c.stop();
    }

    /// When `start()` succeeds, calling it again must be a no-op
    /// (not error). Skipped gracefully on CI without a mic.
    #[test]
    fn double_start_is_idempotent_when_device_present() {
        let mut c = CpalCapture::new().unwrap();
        if c.start().is_err() {
            eprintln!("skipping: no default input device on test runner");
            return;
        }
        // Second call must Ok without doing anything.
        c.start().unwrap();
        c.stop().unwrap();
    }

    /// Wave 4.8: restart after stop must succeed cleanly. Phase 3
    /// dictation involves many hold-release cycles per session;
    /// every hold calls start() and every release calls stop(). The
    /// previous Phase-2 behavior (error on second start) would break
    /// the second-ever dictation in any process lifetime.
    #[test]
    fn restart_after_stop_succeeds() {
        let mut c = CpalCapture::new().unwrap();
        if c.start().is_err() {
            eprintln!("skipping: no default input device on test runner");
            return;
        }
        c.stop().unwrap();
        // Second cycle: must succeed.
        c.start().expect("restart after stop must succeed");
        c.stop().unwrap();
        // Third cycle for good measure — proves it's truly stateless.
        c.start().expect("third start must also succeed");
        c.stop().unwrap();
    }

    /// Drain before any start() returns 0 samples cleanly (no panic
    /// from an unbacked consumer).
    #[test]
    fn drain_before_start_returns_zero() {
        use crate::audio::AudioCapture;
        let mut c = CpalCapture::new().unwrap();
        let mut buf = Vec::new();
        let n = c.drain(&mut buf).unwrap();
        assert_eq!(n, 0);
        assert!(buf.is_empty());
    }

    /// After stop(), any samples produced by the now-defunct stream
    /// must not appear in a subsequent start() / drain() cycle. The
    /// ring-rebuild-on-start contract guarantees a clean slate.
    #[test]
    fn drain_after_restart_does_not_leak_prior_session_samples() {
        use crate::audio::AudioCapture;
        let mut c = CpalCapture::new().unwrap();
        if c.start().is_err() {
            eprintln!("skipping: no default input device on test runner");
            return;
        }
        // Let a bit of audio flow into the prior ring.
        std::thread::sleep(std::time::Duration::from_millis(50));
        c.stop().unwrap();

        // Restart — the consumer is rebuilt fresh.
        c.start().unwrap();
        let mut buf = Vec::new();
        c.drain(&mut buf).unwrap();
        // After ~0 ms of post-restart capture, the new ring should be
        // essentially empty. We allow a tiny grace because cpal may
        // have already delivered one callback by now on fast machines.
        assert!(
            buf.len() < 16_000, // < 1 second @ 16 kHz mono
            "expected near-empty post-restart buffer, got {} samples",
            buf.len()
        );
        c.stop().unwrap();
    }

    #[test]
    fn device_changed_flag_starts_false() {
        let c = CpalCapture::new().unwrap();
        assert!(!c.device_changed());
        assert!(!c.take_device_changed());
    }

    #[test]
    fn stop_is_idempotent() {
        let mut c = CpalCapture::new().unwrap();
        c.stop().unwrap(); // never-started → no-op
        c.stop().unwrap(); // still no-op
    }
}
