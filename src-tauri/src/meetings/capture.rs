//! Twin-stream capture coordinator (ADR 0028).
//!
//! Owns ≤2 simultaneous audio captures (mic + system loopback), each
//! on its own dedicated thread, and routes the resulting PCM through
//! per-channel [`MeetingChunker`]s. Chunks roll into a single shared
//! `mpsc::Sender<ChannelChunk>` that the consumer (the Wave 3
//! `LongFormStt` driver, then the Wave 4 runtime) reads via
//! [`TwinStreamCapture::try_recv_chunks`].
//!
//! ## Why owner-thread-per-stream
//!
//! `cpal::Stream` is `!Send` on Windows — the underlying WASAPI
//! `IAudioClient` is COM-thread-affined. ADR 0028 chose to put both
//! streams on the meetings thread; the same constraint applies here.
//! Each thread:
//!
//!   1. Builds its [`AudioCapture`] on the owner thread itself (a
//!      `CaptureBuilder` closure is moved into the thread, then
//!      invoked), so the `!Send` constraint never crosses a thread
//!      boundary.
//!   2. Calls `capture.start()`.
//!   3. Polls `capture.drain(&mut buf)` every [`POLL_INTERVAL`] ms
//!      and feeds the drained samples into its `MeetingChunker`.
//!   4. Forwards every emitted [`ChunkWritten`] over the shared chunk
//!      channel, wrapped as a [`ChannelChunk`] with its channel tag.
//!   5. On stop signal (the per-thread `mpsc::Sender<()>` is dropped
//!      by the coordinator), performs one final drain + `finalize()`,
//!      then exits cleanly.
//!
//! ## Test seam
//!
//! [`TwinStreamCapture::start_with`] accepts arbitrary [`CaptureBuilder`]s
//! so the unit tests at the bottom of this file can drive the
//! coordinator with a synthetic `StubCapture` that emits pre-loaded
//! sample batches. Production code uses [`TwinStreamCapture::start`],
//! which builds [`crate::audio::capture::CpalCapture`] for mic and
//! [`crate::meetings::loopback_windows::LoopbackCapture`] for system.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::audio::AudioCapture;
use crate::error::{AppError, AppResult};
use crate::meetings::chunker::{ChunkWritten, ChunkerConfig, MeetingChunker};

// ---------------------------------------------------------------------
// MeetingSource (carry-forward from Wave 1)
// ---------------------------------------------------------------------

/// Which audio channel(s) to capture for a meeting.
///
/// Persisted form matches the `meeting_sessions.source` column and the
/// `SettingKey::MeetingDefaultSource` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeetingSource {
    Mic,
    System,
    Both,
}

impl MeetingSource {
    /// Persisted form for the `meeting_sessions.source` column.
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Mic => "mic",
            Self::System => "system",
            Self::Both => "both",
        }
    }

    /// Parse from the persisted form.
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "mic" => Some(Self::Mic),
            "system" => Some(Self::System),
            "both" => Some(Self::Both),
            _ => None,
        }
    }

    pub fn needs_mic(self) -> bool {
        matches!(self, Self::Mic | Self::Both)
    }

    pub fn needs_system(self) -> bool {
        matches!(self, Self::System | Self::Both)
    }
}

/// Source probe — what's actually capturable on this machine right now.
/// Returned by the `meeting_probe_sources` IPC command (Wave 4).
///
/// Wave 3 keeps this as a typed primitive; the *probe* (actually
/// checking whether the endpoint will open) lands in Wave 4 alongside
/// the IPC command. Today it's a stub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeetingSourceProbe {
    pub mic_available: bool,
    pub system_available: bool,
}

/// Stub probe. Wave 4 replaces with a real cpal-driven open-test.
pub fn probe_sources() -> AppResult<MeetingSourceProbe> {
    Ok(MeetingSourceProbe {
        mic_available: true,
        system_available: false,
    })
}

// ---------------------------------------------------------------------
// Channel + ChannelChunk
// ---------------------------------------------------------------------

/// Which side of a meeting this chunk came from. Routed through the
/// shared `mpsc` channel; the consumer fans out by `channel` to
/// per-stream stitching downstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    Mic,
    Sys,
}

impl Channel {
    /// Filename tag used by [`MeetingChunker`]
    /// (`<uuid>_<tag>_<seq>.wav`).
    pub fn tag(self) -> &'static str {
        match self {
            Channel::Mic => "mic",
            Channel::Sys => "sys",
        }
    }
}

/// One rolled chunk with its channel tag attached. The struct the
/// coordinator emits over `mpsc`.
#[derive(Debug)]
pub struct ChannelChunk {
    pub channel: Channel,
    pub chunk: ChunkWritten,
}

// ---------------------------------------------------------------------
// TwinStreamCapture
// ---------------------------------------------------------------------

/// Test-seam factory: a closure that builds an `AudioCapture` on the
/// owner thread (sidestepping the `!Send` constraint on `cpal::Stream`).
///
/// Boxed `FnOnce` so it can be moved into the thread; `Send + 'static`
/// because the closure itself crosses the spawn boundary even though
/// the value it returns does not.
pub type CaptureBuilder = Box<dyn FnOnce() -> AppResult<Box<dyn AudioCapture>> + Send + 'static>;

/// Owner-thread drain poll interval. 50 ms keeps wakeups light while
/// staying well under the chunker's 30 s window (so a stop signal
/// surfaces inside one tick at worst).
pub const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Coordinator handle. Construct via [`start`] (production) or
/// [`start_with`] (tests). Drop or call [`stop`] to tear down.
///
/// [`start`]: Self::start
/// [`start_with`]: Self::start_with
/// [`stop`]: Self::stop
pub struct TwinStreamCapture {
    mic_thread: Option<JoinHandle<AppResult<()>>>,
    sys_thread: Option<JoinHandle<AppResult<()>>>,
    chunk_rx: Receiver<ChannelChunk>,
    /// Held to signal stop by drop. Per-thread (an `mpsc::Sender` is
    /// not broadcast-able; one stop sender per receiver is the
    /// idiomatic shape).
    mic_stop_tx: Option<Sender<()>>,
    sys_stop_tx: Option<Sender<()>>,
    /// Tracks whether `stop()` has already executed; second call is
    /// a no-op. Also short-circuits `Drop`.
    stopped: bool,
}

impl TwinStreamCapture {
    /// Production constructor. Wires `CpalCapture` (mic) and/or
    /// `LoopbackCapture` (system) per the configured `source`.
    pub fn start(
        meeting_uuid: String,
        source: MeetingSource,
        chunk_dir: PathBuf,
        config: ChunkerConfig,
    ) -> AppResult<Self> {
        let mic_builder: Option<CaptureBuilder> = if source.needs_mic() {
            Some(Box::new(build_mic_capture))
        } else {
            None
        };
        let sys_builder: Option<CaptureBuilder> = if source.needs_system() {
            Some(Box::new(build_sys_capture))
        } else {
            None
        };
        Self::start_with(meeting_uuid, chunk_dir, config, mic_builder, sys_builder)
    }

    /// Test seam + production internal entry point.
    ///
    /// Either or both builders may be `None`; at least one must be
    /// `Some` or this returns `AppError::MeetingCapture`.
    pub fn start_with(
        meeting_uuid: String,
        chunk_dir: PathBuf,
        config: ChunkerConfig,
        mic_builder: Option<CaptureBuilder>,
        sys_builder: Option<CaptureBuilder>,
    ) -> AppResult<Self> {
        if mic_builder.is_none() && sys_builder.is_none() {
            return Err(AppError::MeetingCapture(
                "TwinStreamCapture: at least one channel must be active".into(),
            ));
        }
        let (chunk_tx, chunk_rx) = mpsc::channel::<ChannelChunk>();

        let (mic_thread, mic_stop_tx) = spawn_channel_thread(
            Channel::Mic,
            mic_builder,
            &meeting_uuid,
            &chunk_dir,
            &config,
            &chunk_tx,
        )?;
        let (sys_thread, sys_stop_tx) = spawn_channel_thread(
            Channel::Sys,
            sys_builder,
            &meeting_uuid,
            &chunk_dir,
            &config,
            &chunk_tx,
        )?;
        // The originating chunk_tx is dropped here; the clones inside
        // the threads keep the channel alive until both threads exit.
        drop(chunk_tx);

        Ok(Self {
            mic_thread,
            sys_thread,
            chunk_rx,
            mic_stop_tx,
            sys_stop_tx,
            stopped: false,
        })
    }

    /// Non-blocking pull of any chunks that rolled since the last call.
    /// The runtime polls this at ~5 Hz.
    pub fn try_recv_chunks(&mut self) -> Vec<ChannelChunk> {
        let mut out = Vec::new();
        while let Ok(c) = self.chunk_rx.try_recv() {
            out.push(c);
        }
        out
    }

    /// Stop both threads, flush trailing chunks via `chunker.finalize()`,
    /// join. Returns whatever chunks were still in flight at
    /// shutdown — including the per-channel trailing chunk if the
    /// chunker had any residual `pending` samples.
    ///
    /// Idempotent: second call returns `Ok(empty)`.
    pub fn stop(&mut self) -> AppResult<Vec<ChannelChunk>> {
        if self.stopped {
            return Ok(Vec::new());
        }
        self.stopped = true;

        // Drop the per-thread stop senders. The threads will see
        // `RecvTimeoutError::Disconnected` on their next `recv_timeout`
        // and exit the loop.
        drop(self.mic_stop_tx.take());
        drop(self.sys_stop_tx.take());

        // Join threads. Surface thread-side errors AFTER draining
        // chunks (we still want the user's pre-error chunks).
        let mut errors = Vec::new();
        if let Some(h) = self.mic_thread.take() {
            match h.join() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => errors.push(format!("mic: {e}")),
                Err(_) => errors.push("mic thread panicked".to_string()),
            }
        }
        if let Some(h) = self.sys_thread.take() {
            match h.join() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => errors.push(format!("sys: {e}")),
                Err(_) => errors.push("sys thread panicked".to_string()),
            }
        }

        // Threads have exited; the chunk_rx is now fully drained-able.
        let trailing = self.try_recv_chunks();

        if !errors.is_empty() {
            return Err(AppError::MeetingCapture(format!(
                "TwinStreamCapture stop errors: {}",
                errors.join("; ")
            )));
        }
        Ok(trailing)
    }
}

impl Drop for TwinStreamCapture {
    fn drop(&mut self) {
        if !self.stopped {
            // Best-effort shutdown; errors are tracing-logged but not
            // re-raised (we're in Drop, after all).
            if let Err(e) = self.stop() {
                tracing::warn!(
                    target: "meetings",
                    error = %e,
                    "TwinStreamCapture::drop saw stop() error"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------

/// Spawn the owner thread for one channel, if a builder was provided.
/// Returns `(Some(handle), Some(stop_sender))` when a thread was
/// spawned; `(None, None)` when `builder` was `None`. Returns `Err`
/// only if the thread spawn itself failed.
#[allow(clippy::type_complexity)] // tuple shape is local + documented
fn spawn_channel_thread(
    channel: Channel,
    builder: Option<CaptureBuilder>,
    meeting_uuid: &str,
    chunk_dir: &std::path::Path,
    config: &ChunkerConfig,
    chunk_tx: &Sender<ChannelChunk>,
) -> AppResult<(Option<JoinHandle<AppResult<()>>>, Option<Sender<()>>)> {
    let Some(builder) = builder else {
        return Ok((None, None));
    };
    let chunker = MeetingChunker::new(
        meeting_uuid.to_string(),
        channel.tag(),
        chunk_dir.to_path_buf(),
        config.clone(),
    );
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let tx = chunk_tx.clone();
    let thread_name = format!("twin-stream-{}", channel.tag());
    let handle = std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || owner_thread_loop(channel, builder, chunker, stop_rx, tx))
        .map_err(|e| AppError::MeetingCapture(format!("spawn {} thread: {e}", channel.tag())))?;
    Ok((Some(handle), Some(stop_tx)))
}

/// The per-channel owner thread. Builds the capture on this thread
/// (honoring `!Send`), drains in a poll loop, feeds the chunker,
/// finalizes on stop.
fn owner_thread_loop(
    channel: Channel,
    builder: CaptureBuilder,
    mut chunker: MeetingChunker,
    stop_rx: Receiver<()>,
    chunk_tx: Sender<ChannelChunk>,
) -> AppResult<()> {
    let mut capture = builder()?;
    capture.start()?;

    let mut buf: Vec<i16> = Vec::with_capacity(8192);
    loop {
        match stop_rx.recv_timeout(POLL_INTERVAL) {
            // Explicit stop signal (sender sent before drop).
            Ok(()) => break,
            // Stop sender dropped → coordinator wants us out.
            Err(RecvTimeoutError::Disconnected) => break,
            // Normal tick: drain capture + feed chunker.
            Err(RecvTimeoutError::Timeout) => {
                buf.clear();
                let n = capture.drain(&mut buf)?;
                if n > 0 {
                    for cw in chunker.feed(&buf)? {
                        if chunk_tx.send(ChannelChunk { channel, chunk: cw }).is_err() {
                            // Consumer hung up. Stop gracefully.
                            return shutdown_capture(capture);
                        }
                    }
                }
            }
        }
    }

    // Final drain pass — pick up anything captured between the last
    // tick and the stop signal.
    buf.clear();
    let n = capture.drain(&mut buf)?;
    if n > 0 {
        for cw in chunker.feed(&buf)? {
            // Ignore send errors here; we're shutting down.
            let _ = chunk_tx.send(ChannelChunk { channel, chunk: cw });
        }
    }

    // Trailing chunk (the chunker's residual `pending`, if any).
    if let Some(trailing) = chunker.finalize()? {
        let _ = chunk_tx.send(ChannelChunk {
            channel,
            chunk: trailing,
        });
    }

    shutdown_capture(capture)
}

fn shutdown_capture(mut capture: Box<dyn AudioCapture>) -> AppResult<()> {
    capture.stop()
}

// ---------------------------------------------------------------------
// Production builders
// ---------------------------------------------------------------------

fn build_mic_capture() -> AppResult<Box<dyn AudioCapture>> {
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(crate::audio::capture::CpalCapture::new()?) as Box<dyn AudioCapture>)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(AppError::Audio(
            "mic capture not implemented for this platform (Phase 9 macOS/Linux)".into(),
        ))
    }
}

fn build_sys_capture() -> AppResult<Box<dyn AudioCapture>> {
    #[cfg(target_os = "windows")]
    {
        Ok(
            Box::new(crate::meetings::loopback_windows::LoopbackCapture::new()?)
                as Box<dyn AudioCapture>,
        )
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(AppError::Audio(
            "loopback capture not implemented for this platform (Phase 9 macOS/Linux)".into(),
        ))
    }
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;
