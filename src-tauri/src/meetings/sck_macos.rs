//! ScreenCaptureKit system-audio capture (ADR 0068, mb-mac-v1.5.3).
//!
//! macOS analogue of [`loopback_windows`](super::loopback_windows): it
//! supplies the meeting **system** channel (loopback of everything the
//! machine is playing) as a [`crate::audio::AudioCapture`], so
//! [`TwinStreamCapture`](super::capture::TwinStreamCapture) drives it
//! identically to the Windows WASAPI loopback. The mic channel stays on
//! the 4a CoreAudio `CpalCapture` (two independent streams, exactly
//! mirroring Windows cpal + WASAPI) — see ADR 0068 §3.
//!
//! ## Why ScreenCaptureKit (and why system-audio-only)
//!
//! macOS has no WASAPI-loopback equivalent. Since macOS 13 the sanctioned
//! API for capturing other apps' audio is **ScreenCaptureKit (SCK)**, an
//! Objective-C framework gated behind the **Screen Recording** TCC grant.
//! We message it at runtime through the `objc2-screen-capture-kit`
//! bindings (the crate-spike confirmed full coverage — kennel drawer 110),
//! so there is **no Swift/ObjC sidecar and no full Xcode** dependency:
//! the framework links at load time, exactly like ADR 0060's NSWorkspace
//! bindings. Targeting **macOS 13+** — we deliberately do NOT use the
//! macOS-15-only `captureMicrophone` single-session dual-audio path.
//!
//! ## Push → pull bridge
//!
//! SCK is push-based: it invokes an `SCStreamOutput` delegate on a
//! dedicated dispatch queue with `CMSampleBuffer`s. Our `AudioCapture`
//! trait is pull-based (`drain`). We bridge them with a `ringbuf` SPSC
//! ring — the delegate owns the producer, [`SckSysCapture`] owns the
//! consumer — exactly like `loopback_windows.rs` bridges the WASAPI
//! callback. The delegate converts each buffer's PCM (its ASBD is read
//! at runtime; SCK delivers Float32) into 16 kHz mono `i16` by reusing
//! the shared [`AudioPipeline`] (downmix + resample + clamp) — no second
//! DSP pipeline (DRY, ADR 0013).
//!
//! ## The two gotchas (ADR 0068 §6)
//!
//!   * **Preflight the grant.** `start()` fails cleanly when Screen
//!     Recording is ungranted (mic-only meetings still work).
//!   * **Silent-buffer detector.** SCK does NOT error when the grant is
//!     *stale* — it delivers all-zero buffers. The delegate watches for a
//!     run of pure-silence samples and raises a clear `tracing::warn!`.

#![cfg(target_os = "macos")]

use std::ptr::{null_mut, NonNull};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_core_audio_types::{
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsSignedInteger, AudioBuffer, AudioBufferList,
};
use objc2_core_foundation::CFRetained;
use objc2_core_media::{
    kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment,
    CMAudioFormatDescriptionGetStreamBasicDescription, CMBlockBuffer, CMSampleBuffer,
};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol};
use objc2_screen_capture_kit::{
    SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamDelegate,
    SCStreamOutput, SCStreamOutputType, SCWindow,
};
use ringbuf::traits::{Consumer, Split};
use ringbuf::HeapRb;

use crate::audio::capture::{RING_CAPACITY, TARGET_CHANNELS, TARGET_SAMPLE_RATE};
use crate::audio::resampler::AudioPipeline;
use crate::audio::AudioCapture;
use crate::error::{AppError, AppResult};

/// SPSC ring halves. Same concrete type as the dictation path's
/// `SampleProducer`, so [`AudioPipeline::process_f32`] /
/// [`AudioPipeline::process_i16`] accept our producer directly.
type SysProducer = <HeapRb<i16> as Split>::Prod;
type SysConsumer = <HeapRb<i16> as Split>::Cons;

/// How many raw input samples of pure silence we tolerate before
/// concluding the system channel is dead (Screen Recording likely
/// stale). ~4 s at 16 kHz — long enough to avoid a false positive on a
/// genuinely quiet meeting start, short enough to surface early.
const SILENT_SAMPLE_THRESHOLD: u64 = 64_000;

/// How long the owner thread waits for an SCK async completion handler
/// (shareable-content resolution + `startCapture` / `stopCapture`).
const SETUP_TIMEOUT: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------------
// Screen Recording preflight (shared with `probe_sources`)
// ---------------------------------------------------------------------

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    /// Silent read of the current Screen Recording grant (the
    /// `CGRequestScreenCaptureAccess` variant is the one that prompts).
    /// Mirrors the preflight in [`crate::permissions::macos`]; SCK
    /// delivers zero-filled audio rather than erroring when this is
    /// `false`, hence we gate `start()` on it.
    fn CGPreflightScreenCaptureAccess() -> bool;
}

/// Whether this process currently holds the Screen Recording grant.
pub fn screen_recording_granted() -> bool {
    // SAFETY: preflight is a silent read returning a `bool`.
    unsafe { CGPreflightScreenCaptureAccess() }
}

// ---------------------------------------------------------------------
// SCStreamOutput / SCStreamDelegate — the push-side delegate
// ---------------------------------------------------------------------

/// Interior-mutable state shared with the SCK dispatch queue. The
/// [`AudioPipeline`] is built lazily from the FIRST buffer's ASBD so we
/// honor whatever format SCK actually delivers (it resamples to 16 kHz
/// mono when it can, but we resample defensively if it doesn't).
struct DelegateShared {
    pipeline: Option<AudioPipeline>,
    producer: SysProducer,
}

/// Instance variables for the `SCStreamOutput` delegate class.
struct SysAudioDelegateIvars {
    shared: Mutex<DelegateShared>,
    /// Total raw input samples observed (for the silence detector).
    total_samples: AtomicU64,
    /// Total non-zero raw input samples observed.
    nonzero_samples: AtomicU64,
    /// Set once we've raised the silent-channel diagnostic.
    silent_warned: AtomicBool,
}

define_class!(
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - `SysAudioDelegate` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[name = "MockingbirdSckSysAudioDelegate"]
    #[ivars = SysAudioDelegateIvars]
    struct SysAudioDelegate;

    unsafe impl NSObjectProtocol for SysAudioDelegate {}

    unsafe impl SCStreamOutput for SysAudioDelegate {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn stream_did_output(
            &self,
            _stream: &SCStream,
            sample_buffer: &CMSampleBuffer,
            output_type: SCStreamOutputType,
        ) {
            // We only ever registered the audio output, but be defensive:
            // ignore any screen/mic sample buffers.
            if output_type == SCStreamOutputType::Audio {
                self.handle_audio(sample_buffer);
            }
        }
    }

    unsafe impl SCStreamDelegate for SysAudioDelegate {
        #[unsafe(method(stream:didStopWithError:))]
        fn stream_did_stop(&self, _stream: &SCStream, error: &NSError) {
            tracing::warn!(
                target: "meetings",
                error = %ns_error_msg(error),
                "SCK system-audio stream stopped with error"
            );
        }
    }
);

impl SysAudioDelegate {
    fn new(producer: SysProducer) -> Retained<Self> {
        let this = Self::alloc().set_ivars(SysAudioDelegateIvars {
            shared: Mutex::new(DelegateShared {
                pipeline: None,
                producer,
            }),
            total_samples: AtomicU64::new(0),
            nonzero_samples: AtomicU64::new(0),
            silent_warned: AtomicBool::new(false),
        });
        // SAFETY: designated initializer for our NSObject subclass.
        unsafe { msg_send![super(this), init] }
    }

    /// Called on the SCK dispatch queue for every audio sample buffer.
    fn handle_audio(&self, sbuf: &CMSampleBuffer) {
        // --- 1. Read the delivered ASBD (rate / channels / format). ---
        // SAFETY: `format_description` is a read of the buffer's format.
        let Some(fmt) = (unsafe { sbuf.format_description() }) else {
            return;
        };
        // SAFETY: audio format descriptions expose a read-only ASBD ptr;
        // it may be NULL for non-audio descriptions (guarded below).
        let asbd_ptr = unsafe { CMAudioFormatDescriptionGetStreamBasicDescription(&fmt) };
        let Some(asbd) = (unsafe { asbd_ptr.as_ref() }) else {
            return;
        };
        let rate = asbd.mSampleRate as u32;
        let channels = (asbd.mChannelsPerFrame.max(1)) as u16;
        let is_float = asbd.mFormatFlags & kAudioFormatFlagIsFloat != 0;
        let is_int = asbd.mFormatFlags & kAudioFormatFlagIsSignedInteger != 0;
        let bits = asbd.mBitsPerChannel;

        // --- 2. Copy the PCM out into an AudioBufferList. ---
        let mut abl = AudioBufferList {
            mNumberBuffers: 1,
            mBuffers: [AudioBuffer {
                mNumberChannels: 0,
                mDataByteSize: 0,
                mData: null_mut(),
            }],
        };
        let mut block_buffer: *mut CMBlockBuffer = null_mut();
        // SAFETY: `abl`/`block_buffer` are valid out-pointers; we pass the
        // real size of our AudioBufferList and no custom allocators.
        let status = unsafe {
            sbuf.audio_buffer_list_with_retained_block_buffer(
                null_mut(),
                &mut abl,
                std::mem::size_of::<AudioBufferList>(),
                None,
                None,
                kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment,
                &mut block_buffer,
            )
        };
        // Take ownership of the +1-retained block buffer so it releases
        // when this scope ends (the AudioBufferList's data lives in it).
        // SAFETY: the API hands back a retained CMBlockBuffer (a CFType).
        let _bb = NonNull::new(block_buffer).map(|p| unsafe { CFRetained::from_raw(p) });
        if status != 0 {
            return;
        }
        let audio_buf = abl.mBuffers[0];
        if audio_buf.mData.is_null() || audio_buf.mDataByteSize == 0 {
            return;
        }
        let byte_size = audio_buf.mDataByteSize as usize;

        // --- 3. Feed the shared 16 kHz-mono-i16 pipeline. ---
        let mut guard = self
            .ivars()
            .shared
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if guard.pipeline.is_none() {
            match AudioPipeline::new(rate, channels) {
                Ok(p) => guard.pipeline = Some(p),
                Err(e) => {
                    tracing::error!(target: "meetings", error = %e, "SCK: audio pipeline init failed");
                    return;
                }
            }
        }
        let DelegateShared { pipeline, producer } = &mut *guard;
        let Some(pipeline) = pipeline.as_mut() else {
            return;
        };

        let (count, nonzero) = if is_float && bits == 32 {
            let n = byte_size / std::mem::size_of::<f32>();
            // SAFETY: SCK guarantees `mDataByteSize` bytes of valid PCM.
            let samples = unsafe { std::slice::from_raw_parts(audio_buf.mData as *const f32, n) };
            let nz = samples.iter().filter(|s| s.abs() > 1e-6).count() as u64;
            pipeline.process_f32(samples, producer);
            (n as u64, nz)
        } else if is_int && bits == 16 {
            let n = byte_size / std::mem::size_of::<i16>();
            // SAFETY: as above; format flags asserted 16-bit signed int.
            let samples = unsafe { std::slice::from_raw_parts(audio_buf.mData as *const i16, n) };
            let nz = samples.iter().filter(|s| **s != 0).count() as u64;
            pipeline.process_i16(samples, producer);
            (n as u64, nz)
        } else {
            drop(guard);
            self.warn_once(&format!(
                "unexpected SCK audio format (float={is_float}, int={is_int}, bits={bits}); dropping buffer"
            ));
            return;
        };
        drop(guard);

        self.update_silence(count, nonzero);
    }

    /// Silent-buffer detector (ADR 0068 §6, gotcha B): SCK does not error
    /// on a stale Screen Recording grant — it delivers zero-filled
    /// buffers. If we accumulate a threshold of pure silence, surface the
    /// permission diagnostic exactly once.
    fn update_silence(&self, count: u64, nonzero: u64) {
        let iv = self.ivars();
        let total = iv.total_samples.fetch_add(count, Ordering::Relaxed) + count;
        iv.nonzero_samples.fetch_add(nonzero, Ordering::Relaxed);
        if total >= SILENT_SAMPLE_THRESHOLD
            && iv.nonzero_samples.load(Ordering::Relaxed) == 0
            && !iv.silent_warned.swap(true, Ordering::Relaxed)
        {
            tracing::warn!(
                target: "meetings",
                "system audio silent — Screen Recording permission may be missing or stale \
                 (ScreenCaptureKit delivers zero-filled buffers instead of erroring when the \
                 grant is not live; re-grant in System Settings → Privacy & Security → Screen Recording)"
            );
        }
    }

    fn warn_once(&self, msg: &str) {
        if !self.ivars().silent_warned.swap(true, Ordering::Relaxed) {
            tracing::warn!(target: "meetings", "SCK system audio: {msg}");
        }
    }
}

// ---------------------------------------------------------------------
// SckSysCapture — the pull-side AudioCapture handle
// ---------------------------------------------------------------------

/// System-audio capture via ScreenCaptureKit. Implements
/// [`AudioCapture`] by draining the ring the [`SysAudioDelegate`]
/// fills. Not `Send` (like `CpalCapture`): the SCK objects are built on
/// the owner thread inside the `CaptureBuilder` closure and never cross
/// a thread boundary (ADR 0028).
pub struct SckSysCapture {
    /// Consumer half of the *current* ring. Rebuilt on every `start()`;
    /// an orphan (producer-less) ring before the first `start()` so
    /// `drain()` returns 0 cleanly.
    consumer: SysConsumer,
    stream: Option<Retained<SCStream>>,
    /// Retained so SCK's weak delegate reference stays alive for the
    /// stream's lifetime.
    delegate: Option<Retained<SysAudioDelegate>>,
    /// The sample-handler dispatch queue; retained for the stream's life.
    queue: Option<DispatchRetained<DispatchQueue>>,
    started: bool,
}

impl SckSysCapture {
    /// Construct a handle. Does NOT touch SCK or the permission — the
    /// stream is built at `start()` (lazy, matching `CpalCapture`).
    pub fn new() -> AppResult<Self> {
        let rb = HeapRb::<i16>::new(RING_CAPACITY);
        let (_orphan_producer, consumer) = rb.split();
        Ok(Self {
            consumer,
            stream: None,
            delegate: None,
            queue: None,
            started: false,
        })
    }
}

impl AudioCapture for SckSysCapture {
    fn start(&mut self) -> AppResult<()> {
        if self.started {
            return Ok(());
        }

        // Gotcha A (ADR 0068 §6): preflight. SCK is silent (not errorful)
        // when ungranted, so gate here and fail the meeting-start cleanly
        // — mic-only meetings still work.
        if !screen_recording_granted() {
            return Err(AppError::Audio(
                "Screen Recording permission not granted — enable it in System Settings → \
                 Privacy & Security → Screen Recording, then restart the meeting"
                    .into(),
            ));
        }

        // Fresh ring; producer moves into the delegate.
        let rb = HeapRb::<i16>::new(RING_CAPACITY);
        let (producer, consumer) = rb.split();
        self.consumer = consumer;
        let delegate = SysAudioDelegate::new(producer);

        // Dummy content filter: SCK needs a video target even for
        // audio-only. Pick a display, exclude nothing; we register only
        // the audio output and never consume a screen buffer.
        let content = get_shareable_content()?;
        // SAFETY: `displays` is a read of the resolved shareable content.
        let displays = unsafe { content.displays() };
        let display = displays.firstObject().ok_or_else(|| {
            AppError::Audio("SCK: no display available for content filter".into())
        })?;
        let empty_windows: Retained<NSArray<SCWindow>> = NSArray::new();
        // SAFETY: valid display + (empty) window array.
        let filter = unsafe {
            SCContentFilter::initWithDisplay_excludingWindows(
                SCContentFilter::alloc(),
                &display,
                &empty_windows,
            )
        };

        // Audio-only configuration (ADR 0068 §4).
        // SAFETY: setters mutate a fresh config object we own.
        let config = unsafe { SCStreamConfiguration::new() };
        unsafe {
            config.setCapturesAudio(true);
            config.setSampleRate(TARGET_SAMPLE_RATE as isize);
            config.setChannelCount(TARGET_CHANNELS as isize);
            config.setExcludesCurrentProcessAudio(true);
            // Minimal video surface; every screen buffer is ignored.
            config.setWidth(2);
            config.setHeight(2);
        }

        // Build the stream with our object as the SCStreamDelegate.
        // SAFETY: filter + config are valid; delegate outlives the stream.
        let stream = unsafe {
            SCStream::initWithFilter_configuration_delegate(
                SCStream::alloc(),
                &filter,
                &config,
                Some(ProtocolObject::from_ref(&*delegate)),
            )
        };

        // Register ONLY the audio output on a dedicated serial queue.
        let queue = DispatchQueue::new("com.mockingbird.sck-sys-audio", None);
        // SAFETY: valid output object + audio type + serial queue.
        unsafe {
            stream.addStreamOutput_type_sampleHandlerQueue_error(
                ProtocolObject::from_ref(&*delegate),
                SCStreamOutputType::Audio,
                Some(&queue),
            )
        }
        .map_err(|e| AppError::Audio(format!("SCK addStreamOutput: {}", ns_error_msg(&e))))?;

        // Start capture (async API bridged to sync).
        start_capture_blocking(&stream)?;

        self.delegate = Some(delegate);
        self.queue = Some(queue);
        self.stream = Some(stream);
        self.started = true;
        tracing::info!(target: "meetings", "SCK system-audio capture started (16 kHz mono)");
        Ok(())
    }

    fn stop(&mut self) -> AppResult<()> {
        if let Some(stream) = self.stream.take() {
            let (tx, rx) = mpsc::channel::<()>();
            let handler = RcBlock::new(move |_err: *mut NSError| {
                let _ = tx.send(());
            });
            // SAFETY: valid stream + completion block.
            unsafe { stream.stopCaptureWithCompletionHandler(Some(&handler)) };
            // Best-effort: don't hang teardown on a wedged stop.
            let _ = rx.recv_timeout(SETUP_TIMEOUT);
            tracing::info!(target: "meetings", "SCK system-audio capture stopped");
        }
        // Dropping the delegate/queue releases the SCK-side references.
        self.delegate = None;
        self.queue = None;
        self.started = false;
        Ok(())
    }

    fn drain(&mut self, buf: &mut Vec<i16>) -> AppResult<usize> {
        let before = buf.len();
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

impl Drop for SckSysCapture {
    fn drop(&mut self) {
        if self.started {
            let _ = self.stop();
        }
    }
}

// ---------------------------------------------------------------------
// Completion-handler → synchronous bridges
// ---------------------------------------------------------------------

/// Resolve the shareable content (displays/windows/apps) synchronously.
///
/// SCK's API is completion-handler based and dispatches the handler on
/// an internal background queue, so blocking the owner thread on a
/// channel cannot self-deadlock. Object ownership is transferred across
/// the thread boundary as a raw pointer address (`usize` is `Send`;
/// `Retained<SCShareableContent>` is not) — the +1 retain taken on the
/// handler thread is reconstructed here.
fn get_shareable_content() -> AppResult<Retained<SCShareableContent>> {
    let (tx, rx) = mpsc::channel::<Result<usize, String>>();
    let handler = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
        // SAFETY: SCK hands back a +0 autoreleased content object; retain
        // it to own it, then transfer as a raw address.
        let res = match unsafe { Retained::retain(content) } {
            Some(c) => Ok(Retained::into_raw(c) as usize),
            None => Err(ns_error_msg_opt(err)),
        };
        let _ = tx.send(res);
    });
    // SAFETY: valid completion block.
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&handler) };

    match rx.recv_timeout(SETUP_TIMEOUT) {
        Ok(Ok(addr)) => {
            let ptr = addr as *mut SCShareableContent;
            // SAFETY: reconstruct the Retained transferred from the
            // handler thread (a +1 object we now own).
            unsafe { Retained::from_raw(ptr) }
                .ok_or_else(|| AppError::Audio("SCK: null shareable content".into()))
        }
        Ok(Err(e)) => Err(AppError::Audio(format!("SCK shareable content: {e}"))),
        Err(_) => Err(AppError::Audio("SCK shareable content timed out".into())),
    }
}

/// Start the stream and block until SCK's completion handler fires.
fn start_capture_blocking(stream: &SCStream) -> AppResult<()> {
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    let handler = RcBlock::new(move |err: *mut NSError| {
        // SAFETY: `err` is nil on success, else a borrowed NSError.
        let res = match unsafe { err.as_ref() } {
            Some(e) => Err(ns_error_msg(e)),
            None => Ok(()),
        };
        let _ = tx.send(res);
    });
    // SAFETY: valid stream + completion block.
    unsafe { stream.startCaptureWithCompletionHandler(Some(&handler)) };

    match rx.recv_timeout(SETUP_TIMEOUT) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(AppError::Audio(format!("SCK startCapture: {e}"))),
        Err(_) => Err(AppError::Audio("SCK startCapture timed out".into())),
    }
}

// ---------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------

/// `NSError.localizedDescription` as an owned `String`.
fn ns_error_msg(error: &NSError) -> String {
    error.localizedDescription().to_string()
}

/// `ns_error_msg` for a raw (possibly nil) `*mut NSError`.
fn ns_error_msg_opt(error: *mut NSError) -> String {
    // SAFETY: borrow-only; guarded against nil.
    match unsafe { error.as_ref() } {
        Some(e) => ns_error_msg(e),
        None => "nil content and nil error".to_string(),
    }
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Constructor must not open SCK or require any permission; it only
    /// wires an orphan ring. Mirrors `LoopbackCapture::new`'s contract.
    #[test]
    fn new_does_not_touch_sck() {
        let _c = SckSysCapture::new().expect("construct SckSysCapture handle");
    }

    /// Trait-conformance guarantee: the consumer always sees 16 kHz mono
    /// i16 regardless of what SCK delivers (the pipeline enforces it).
    #[test]
    fn sample_rate_and_channels_are_16khz_mono() {
        let c = SckSysCapture::new().unwrap();
        assert_eq!(c.sample_rate(), 16_000);
        assert_eq!(c.channels(), 1);
    }

    /// `drain()` before any `start()` returns 0 cleanly (orphan ring),
    /// never panicking on an unbacked consumer.
    #[test]
    fn drain_before_start_returns_zero() {
        let mut c = SckSysCapture::new().unwrap();
        let mut buf = Vec::new();
        let n = c.drain(&mut buf).unwrap();
        assert_eq!(n, 0);
        assert!(buf.is_empty());
    }

    /// `stop()` before any `start()` is a clean no-op, and idempotent.
    #[test]
    fn stop_is_idempotent_without_start() {
        let mut c = SckSysCapture::new().unwrap();
        c.stop().expect("stop on never-started capture");
        c.stop().expect("second stop is also a no-op");
    }

    /// `start()` without the Screen Recording grant fails cleanly (not a
    /// panic) with a diagnostic that names the permission. On a dev box
    /// that HAS granted it, `start()` may instead succeed — either way it
    /// must not panic, and `stop()` cleans up.
    #[test]
    fn start_without_grant_errs_cleanly_or_starts() {
        let mut c = SckSysCapture::new().unwrap();
        match c.start() {
            Err(e) => {
                let msg = format!("{e}");
                assert!(
                    msg.contains("Screen Recording") || msg.contains("SCK"),
                    "unexpected start() error: {msg}"
                );
            }
            Ok(()) => {
                // Granted on this box; tear back down.
                c.stop().expect("stop after a successful start");
            }
        }
    }
}
