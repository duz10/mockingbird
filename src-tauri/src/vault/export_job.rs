//! Outbound reconciliation engine -- ADR 0046 §5/§6.
//!
//! [`VaultRuntime`] is the runtime owner of the export pipeline. It
//! carries the (in-memory) [`VaultConfig`] derived from the four
//! `SettingKey::{MobileSyncEnabled, VaultPath, VaultSyncRecordTypes,
//! VaultRetentionDays}` rows, a `job_lock` that serializes
//! reconciliation passes so only one is in flight at any time, and a
//! coalescing flag so concurrent triggers fold into a single re-run
//! tail.
//!
//! ## Reconciliation pass (`run_once`)
//!
//! 1. [`VaultLayout::ensure_zones`] -- idempotent dir creation. Done
//!    every pass so a user deleting the vault root mid-run recovers
//!    on the next trigger without intervention.
//! 2. [`Manifest::load`] -- absent file -> fresh empty manifest.
//! 3. Query in-scope canonical records (sessions + meetings filtered
//!    by record-type setting + retention window).
//! 4. For each record:
//!    - Build a [`ProjectionInput`], call [`project`].
//!    - If `project()`'s `content_sha256` matches the manifest's
//!      stored hash, do nothing (byte-identical re-export skip).
//!    - Otherwise [`write_atomic`] to `<vault>/history/<filename>`
//!      and stamp / update the manifest entry.
//! 5. For each manifest entry whose UUID is NOT in the freshly
//!    queried in-scope set, move its file under
//!    `<vault>/history/_archive/` and drop the manifest entry.
//!    Idempotent: an already-archived (file missing) record is just
//!    removed from the manifest.
//! 6. [`Manifest::save_atomic`] writes the new manifest.
//!
//! ## Trigger semantics
//!
//! - [`trigger`](VaultRuntime::trigger) is fire-and-forget. It tries
//!   to acquire `job_lock`; on success it spawns a worker that runs
//!   `run_once` then loops while `coalesced` is set (each loop also
//!   resets the flag). On contention it just sets `coalesced=true`
//!   so the in-flight worker picks the pending run up on its next
//!   tick.
//! - [`run_once_blocking`](VaultRuntime::run_once_blocking) is the
//!   synchronous path used by the manual "Export now" IPC + the
//!   initial backfill triggered by a settings toggle. Same lock; if
//!   another worker is in flight, it waits.
//!
//! ## What this module does NOT do
//!
//! - No transcript / DB writes -- the canonical SQLite tables are
//!   read-only here. ADR §3: the export job is a *projection*.
//! - No LLM calls. No network. Pure DB-read + filesystem-write.
//! - No watch loop. ADR §4: trigger is post-commit, not polling.
//!   Iter 3 will add an inbound courier watcher; that lives in its
//!   own module.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::settings::model::SettingKey;
use crate::settings::Settings;
use crate::vault::layout::VaultLayout;
use crate::vault::manifest::{Manifest, ManifestRecord, RecordType};
use crate::vault::project::{project, ProjectionInput};

// --------------------------------------------------------------------
// Public types
// --------------------------------------------------------------------

/// Which canonical-table record families a given pass exports.
///
/// Mirrors the `VaultSyncRecordTypes` setting's three legal values.
/// Default is [`RecordTypeFilter::Both`] -- if the on-disk string is
/// garbage we still want to do *something* sensible (export
/// everything) rather than silently no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordTypeFilter {
    /// Export rows from `sessions` only.
    Dictation,
    /// Export rows from `meeting_sessions` only.
    Meeting,
    /// Export rows from both tables.
    Both,
}

impl RecordTypeFilter {
    fn includes(self, kind: RecordType) -> bool {
        matches!(
            (self, kind),
            (Self::Both, _)
                | (Self::Dictation, RecordType::Dictation)
                | (Self::Meeting, RecordType::Meeting)
        )
    }

    fn parse(s: &str) -> Self {
        match s {
            "dictation" => Self::Dictation,
            "meeting" => Self::Meeting,
            _ => Self::Both,
        }
    }
}

/// Snapshot of the four Mobile-Sync settings keys plus a derived
/// "is this configuration usable" flag.
#[derive(Debug, Clone)]
pub struct VaultConfig {
    /// `MobileSyncEnabled`. False -> [`run_once`] returns
    /// `Ok(ReconciliationSummary::skipped())` immediately.
    pub enabled: bool,
    /// `VaultPath`. None when the user hasn't entered one yet OR the
    /// stored value is empty.
    pub vault_path: Option<PathBuf>,
    /// `VaultSyncRecordTypes`.
    pub record_types: RecordTypeFilter,
    /// `VaultRetentionDays`. `0` means "forever" (no retention
    /// filter). Negative values are clamped to 0.
    pub retention_days: i64,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            vault_path: None,
            record_types: RecordTypeFilter::Both,
            retention_days: 30,
        }
    }
}

impl VaultConfig {
    /// Returns true when [`run_once`] should actually attempt I/O.
    pub fn is_active(&self) -> bool {
        self.enabled && self.vault_path.is_some()
    }

    /// Load the config from settings. Read errors per-key fall back
    /// to the [`Default`] value -- a settings-row hiccup must never
    /// crash the reconciliation thread.
    pub fn load(db: &Arc<Mutex<Connection>>) -> AppResult<Self> {
        let conn = db
            .lock()
            .map_err(|_| AppError::Other("vault: db mutex poisoned".into()))?;
        let s = Settings::new(&conn);
        let enabled = s
            .get::<bool>(SettingKey::MobileSyncEnabled)
            .unwrap_or(false);
        let vault_path = match s.get::<Option<String>>(SettingKey::VaultPath) {
            Ok(Some(p)) if !p.trim().is_empty() => Some(PathBuf::from(p)),
            _ => None,
        };
        let record_types_str = s
            .get::<String>(SettingKey::VaultSyncRecordTypes)
            .unwrap_or_else(|_| "both".into());
        let record_types = RecordTypeFilter::parse(&record_types_str);
        let retention_days = s
            .get::<i64>(SettingKey::VaultRetentionDays)
            .unwrap_or(30)
            .max(0);
        Ok(Self {
            enabled,
            vault_path,
            record_types,
            retention_days,
        })
    }
}

/// Summary of one reconciliation pass. Returned from
/// [`VaultRuntime::run_once_blocking`] and surfaced to the UI by the
/// `vault_export_now` IPC.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconciliationSummary {
    /// Records considered in scope this pass (post record-type +
    /// retention filtering).
    pub total: usize,
    /// New or re-written files this pass.
    pub changes: usize,
    /// Records archived (moved to `history/_archive/`).
    pub archived: usize,
    /// True when the pass was a no-op because sync isn't enabled or
    /// no vault path is configured. Lets the IPC surface a different
    /// toast.
    pub skipped: bool,
}

impl ReconciliationSummary {
    fn skipped() -> Self {
        Self {
            skipped: true,
            ..Self::default()
        }
    }
}

// --------------------------------------------------------------------
// VaultRuntime
// --------------------------------------------------------------------

/// Runtime handle for the export pipeline.
///
/// Cheaply [`Clone`]-able (every field is an [`Arc`]). The shape that
/// gets `.manage()`-ed into Tauri state is `Arc<VaultRuntime>` so the
/// IPC handlers and the per-subsystem triggers all reach the same
/// underlying lock + flag.
pub struct VaultRuntime {
    config: Arc<RwLock<VaultConfig>>,
    /// Held for the duration of one synchronous `run_once_blocking`
    /// pass. The async trigger path uses `running` (below) instead
    /// to avoid a non-Send `MutexGuard` crossing into the worker
    /// thread; this lock exists so blocking callers waiting for an
    /// in-flight async job will queue cleanly.
    blocking_lock: Arc<Mutex<()>>,
    /// `true` when an async worker is in flight. Spawn-or-coalesce
    /// decision: a `swap(true, ...)` returning `false` means the
    /// caller won the race and owns the worker. Workers reset to
    /// `false` only after re-checking [`coalesced`].
    running: Arc<AtomicBool>,
    /// Set whenever a trigger arrives while a job is in flight; the
    /// in-flight worker reads + clears it before deciding to loop.
    coalesced: Arc<AtomicBool>,
}

impl VaultRuntime {
    /// Build a fresh runtime by reading the current settings.
    pub fn new(db: &Arc<Mutex<Connection>>) -> AppResult<Self> {
        let cfg = VaultConfig::load(db)?;
        Ok(Self {
            config: Arc::new(RwLock::new(cfg)),
            blocking_lock: Arc::new(Mutex::new(())),
            running: Arc::new(AtomicBool::new(false)),
            coalesced: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Re-read all four `Vault*` / `MobileSync*` settings rows.
    /// Called from the settings-update IPC right before triggering a
    /// backfill so a "just-flipped-on" toggle picks up the new
    /// config in the same tick.
    pub fn refresh_config(&self, db: &Arc<Mutex<Connection>>) -> AppResult<()> {
        let new_cfg = VaultConfig::load(db)?;
        let mut guard = self
            .config
            .write()
            .map_err(|_| AppError::Other("vault: config rwlock poisoned".into()))?;
        *guard = new_cfg;
        Ok(())
    }

    /// Read-only snapshot of the current config -- handy for the
    /// Settings UI "current status" probe.
    pub fn current_config(&self) -> VaultConfig {
        self.config.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Fire-and-forget trigger. Spawns a worker that runs [`run_once`]
    /// when no other async pass is running; sets the coalesced
    /// flag otherwise so the in-flight worker picks the pending
    /// run up on its tail.
    pub fn trigger(&self, db: Arc<Mutex<Connection>>) {
        // Fast-skip when sync is off so we never spawn a worker.
        // Note: this read is also done inside `run_once`, but
        // gating here avoids one thread-spawn per session-saved
        // event in the steady-state-disabled case.
        if !self.config.read().map(|g| g.is_active()).unwrap_or(false) {
            return;
        }
        // Claim the running flag. If someone else already owns it,
        // mark the pending re-run and bail -- the in-flight worker
        // re-checks `coalesced` before exiting.
        let prev = self.running.swap(true, Ordering::SeqCst);
        if prev {
            self.coalesced.store(true, Ordering::SeqCst);
            tracing::debug!(target: "vault", "trigger coalesced");
            return;
        }
        let cfg_arc = Arc::clone(&self.config);
        let coalesced = Arc::clone(&self.coalesced);
        let running = Arc::clone(&self.running);
        let blocking_lock = Arc::clone(&self.blocking_lock);
        let spawn_res = std::thread::Builder::new()
            .name("mockingbird-vault-export".into())
            .spawn(move || {
                // Run at least once; loop if another trigger landed
                // while we were running. The `blocking_lock` is
                // acquired per-pass so a synchronous
                // `run_once_blocking` caller queues behind us.
                loop {
                    coalesced.store(false, Ordering::SeqCst);
                    let pass_result = {
                        let _hold = match blocking_lock.lock() {
                            Ok(g) => g,
                            Err(p) => p.into_inner(),
                        };
                        run_once(&cfg_arc, &db)
                    };
                    if let Err(e) = pass_result {
                        tracing::warn!(target: "vault", error = ?e, "export pass failed");
                    }
                    if !coalesced.swap(false, Ordering::SeqCst) {
                        break;
                    }
                }
                running.store(false, Ordering::SeqCst);
            });
        if let Err(e) = spawn_res {
            tracing::warn!(target: "vault", error = ?e, "vault worker spawn failed");
            // Restore the flag so the next trigger can try again.
            self.running.store(false, Ordering::SeqCst);
        }
    }

    /// Synchronous one-shot pass. Used by the manual "Export now"
    /// IPC + the initial backfill from a just-toggled-on settings
    /// flip. Acquires `blocking_lock` so it queues cleanly behind
    /// any async worker already running.
    pub fn run_once_blocking(
        &self,
        db: &Arc<Mutex<Connection>>,
    ) -> AppResult<ReconciliationSummary> {
        let _hold: MutexGuard<'_, ()> = match self.blocking_lock.lock() {
            Ok(g) => g,
            // Poisoning on this lock means a previous pass panicked
            // mid-run; we still want to give the manual trigger a
            // shot at making forward progress.
            Err(p) => p.into_inner(),
        };
        // Clear any pending coalesce -- this manual run absorbs it.
        self.coalesced.store(false, Ordering::SeqCst);
        run_once(&self.config, db)
    }
}

// --------------------------------------------------------------------
// One reconciliation pass
// --------------------------------------------------------------------

fn run_once(
    cfg_arc: &RwLock<VaultConfig>,
    db: &Arc<Mutex<Connection>>,
) -> AppResult<ReconciliationSummary> {
    let cfg = cfg_arc
        .read()
        .map_err(|_| AppError::Other("vault: config rwlock poisoned".into()))?
        .clone();
    if !cfg.is_active() {
        return Ok(ReconciliationSummary::skipped());
    }
    let root = cfg
        .vault_path
        .as_deref()
        .expect("is_active guarantees Some");
    let layout = VaultLayout::new(root);
    layout.ensure_zones()?;

    let manifest_path = layout.manifest_path();
    let mut manifest = Manifest::load(&manifest_path)?;

    let records = query_records_in_scope(db, &cfg)?;
    let total = records.len();
    let mut changes = 0usize;
    let mut seen_uuids = std::collections::HashSet::with_capacity(total);

    for row in &records {
        seen_uuids.insert(row.uuid.clone());
        let tags = row.tags_for_filter(cfg.record_types);
        let tags_ref: Vec<&str> = tags.iter().map(|t| t.as_str()).collect();
        let input = ProjectionInput {
            uuid: &row.uuid,
            record_type: row.record_type,
            created_iso: &row.created_iso,
            duration_ms: row.duration_ms,
            title: row.title.as_deref(),
            tags: &tags_ref,
            source: &row.source,
            body: &row.body,
        };
        let out = project(input)?;

        let needs_write = match manifest.records.get(&row.uuid) {
            Some(existing) => existing.content_sha256 != out.content_sha256,
            None => true,
        };
        if !needs_write {
            continue;
        }

        let target = layout.history().join(&out.filename);
        write_atomic(&target, out.content.as_bytes())?;
        manifest.records.insert(
            row.uuid.clone(),
            ManifestRecord {
                path: format!("history/{}", out.filename),
                content_sha256: out.content_sha256,
                last_exported_iso: now_iso_utc(),
                record_type: row.record_type,
            },
        );
        changes += 1;
    }

    // Stale-record sweep: anything in the manifest not in seen_uuids
    // (out of scope or hard-deleted) gets archived + dropped.
    let stale: Vec<String> = manifest
        .records
        .keys()
        .filter(|uuid| !seen_uuids.contains(*uuid))
        .cloned()
        .collect();
    let mut archived = 0usize;
    for uuid in &stale {
        if let Some(entry) = manifest.records.remove(uuid) {
            archive_record(&layout, &entry)?;
            archived += 1;
        }
    }

    manifest.save_atomic(&manifest_path)?;

    Ok(ReconciliationSummary {
        total,
        changes,
        archived,
        skipped: false,
    })
}

/// Move `<vault>/history/<filename>` to `<vault>/history/_archive/<filename>`.
/// Tolerates a missing source file (already archived / user-deleted).
fn archive_record(layout: &VaultLayout<'_>, entry: &ManifestRecord) -> AppResult<()> {
    // entry.path is `history/<filename>` -- strip the prefix to get
    // the bare filename. Fall back to the whole string if prefix
    // missing (shouldn't happen, but defensive).
    let filename = entry.path.strip_prefix("history/").unwrap_or(&entry.path);
    let src = layout.history().join(filename);
    let dst = layout.history_archive().join(filename);
    if !src.exists() {
        // Already gone (manual user delete, prior failed move).
        // Nothing to do; the manifest drop already happened in the
        // caller.
        return Ok(());
    }
    // Best-effort: if rename fails (e.g. cross-device, target
    // exists), try copy + remove.
    if let Err(_e) = std::fs::rename(&src, &dst) {
        std::fs::copy(&src, &dst).map_err(|e| {
            AppError::Vault(format!(
                "archive: copy {} -> {} -- {}",
                src.display(),
                dst.display(),
                e
            ))
        })?;
        let _ = std::fs::remove_file(&src);
    }
    Ok(())
}

/// Atomic write: tmp + rename. Same rationale as the manifest's
/// save_atomic -- Obsidian Sync must never see a half-written
/// markdown file.
fn write_atomic(target: &Path, content: &[u8]) -> AppResult<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            AppError::Vault(format!(
                "write_atomic: ensure parent {} -- {}",
                parent.display(),
                e
            ))
        })?;
    }
    let tmp = target.with_extension("md.tmp");
    std::fs::write(&tmp, content).map_err(|e| {
        AppError::Vault(format!(
            "write_atomic: write tmp {} -- {}",
            tmp.display(),
            e
        ))
    })?;
    std::fs::rename(&tmp, target).map_err(|e| {
        AppError::Vault(format!(
            "write_atomic: rename {} -> {} -- {}",
            tmp.display(),
            target.display(),
            e
        ))
    })?;
    Ok(())
}

// --------------------------------------------------------------------
// DB query: in-scope records
// --------------------------------------------------------------------

/// In-memory representation of one canonical row enriched with the
/// transcript body the projector needs. Fields land here after
/// joining `sessions`/`meeting_sessions` with their respective
/// transcript tables.
#[derive(Debug, Clone)]
pub struct RecordRow {
    /// `sessions.uuid` / `meeting_sessions.uuid` -- the manifest key.
    pub uuid: String,
    /// Which canonical table this row came from.
    pub record_type: RecordType,
    /// `sessions.started_at` / `meeting_sessions.started_at` (RFC-3339).
    pub created_iso: String,
    /// `sessions.audio_duration_ms` / `meeting_sessions.total_duration_ms`.
    pub duration_ms: Option<i64>,
    /// `meeting_sessions.title` for meetings; reserved for a future
    /// auto-titled-dictation feature on the dictation side.
    pub title: Option<String>,
    /// `sessions.source` string for dictations; literal `"meeting"`
    /// for meetings. Threaded into the front-matter `source:` field.
    pub source: String,
    /// Best-available transcript body -- final / cleaned / raw for
    /// dictations, merged / mic / system for meetings.
    pub body: String,
    /// `modes.slug` joined off `sessions.mode_id` (dictations only).
    /// Used as the third front-matter tag.
    pub mode_slug: Option<String>,
}

impl RecordRow {
    /// Build the per-record tag list. Currently:
    /// - record-type tag (`dictation` / `meeting`)
    /// - source tag (the raw `sessions.source` / `"meeting"` string)
    /// - mode-slug tag for dictations when present (e.g. `normal`).
    fn tags_for_filter(&self, _filter: RecordTypeFilter) -> Vec<String> {
        let mut out = Vec::with_capacity(3);
        out.push(match self.record_type {
            RecordType::Dictation => "dictation".to_string(),
            RecordType::Meeting => "meeting".to_string(),
        });
        if !self.source.is_empty() && self.source != "meeting" {
            out.push(self.source.clone());
        }
        if let Some(slug) = self.mode_slug.as_deref() {
            if !slug.is_empty() {
                out.push(slug.to_string());
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

/// Join sessions / meetings with their transcripts, filtered by
/// retention + record-type setting.
fn query_records_in_scope(
    db: &Arc<Mutex<Connection>>,
    cfg: &VaultConfig,
) -> AppResult<Vec<RecordRow>> {
    let conn = db
        .lock()
        .map_err(|_| AppError::Other("vault: db mutex poisoned".into()))?;
    let cutoff_iso = retention_cutoff_iso(cfg.retention_days);
    let mut out = Vec::new();
    if cfg.record_types.includes(RecordType::Dictation) {
        out.extend(query_dictations(&conn, cutoff_iso.as_deref())?);
    }
    if cfg.record_types.includes(RecordType::Meeting) {
        out.extend(query_meetings(&conn, cutoff_iso.as_deref())?);
    }
    Ok(out)
}

/// Return the cutoff ISO-8601 string for retention, or None when
/// `retention_days == 0` (forever).
fn retention_cutoff_iso(retention_days: i64) -> Option<String> {
    if retention_days <= 0 {
        return None;
    }
    let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days);
    Some(cutoff.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn query_dictations(conn: &Connection, cutoff_iso: Option<&str>) -> AppResult<Vec<RecordRow>> {
    // Pick the best-available transcript stage: final > cleaned >
    // raw. The raw fallback exists for sessions that errored after
    // STT but before injection.
    //
    // We use a correlated subquery rather than three LEFT JOINs to
    // keep the SQL legible. SQLite's planner indexes
    // `transcripts(session_id)` so this is fine for the kinds of
    // counts we're dealing with (< 100k sessions in practice).
    let base_sql = "\
        SELECT s.uuid, s.started_at, s.audio_duration_ms, s.source, \
               COALESCE(m.slug, ''), \
               COALESCE( \
                 (SELECT t.text FROM transcripts t WHERE t.session_id = s.id AND t.stage = 'final'), \
                 (SELECT t.text FROM transcripts t WHERE t.session_id = s.id AND t.stage = 'cleaned'), \
                 (SELECT t.text FROM transcripts t WHERE t.session_id = s.id AND t.stage = 'raw'), \
                 '' \
               ) AS body \
        FROM sessions s \
        LEFT JOIN modes m ON m.id = s.mode_id \
        WHERE s.status = 'complete' ";
    let mut rows: Vec<RecordRow> = if let Some(cutoff) = cutoff_iso {
        let sql = format!("{base_sql} AND s.started_at >= ?1 ORDER BY s.started_at DESC");
        let mut stmt = conn.prepare(&sql)?;
        let mapped = stmt.query_map(params![cutoff], map_dictation_row)?;
        mapped.collect::<Result<Vec<_>, _>>()?
    } else {
        let sql = format!("{base_sql} ORDER BY s.started_at DESC");
        let mut stmt = conn.prepare(&sql)?;
        let mapped = stmt.query_map([], map_dictation_row)?;
        mapped.collect::<Result<Vec<_>, _>>()?
    };
    // Drop empty-body rows -- nothing meaningful to project.
    rows.retain(|r| !r.body.trim().is_empty());
    Ok(rows)
}

fn map_dictation_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecordRow> {
    let uuid: String = row.get(0)?;
    let started_at: String = row.get(1)?;
    let audio_duration_ms: i64 = row.get(2)?;
    let source: String = row.get(3)?;
    let mode_slug: String = row.get(4)?;
    let body: String = row.get(5)?;
    Ok(RecordRow {
        uuid,
        record_type: RecordType::Dictation,
        created_iso: started_at,
        duration_ms: Some(audio_duration_ms),
        title: None,
        source,
        body,
        mode_slug: if mode_slug.is_empty() {
            None
        } else {
            Some(mode_slug)
        },
    })
}

fn query_meetings(conn: &Connection, cutoff_iso: Option<&str>) -> AppResult<Vec<RecordRow>> {
    // Best body: formatted merged > formatted mic > formatted system.
    // Same coalesce pattern as dictations.
    let base_sql = "\
        SELECT ms.uuid, ms.started_at, ms.total_duration_ms, ms.title, \
               COALESCE( \
                 (SELECT mt.text FROM meeting_transcripts mt \
                   WHERE mt.meeting_session_id = ms.id AND mt.channel = 'merged' AND mt.stage = 'formatted'), \
                 (SELECT mt.text FROM meeting_transcripts mt \
                   WHERE mt.meeting_session_id = ms.id AND mt.channel = 'mic' AND mt.stage = 'formatted'), \
                 (SELECT mt.text FROM meeting_transcripts mt \
                   WHERE mt.meeting_session_id = ms.id AND mt.channel = 'system' AND mt.stage = 'formatted'), \
                 '' \
               ) AS body \
        FROM meeting_sessions ms \
        WHERE ms.status = 'complete' ";
    let mut rows: Vec<RecordRow> = if let Some(cutoff) = cutoff_iso {
        let sql = format!("{base_sql} AND ms.started_at >= ?1 ORDER BY ms.started_at DESC");
        let mut stmt = conn.prepare(&sql)?;
        let mapped = stmt.query_map(params![cutoff], map_meeting_row)?;
        mapped.collect::<Result<Vec<_>, _>>()?
    } else {
        let sql = format!("{base_sql} ORDER BY ms.started_at DESC");
        let mut stmt = conn.prepare(&sql)?;
        let mapped = stmt.query_map([], map_meeting_row)?;
        mapped.collect::<Result<Vec<_>, _>>()?
    };
    rows.retain(|r| !r.body.trim().is_empty());
    Ok(rows)
}

fn map_meeting_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecordRow> {
    let uuid: String = row.get(0)?;
    let started_at: String = row.get(1)?;
    let total_duration_ms: i64 = row.get(2)?;
    let title: Option<String> = row.get(3)?;
    let body: String = row.get(4)?;
    Ok(RecordRow {
        uuid,
        record_type: RecordType::Meeting,
        created_iso: started_at,
        duration_ms: Some(total_duration_ms),
        title,
        source: "meeting".to_string(),
        body,
        mode_slug: None,
    })
}

// --------------------------------------------------------------------
// Helpers
// --------------------------------------------------------------------

fn now_iso_utc() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    //! NOTE: like the rest of `src-tauri/src/**`, these tests are NOT
    //! runnable on this Windows box via `cargo test --release`
    //! (`STATUS_ENTRYPOINT_NOT_FOUND`, LESSONS 2026-05-17 / mb-0n8c).
    //! They DO compile via `cargo test --release --no-run` and are
    //! live-exercised via the throwaway-crate recipe in
    //! `$env:TEMP\vault_export_throwaway\` -- see LESSONS
    //! 2026-05-27 [adr-0046-iter2] for the submodule-mirroring
    //! variant.

    use super::*;

    #[test]
    fn record_type_filter_includes_matches_setting() {
        assert!(RecordTypeFilter::Both.includes(RecordType::Dictation));
        assert!(RecordTypeFilter::Both.includes(RecordType::Meeting));
        assert!(RecordTypeFilter::Dictation.includes(RecordType::Dictation));
        assert!(!RecordTypeFilter::Dictation.includes(RecordType::Meeting));
        assert!(RecordTypeFilter::Meeting.includes(RecordType::Meeting));
        assert!(!RecordTypeFilter::Meeting.includes(RecordType::Dictation));
    }

    #[test]
    fn record_type_filter_parses_unknown_as_both() {
        assert_eq!(
            RecordTypeFilter::parse("dictation"),
            RecordTypeFilter::Dictation
        );
        assert_eq!(
            RecordTypeFilter::parse("meeting"),
            RecordTypeFilter::Meeting
        );
        assert_eq!(RecordTypeFilter::parse("both"), RecordTypeFilter::Both);
        assert_eq!(RecordTypeFilter::parse("garbage"), RecordTypeFilter::Both);
    }

    #[test]
    fn vault_config_default_is_inactive() {
        let c = VaultConfig::default();
        assert!(!c.is_active(), "default config must NOT trigger I/O");
    }

    #[test]
    fn vault_config_active_requires_both_flag_and_path() {
        let mut c = VaultConfig::default();
        c.enabled = true;
        assert!(!c.is_active(), "enabled alone isn't enough -- need a path");
        c.vault_path = Some(PathBuf::from("/tmp/vault"));
        assert!(c.is_active());
        c.enabled = false;
        assert!(!c.is_active(), "path alone isn't enough -- need the toggle");
    }

    #[test]
    fn retention_cutoff_zero_means_forever() {
        assert!(retention_cutoff_iso(0).is_none());
        assert!(retention_cutoff_iso(-1).is_none());
        assert!(retention_cutoff_iso(1).is_some());
    }

    #[test]
    fn record_row_tags_for_dictation_include_source_and_mode() {
        let r = RecordRow {
            uuid: "u".into(),
            record_type: RecordType::Dictation,
            created_iso: "2026-05-27T14:08:42Z".into(),
            duration_ms: Some(1000),
            title: None,
            source: "desktop".into(),
            body: "x".into(),
            mode_slug: Some("normal".into()),
        };
        let t = r.tags_for_filter(RecordTypeFilter::Both);
        assert!(t.contains(&"dictation".to_string()));
        assert!(t.contains(&"desktop".to_string()));
        assert!(t.contains(&"normal".to_string()));
    }

    #[test]
    fn record_row_tags_for_meeting_omit_source_tag() {
        // Source for meetings is the literal `"meeting"`; we
        // already emit the `meeting` record-type tag so the source
        // tag would be a duplicate.
        let r = RecordRow {
            uuid: "u".into(),
            record_type: RecordType::Meeting,
            created_iso: "2026-05-27T14:08:42Z".into(),
            duration_ms: Some(1000),
            title: Some("Sync".into()),
            source: "meeting".into(),
            body: "x".into(),
            mode_slug: None,
        };
        let t = r.tags_for_filter(RecordTypeFilter::Both);
        assert_eq!(t, vec!["meeting".to_string()]);
    }

    #[test]
    fn summary_skipped_marker_is_set() {
        let s = ReconciliationSummary::skipped();
        assert!(s.skipped);
        assert_eq!(s.total, 0);
        assert_eq!(s.changes, 0);
        assert_eq!(s.archived, 0);
    }
}
