//! Vault-related Tauri commands -- ADR 0046 Iter 2 / mb-lvzw.
//!
//! Two surfaces:
//!
//! - **Settings IPC** (`vault_settings_get` / `vault_settings_set`) --
//!   typed get/set for the four `Vault*` / `MobileSync*` keys. Same
//!   shape as `meeting_settings_*`; lives here rather than in
//!   `settings.rs` so the vault subsystem owns its own UI surface
//!   end-to-end. Every `set` ALSO refreshes the runtime config and
//!   fires a backfill trigger so the user's settings flip takes
//!   effect immediately.
//!
//! - **Manual reconciliation** (`vault_export_now`) -- synchronous
//!   one-shot pass for the Settings UI "Export now" button. Returns
//!   a small summary the toast renders verbatim.
//!
//! All commands honor the runtime's coalesce / blocking-lock
//! semantics so spamming the button can't pile up worker threads or
//! corrupt the manifest.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime, State};
use tauri_plugin_dialog::DialogExt;

use crate::commands::{into_err, lock_db, AppStateHandle};
use crate::inbox::runtime::InboxRuntime;
use crate::settings::{model::SettingKey, Settings};
use crate::vault::export_job::{ReconciliationSummary, VaultRuntime};
use crate::vault::kg_inbox_runtime::KgInboxRuntime;
use crate::vault::layout::{detect_nested_vault, suggest_sibling_vault, VaultLayout};

// --------------------------------------------------------------------
// Settings get/set
// --------------------------------------------------------------------

/// Snapshot of every UI-visible vault / mobile-sync setting.
///
/// Lowercase JSON field names (camelCase via `rename_all`) align with
/// the rest of the typed settings IPC contracts. The Iter-4 polish
/// surfaces four additional keys on the same snapshot so the new
/// Mobile Sync settings tab can render and persist them via the
/// already-allowlisted `vault_settings_set` IPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSettingsSnapshot {
    pub mobile_sync_enabled: bool,
    pub vault_path: Option<String>,
    pub vault_sync_record_types: String,
    pub vault_retention_days: i64,
    // ADR 0046 Iter 4 — surfaced via the Mobile Sync tab.
    /// One of: `"obsidian-sync-standard"`, `"obsidian-sync-plus"`,
    /// `"manual"`. Drives the default byte-cap warning + iOS
    /// Shortcut tier hints.
    pub vault_sync_backend: String,
    /// Per-file size warning threshold in bytes; default matches
    /// the selected backend (5 MB for Standard, 200 MB for Plus,
    /// effectively no cap for Manual).
    pub sync_tier_byte_cap: i64,
    /// When true, the inbox courier moves processed files to the
    /// `_archive/` zone instead of deleting them. Default ON so
    /// users can re-transcribe after a model upgrade.
    pub keep_audio_blobs: bool,
    /// Developer-only retention of intermediate courier files
    /// (`_keep/` zone). OFF in normal use.
    pub vault_debug_keep_couriers: bool,
}

#[tauri::command]
pub fn vault_settings_get(db: State<'_, AppStateHandle>) -> Result<VaultSettingsSnapshot, String> {
    let conn = lock_db(&db)?;
    let s = Settings::new(&conn);
    Ok(VaultSettingsSnapshot {
        mobile_sync_enabled: s
            .get::<bool>(SettingKey::MobileSyncEnabled)
            .map_err(into_err)?,
        vault_path: s
            .get::<Option<String>>(SettingKey::VaultPath)
            .map_err(into_err)?,
        vault_sync_record_types: s
            .get::<String>(SettingKey::VaultSyncRecordTypes)
            .map_err(into_err)?,
        vault_retention_days: s
            .get::<i64>(SettingKey::VaultRetentionDays)
            .map_err(into_err)?,
        vault_sync_backend: s
            .get::<String>(SettingKey::VaultSyncBackend)
            .map_err(into_err)?,
        sync_tier_byte_cap: s
            .get::<i64>(SettingKey::SyncTierByteCap)
            .map_err(into_err)?,
        keep_audio_blobs: s
            .get::<bool>(SettingKey::KeepAudioBlobs)
            .map_err(into_err)?,
        vault_debug_keep_couriers: s
            .get::<bool>(SettingKey::VaultDebugKeepCouriers)
            .map_err(into_err)?,
    })
}

/// Write one of the four vault settings keys + refresh runtime + fire
/// a backfill trigger. The trigger is gated by the new config: if
/// `MobileSyncEnabled` is still false or the path is unset, the
/// trigger is a no-op (same gate as the post-commit hooks).
#[tauri::command]
pub fn vault_settings_set(
    db: State<'_, AppStateHandle>,
    vault: State<'_, Arc<VaultRuntime>>,
    inbox: State<'_, Arc<InboxRuntime>>,
    kg_inbox: State<'_, Arc<KgInboxRuntime>>,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    let setting_key = SettingKey::try_parse(&key).map_err(into_err)?;
    if !is_vault_setting_allowed_for_ui(setting_key) {
        return Err(format!(
            "setting key {key:?} cannot be written via vault_settings_set"
        ));
    }
    // Drop the lock before refreshing the runtime config (which
    // re-acquires it). Avoids a self-deadlock and keeps the
    // critical section minimal.
    {
        let conn = lock_db(&db)?;
        Settings::new(&conn)
            .set_raw(setting_key, &value)
            .map_err(into_err)?;
    }
    let db_arc = Arc::clone(&db.inner().db);
    vault.refresh_config(&db_arc).map_err(into_err)?;
    // ADR 0046 Iter 3 / mb-3ivf — the inbox runtime is gated by the
    // SAME MobileSyncEnabled + VaultPath keys as the outbound vault
    // projection, so it needs the same refresh on every settings
    // write. start/stop transitions happen inside refresh_config.
    inbox.refresh_config(&db_arc).map_err(into_err)?;
    // Phase 1E Wave 1E.6 (`mb-i46v`) -- the KG-Inbox runtime is
    // gated by `KgGraphEnabled` + `VaultPath`. Writes that flip
    // `VaultPath` (or any other vault key the user happens to
    // change before flipping `KgGraphEnabled`) need to thread
    // through here too so a path change starts/restarts the
    // KG-Inbox watcher in lockstep with the ADR 0046 inbox.
    kg_inbox.refresh_config(&db_arc).map_err(into_err)?;
    // Fire-and-forget backfill so a freshly-enabled vault populates
    // without the user needing to click "Export now". Coalesces if
    // a worker is already running.
    vault.trigger(db_arc);
    Ok(())
}

fn is_vault_setting_allowed_for_ui(k: SettingKey) -> bool {
    matches!(
        k,
        SettingKey::MobileSyncEnabled
            | SettingKey::VaultPath
            | SettingKey::VaultSyncRecordTypes
            | SettingKey::VaultRetentionDays
            | SettingKey::VaultSyncBackend
            | SettingKey::SyncTierByteCap
            | SettingKey::KeepAudioBlobs
            | SettingKey::VaultDebugKeepCouriers
    )
}

// --------------------------------------------------------------------
// Manual export trigger
// --------------------------------------------------------------------

/// What the "Export now" button sees back. Same shape as
/// [`ReconciliationSummary`] but with camelCase field names for the
/// React side. Skipped runs surface as a single boolean rather than
/// pretending zero records were exported, so the UI can render
/// "Sync is disabled" vs. "No new records" distinctly.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultExportSummary {
    pub total: usize,
    pub changes: usize,
    pub archived: usize,
    pub skipped: bool,
}

impl From<ReconciliationSummary> for VaultExportSummary {
    fn from(s: ReconciliationSummary) -> Self {
        Self {
            total: s.total,
            changes: s.changes,
            archived: s.archived,
            skipped: s.skipped,
        }
    }
}

/// Open a native directory picker so the Settings UI doesn't have
/// to take a dependency on `@tauri-apps/plugin-dialog`. Returns
/// `Ok(None)` on cancel and `Ok(Some(path))` on confirm. Uses
/// `pick_folder` (synchronous blocking variant, same plugin as the
/// dictation file picker).
#[tauri::command]
pub async fn vault_pick_directory<R: Runtime>(app: AppHandle<R>) -> Result<Option<String>, String> {
    let picked: Option<PathBuf> = tokio::task::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Choose your Obsidian vault folder")
            .blocking_pick_folder()
            .and_then(|fp| fp.into_path().ok())
    })
    .await
    .map_err(|e| format!("dialog task panicked: {e}"))?;
    Ok(picked.map(|p| p.to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn vault_export_now(
    db: State<'_, AppStateHandle>,
    vault: State<'_, Arc<VaultRuntime>>,
) -> Result<VaultExportSummary, String> {
    // Snapshot Arcs before the async boundary -- `State` isn't Send.
    let db_arc = Arc::clone(&db.inner().db);
    let vault_arc = Arc::clone(&vault);
    // Run on the blocking pool so we don't park Tauri's async
    // executor while a large reconciliation pass crunches.
    tokio::task::spawn_blocking(move || vault_arc.run_once_blocking(&db_arc))
        .await
        .map_err(|e| format!("export task panicked: {e}"))?
        .map(VaultExportSummary::from)
        .map_err(into_err)
}

// --------------------------------------------------------------------
// Runtime status (Iter 4 / mb-vg3p — Mobile Sync tab health card)
// --------------------------------------------------------------------

/// Snapshot of the outbound vault projection runtime. Cheap to
/// compute on demand: reads the runtime config + stats the manifest
/// on disk. The Settings tab polls this every 5s while visible.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultRuntimeStatus {
    /// True when the runtime is configured to project to a vault
    /// (`MobileSyncEnabled` + a non-empty `VaultPath`). Does not
    /// imply the path validates — stat errors are reported via
    /// `last_error` for the UI to surface.
    pub running: bool,
    /// Milliseconds since the last `manifest.json` write, or `None`
    /// if the manifest hasn't been written yet (fresh install, or
    /// path mis-configured).
    pub manifest_age_ms: Option<u64>,
    /// Latest mtime of `manifest.json` formatted as RFC 3339 for
    /// the UI to render verbatim.
    pub manifest_modified_iso: Option<String>,
    /// Last filesystem error encountered while statting (path
    /// missing, permission denied, etc.). Drives the "Path is
    /// unreachable" copy in the health card.
    pub last_error: Option<String>,
}

#[tauri::command]
pub fn vault_runtime_status(
    vault: State<'_, Arc<VaultRuntime>>,
) -> Result<VaultRuntimeStatus, String> {
    let cfg = vault.current_config();
    let running = cfg.is_active();
    let (manifest_age_ms, manifest_modified_iso, last_error) = match cfg.vault_path.as_ref() {
        Some(p) => stat_manifest(p),
        None => (None, None, None),
    };
    Ok(VaultRuntimeStatus {
        running,
        manifest_age_ms,
        manifest_modified_iso,
        last_error,
    })
}

/// Stat `<vault>/.mockingbird/manifest.json`, returning the age in
/// milliseconds + an RFC 3339 string. `(None, None, None)` means
/// the file simply doesn't exist yet (which is a normal pre-first-
/// export state, not an error). Real I/O errors flow through
/// `last_error`.
fn stat_manifest(vault_path: &Path) -> (Option<u64>, Option<String>, Option<String>) {
    let manifest = VaultLayout::new(vault_path).manifest_path();
    let meta = match std::fs::metadata(&manifest) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (None, None, None),
        Err(e) => return (None, None, Some(e.to_string())),
    };
    let modified = match meta.modified() {
        Ok(m) => m,
        Err(e) => return (None, None, Some(e.to_string())),
    };
    let age_ms = SystemTime::now()
        .duration_since(modified)
        .ok()
        .map(|d| d.as_millis() as u64);
    let iso = chrono::DateTime::<chrono::Utc>::from(modified).to_rfc3339();
    (age_ms, Some(iso), None)
}

/// Snapshot of the inbox courier runtime. Like
/// [`VaultRuntimeStatus`] but reports the inbound side: whether the
/// watcher is on, when the last file was successfully archived, and
/// how many couriers are sitting in `_failed/`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxRuntimeStatus {
    pub running: bool,
    pub watch_path: Option<String>,
    /// Mtime of the newest file under `<vault>/inbox/_archive/`.
    pub last_archived_iso: Option<String>,
    /// Count of regular files under `<vault>/inbox/_failed/`. The
    /// Settings UI exposes a link to open the folder when > 0.
    pub failed_count: u32,
    pub last_error: Option<String>,
}

#[tauri::command]
pub fn inbox_runtime_status(
    inbox: State<'_, Arc<InboxRuntime>>,
) -> Result<InboxRuntimeStatus, String> {
    let cfg = inbox.current_config();
    let running = cfg.is_active();
    let watch_path = cfg
        .vault_path
        .as_ref()
        .map(|p| VaultLayout::new(p).inbox().to_string_lossy().into_owned());
    let (last_archived_iso, failed_count, last_error) = match cfg.vault_path.as_ref() {
        Some(p) => stat_inbox_dirs(p),
        None => (None, 0, None),
    };
    Ok(InboxRuntimeStatus {
        running,
        watch_path,
        last_archived_iso,
        failed_count,
        last_error,
    })
}

fn stat_inbox_dirs(vault_path: &Path) -> (Option<String>, u32, Option<String>) {
    let layout = VaultLayout::new(vault_path);
    let archive_root = layout.inbox().join("_archive");
    let failed_dir = layout.inbox_failed();

    let mut last_err: Option<String> = None;

    // Newest mtime under _archive/<date>/<file>. Two-level scan,
    // bounded by the number of date subdirs the user has accrued.
    let last_archived_iso = newest_mtime_iso(&archive_root).unwrap_or_else(|e| {
        if e.kind() != std::io::ErrorKind::NotFound {
            last_err = Some(e.to_string());
        }
        None
    });

    let failed_count = count_regular_files(&failed_dir).unwrap_or_else(|e| {
        if e.kind() != std::io::ErrorKind::NotFound && last_err.is_none() {
            last_err = Some(e.to_string());
        }
        0
    });

    (last_archived_iso, failed_count, last_err)
}

fn newest_mtime_iso(root: &Path) -> std::io::Result<Option<String>> {
    let mut newest: Option<SystemTime> = None;
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            // Date subdir: scan its immediate children.
            for child in std::fs::read_dir(&p)?.flatten() {
                if let Ok(meta) = child.metadata() {
                    if let Ok(m) = meta.modified() {
                        newest = Some(match newest {
                            Some(n) if n > m => n,
                            _ => m,
                        });
                    }
                }
            }
        } else if let Ok(meta) = entry.metadata() {
            if let Ok(m) = meta.modified() {
                newest = Some(match newest {
                    Some(n) if n > m => n,
                    _ => m,
                });
            }
        }
    }
    Ok(newest.map(|m| chrono::DateTime::<chrono::Utc>::from(m).to_rfc3339()))
}

fn count_regular_files(dir: &Path) -> std::io::Result<u32> {
    let mut count: u32 = 0;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            count = count.saturating_add(1);
        }
    }
    Ok(count)
}

// --------------------------------------------------------------------
// Nested-vault detection (Iter 4 / mb-3xww)
// --------------------------------------------------------------------

/// Result of pre-flighting a candidate vault path. The Settings UI
/// runs this BEFORE writing `VaultPath` so it can surface a guided
/// dialog on the nested-vault trap (ADR 0046 Iter 2 smoke gotcha)
/// without changing the well-trodden `vault_settings_set` path.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum VaultPathCheck {
    /// Path is fine to use as-is.
    Ok,
    /// Path is INSIDE an existing Obsidian vault — both vaults
    /// would race to own `.obsidian/`. UI shows the guided dialog.
    NestedVault {
        parent_vault: String,
        suggested_sibling: Option<String>,
    },
}

#[tauri::command]
pub fn vault_check_path(path: String) -> Result<VaultPathCheck, String> {
    let p = PathBuf::from(&path);
    if let Some(parent_vault) = detect_nested_vault(&p) {
        let suggested =
            suggest_sibling_vault(&parent_vault).map(|s| s.to_string_lossy().into_owned());
        return Ok(VaultPathCheck::NestedVault {
            parent_vault: parent_vault.to_string_lossy().into_owned(),
            suggested_sibling: suggested,
        });
    }
    Ok(VaultPathCheck::Ok)
}

/// Create a directory (and any missing parents) at `path`. The
/// Settings UI calls this when the user accepts the "Use a sibling
/// location" recommendation — the suggested path won't exist yet,
/// and we'd rather not open the folder picker again just to let
/// the user navigate to it.
#[tauri::command]
pub fn vault_ensure_dir(path: String) -> Result<(), String> {
    std::fs::create_dir_all(&path).map_err(|e| format!("ensure_dir {path:?}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_accepts_every_iter4_vault_key() {
        for k in [
            SettingKey::MobileSyncEnabled,
            SettingKey::VaultPath,
            SettingKey::VaultSyncRecordTypes,
            SettingKey::VaultRetentionDays,
            SettingKey::VaultSyncBackend,
            SettingKey::SyncTierByteCap,
            SettingKey::KeepAudioBlobs,
            SettingKey::VaultDebugKeepCouriers,
        ] {
            assert!(is_vault_setting_allowed_for_ui(k), "{k:?}");
        }
    }

    #[test]
    fn allowlist_rejects_non_vault_keys() {
        assert!(!is_vault_setting_allowed_for_ui(SettingKey::Theme));
        assert!(!is_vault_setting_allowed_for_ui(
            SettingKey::MeetingHotkeyModifier
        ));
    }

    #[test]
    fn count_regular_files_skips_subdirs() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.m4a"), b"x").unwrap();
        std::fs::write(tmp.path().join("b.m4a"), b"x").unwrap();
        std::fs::create_dir_all(tmp.path().join("subdir")).unwrap();
        std::fs::write(tmp.path().join("subdir/c.m4a"), b"x").unwrap();
        let n = count_regular_files(tmp.path()).unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn newest_mtime_iso_handles_empty_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("_archive");
        std::fs::create_dir_all(&root).unwrap();
        assert!(newest_mtime_iso(&root).unwrap().is_none());
    }

    #[test]
    fn newest_mtime_iso_walks_date_subdirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("_archive");
        std::fs::create_dir_all(root.join("2026-05-27")).unwrap();
        std::fs::write(root.join("2026-05-27/Memo.m4a"), b"x").unwrap();
        let out = newest_mtime_iso(&root).unwrap();
        assert!(out.is_some());
    }

    #[test]
    fn summary_conversion_preserves_skipped_marker() {
        let s = ReconciliationSummary {
            total: 0,
            changes: 0,
            archived: 0,
            skipped: true,
        };
        let v: VaultExportSummary = s.into();
        assert!(v.skipped);
    }
}
