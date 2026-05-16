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
use ringbuf::traits::{Consumer, Producer, Split};
#[cfg(target_os = "windows")]
use ringbuf::HeapRb;

#[cfg(target_os = "windows")]
use crate::error::AppError;
use crate::error::AppResult;

/// Target audio format. Locked in by ADR 0013.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;
/// Target channel count. Locked in by ADR 0013.
pub const TARGET_CHANNELS: u16 = 1;
/// Ring buffer capacity (~30 s @ 16 kHz mono i16 ≈ 960 KB).
pub const RING_CAPACITY: usize = 480_000;
/// Device-watcher poll interval.
pub const DEVICE_POLL_INTERVAL: Duration = Duration::from_secs(1);

#[cfg(target_os = "windows")]
type SampleProducer = <HeapRb<i16> as Split>::Prod;
#[cfg(target_os = "windows")]
type SampleConsumer = <HeapRb<i16> as Split>::Cons;

#[cfg(target_os = "windows")]
pub struct CpalCapture {
    host: Host,
    /// Currently-active stream. None when stopped or pre-start.
    stream: Option<Stream>,
    /// Consumer half of the ring.
    consumer: SampleConsumer,
    /// Producer half — held in a slot so `start()` can move it into
    /// the cpal callback closure exactly once per build.
    producer_slot: Option<SampleProducer>,
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
        let host = cpal::default_host();
        let rb = HeapRb::<i16>::new(RING_CAPACITY);
        let (producer, consumer) = rb.split();
        Ok(Self {
            host,
            stream: None,
            consumer,
            producer_slot: Some(producer),
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
    fn build_stream(device: &Device, mut producer: SampleProducer) -> AppResult<Stream> {
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
        let needs_resample =
            device_rate != TARGET_SAMPLE_RATE || device_channels != TARGET_CHANNELS;
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

    /// Spawn the device-watcher thread. Runs until `watcher_alive`
    /// flips false (which `stop()` does).
    ///
    /// `cpal::Host` isn't `Clone`, so the watcher thread calls
    /// `cpal::default_host()` itself — Host is effectively a
    /// per-platform singleton (cheap to re-resolve).
    fn spawn_watcher(&self, initial_name: Option<String>) -> JoinHandle<()> {
        let alive = self.watcher_alive.clone();
        let changed = self.device_changed.clone();
        std::thread::spawn(move || {
            let host = cpal::default_host();
            let mut current = initial_name;
            while alive.load(Ordering::Relaxed) {
                std::thread::sleep(DEVICE_POLL_INTERVAL);
                if !alive.load(Ordering::Relaxed) {
                    break;
                }
                let next: Option<String> = host
                    .default_input_device()
                    .and_then(|d: Device| d.name().ok());
                if next != current {
                    tracing::info!(
                        target: "audio",
                        old = ?current,
                        new = ?next,
                        "default input device changed"
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
            .host
            .default_input_device()
            .ok_or_else(|| AppError::Audio("no default input device".into()))?;
        let device_name = device.name().ok();

        let producer = self.producer_slot.take().ok_or_else(|| {
            AppError::Audio(
                "producer already consumed by a prior start(); restart after stop() \
                 is not supported in Phase 2 (Phase 5 wires recording lifecycle)"
                    .into(),
            )
        })?;

        let stream = Self::build_stream(&device, producer)?;
        stream
            .play()
            .map_err(|e| AppError::Audio(format!("stream play: {e}")))?;

        self.current_device_name = device_name.clone();
        self.stream = Some(stream);

        self.watcher_alive.store(true, Ordering::Relaxed);
        self.device_changed.store(false, Ordering::Relaxed);
        self.watcher_handle = Some(self.spawn_watcher(device_name.clone()));

        tracing::info!(target: "audio", ?device_name, "capture started");
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

    /// Restart after stop is intentionally NOT supported in Phase 2.
    /// The second start() returns AppError::Audio with a specific
    /// message — and crucially does NOT panic.
    #[test]
    fn restart_after_stop_errors_cleanly() {
        let mut c = CpalCapture::new().unwrap();
        if c.start().is_err() {
            eprintln!("skipping: no default input device on test runner");
            return;
        }
        c.stop().unwrap();
        let err = c.start().expect_err("restart should error in Phase 2");
        let msg = err.to_string();
        assert!(
            msg.contains("restart"),
            "expected restart-not-supported message, got: {msg}"
        );
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
