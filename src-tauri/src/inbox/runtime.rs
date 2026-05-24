//! Inbox runtime + lifecycle (Wave 3.3) — ADR 0046 Iter 3 / mb-3ivf.
//!
//! Mirrors [`crate::vault::export_job::VaultRuntime`]'s shape: a
//! cheaply-clonable handle stashed in Tauri managed state, gated by
//! the same `MobileSyncEnabled` + `VaultPath` settings that gate the
//! outbound projection. When both are on (and the path validates),
//! we spawn the watcher (Wave 3.1) + courier (Wave 3.2) and wire
//! their crossbeam channel between them.
//!
//! ## Lifecycle transitions
//!
//! Driven by [`InboxRuntime::refresh_config`], which the settings-set
//! IPC calls every time a `MobileSync*` / `Vault*` key changes:
//!
//! | Previous state | New `is_active()` | Action                  |
//! |----------------|-------------------|-------------------------|
//! | Stopped        | false             | (no-op)                 |
//! | Stopped        | true              | start                   |
//! | Running        | false             | stop                    |
//! | Running (path) | true (same path)  | (no-op)                 |
//! | Running (a)    | true (path b)     | stop + start (restart)  |
//!
//! ## Initial scan on start
//!
//! Before spawning the live watcher, [`start`](InboxRuntime::start)
//! walks `<vault>/inbox/` non-recursively into the candidate channel.
//! This closes the gap where the user records a memo while the
//! desktop app is off: the file has been sitting in `inbox/` since
//! before the watcher could see it, so a debouncer subscription
//! alone would never fire. We pre-fill the channel with the same
//! [`StableInboxFile`] events the watcher would have emitted, then
//! start the watcher; the courier processes them serially.
//!
//! ## Why the construction order matters
//!
//! The runtime publishes the channel SENDER first and constructs the
//! receiver before either the watcher or the courier are spawned.
//! That way the initial-scan pre-fill is buffered in the channel
//! waiting for the courier to come online — no events are lost to a
//! "started watching before the courier was reading" race.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use rusqlite::Connection;

use super::courier::{Courier, CourierHandle};
use super::watcher::{InboxWatcher, InboxWatcherHandle, StableInboxFile};
use crate::dictation::ingest_channel::HeadlessIngestSender;
use crate::dictation::ingest_progress::{IngestProgressBus, NoopIngestProgressBus};
use crate::error::{AppError, AppResult};
use crate::settings::{model::SettingKey, Settings};
use crate::vault::layout::VaultLayout;

// --------------------------------------------------------------------
// Config snapshot (lighter than VaultConfig — we only need two keys)
// --------------------------------------------------------------------

/// The settings rows that gate the inbox subsystem.
///
/// Kept as its own struct (rather than reusing `vault::VaultConfig`)
/// so inbox-specific knobs (`KeepAudioBlobs`, future `InboxDebounceMs`
/// …) can land here without dragging the export job into a config
/// it doesn't care about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxConfig {
    /// Mirror of `MobileSyncEnabled`. False → runtime stays Stopped.
    pub enabled: bool,
    /// Mirror of `VaultPath`. None → runtime stays Stopped even if
    /// `enabled` is true (the user toggled sync on before picking
    /// a directory).
    pub vault_path: Option<PathBuf>,
    /// ADR 0046 Iter 4 — mirror of `KeepAudioBlobs`. When false,
    /// the courier DELETES the source audio after a successful
    /// ingest instead of moving it to `_archive/`. Hot-reload is
    /// achieved by restarting the courier on change (handled by
    /// `refresh_config`).
    pub keep_audio_blobs: bool,
}

impl InboxConfig {
    /// True when the runtime should be Running.
    pub fn is_active(&self) -> bool {
        self.enabled && self.vault_path.is_some()
    }

    /// Read both keys from the settings DB. Per-key read failures
    /// fall back to defaults — never crash on a single bad row.
    pub fn load(db: &Arc<Mutex<Connection>>) -> AppResult<Self> {
        let conn = db
            .lock()
            .map_err(|_| AppError::Other("inbox: db mutex poisoned".into()))?;
        let s = Settings::new(&conn);
        let enabled = s
            .get::<bool>(SettingKey::MobileSyncEnabled)
            .unwrap_or(false);
        let vault_path = match s.get::<Option<String>>(SettingKey::VaultPath) {
            Ok(Some(p)) if !p.trim().is_empty() => Some(PathBuf::from(p)),
            _ => None,
        };
        // Default to true (matches `SettingKey::KeepAudioBlobs`
        // default_value()) so an existing user whose DB hasn't yet
        // seen a write of this key gets the safer keep-the-audio
        // behaviour.
        let keep_audio_blobs = s.get::<bool>(SettingKey::KeepAudioBlobs).unwrap_or(true);
        Ok(Self {
            enabled,
            vault_path,
            keep_audio_blobs,
        })
    }
}

// --------------------------------------------------------------------
// Runtime state machine
// --------------------------------------------------------------------

/// Internal state — either the watcher + courier are running together,
/// or neither is.
enum InboxState {
    Stopped,
    Running {
        /// Path the currently-running pair is watching. Stored so
        /// `refresh_config` can detect a path-change while running
        /// and trigger a restart.
        vault_path: PathBuf,
        /// `KeepAudioBlobs` value the running courier was started
        /// with. Stored so `refresh_config` can detect a toggle
        /// (without path change) and restart the courier.
        keep_audio_blobs: bool,
        watcher: Option<InboxWatcherHandle>,
        courier: Option<CourierHandle>,
    },
}

/// The runtime itself. [`Arc::clone`] is cheap (every field is
/// already shared); Tauri's `app.manage(Arc::clone(&runtime))` is
/// the same pattern as [`crate::vault::export_job::VaultRuntime`].
pub struct InboxRuntime {
    config: Arc<RwLock<InboxConfig>>,
    state: Arc<Mutex<InboxState>>,
    headless_ingest_tx: HeadlessIngestSender,
    /// Shared progress bus the spawned courier emits through (ADR
    /// 0046 Iter 4 / mb-q1xt). Stored on the runtime rather than
    /// per-courier so the same `AppIngestProgressBus` instance
    /// survives stop/restart cycles -- avoiding a window where
    /// emits silently drop because the bus was being reconstructed.
    progress: Arc<dyn IngestProgressBus>,
}

impl InboxRuntime {
    /// Construct a fresh runtime in the [`InboxState::Stopped`]
    /// state. Call [`refresh_config`](Self::refresh_config) right
    /// after to read settings + start if appropriate.
    pub fn new(headless_ingest_tx: HeadlessIngestSender) -> Self {
        Self::new_with_progress(headless_ingest_tx, Arc::new(NoopIngestProgressBus))
    }

    /// Construct with a custom progress bus. Used by `lib.rs::run`
    /// to plug in the shared [`crate::dictation::ingest_progress::AppIngestProgressBus`]
    /// so mobile-inbox arrivals light up the desktop progress overlay
    /// alongside the IPC-driven `+ Audio file` path.
    pub fn new_with_progress(
        headless_ingest_tx: HeadlessIngestSender,
        progress: Arc<dyn IngestProgressBus>,
    ) -> Self {
        Self {
            config: Arc::new(RwLock::new(InboxConfig::default())),
            state: Arc::new(Mutex::new(InboxState::Stopped)),
            headless_ingest_tx,
            progress,
        }
    }

    /// Read-only snapshot of the current config — handy for the
    /// Settings UI "status" probe + tests.
    pub fn current_config(&self) -> InboxConfig {
        self.config.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Re-read settings + transition state to match.
    ///
    /// Called from `vault_settings_set` after every write so the
    /// runtime reflects the new config in the same IPC tick the
    /// user clicked Save. Idempotent — a no-change config is a
    /// no-op.
    pub fn refresh_config(&self, db: &Arc<Mutex<Connection>>) -> AppResult<()> {
        let new_cfg = InboxConfig::load(db)?;

        // Snapshot the previous active-path AND the previously-applied
        // KeepAudioBlobs value so we can detect when a config-only
        // toggle (no path change) requires a courier restart.
        let (prev_active_path, prev_keep) = match self.state.lock() {
            Ok(g) => match &*g {
                InboxState::Running {
                    vault_path,
                    keep_audio_blobs,
                    ..
                } => (Some(vault_path.clone()), Some(*keep_audio_blobs)),
                InboxState::Stopped => (None, None),
            },
            Err(_) => (None, None),
        };

        // Write the new config first so any concurrent
        // `current_config` reader sees the freshest snapshot even
        // before we finish the state transition.
        {
            let mut cfg_guard = self
                .config
                .write()
                .map_err(|_| AppError::Other("inbox: config rwlock poisoned".into()))?;
            *cfg_guard = new_cfg.clone();
        }

        let now_active = new_cfg.is_active();
        let now_path = new_cfg.vault_path.clone();

        let keep = new_cfg.keep_audio_blobs;
        match (prev_active_path, now_active, now_path) {
            (None, false, _) => Ok(()),
            (None, true, Some(p)) => self.start(&p, keep),
            (Some(_), false, _) => self.stop(),
            (Some(prev), true, Some(new_p))
                if prev == new_p && prev_keep.map(|k| k == keep).unwrap_or(false) =>
            {
                Ok(())
            }
            (Some(_), true, Some(new_p)) => {
                // Path or KeepAudioBlobs changed under us — full
                // restart so the courier picks up the new value.
                self.stop()?;
                self.start(&new_p, keep)
            }
            // (Some(_), true, None) is impossible because is_active()
            // returning true requires vault_path = Some(_) — but match
            // completeness wants it. Treat as "new config is busted,
            // stop" which is the safest non-action.
            (Some(_), true, None) => self.stop(),
            // Same unreachable, mirror handler.
            (None, true, None) => Ok(()),
        }
    }

    /// Start the watcher + courier pair against `vault_path`.
    ///
    /// Validates the vault layout (creates `inbox/` + `_failed/` if
    /// missing), constructs the watcher→courier channel, pre-fills
    /// it with the initial scan, then spawns the two workers.
    fn start(&self, vault_path: &std::path::Path, keep_audio_blobs: bool) -> AppResult<()> {
        // Ensure zones exist (idempotent — the export job does this
        // too, but the inbox runtime can outlive a `_failed/` rmdir
        // by the user, so re-creating on every start keeps us
        // robust to that).
        let layout = VaultLayout::new(vault_path);
        layout.ensure_zones()?;
        let inbox_path = layout.inbox();

        // Build the watcher → courier channel. Unbounded because
        // (a) the watcher emits at human-input rate (one Voice
        // Memo per courier flow) and (b) the courier is the only
        // consumer.
        let (file_tx, file_rx) = crossbeam_channel::unbounded::<StableInboxFile>();

        // Pre-fill with the initial scan BEFORE spawning either
        // worker. Anything sitting in `inbox/` from a prior
        // off-app period gets queued for the courier to handle in
        // FIFO order alongside live watcher events.
        let prefill = initial_scan(&inbox_path);
        let prefill_count = prefill.len();
        for stable in prefill {
            // The channel is unbounded so send never blocks.
            let _ = file_tx.send(stable);
        }
        if prefill_count > 0 {
            tracing::info!(
                target: "inbox::runtime",
                count = prefill_count,
                "initial scan queued pre-existing files for ingest"
            );
        }

        // Spawn the courier FIRST so it's already reading by the
        // time the watcher publishes its first live event.
        let courier = Courier::new(
            inbox_path.clone(),
            file_rx,
            self.headless_ingest_tx.clone(),
            keep_audio_blobs,
            Arc::clone(&self.progress),
        );
        let courier_handle = courier.start()?;

        let watcher = InboxWatcher::new(inbox_path.clone(), file_tx);
        let watcher_handle = watcher.start()?;

        let mut state_guard = self
            .state
            .lock()
            .map_err(|_| AppError::Other("inbox: state mutex poisoned".into()))?;
        *state_guard = InboxState::Running {
            vault_path: vault_path.to_path_buf(),
            keep_audio_blobs,
            watcher: Some(watcher_handle),
            courier: Some(courier_handle),
        };
        tracing::info!(
            target: "inbox::runtime",
            vault = %vault_path.display(),
            "inbox runtime started"
        );
        Ok(())
    }

    /// Stop both workers cleanly. Idempotent — calling on an
    /// already-Stopped runtime returns Ok.
    fn stop(&self) -> AppResult<()> {
        let mut state_guard = self
            .state
            .lock()
            .map_err(|_| AppError::Other("inbox: state mutex poisoned".into()))?;
        // Pull handles out by swapping in a Stopped sentinel; we
        // drop the lock before joining the threads so the threads
        // can't deadlock waiting on something we own.
        let prev = std::mem::replace(&mut *state_guard, InboxState::Stopped);
        drop(state_guard);

        if let InboxState::Running {
            watcher, courier, ..
        } = prev
        {
            // Stop the watcher first so no new events arrive while
            // the courier is draining.
            if let Some(w) = watcher {
                if let Err(e) = w.stop() {
                    tracing::warn!(
                        target: "inbox::runtime",
                        error = ?e,
                        "watcher stop failed"
                    );
                }
            }
            if let Some(c) = courier {
                if let Err(e) = c.stop() {
                    tracing::warn!(
                        target: "inbox::runtime",
                        error = ?e,
                        "courier stop failed"
                    );
                }
            }
            tracing::info!(target: "inbox::runtime", "inbox runtime stopped");
        }
        Ok(())
    }
}

impl Drop for InboxRuntime {
    /// Defensive: Tauri drops managed state on shutdown, which
    /// drops the `Arc<InboxRuntime>`. If we're the last Arc the
    /// state needs to stop cleanly so the watcher's OS handle is
    /// released. `stop()` is idempotent, so a double-drop (from
    /// the InboxState handles' own Drop impls) is harmless.
    fn drop(&mut self) {
        if let Err(e) = self.stop() {
            tracing::warn!(
                target: "inbox::runtime",
                error = ?e,
                "stop on drop failed"
            );
        }
    }
}

// --------------------------------------------------------------------
// Initial scan
// --------------------------------------------------------------------

/// Walk the inbox directory's IMMEDIATE children (not recursive) for
/// audio files already on disk. Returns them as synthetic
/// [`StableInboxFile`] events ready to be pushed into the
/// watcher→courier channel.
///
/// Recursive walk is intentionally avoided: the only non-immediate
/// subdirectories under `inbox/` are our own `_archive/`, `_failed/`,
/// and `_keep/` zones, plus possibly `.obsidian/` artefacts the
/// vault drops randomly. Recursing would either re-process
/// archived files or churn on Obsidian's `workspace.json`.
fn initial_scan(inbox_path: &std::path::Path) -> Vec<StableInboxFile> {
    use std::time::SystemTime;

    let read_dir = match std::fs::read_dir(inbox_path) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                target: "inbox::runtime",
                path = %inbox_path.display(),
                error = %e,
                "initial_scan: cannot read inbox dir; skipping"
            );
            return Vec::new();
        }
    };

    let mut out = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        // Apply the same path filter the watcher uses so we don't
        // accidentally pick up `_archive/` or `_failed/` entries.
        if !super::watcher::should_consider_path(&path) {
            continue;
        }
        // We only want regular files; skip directories.
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let size = match entry.metadata() {
            Ok(m) => m.len(),
            Err(_) => continue,
        };
        if size == 0 {
            // Same gate the watcher's stability check applies —
            // never enqueue a zero-byte file.
            continue;
        }
        out.push(StableInboxFile {
            path,
            size,
            observed_at: SystemTime::now(),
        });
    }
    out
}

// --------------------------------------------------------------------
// Tests
// --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_is_active_requires_both_enabled_and_path() {
        let off = InboxConfig {
            enabled: false,
            vault_path: Some(PathBuf::from("/v")),
            keep_audio_blobs: true,
        };
        let no_path = InboxConfig {
            enabled: true,
            vault_path: None,
            keep_audio_blobs: true,
        };
        let both = InboxConfig {
            enabled: true,
            vault_path: Some(PathBuf::from("/v")),
            keep_audio_blobs: true,
        };
        assert!(!off.is_active());
        assert!(!no_path.is_active());
        assert!(both.is_active());
    }

    #[test]
    fn initial_scan_picks_up_allowlisted_files() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = tmp.path().join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        // Three audio files + one noise file.
        std::fs::write(inbox.join("a.m4a"), b"audio-bytes").unwrap();
        std::fs::write(inbox.join("b.wav"), b"audio-bytes").unwrap();
        std::fs::write(inbox.join("notes.txt"), b"not audio").unwrap();
        // Zero-byte file — should NOT come back.
        std::fs::write(inbox.join("empty.m4a"), b"").unwrap();

        let scanned = initial_scan(&inbox);
        let names: Vec<String> = scanned
            .iter()
            .filter_map(|s| s.path.file_name()?.to_str().map(|s| s.to_string()))
            .collect();
        assert_eq!(names.len(), 2, "got: {names:?}");
        assert!(names.contains(&"a.m4a".to_string()));
        assert!(names.contains(&"b.wav".to_string()));
    }

    #[test]
    fn initial_scan_skips_archive_and_failed_subdirs() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = tmp.path().join("inbox");
        std::fs::create_dir_all(inbox.join("_archive/2026-05-27")).unwrap();
        std::fs::create_dir_all(inbox.join("_failed")).unwrap();
        std::fs::write(
            inbox.join("_archive/2026-05-27/old.m4a"),
            b"already processed",
        )
        .unwrap();
        std::fs::write(inbox.join("_failed/broken.m4a"), b"quarantined").unwrap();
        std::fs::write(inbox.join("fresh.m4a"), b"new").unwrap();

        let scanned = initial_scan(&inbox);
        // Non-recursive scan would naturally skip the subdir
        // contents, but the file `_archive/` itself as a directory
        // also shouldn't appear. Verify by name.
        assert_eq!(scanned.len(), 1);
        assert_eq!(
            scanned[0].path.file_name().and_then(|s| s.to_str()),
            Some("fresh.m4a")
        );
    }

    #[test]
    fn initial_scan_returns_empty_for_missing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let scanned = initial_scan(&tmp.path().join("does-not-exist"));
        assert!(scanned.is_empty());
    }
}
