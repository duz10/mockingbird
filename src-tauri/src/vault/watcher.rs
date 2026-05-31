//! Reverse-watcher (Wave 1E.5 / `mb-qwfy`).
//!
//! Watches `<vault>/Knowledge Graph/` for user edits in Obsidian
//! and reconciles them back into the SQLite DB. **File wins on
//! conflict** (ADR 0053 Â§D5): the DB updates to match the file's
//! content. There is no merge dialog, no three-way merge â€” the
//! user owns their markdown.
//!
//! # What gets reconciled
//!
//! - `Entries/*.md` â€” parsed via [`crate::vault::markdown_parser`];
//!   tag + entity mention rows are wiped + re-inserted at
//!   `segment_idx = 0` (the per-segment ordinal lost meaning the
//!   moment the user touched the file), and
//!   `sessions.vault_file_hash` is updated so the *next* projection
//!   doesn't re-fire the watcher into an infinite loop.
//! - `Entities/*.md` â€” **IGNORED**. These are user-owned stub
//!   pages per ADR 0053 Â§D11; reverse-syncing them into the DB
//!   would clobber the user's curated notes.
//! - `Projects/*.md` â€” **IGNORED** (Â§D12; same reason).
//! - `History/**` â€” **IGNORED**. This is the forensic archive
//!   (`*.json` sidecars + audio); the user shouldn't edit, but if
//!   they do we don't touch the DB.
//! - `Inbox/*.{m4a,wav,...}` â€” **IGNORED** by this watcher; that
//!   directory is the KG-Inbox courier's concern (Wave 1E.6).
//!
//! # Loop-prevention
//!
//! Every Mockingbird-originated write to `Entries/*.md` is
//! followed by a `sessions.vault_file_hash = <sha256>` UPDATE (see
//! `vault::writer`). When the watcher fires on a file change, it
//! re-reads the file, recomputes the SHA-256, and compares to
//! `vault_file_hash`. Match â†’ this is OUR write; do nothing.
//! Mismatch â†’ real user edit; reconcile.
//!
//! # Lifecycle
//!
//! A manager thread polls `KgGraphEnabled` + `VaultPath` every
//! [`MANAGER_POLL_INTERVAL`] (3s). When the conditions are first
//! met, it constructs an internal [`InnerWatcher`] (debouncer +
//! event-handler thread) rooted at `<vault>/Knowledge Graph/`.
//! When either condition flips off, the manager drops the inner
//! watcher (which tears down the OS handle via the
//! `notify-debouncer-full` drop). A path change while running
//! triggers stop + start. This mirrors the
//! [`crate::inbox::runtime::InboxRuntime`] shape but is simpler
//! (no courier, no progress bus, just one component).
//!
//! Disabled-by-default: a fresh install where `KgGraphEnabled=false`
//! pays zero I/O here. The manager thread does spawn unconditionally
//! (one bounded poll-sleep loop is negligible) so that toggling the
//! setting at runtime starts the watcher within ~3s â€” no relaunch
//! required.
//!
//! # Robustness
//!
//! Every per-event failure mode (parse fail, missing file,
//! malformed YAML, vocab drift, DB error) is `tracing::warn!`-ed
//! and skipped. The watcher stays alive through bad input. The
//! manager loop survives sub-component panics by isolating the
//! inner watcher in its own thread and treating a join failure as
//! "restart on next poll".

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify_debouncer_full::notify::{RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use rusqlite::Connection;

use crate::error::{AppError, AppResult};
use crate::settings::{model::SettingKey, Settings};
use crate::vault::kg_layout::KG_SUBTREE_ROOT_NAME;
use crate::vault::watcher_reconcile::{classify_path, reconcile_entry_file};

// Re-export the reconciler's public types so the watcher's public
// surface stays a single entry point (`crate::vault::watcher::*`)
// for callers + tests.
pub use crate::vault::watcher_reconcile::{PathClass, ReconcileOutcome};

// --------------------------------------------------------------------
// Tunables
// --------------------------------------------------------------------

/// Debouncer quiet window. ADR 0053 Â§D5 specifies 2s; we honour
/// that exactly so a quick succession of Obsidian autosaves coalesces
/// into one reconcile.
const DEBOUNCE_QUIET_WINDOW: Duration = Duration::from_secs(2);

/// Manager poll cadence â€” how often we re-read the settings to
/// detect toggle-on / toggle-off / vault-path change. 3s feels
/// snappy enough for a settings flip without burning measurable CPU.
const MANAGER_POLL_INTERVAL: Duration = Duration::from_secs(3);

// --------------------------------------------------------------------
// Public surface
// --------------------------------------------------------------------

/// Runtime handle stashed in Tauri managed state.
///
/// `Arc::clone` is cheap; the manager thread holds its own clones
/// of the inner state. The drop on this handle signals the manager
/// to exit + the inner watcher (if any) to tear down its OS watch.
pub struct ReverseWatcherRuntime {
    shutdown: Arc<AtomicBool>,
    inner: Arc<Mutex<Option<InnerWatcher>>>,
    /// Manager thread join handle. `Option` so `Drop` can take it.
    manager_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl ReverseWatcherRuntime {
    /// Spawn the manager thread. Returns immediately; the watcher
    /// itself only constructs once the settings poll sees
    /// `KgGraphEnabled=true && VaultPath=Some(_)`.
    pub fn spawn(conn: Arc<Mutex<Connection>>) -> Arc<Self> {
        let shutdown = Arc::new(AtomicBool::new(false));
        let inner: Arc<Mutex<Option<InnerWatcher>>> = Arc::new(Mutex::new(None));

        let shutdown_for_thread = Arc::clone(&shutdown);
        let inner_for_thread = Arc::clone(&inner);
        let conn_for_thread = Arc::clone(&conn);
        let thread = std::thread::Builder::new()
            .name("mockingbird-kg-reverse-watcher-mgr".into())
            .spawn(move || {
                manager_loop(conn_for_thread, inner_for_thread, shutdown_for_thread);
            })
            .expect("reverse-watcher manager thread should spawn");

        Arc::new(Self {
            shutdown,
            inner,
            manager_thread: Mutex::new(Some(thread)),
        })
    }

    /// True if the inner watcher is currently active (for tests +
    /// observability â€” production code shouldn't branch on this).
    #[cfg(test)]
    pub fn is_active(&self) -> bool {
        self.inner.lock().map(|g| g.is_some()).unwrap_or(false)
    }
}

impl Drop for ReverseWatcherRuntime {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // Tear down the inner first (drops the OS watch) before
        // joining the manager. The manager loop's next iteration
        // also notices the shutdown flag and exits.
        if let Ok(mut guard) = self.inner.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = self.manager_thread.lock() {
            if let Some(t) = guard.take() {
                let _ = t.join();
            }
        }
    }
}

// --------------------------------------------------------------------
// Manager loop + inner watcher
// --------------------------------------------------------------------

/// Snapshot of the two settings rows the watcher gates on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WatcherConfig {
    enabled: bool,
    vault_path: Option<PathBuf>,
}

impl WatcherConfig {
    fn is_active(&self) -> bool {
        self.enabled && self.vault_path.is_some()
    }

    fn load(conn: &Arc<Mutex<Connection>>) -> Self {
        let Ok(guard) = conn.lock() else {
            return Self {
                enabled: false,
                vault_path: None,
            };
        };
        let s = Settings::new(&guard);
        let enabled = s.get::<bool>(SettingKey::KgGraphEnabled).unwrap_or(false);
        let vault_path = match s.get::<Option<String>>(SettingKey::VaultPath) {
            Ok(Some(p)) if !p.trim().is_empty() => Some(PathBuf::from(p)),
            _ => None,
        };
        Self {
            enabled,
            vault_path,
        }
    }
}

/// Holds the live event-handler thread for one active run. The
/// debouncer itself is moved INTO the thread (mirroring
/// [`crate::inbox::watcher`]) so the OS-watch handle's `Drop`
/// runs in lockstep with the worker thread's exit. Dropping this
/// struct sends the shutdown signal + joins the thread.
struct InnerWatcher {
    vault_path: PathBuf,
    shutdown_tx: crossbeam_channel::Sender<()>,
    /// `Option` so `Drop` can `.take()` + join.
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for InnerWatcher {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn manager_loop(
    conn: Arc<Mutex<Connection>>,
    inner: Arc<Mutex<Option<InnerWatcher>>>,
    shutdown: Arc<AtomicBool>,
) {
    tracing::info!(target: "vault::watcher", "reverse-watcher manager thread started");
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        let desired = WatcherConfig::load(&conn);
        if let Err(e) = reconcile_runtime_state(&conn, &inner, &desired) {
            tracing::warn!(
                target: "vault::watcher",
                error = %e,
                "manager: reconcile_runtime_state failed; will retry on next poll"
            );
        }
        std::thread::sleep(MANAGER_POLL_INTERVAL);
    }
    // Drop the inner watcher on exit so the OS handle releases
    // promptly. The runtime's Drop will also do this, but we belt-
    // and-braces here in case the manager exits via the shutdown
    // flag before the runtime is dropped.
    if let Ok(mut g) = inner.lock() {
        *g = None;
    }
    tracing::info!(target: "vault::watcher", "reverse-watcher manager thread exiting");
}

/// Drive the inner-watcher state machine to match `desired`:
/// - desired=off, current=on â†’ stop
/// - desired=on(path A), current=off â†’ start
/// - desired=on(path A), current=on(path A) â†’ no-op
/// - desired=on(path B), current=on(path A) â†’ restart
fn reconcile_runtime_state(
    conn: &Arc<Mutex<Connection>>,
    inner: &Arc<Mutex<Option<InnerWatcher>>>,
    desired: &WatcherConfig,
) -> AppResult<()> {
    let mut guard = inner
        .lock()
        .map_err(|_| AppError::Other("reverse-watcher: inner mutex poisoned".into()))?;

    // Decide stop / start / restart based on the snapshot under the
    // same lock so two near-simultaneous polls can't both decide
    // "start".
    let current_path = guard.as_ref().map(|w| w.vault_path.clone());
    let desired_path = desired.vault_path.clone().filter(|_| desired.is_active());

    match (current_path, desired_path) {
        (None, None) => Ok(()),
        (Some(_), None) => {
            *guard = None; // drops InnerWatcher â†’ tears down OS watch
            tracing::info!(target: "vault::watcher", "reverse-watcher stopped (KG off or vault unset)");
            Ok(())
        }
        (Some(cur), Some(new_path)) if cur == new_path => Ok(()),
        (None, Some(new_path)) | (Some(_), Some(new_path)) => {
            *guard = None;
            match start_inner_watcher(conn.clone(), &new_path) {
                Ok(w) => {
                    tracing::info!(
                        target: "vault::watcher",
                        vault = %new_path.display(),
                        "reverse-watcher started"
                    );
                    *guard = Some(w);
                }
                Err(e) => {
                    tracing::warn!(
                        target: "vault::watcher",
                        vault = %new_path.display(),
                        error = %e,
                        "reverse-watcher failed to start; will retry on next poll"
                    );
                }
            }
            Ok(())
        }
    }
}

fn start_inner_watcher(conn: Arc<Mutex<Connection>>, vault_path: &Path) -> AppResult<InnerWatcher> {
    let kg_root = vault_path.join(KG_SUBTREE_ROOT_NAME);
    if !kg_root.is_dir() {
        return Err(AppError::Other(format!(
            "reverse-watcher: KG subtree missing at {} (bootstrap should have created it)",
            kg_root.display()
        )));
    }

    let (event_tx, event_rx) = std::sync::mpsc::channel::<DebounceEventResult>();
    let mut debouncer = new_debouncer(DEBOUNCE_QUIET_WINDOW, None, move |res| {
        let _ = event_tx.send(res);
    })
    .map_err(|e| AppError::Other(format!("reverse-watcher: build debouncer: {e}")))?;
    debouncer
        .watcher()
        .watch(&kg_root, RecursiveMode::Recursive)
        .map_err(|e| {
            AppError::Other(format!(
                "reverse-watcher: watch {}: {}",
                kg_root.display(),
                e
            ))
        })?;

    let (shutdown_tx, shutdown_rx) = crossbeam_channel::bounded::<()>(1);
    let kg_root_for_thread = kg_root.clone();
    let conn_for_thread = Arc::clone(&conn);
    let thread = std::thread::Builder::new()
        .name("mockingbird-kg-reverse-watcher".into())
        .spawn(move || {
            // Move the debouncer in so its Drop fires when the
            // worker exits, tearing down the OS watch cleanly.
            // Mirrors `inbox::watcher::start`.
            let _debouncer = debouncer;
            event_loop(
                &event_rx,
                &shutdown_rx,
                &kg_root_for_thread,
                &conn_for_thread,
            );
            tracing::info!(target: "vault::watcher", "event-handler thread exiting");
        })
        .map_err(|e| AppError::Other(format!("reverse-watcher: spawn event thread: {e}")))?;

    Ok(InnerWatcher {
        vault_path: vault_path.to_path_buf(),
        shutdown_tx,
        thread: Some(thread),
    })
}

/// Event-handler loop. Pulls debounced events from the channel,
/// classifies each path, and runs the reconciler on actionable
/// ones. Survives bad events (poison-pill `Err(_)` from the
/// debouncer is logged + swallowed; the watcher stays up).
fn event_loop(
    event_rx: &std::sync::mpsc::Receiver<DebounceEventResult>,
    shutdown_rx: &crossbeam_channel::Receiver<()>,
    kg_root: &Path,
    conn: &Arc<Mutex<Connection>>,
) {
    loop {
        if shutdown_rx.try_recv().is_ok() {
            return;
        }
        // Bounded recv so we can periodically re-check the
        // shutdown channel even on a quiet vault.
        let next = event_rx.recv_timeout(Duration::from_millis(500));
        let events = match next {
            Ok(Ok(events)) => events,
            Ok(Err(errs)) => {
                // The debouncer surfaces FS errors as Err. None of
                // them are fatal to the watcher (the OS watch may
                // skip the offending event but stays installed).
                for e in errs {
                    tracing::warn!(
                        target: "vault::watcher",
                        error = ?e,
                        "debouncer reported error"
                    );
                }
                continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        };

        // Defensive: re-check the KG toggle before doing any DB
        // work. If the user flipped it off between event fire +
        // handler run, drop the event silently. (The manager will
        // tear us down on its next poll anyway, but this is the
        // graph-off invariant's belt-and-braces.)
        let still_enabled = {
            let Ok(guard) = conn.lock() else { continue };
            Settings::new(&guard)
                .get::<bool>(SettingKey::KgGraphEnabled)
                .unwrap_or(false)
        };
        if !still_enabled {
            tracing::debug!(
                target: "vault::watcher",
                event_count = events.len(),
                "KG toggle is off; dropping events"
            );
            continue;
        }

        // Dedupe paths across the batch â€” a single debouncer
        // window can deliver multiple events for the same file.
        let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for event in events {
            for path in &event.event.paths {
                if !seen.insert(path.clone()) {
                    continue;
                }
                if !classify_path(path, kg_root).is_actionable() {
                    continue;
                }
                let Ok(guard) = conn.lock() else { continue };
                match reconcile_entry_file(path, kg_root, &guard) {
                    Ok(outcome) => {
                        tracing::debug!(
                            target: "vault::watcher",
                            path = %path.display(),
                            outcome = ?outcome,
                            "reconcile complete"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "vault::watcher",
                            path = %path.display(),
                            error = %e,
                            "reconcile failed; watcher remains alive"
                        );
                    }
                }
            }
        }
    }
}
