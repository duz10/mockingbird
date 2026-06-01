//! KG-Inbox runtime + lifecycle (Phase 1E Wave 1E.6 / `mb-i46v`,
//! ADR 0053 Section "KG-Inbox courier").
//!
//! Sibling of [`crate::inbox::runtime::InboxRuntime`]. Owns the
//! watcher + courier pair that observe
//! `<vault>/Knowledge Graph/Inbox/` for audio dropped via the iOS
//! Shortcut OR a desktop drag-and-drop.
//!
//! ## Gating
//!
//! Two settings keys jointly gate this runtime, distinct from the
//! ADR 0046 inbox:
//!
//! - `KgGraphEnabled` -- the KG master switch. False -> Stopped.
//! - `VaultPath` -- where to find the `Knowledge Graph/Inbox/` dir.
//!   None / empty -> Stopped (the user enabled KG but hasn't picked
//!   a vault yet).
//!
//! NOT `MobileSyncEnabled` -- that key gates the ADR 0046 inbox
//! specifically (the iOS Shortcut drop into `<vault>/inbox/`). The
//! KG-Inbox is a function of \"is the KG turned on?\" rather than
//! \"is mobile sync turned on?\" -- a user might use the KG with
//! desktop drag-and-drop only and never wire up the iOS Shortcut.
//!
//! ## Lifecycle
//!
//! Driven by [`KgInboxRuntime::refresh_config`], which the
//! `kg_settings_set` IPC AND the `vault_settings_set` IPC both call
//! after every relevant write. Transitions match
//! `InboxRuntime::refresh_config`:
//!
//! | Previous       | New `is_active()`   | Action                  |
//! |----------------|---------------------|-------------------------|
//! | Stopped        | false               | (no-op)                 |
//! | Stopped        | true                | start                   |
//! | Running        | false               | stop                    |
//! | Running (path) | true (same path)    | (no-op)                 |
//! | Running (a)    | true (path b)       | stop + start (restart)  |
//!
//! ## Initial scan
//!
//! Before spawning the live watcher, [`KgInboxRuntime::start`]
//! walks `Knowledge Graph/Inbox/` non-recursively and pre-fills the
//! channel with any pre-existing audio files. Same rationale as
//! the ADR 0046 inbox: closes the off-app window. The KG-Inbox
//! courier's idempotency probe ([`super::kg_inbox_courier::process_one`]'s
//! `already_ingested` step) skips files whose session row already
//! exists, so a re-emit on app restart doesn't duplicate.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use rusqlite::Connection;

use super::kg_inbox_courier::{KgInboxCourier, KgInboxCourierHandle};
use super::kg_layout::{bootstrap_kg_subtree, kg_subtree_paths};
use crate::dictation::ingest_channel::HeadlessIngestSender;
use crate::dictation::ingest_progress::{IngestProgressBus, NoopIngestProgressBus};
use crate::error::{AppError, AppResult};
use crate::inbox::watcher::{
    should_consider_path, InboxWatcher, InboxWatcherHandle, StableInboxFile,
};
use crate::settings::{model::SettingKey, Settings};

// --------------------------------------------------------------------
// Config snapshot
// --------------------------------------------------------------------

/// Settings rows that gate the KG-Inbox subsystem.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KgInboxConfig {
    /// Mirror of `KgGraphEnabled`. False -> runtime stays Stopped.
    pub enabled: bool,
    /// Mirror of `VaultPath`. None -> runtime stays Stopped.
    pub vault_path: Option<PathBuf>,
}

impl KgInboxConfig {
    /// True when the runtime should be Running.
    pub fn is_active(&self) -> bool {
        self.enabled && self.vault_path.is_some()
    }

    /// Read both keys from the settings DB. Per-key read failures
    /// fall back to defaults -- never crash on a single bad row.
    pub fn load(db: &Arc<Mutex<Connection>>) -> AppResult<Self> {
        let conn = db
            .lock()
            .map_err(|_| AppError::Other("kg-inbox: db mutex poisoned".into()))?;
        let s = Settings::new(&conn);
        let enabled = s.get::<bool>(SettingKey::KgGraphEnabled).unwrap_or(false);
        let vault_path = match s.get::<Option<String>>(SettingKey::VaultPath) {
            Ok(Some(p)) if !p.trim().is_empty() => Some(PathBuf::from(p)),
            _ => None,
        };
        Ok(Self {
            enabled,
            vault_path,
        })
    }
}

// --------------------------------------------------------------------
// Runtime state machine
// --------------------------------------------------------------------

enum KgInboxState {
    Stopped,
    Running {
        vault_path: PathBuf,
        watcher: Option<InboxWatcherHandle>,
        courier: Option<KgInboxCourierHandle>,
    },
}

/// The KG-Inbox runtime. Cheaply clonable handle stashed in Tauri
/// managed state; mirror of [`crate::inbox::runtime::InboxRuntime`].
pub struct KgInboxRuntime {
    config: Arc<RwLock<KgInboxConfig>>,
    state: Arc<Mutex<KgInboxState>>,
    headless_ingest_tx: HeadlessIngestSender,
    /// Shared DB handle. Threaded into the courier so its
    /// idempotency probe can query `sessions.audio_blob_path`.
    db: Arc<Mutex<Connection>>,
    progress: Arc<dyn IngestProgressBus>,
}

impl KgInboxRuntime {
    /// Construct a fresh runtime in [`KgInboxState::Stopped`]. Call
    /// [`Self::refresh_config`] to load settings + start.
    pub fn new(headless_ingest_tx: HeadlessIngestSender, db: Arc<Mutex<Connection>>) -> Self {
        Self::new_with_progress(headless_ingest_tx, db, Arc::new(NoopIngestProgressBus))
    }

    /// Construct with a custom progress bus. Used by `lib.rs::run`
    /// to plug in the shared `AppIngestProgressBus`.
    pub fn new_with_progress(
        headless_ingest_tx: HeadlessIngestSender,
        db: Arc<Mutex<Connection>>,
        progress: Arc<dyn IngestProgressBus>,
    ) -> Self {
        Self {
            config: Arc::new(RwLock::new(KgInboxConfig::default())),
            state: Arc::new(Mutex::new(KgInboxState::Stopped)),
            headless_ingest_tx,
            db,
            progress,
        }
    }

    /// Read-only snapshot of the current config -- handy for the
    /// Settings UI status probe + tests.
    pub fn current_config(&self) -> KgInboxConfig {
        self.config.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// True when the watcher + courier pair are running. Mirror of
    /// [`crate::inbox::runtime::InboxRuntime`]'s implicit
    /// observable state (the IPC layer reads this for the Settings
    /// UI status row).
    pub fn is_running(&self) -> bool {
        matches!(
            self.state.lock().as_deref(),
            Ok(KgInboxState::Running { .. })
        )
    }

    /// Re-read settings + transition state to match. Called from
    /// `kg_settings_set` AND `vault_settings_set` after every
    /// write. Idempotent.
    pub fn refresh_config(&self, db: &Arc<Mutex<Connection>>) -> AppResult<()> {
        let new_cfg = KgInboxConfig::load(db)?;

        let prev_active_path = match self.state.lock() {
            Ok(g) => match &*g {
                KgInboxState::Running { vault_path, .. } => Some(vault_path.clone()),
                KgInboxState::Stopped => None,
            },
            Err(_) => None,
        };

        // Write the new config first so concurrent readers see the
        // freshest snapshot even before the state transition lands.
        {
            let mut cfg_guard = self
                .config
                .write()
                .map_err(|_| AppError::Other("kg-inbox: config rwlock poisoned".into()))?;
            *cfg_guard = new_cfg.clone();
        }

        let now_active = new_cfg.is_active();
        let now_path = new_cfg.vault_path.clone();

        match (prev_active_path, now_active, now_path) {
            (None, false, _) => Ok(()),
            (None, true, Some(p)) => self.start(&p),
            (Some(_), false, _) => self.stop(),
            (Some(prev), true, Some(new_p)) if prev == new_p => Ok(()),
            (Some(_), true, Some(new_p)) => {
                self.stop()?;
                self.start(&new_p)
            }
            (Some(_), true, None) => self.stop(),
            (None, true, None) => Ok(()),
        }
    }

    /// Start the watcher + courier pair against
    /// `<vault_path>/Knowledge Graph/Inbox/`.
    fn start(&self, vault_path: &Path) -> AppResult<()> {
        // Bootstrap the KG subtree first -- the watcher errors out
        // if the inbox directory doesn't exist, and we'd rather
        // create the subtree on KG activation than depend on a
        // separate IPC call ordering. `bootstrap_kg_subtree` is
        // idempotent (Cell A/B/C from ADR 0053 D1).
        bootstrap_kg_subtree(vault_path)?;
        let kg_paths = kg_subtree_paths(vault_path);
        let kg_inbox_path = kg_paths.inbox;

        // Build the watcher -> courier channel BEFORE spawning
        // either worker, so the initial scan pre-fill is buffered
        // and the courier sees it as soon as it comes online.
        let (file_tx, file_rx) = crossbeam_channel::unbounded::<StableInboxFile>();

        let prefill = initial_scan(&kg_inbox_path);
        let prefill_count = prefill.len();
        for stable in prefill {
            let _ = file_tx.send(stable);
        }
        if prefill_count > 0 {
            tracing::info!(
                target: "kg_inbox::runtime",
                count = prefill_count,
                "initial scan queued pre-existing KG-Inbox files for ingest"
            );
        }

        // Spawn the courier first so it's reading by the time the
        // watcher publishes a live event.
        let courier = KgInboxCourier::new(
            kg_inbox_path.clone(),
            file_rx,
            self.headless_ingest_tx.clone(),
            Arc::clone(&self.db),
            Arc::clone(&self.progress),
        );
        let courier_handle = courier.start()?;

        let watcher = InboxWatcher::new(kg_inbox_path.clone(), file_tx);
        let watcher_handle = watcher.start()?;

        let mut state_guard = self
            .state
            .lock()
            .map_err(|_| AppError::Other("kg-inbox: state mutex poisoned".into()))?;
        *state_guard = KgInboxState::Running {
            vault_path: vault_path.to_path_buf(),
            watcher: Some(watcher_handle),
            courier: Some(courier_handle),
        };
        tracing::info!(
            target: "kg_inbox::runtime",
            vault = %vault_path.display(),
            kg_inbox = %kg_inbox_path.display(),
            "KG-Inbox runtime started"
        );
        Ok(())
    }

    /// Stop both workers cleanly. Idempotent.
    fn stop(&self) -> AppResult<()> {
        let mut state_guard = self
            .state
            .lock()
            .map_err(|_| AppError::Other("kg-inbox: state mutex poisoned".into()))?;
        let prev = std::mem::replace(&mut *state_guard, KgInboxState::Stopped);
        drop(state_guard);

        if let KgInboxState::Running {
            watcher, courier, ..
        } = prev
        {
            // Stop the watcher first so no new events arrive
            // while the courier drains.
            if let Some(w) = watcher {
                if let Err(e) = w.stop() {
                    tracing::warn!(
                        target: "kg_inbox::runtime",
                        error = ?e,
                        "watcher stop failed"
                    );
                }
            }
            if let Some(c) = courier {
                if let Err(e) = c.stop() {
                    tracing::warn!(
                        target: "kg_inbox::runtime",
                        error = ?e,
                        "courier stop failed"
                    );
                }
            }
            tracing::info!(target: "kg_inbox::runtime", "KG-Inbox runtime stopped");
        }
        Ok(())
    }
}

impl Drop for KgInboxRuntime {
    fn drop(&mut self) {
        if let Err(e) = self.stop() {
            tracing::warn!(
                target: "kg_inbox::runtime",
                error = ?e,
                "stop on drop failed"
            );
        }
    }
}

// --------------------------------------------------------------------
// Initial scan
// --------------------------------------------------------------------

/// Walk the KG-Inbox directory's IMMEDIATE children (non-recursive)
/// for audio files already on disk. Returns synthetic
/// [`StableInboxFile`] events ready to be pushed into the
/// watcher -> courier channel.
///
/// Non-recursive on purpose: the only valid subdirectory under
/// `Knowledge Graph/Inbox/` is `_failed/`, which we deliberately
/// don't re-pick-up. The watcher's
/// [`should_consider_path`] filter excludes it regardless, but the
/// initial scan short-circuits the check by simply not descending.
fn initial_scan(kg_inbox_path: &Path) -> Vec<StableInboxFile> {
    use std::time::SystemTime;

    let read_dir = match std::fs::read_dir(kg_inbox_path) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                target: "kg_inbox::runtime",
                path = %kg_inbox_path.display(),
                error = %e,
                "initial_scan: cannot read KG-Inbox dir; skipping"
            );
            return Vec::new();
        }
    };

    let mut out = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        if !should_consider_path(&path) {
            continue;
        }
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let size = match entry.metadata() {
            Ok(m) => m.len(),
            Err(_) => continue,
        };
        if size == 0 {
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
        let off = KgInboxConfig {
            enabled: false,
            vault_path: Some(PathBuf::from("/v")),
        };
        let no_path = KgInboxConfig {
            enabled: true,
            vault_path: None,
        };
        let both = KgInboxConfig {
            enabled: true,
            vault_path: Some(PathBuf::from("/v")),
        };
        assert!(!off.is_active());
        assert!(!no_path.is_active());
        assert!(both.is_active());
    }

    #[test]
    fn initial_scan_picks_up_allowlisted_files() {
        let tmp = tempfile::tempdir().unwrap();
        let kg_inbox = tmp.path().join("Knowledge Graph").join("Inbox");
        std::fs::create_dir_all(&kg_inbox).unwrap();
        std::fs::write(kg_inbox.join("a.m4a"), b"audio-bytes").unwrap();
        std::fs::write(kg_inbox.join("b.wav"), b"audio-bytes").unwrap();
        std::fs::write(kg_inbox.join("c.mp3"), b"audio-bytes").unwrap();
        std::fs::write(kg_inbox.join("notes.txt"), b"not audio").unwrap();
        std::fs::write(kg_inbox.join("empty.m4a"), b"").unwrap();

        let scanned = initial_scan(&kg_inbox);
        let names: Vec<String> = scanned
            .iter()
            .filter_map(|s| s.path.file_name()?.to_str().map(|s| s.to_string()))
            .collect();
        assert_eq!(names.len(), 3, "got: {names:?}");
        assert!(names.contains(&"a.m4a".to_string()));
        assert!(names.contains(&"b.wav".to_string()));
        assert!(names.contains(&"c.mp3".to_string()));
    }

    #[test]
    fn initial_scan_skips_failed_subdir() {
        let tmp = tempfile::tempdir().unwrap();
        let kg_inbox = tmp.path().join("Knowledge Graph").join("Inbox");
        std::fs::create_dir_all(kg_inbox.join("_failed")).unwrap();
        std::fs::write(kg_inbox.join("_failed/broken.m4a"), b"quarantined").unwrap();
        std::fs::write(kg_inbox.join("fresh.m4a"), b"new").unwrap();

        let scanned = initial_scan(&kg_inbox);
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
