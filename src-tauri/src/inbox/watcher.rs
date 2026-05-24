//! Inbox file-watcher (Wave 3.1) — ADR 0046 Iter 3 / mb-9lgi.
//!
//! Watches `<vault>/inbox/` recursively for audio files arriving via
//! Obsidian Sync from the iPhone (iOS Shortcut → Voice Memo → Files
//! → vault). Emits [`StableInboxFile`] notifications down a
//! [`crossbeam_channel::Sender`] once each candidate has cleared two
//! consecutive size-stability checks, ready for the courier
//! (Wave 3.2) to validate + decode + ingest.
//!
//! ## Why a stability state machine on top of a debouncer
//!
//! The Wave 0 spike (`docs/spikes/iter3-sync-layer-findings.md`)
//! exposed two FS-event behaviours we have to design around:
//!
//! - **Finding 3** — every logical change fires **3–4 duplicate
//!   events within ~5–12 ms**. Raw `notify` would route every burst
//!   straight to our handler. `notify-debouncer-full` coalesces the
//!   burst into a single [`DebouncedEvent`] per path per quiet
//!   window. We use a **100 ms** quiet window (comfortable headroom
//!   over the observed ~12 ms maximum).
//!
//! - **Findings 1, 4** — binary audio arrives **atomically** at full
//!   size (Round 4b: 258,743 B in one `Created` event). The
//!   stability check is therefore "cheap insurance" rather than a
//!   functional requirement for the iOS Shortcut courier — but it
//!   future-proofs against (a) larger payloads if Obsidian ever
//!   switches to chunked binary delivery and (b) the local-FS edge
//!   case where a user drags a half-copied file into `inbox/`.
//!
//! ## Stability state machine
//!
//! - On the first debounced event for an allowlisted path, the
//!   watcher records a [`Candidate`] with `stable_count = 0`.
//! - Every [`STABILITY_INTERVAL`] (2 s) the watcher re-reads the
//!   file size. Two consecutive identical readings (other than the
//!   initial reading) → emit [`StableInboxFile`].
//! - Size change between checks → reset `stable_count`, bump
//!   `retries`. After [`MAX_RETRIES`] (5), the candidate is dropped
//!   with a warning. Total worst-case wall time: ~10 s.
//! - Zero-byte files block on the first check (Obsidian streams
//!   text as filename-at-0 B → content; we never want to dispatch
//!   a zero-byte file to STT).
//!
//! ## What gets filtered before it ever reaches a candidate
//!
//! Per Findings 7 + 8 and ADR 0046 §6:
//!
//! - **Path segment exclusions** — anything under `.obsidian/`,
//!   `.git/`, `.mockingbird/`, `inbox/_archive/`, `inbox/_failed/`,
//!   `inbox/_keep/`. The `.obsidian/` exclusion is load-bearing:
//!   `workspace.json` fires every few seconds (Finding 7) and
//!   would otherwise blow up the candidate table.
//! - **Filename / extension exclusions** — `.tmp`, `.partial`,
//!   `.crdownload`, `.icloud`, `.swp`, `.lock` (defensive against
//!   future transports; none observed in the spike per Finding 1),
//!   plus `~$` prefix (Office lockfile pattern).
//! - **Extension allowlist** — `.m4a` (primary), `.wav`, `.mp3`.
//!
//! ## What this module does NOT do
//!
//! - No decode, no STT, no DB write. That's the courier's job
//!   ([`super::courier`], Phase B of this dispatch).
//! - No vault-zone creation. [`crate::vault::layout::VaultLayout::ensure_zones`]
//!   already creates `inbox/` + `inbox/_failed/`; the runtime
//!   ([`super::runtime`], Phase C) calls it before spawning us.
//! - No conflict-file quarantine. ADR 0046 §6's conflict-file
//!   detection + dedup-ledger + placeholder-note flow are Iter 4
//!   Wave 5 hardening matrix items (`mb-qxrm`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::Sender;
// `notify` itself isn't a direct dep — we use the re-export from
// `notify_debouncer_full` so the dep graph stays minimal.
use notify_debouncer_full::notify::{RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult};

use crate::error::{AppError, AppResult};

// --------------------------------------------------------------------
// Tunables (constants, not config — the spike findings nail these
// down well enough that exposing them as settings would be premature
// per YAGNI. Promote to settings only if a real-world incident
// surfaces a need.)
// --------------------------------------------------------------------

/// `notify-debouncer-full` quiet window. 100 ms easily absorbs the
/// observed 3–4 events / ~12 ms duplicate-burst pattern (Finding 3)
/// without adding noticeable courier latency.
const DEBOUNCE_QUIET_WINDOW: Duration = Duration::from_millis(100);

/// Interval between consecutive size-stability probes for a single
/// candidate. Two consecutive matches → stable.
const STABILITY_INTERVAL: Duration = Duration::from_secs(2);

/// Number of consecutive equal size readings required for "stable".
/// First check anchors the reading, subsequent equal checks count.
const REQUIRED_STABLE_READS: u32 = 2;

/// Cap on retries before the watcher gives up on a file whose size
/// keeps changing (continuously-written log file, or a paused upload).
/// Prevents pathological candidates from living in the table forever.
const MAX_RETRIES: u32 = 5;

/// How often the worker loop wakes up to advance candidate stability
/// checks. Smaller than [`STABILITY_INTERVAL`] so we tick on time
/// even when no FS event arrives. 500 ms keeps idle CPU near zero.
const TICK_INTERVAL: Duration = Duration::from_millis(500);

/// Path-segment exclusions: any debounced path whose components
/// contain ONE of these strings (case-sensitive) is dropped before
/// candidacy. The slash form is intentional — we want
/// `inbox/_archive` to match anywhere under the watched root, but
/// not e.g. a user file literally named `_archive.m4a`.
const PATH_SEGMENT_EXCLUSIONS: &[&str] = &[
    ".obsidian",
    ".git",
    ".mockingbird",
    "_archive",
    "_failed",
    "_keep",
];

/// Filename / extension exclusions. Suffix-matched against the
/// filename's lowercased form so `.M4A` and `.m4a` are both
/// matched correctly on the allowlist side as well.
const EXTENSION_EXCLUSIONS: &[&str] = &[
    ".tmp",
    ".partial",
    ".crdownload",
    ".icloud",
    ".swp",
    ".lock",
];

/// Filename prefix exclusions (Office lockfile + dotfile patterns).
const FILENAME_PREFIX_EXCLUSIONS: &[&str] = &["~$"];

/// Allowlisted audio extensions. Lowercased form — caller compares
/// against the filename's lowercased extension.
const EXTENSION_ALLOWLIST: &[&str] = &["m4a", "wav", "mp3"];

// --------------------------------------------------------------------
// Public types
// --------------------------------------------------------------------

/// A file that has cleared every check the watcher imposes —
/// allowlisted, non-empty, size-stable. Handed to the courier
/// (Wave 3.2) for validation, decode, and ingest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableInboxFile {
    /// Absolute path. Always under the watched root.
    pub path: PathBuf,
    /// File size in bytes at the moment of the second stable read.
    /// The courier re-checks before opening so a race here is
    /// non-fatal — but persisting it lets the courier log the
    /// "expected vs. actual" delta if it does drift.
    pub size: u64,
    /// Wall-clock timestamp when the file first appeared in our
    /// candidate table. Used as `received_at_iso` provenance for
    /// the eventual `sessions` row.
    pub observed_at: SystemTime,
}

/// Handle for stopping the watcher cleanly. Sending on the shutdown
/// channel + joining the worker thread tears the debouncer down.
/// Returned by [`InboxWatcher::start`].
pub struct InboxWatcherHandle {
    shutdown_tx: crossbeam_channel::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl InboxWatcherHandle {
    /// Signal the worker to exit, then wait for the thread. Returns
    /// `Err` if joining failed (worker panicked); on the happy path
    /// the debouncer's drop in the worker tears down the OS watch.
    pub fn stop(mut self) -> AppResult<()> {
        // A best-effort send — if the receiver is already gone the
        // worker is already shutting down, which is the desired
        // state anyway.
        let _ = self.shutdown_tx.send(());
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| AppError::Other("inbox watcher thread panicked".into()))?;
        }
        Ok(())
    }
}

impl Drop for InboxWatcherHandle {
    /// Defensive shutdown — the runtime layer should always call
    /// [`stop`](Self::stop), but a panic in the runtime construction
    /// path could drop us early. Signalling the worker keeps that
    /// case from leaking an OS watch.
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
        if let Some(thread) = self.thread.take() {
            // We can't propagate a panic in Drop, so swallow it.
            // The shutdown signal above is what matters.
            let _ = thread.join();
        }
    }
}

/// The watcher's main configuration + entry point.
pub struct InboxWatcher {
    inbox_path: PathBuf,
    output_tx: Sender<StableInboxFile>,
}

impl InboxWatcher {
    /// Build a watcher rooted at `<vault>/inbox/`. The path is NOT
    /// validated here — [`crate::vault::layout::VaultLayout::ensure_zones`]
    /// is the right place for that, and the runtime calls it before
    /// constructing us. If the path doesn't exist when [`start`] is
    /// called, the debouncer returns a clean `Err`.
    pub fn new(inbox_path: PathBuf, output_tx: Sender<StableInboxFile>) -> Self {
        Self {
            inbox_path,
            output_tx,
        }
    }

    /// Spawn the watcher worker thread. Returns a handle for
    /// shutdown. The watcher claims the [`Sender`] passed to
    /// [`new`](Self::new) and runs until [`InboxWatcherHandle::stop`]
    /// (or [`Drop`]) signals.
    pub fn start(self) -> AppResult<InboxWatcherHandle> {
        let (shutdown_tx, shutdown_rx) = crossbeam_channel::bounded::<()>(1);
        let (event_tx, event_rx) = std::sync::mpsc::channel::<DebounceEventResult>();

        // The debouncer holds the OS handle for the duration of the
        // worker thread; constructing it here (rather than inside
        // the spawned thread) means a config error surfaces
        // synchronously on `start()` instead of becoming a silent
        // background failure.
        let mut debouncer = new_debouncer(DEBOUNCE_QUIET_WINDOW, None, move |result| {
            // Send-failures here mean the worker thread is gone,
            // which is also the only consumer for these events —
            // safe to ignore.
            let _ = event_tx.send(result);
        })
        .map_err(|e| AppError::Other(format!("inbox watcher: build debouncer: {e}")))?;

        debouncer
            .watcher()
            .watch(&self.inbox_path, RecursiveMode::Recursive)
            .map_err(|e| {
                AppError::Other(format!(
                    "inbox watcher: watch {}: {}",
                    self.inbox_path.display(),
                    e
                ))
            })?;

        let inbox_path = self.inbox_path.clone();
        let output_tx = self.output_tx.clone();
        let thread = std::thread::Builder::new()
            .name("mockingbird-inbox-watcher".into())
            .spawn(move || {
                // Move debouncer in so its Drop runs when the
                // worker exits, tearing down the OS watch cleanly.
                let _debouncer = debouncer;
                worker_loop(&inbox_path, &event_rx, &shutdown_rx, &output_tx);
                tracing::info!(target: "inbox::watcher", "worker exiting");
            })
            .map_err(|e| AppError::Other(format!("inbox watcher: spawn thread: {e}")))?;

        tracing::info!(
            target: "inbox::watcher",
            path = %self.inbox_path.display(),
            "watcher started"
        );

        Ok(InboxWatcherHandle {
            shutdown_tx,
            thread: Some(thread),
        })
    }
}

// --------------------------------------------------------------------
// Pure filter logic (unit-testable without FS or notify)
// --------------------------------------------------------------------

/// True if a path should be considered as an inbox candidate. False
/// means we drop the event silently. Splits the decision into
/// independently-testable axes so each rule can be exercised on its
/// own.
pub(crate) fn should_consider_path(path: &Path) -> bool {
    let filename = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        // Non-UTF8 filenames are rejected — every supported audio
        // format we care about is named via the iOS picker or
        // Obsidian Sync, both of which produce ASCII/UTF-8 names.
        None => return false,
    };
    let filename_lower = filename.to_ascii_lowercase();

    // Prefix exclusions (Office lockfile etc.).
    for prefix in FILENAME_PREFIX_EXCLUSIONS {
        if filename_lower.starts_with(prefix) {
            return false;
        }
    }

    // Extension-suffix exclusions (partial-write extensions).
    for ext in EXTENSION_EXCLUSIONS {
        if filename_lower.ends_with(ext) {
            return false;
        }
    }

    // Path-segment exclusions. We compare against each component
    // independently (rather than substring-matching the full path)
    // so a user file literally named `inbox.m4a` doesn't get caught
    // by the `inbox` substring inside a path like
    // `<vault>/inbox/_archive/`.
    for component in path.components() {
        let comp_str = match component.as_os_str().to_str() {
            Some(s) => s,
            None => continue,
        };
        for excl in PATH_SEGMENT_EXCLUSIONS {
            if comp_str == *excl {
                return false;
            }
        }
    }

    // Allowlist check — extension must be one of the audio formats
    // headless ingest knows how to decode.
    let ext_lower = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());
    match ext_lower.as_deref() {
        Some(ext) => EXTENSION_ALLOWLIST.contains(&ext),
        None => false,
    }
}

// --------------------------------------------------------------------
// Candidate state machine (testable in isolation via injectable
// `now` + `size_of` so we don't need real FS or `Instant::now()`)
// --------------------------------------------------------------------

/// One in-flight candidate the watcher is shepherding through the
/// stability check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Candidate {
    /// Wall-clock first observation (used as `observed_at` in the
    /// eventual [`StableInboxFile`]).
    pub first_seen_wall: SystemTime,
    /// Monotonic timestamp of the most-recent size probe.
    pub last_probe_at: Instant,
    /// Last size we saw. `None` before the first probe.
    pub last_size: Option<u64>,
    /// Number of consecutive equal probes (counting the most
    /// recent). Reset to 0 on size change.
    pub stable_count: u32,
    /// Reset retries — incremented every time the size CHANGED
    /// between probes. Capped at [`MAX_RETRIES`].
    pub retries: u32,
    /// True when the candidate has been emitted as
    /// [`StableInboxFile`] and is about to be removed from the
    /// table. Lets us distinguish "emit-then-remove" from "drop"
    /// in logs without resurrecting the path.
    pub emitted: bool,
}

impl Candidate {
    fn new(now_wall: SystemTime, now_mono: Instant) -> Self {
        Self {
            first_seen_wall: now_wall,
            last_probe_at: now_mono,
            last_size: None,
            stable_count: 0,
            retries: 0,
            emitted: false,
        }
    }
}

/// Outcome of advancing one candidate.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AdvanceOutcome {
    /// Not yet time to probe — leave the candidate alone.
    NotYet,
    /// File missing or zero-byte — keep waiting, no state change.
    EmptyOrMissing,
    /// Probe succeeded; advance the state machine and continue.
    Continue,
    /// Two consecutive equal reads — emit this stable file.
    Emit(StableInboxFile),
    /// `retries` exceeded [`MAX_RETRIES`] — drop the candidate.
    GiveUp,
}

/// Pure state-machine step. Drives one candidate forward by one
/// probe. The `now_mono` / `size_fn` injection keeps this testable
/// without real disk I/O or clock.
pub(crate) fn advance_candidate(
    path: &Path,
    candidate: &mut Candidate,
    now_mono: Instant,
    size_fn: impl FnOnce(&Path) -> Option<u64>,
) -> AdvanceOutcome {
    if now_mono.saturating_duration_since(candidate.last_probe_at) < STABILITY_INTERVAL {
        return AdvanceOutcome::NotYet;
    }
    candidate.last_probe_at = now_mono;

    let size = match size_fn(path) {
        Some(s) if s > 0 => s,
        _ => {
            // File missing or zero-byte. Per Finding 4 the filename
            // shows up before content for text files; for audio it
            // shouldn't, but the same handling is harmless.
            tracing::debug!(
                target: "inbox::watcher",
                path = %path.display(),
                "candidate empty / missing on probe; waiting"
            );
            return AdvanceOutcome::EmptyOrMissing;
        }
    };

    match candidate.last_size {
        Some(prev) if prev == size => {
            candidate.stable_count = candidate.stable_count.saturating_add(1);
            if candidate.stable_count >= REQUIRED_STABLE_READS {
                candidate.emitted = true;
                return AdvanceOutcome::Emit(StableInboxFile {
                    path: path.to_path_buf(),
                    size,
                    observed_at: candidate.first_seen_wall,
                });
            }
            AdvanceOutcome::Continue
        }
        Some(_) => {
            // Size changed — reset stability progress, bump
            // retries. The first probe (where last_size was None)
            // does NOT count as a retry — that's just the anchor
            // reading.
            candidate.stable_count = 0;
            candidate.last_size = Some(size);
            candidate.retries = candidate.retries.saturating_add(1);
            // `>=` (not `>`) so [`MAX_RETRIES`] is the actual count
            // of size-change events tolerated. With
            // STABILITY_INTERVAL = 2 s and MAX_RETRIES = 5, the
            // worst-case wall time before GiveUp is ~10 s.
            if candidate.retries >= MAX_RETRIES {
                tracing::warn!(
                    target: "inbox::watcher",
                    path = %path.display(),
                    retries = candidate.retries,
                    "candidate exceeded MAX_RETRIES; dropping"
                );
                return AdvanceOutcome::GiveUp;
            }
            AdvanceOutcome::Continue
        }
        None => {
            // First successful probe — anchor the reading.
            candidate.last_size = Some(size);
            candidate.stable_count = 1;
            AdvanceOutcome::Continue
        }
    }
}

// --------------------------------------------------------------------
// Worker loop (the glue between notify-debouncer-full and the
// candidate state machine)
// --------------------------------------------------------------------

fn worker_loop(
    inbox_path: &Path,
    event_rx: &std::sync::mpsc::Receiver<DebounceEventResult>,
    shutdown_rx: &crossbeam_channel::Receiver<()>,
    output_tx: &Sender<StableInboxFile>,
) {
    let mut candidates: HashMap<PathBuf, Candidate> = HashMap::new();

    loop {
        // 1. Honour any shutdown request before doing more work.
        if shutdown_rx.try_recv().is_ok() {
            tracing::debug!(target: "inbox::watcher", "shutdown requested");
            break;
        }

        // 2. Drain any new debounced events from notify. The
        //    recv_timeout cadence is our combined "tick clock" — if
        //    no events arrive we still wake every TICK_INTERVAL to
        //    advance candidates.
        match event_rx.recv_timeout(TICK_INTERVAL) {
            Ok(Ok(events)) => {
                for ev in events {
                    // DebouncedEvent carries the underlying
                    // notify::Event; iterate its `paths`.
                    for p in &ev.event.paths {
                        ingest_event_path(p, inbox_path, &mut candidates);
                    }
                }
            }
            Ok(Err(errors)) => {
                for e in errors {
                    tracing::warn!(
                        target: "inbox::watcher",
                        error = ?e,
                        "debouncer surfaced error"
                    );
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // No events this tick — proceed to advance pass.
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                tracing::warn!(target: "inbox::watcher", "event channel disconnected");
                break;
            }
        }

        // 3. Advance every candidate by one step (those whose
        //    STABILITY_INTERVAL has not yet elapsed return NotYet
        //    and stay put).
        advance_all_candidates(&mut candidates, output_tx);
    }
}

/// Decide whether a single event-path is candidate-worthy. Either
/// inserts it into the table (refreshing `first_seen_wall` only if
/// brand-new) or drops it.
fn ingest_event_path(
    raw_path: &Path,
    inbox_root: &Path,
    candidates: &mut HashMap<PathBuf, Candidate>,
) {
    // Canonicalize-ish: notify-debouncer-full gives us paths under
    // the watched root, but they're not always absolute on every
    // backend. Re-anchor against inbox_root so the candidate key
    // and the StableInboxFile.path are consistent.
    let path = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        inbox_root.join(raw_path)
    };

    if !should_consider_path(&path) {
        tracing::trace!(
            target: "inbox::watcher",
            path = %path.display(),
            "event filtered (exclusion or non-audio extension)"
        );
        return;
    }

    // Insert-if-absent; we deliberately don't refresh
    // `first_seen_wall` on re-events because the spike showed
    // duplicate-event bursts (Finding 3) — refreshing on every
    // burst would reset the stability clock perversely.
    if !candidates.contains_key(&path) {
        let now_wall = SystemTime::now();
        let now_mono = Instant::now();
        // Anchor `last_probe_at` `STABILITY_INTERVAL` in the past
        // so the FIRST probe fires on the next worker tick — we
        // don't want to wait an extra cycle just to take the
        // anchor reading.
        let anchor = now_mono.checked_sub(STABILITY_INTERVAL).unwrap_or(now_mono);
        let mut c = Candidate::new(now_wall, now_mono);
        c.last_probe_at = anchor;
        candidates.insert(path.clone(), c);
        tracing::info!(
            target: "inbox::watcher",
            path = %path.display(),
            "candidate registered"
        );
    }
}

fn advance_all_candidates(
    candidates: &mut HashMap<PathBuf, Candidate>,
    output_tx: &Sender<StableInboxFile>,
) {
    let now_mono = Instant::now();
    // Collect the keys to iterate so we can mutate `candidates`
    // inside the loop without fighting the borrow checker.
    let paths: Vec<PathBuf> = candidates.keys().cloned().collect();
    for path in paths {
        // Re-borrow per-iteration; advance_candidate needs &mut.
        let outcome = {
            let candidate = match candidates.get_mut(&path) {
                Some(c) => c,
                None => continue,
            };
            advance_candidate(&path, candidate, now_mono, probe_file_size)
        };
        match outcome {
            AdvanceOutcome::NotYet | AdvanceOutcome::EmptyOrMissing | AdvanceOutcome::Continue => { /* keep waiting */
            }
            AdvanceOutcome::Emit(stable) => {
                tracing::info!(
                    target: "inbox::watcher",
                    path = %stable.path.display(),
                    size = stable.size,
                    "candidate stable — emitting"
                );
                if let Err(e) = output_tx.send(stable) {
                    tracing::warn!(
                        target: "inbox::watcher",
                        error = ?e,
                        "output channel closed; dropping stable event"
                    );
                }
                candidates.remove(&path);
            }
            AdvanceOutcome::GiveUp => {
                candidates.remove(&path);
            }
        }
    }
}

/// Production size probe. Returns `None` on any I/O error so the
/// state machine treats "missing" the same as "still 0 B" — the
/// candidate stays in the table until it either stabilises with a
/// real size or hits [`MAX_RETRIES`].
fn probe_file_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
}

// --------------------------------------------------------------------
// Tests
// --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    //! NOTE: per LESSONS PINNED P2, `cargo test --release` does not
    //! run on this box (STATUS_ENTRYPOINT_NOT_FOUND); these tests
    //! compile + link via `cargo test --release --no-run` and are
    //! live-exercised via the throwaway-crate recipe. Every test
    //! in this module is pure-Rust (no whisper-rs / ort / CUDA)
    //! and so is throwaway-crate-friendly.

    use super::*;
    use std::path::PathBuf;

    // ----- should_consider_path filter coverage -----

    #[test]
    fn allowlist_accepts_lowercase_audio_extensions() {
        assert!(should_consider_path(Path::new("inbox/foo.m4a")));
        assert!(should_consider_path(Path::new("inbox/foo.wav")));
        assert!(should_consider_path(Path::new("inbox/foo.mp3")));
    }

    #[test]
    fn allowlist_accepts_uppercase_audio_extensions() {
        // iOS Files app sometimes uppercases extensions.
        assert!(should_consider_path(Path::new("inbox/Foo.M4A")));
        assert!(should_consider_path(Path::new("inbox/foo.WAV")));
    }

    #[test]
    fn rejects_text_and_non_audio_extensions() {
        assert!(!should_consider_path(Path::new("inbox/note.md")));
        assert!(!should_consider_path(Path::new("inbox/photo.jpg")));
        assert!(!should_consider_path(Path::new("inbox/data.json")));
    }

    #[test]
    fn rejects_partial_write_extensions() {
        assert!(!should_consider_path(Path::new("inbox/foo.m4a.tmp")));
        assert!(!should_consider_path(Path::new("inbox/foo.m4a.partial")));
        assert!(!should_consider_path(Path::new("inbox/foo.m4a.crdownload")));
        assert!(!should_consider_path(Path::new("inbox/foo.m4a.icloud")));
        assert!(!should_consider_path(Path::new("inbox/foo.m4a.swp")));
        assert!(!should_consider_path(Path::new("inbox/foo.m4a.lock")));
    }

    #[test]
    fn rejects_obsidian_workspace_churn() {
        // Finding 7 — the load-bearing exclusion.
        assert!(!should_consider_path(Path::new(
            "vault/.obsidian/workspace.json"
        )));
        assert!(!should_consider_path(Path::new(
            "vault/.obsidian/plugins/x.m4a"
        )));
    }

    #[test]
    fn rejects_archive_failed_keep_zones() {
        assert!(!should_consider_path(Path::new(
            "vault/inbox/_archive/2026-05-27/foo.m4a"
        )));
        assert!(!should_consider_path(Path::new(
            "vault/inbox/_failed/foo.m4a"
        )));
        assert!(!should_consider_path(Path::new(
            "vault/inbox/_keep/foo.m4a"
        )));
    }

    #[test]
    fn rejects_dot_dirs_anywhere_in_path() {
        assert!(!should_consider_path(Path::new("vault/.git/x.m4a")));
        assert!(!should_consider_path(Path::new(
            "vault/.mockingbird/manifest.json"
        )));
    }

    #[test]
    fn rejects_office_lockfile_prefix() {
        assert!(!should_consider_path(Path::new("inbox/~$file.m4a")));
    }

    #[test]
    fn substring_match_does_not_trap_user_named_files() {
        // A user file literally named `_archive.m4a` should NOT be
        // caught by the `_archive` path-segment exclusion — only
        // actual `_archive/` directory components match.
        assert!(should_consider_path(Path::new("inbox/_archive.m4a")));
        // But the same file inside _archive/ does match.
        assert!(!should_consider_path(Path::new("inbox/_archive/a.m4a")));
    }

    #[test]
    fn rejects_extensionless_files() {
        assert!(!should_consider_path(Path::new("inbox/no-extension")));
    }

    // ----- advance_candidate state-machine coverage -----

    fn synthetic_candidate(seen_age: Duration) -> Candidate {
        // Build a candidate whose `first_seen_wall` is `seen_age`
        // in the past from now. We don't use it in the state
        // machine — just in the emitted StableInboxFile.observed_at.
        Candidate {
            first_seen_wall: SystemTime::now() - seen_age,
            last_probe_at: Instant::now() - STABILITY_INTERVAL,
            last_size: None,
            stable_count: 0,
            retries: 0,
            emitted: false,
        }
    }

    #[test]
    fn advance_not_yet_when_inside_interval() {
        let mut c = synthetic_candidate(Duration::from_secs(0));
        // Pretend we probed 100 ms ago (well inside the 2 s window).
        c.last_probe_at = Instant::now() - Duration::from_millis(100);
        let outcome = advance_candidate(Path::new("/inbox/x.m4a"), &mut c, Instant::now(), |_| {
            Some(100)
        });
        assert_eq!(outcome, AdvanceOutcome::NotYet);
        // No state change.
        assert_eq!(c.last_size, None);
    }

    #[test]
    fn advance_anchors_first_size_then_emits_after_two_matches() {
        let mut c = synthetic_candidate(Duration::from_secs(1));
        // Probe 1: anchors at 1024.
        let out1 = advance_candidate(Path::new("/inbox/x.m4a"), &mut c, Instant::now(), |_| {
            Some(1024)
        });
        assert_eq!(out1, AdvanceOutcome::Continue);
        assert_eq!(c.last_size, Some(1024));
        assert_eq!(c.stable_count, 1);

        // Probe 2: equal → stable_count = 2 → emit (REQUIRED = 2).
        // Need to bump our pretend clock 2 s forward so the
        // STABILITY_INTERVAL gate passes.
        c.last_probe_at -= STABILITY_INTERVAL;
        let out2 = advance_candidate(Path::new("/inbox/x.m4a"), &mut c, Instant::now(), |_| {
            Some(1024)
        });
        match out2 {
            AdvanceOutcome::Emit(stable) => {
                assert_eq!(stable.path, PathBuf::from("/inbox/x.m4a"));
                assert_eq!(stable.size, 1024);
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    #[test]
    fn advance_resets_stability_and_increments_retries_on_size_change() {
        let mut c = synthetic_candidate(Duration::from_secs(1));
        // Anchor first reading.
        let _ = advance_candidate(Path::new("/inbox/x.m4a"), &mut c, Instant::now(), |_| {
            Some(1024)
        });
        // Size changes on next probe.
        c.last_probe_at -= STABILITY_INTERVAL;
        let out = advance_candidate(Path::new("/inbox/x.m4a"), &mut c, Instant::now(), |_| {
            Some(2048)
        });
        assert_eq!(out, AdvanceOutcome::Continue);
        assert_eq!(c.last_size, Some(2048));
        assert_eq!(c.stable_count, 0);
        assert_eq!(c.retries, 1);
    }

    #[test]
    fn advance_gives_up_after_max_retries() {
        let mut c = synthetic_candidate(Duration::from_secs(1));
        // Anchor probe (last_size = Some(1), stable_count = 1,
        // retries = 0).
        let _ = advance_candidate(Path::new("/inbox/x.m4a"), &mut c, Instant::now(), |_| {
            Some(1)
        });
        // Each iteration changes the size once, bumping retries.
        // After MAX_RETRIES iterations (5 changes total) the
        // state machine returns GiveUp on that 5th change.
        for n in 2..=(MAX_RETRIES as u64 + 1) {
            c.last_probe_at -= STABILITY_INTERVAL;
            let out = advance_candidate(Path::new("/inbox/x.m4a"), &mut c, Instant::now(), |_| {
                Some(n)
            });
            // n = 2 -> retries 1, ..., n = MAX_RETRIES (5) -> retries 4 -> Continue.
            // n = MAX_RETRIES + 1 (6)         -> retries 5 -> GiveUp.
            if (n - 1) < MAX_RETRIES as u64 {
                assert_eq!(out, AdvanceOutcome::Continue, "iter {n} still continues");
            } else {
                assert_eq!(out, AdvanceOutcome::GiveUp, "iter {n} should give up");
            }
        }
    }

    #[test]
    fn advance_treats_zero_and_missing_as_empty() {
        let mut c = synthetic_candidate(Duration::from_secs(1));
        let out_zero = advance_candidate(Path::new("/x.m4a"), &mut c, Instant::now(), |_| Some(0));
        assert_eq!(out_zero, AdvanceOutcome::EmptyOrMissing);
        // last_size NOT advanced — still None.
        assert_eq!(c.last_size, None);
        c.last_probe_at -= STABILITY_INTERVAL;
        let out_missing = advance_candidate(Path::new("/x.m4a"), &mut c, Instant::now(), |_| None);
        assert_eq!(out_missing, AdvanceOutcome::EmptyOrMissing);
    }

    // ----- atomic-arrival happy path (Finding 1) -----
    //
    // Binary delivery is atomic per the spike — full size shows up
    // in one event. We anchor at full size on probe 1, see the
    // same size on probe 2, emit. Total wall-time ~ STABILITY_INTERVAL.

    #[test]
    fn atomic_binary_arrival_emits_after_two_equal_reads() {
        let mut c = synthetic_candidate(Duration::ZERO);
        // Probe 1 (anchor).
        let out1 = advance_candidate(Path::new("/x.m4a"), &mut c, Instant::now(), |_| {
            Some(258_743)
        });
        assert_eq!(out1, AdvanceOutcome::Continue);
        // Probe 2 (equal).
        c.last_probe_at -= STABILITY_INTERVAL;
        let out2 = advance_candidate(Path::new("/x.m4a"), &mut c, Instant::now(), |_| {
            Some(258_743)
        });
        assert!(matches!(out2, AdvanceOutcome::Emit(_)));
    }

    #[test]
    fn duplicate_burst_does_not_emit_multiple_stable_files() {
        // Finding 3 — every event fires 3-4x. The candidate table
        // is keyed on PathBuf so re-inserts collapse to one entry;
        // verify by directly exercising ingest_event_path.
        let mut candidates: HashMap<PathBuf, Candidate> = HashMap::new();
        let root = Path::new("/vault");
        // Simulate the 4-burst pattern.
        for _ in 0..4 {
            ingest_event_path(&root.join("inbox/Memo.m4a"), root, &mut candidates);
        }
        assert_eq!(candidates.len(), 1, "burst must collapse to one entry");
    }

    #[test]
    fn ingest_relative_path_anchors_under_inbox_root() {
        let mut candidates: HashMap<PathBuf, Candidate> = HashMap::new();
        let root = Path::new("/vault/inbox");
        ingest_event_path(Path::new("foo.m4a"), root, &mut candidates);
        // The key MUST be the rooted absolute path, not the
        // relative one — otherwise the courier would look up
        // sizes in the wrong place.
        assert!(candidates.contains_key(&PathBuf::from("/vault/inbox/foo.m4a")));
    }
}
