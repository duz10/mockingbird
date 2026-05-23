//! Headless-ingest request channel — the sibling input feeding the
//! orchestrator's `run` loop alongside the existing `StateAction`
//! stream.
//!
//! ## Why this exists (ADR 0046 §3.2)
//!
//! `DictationOrchestrator` owns heavyweight, `!Send`/`!Sync`-on-Windows
//! resources (Silero VAD, whisper-rs CUDA context, Ollama HTTP client).
//! Constructing fresh copies per file import would re-pay whisper-rs's
//! ~3-4 GB VRAM allocation + cold-load latency on every click of the
//! "+ Audio file" button. So headless ingest requests have to be
//! forwarded INTO the dictation thread to reuse those deps.
//!
//! The existing input — `std::sync::mpsc::Receiver<StateAction>` from
//! `StateDriver::start` — cannot be `select!`ed alongside another
//! channel: `std::sync::mpsc` has no select primitive. Extending
//! `StateAction` with a variant would couple the hotkey FSM to a
//! concern outside its responsibility AND violate the §3 boundary
//! list. So we add a SIBLING `crossbeam-channel` carrying
//! [`HeadlessIngestRequest`]s, and the orchestrator's `run` loop
//! `select!`s between the two (with an in-thread bridge converting
//! the std-mpsc `StateAction` stream to crossbeam so both legs of
//! the select are the same flavor).
//!
//! ## Producers
//!
//! - **Iter 1** — `commands::dictation::dictation_import_file` IPC.
//!   User picks an audio file; the handler decodes it via
//!   [`crate::audio::decode::decode_to_pcm16_mono_16k`] on the IPC
//!   worker thread (NOT the orchestrator thread — never block the
//!   orchestrator on file I/O), then sends a `HeadlessIngestRequest`
//!   carrying the PCM + a per-request reply channel.
//! - **Iter 3** — vault inbox watcher (ADR 0046 §6). Same shape:
//!   decode off-thread, send the request, await reply.
//!
//! Both producers share the same `Sender<HeadlessIngestRequest>` via
//! `Clone` (crossbeam senders are multi-producer by construction).
//!
//! ## Consumer
//!
//! Exactly one — the orchestrator thread. The receiver lives inside
//! the orchestrator after `DictationOrchestrator::new`; on each
//! `select!` arm that fires, `dictation::ingest::headless_ingest` is
//! invoked with the orchestrator's existing VAD/STT/Cleaner deps and
//! the result flows back via the per-request reply channel.
//!
//! ## Reply channel discipline
//!
//! Bounded(1). The orchestrator sends exactly one value per request
//! (the `AppResult<i64>`); the caller is the only receiver. If the
//! caller drops its `reply_rx` before we send (IPC handler panicked,
//! browser tab closed mid-import), our send returns `Err` — we log
//! and move on. No backpressure, no buffering.

use crossbeam_channel::Sender;

use super::ingest::IngestProvenance;
use crate::error::AppResult;

/// One headless ingest request en route to the orchestrator.
///
/// All fields are owned (no borrows) because the request crosses a
/// thread boundary and the producer's stack is gone by the time the
/// orchestrator processes the request.
pub struct HeadlessIngestRequest {
    /// 16 kHz mono i16 PCM. Caller decoded via
    /// [`crate::audio::decode::decode_to_pcm16_mono_16k`] before
    /// queueing — this keeps the symphonia codec pass off the
    /// orchestrator's thread.
    pub samples: Vec<i16>,

    /// Source + filename + received-at iso. Threaded into the
    /// `sessions` row by [`crate::dictation::ingest::headless_ingest`].
    pub provenance: IngestProvenance,

    /// Per-request reply channel. The orchestrator sends exactly one
    /// `AppResult<i64>` (the new `sessions.id`, or the propagated
    /// error) and drops its half. The caller is expected to hold the
    /// matching `Receiver` and `.recv()` it synchronously.
    ///
    /// A bounded(1) channel is appropriate: there's exactly one
    /// reply, the caller is already waiting, no buffering needed.
    pub reply_tx: Sender<AppResult<i64>>,
}

/// Sender half of the headless-ingest channel.
///
/// Cloneable — every IPC handler / watcher loop that needs to enqueue
/// requests gets its own `Sender` by cloning the one Tauri publishes
/// via managed state.
pub type HeadlessIngestSender = Sender<HeadlessIngestRequest>;

/// Build a fresh unbounded headless-ingest channel.
///
/// Unbounded because (a) headless ingest is rare (interactive
/// file-pick or single-file-per-courier — never a burst), and (b)
/// blocking the IPC handler on a bounded send would freeze the UI.
/// The natural backpressure is whisper-rs's serialized
/// transcribe-per-request behaviour — only one ingest runs at a time
/// on the orchestrator thread regardless of queue depth.
pub fn channel() -> (
    HeadlessIngestSender,
    crossbeam_channel::Receiver<HeadlessIngestRequest>,
) {
    crossbeam_channel::unbounded()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::sessions::SessionSource;

    /// The reply channel must be bounded(1) per the doc comment, so
    /// a sender clone-and-drop pattern doesn't accidentally leave
    /// buffered replies in flight after the caller goes away. This
    /// test just locks that convention in.
    #[test]
    fn reply_channel_holds_exactly_one_value() {
        let (reply_tx, reply_rx) = crossbeam_channel::bounded::<AppResult<i64>>(1);
        reply_tx
            .send(Ok(42))
            .expect("first send fits in bounded(1)");
        // A second send would block forever; verify the channel is
        // full by trying a non-blocking try_send.
        assert!(reply_tx.try_send(Ok(99)).is_err());
        assert_eq!(reply_rx.recv().unwrap().unwrap(), 42);
    }

    /// Smoke: queue a request, dequeue it on a different thread, and
    /// verify the reply round-trips. Asserts the channel topology
    /// works as documented without involving any of the real VAD /
    /// STT / Cleaner machinery.
    #[test]
    fn request_round_trips_through_channel() {
        let (req_tx, req_rx) = channel();
        let (reply_tx, reply_rx) = crossbeam_channel::bounded::<AppResult<i64>>(1);

        // "Producer" — IPC handler shape.
        req_tx
            .send(HeadlessIngestRequest {
                samples: vec![0i16; 16],
                provenance: IngestProvenance::desktop_import(
                    "a.m4a".into(),
                    "2026-05-27T00:00:00Z".into(),
                ),
                reply_tx,
            })
            .expect("send");

        // "Consumer" — orchestrator shape, very simplified.
        let consumer = std::thread::spawn(move || {
            let req = req_rx.recv().expect("recv");
            assert_eq!(req.provenance.source, SessionSource::DesktopImport);
            req.reply_tx.send(Ok(7)).expect("reply send");
        });

        assert_eq!(reply_rx.recv().unwrap().unwrap(), 7);
        consumer.join().expect("join");
    }
}
