#![allow(missing_docs)] // Self-documenting field set; module-level doc is the API surface.

//! Sessions repository — the central provenance pivot.
//!
//! `NewSession` requires provenance FKs (`prompt_id`,
//! `dictionary_snapshot_id`, `example_set_id`) at the type level, even
//! though the SQL schema allows them to be NULL. This is the
//! application-layer enforcement of the "provenance is total" rule
//! (PLAN §12, AGENTS.md principle 2). The schema and the API
//! deliberately disagree.

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct NewSession {
    pub uuid: String,
    pub mode_id: i64,
    pub hotkey_pressed: String,
    pub started_at: String,
    pub recording_ended_at: String,
    pub status: SessionStatus,
    pub foreground_app: Option<String>,
    pub foreground_window_title: Option<String>,
    pub audio_duration_ms: i64,
    pub audio_blob_path: Option<String>,

    // Provenance — REQUIRED at application layer.
    pub prompt_id: i64,
    pub dictionary_snapshot_id: i64,
    pub example_set_id: i64,

    /// ADR 0045 + mb-tfyp — which start path produced this session.
    /// `Ptt` for hotkey-triggered, `InApp` for programmatic
    /// (`dictation_start` IPC) sessions. Persisted as TEXT (migration
    /// 017).
    pub start_mode: StartMode,

    /// ADR 0046 + mb-jqhw — WHERE the audio came from. Orthogonal to
    /// `start_mode`: a `Desktop` session may be either PTT or in-app,
    /// but `DesktopImport` and `MobileInbox` are always `InApp`.
    /// Persisted as TEXT (migration 018), default 'desktop'.
    pub source: SessionSource,

    /// ADR 0052 + mb-pxzk (KG Phase 1D Wave 1D.1) — WHAT kind of
    /// capture this session represents. Orthogonal to both
    /// `start_mode` (activation mechanism) and `source` (audio
    /// origin); a `'kg-note'` capture is by definition `InApp`
    /// `Desktop` (KG audio note button programmatically starts a
    /// live-mic session, no headless ingest), but the converse
    /// doesn't hold. Drives the dictation-tail KG filing source-
    /// gate (only `KgNote` sessions enqueue when the graph is on).
    /// Persisted as TEXT (migration 025), default 'dictation'.
    ///
    /// Note: `sessions.category` (ADR 0052 + mb-oji5) is
    /// deliberately NOT exposed on `NewSession`. That column is
    /// worker-write-only — the KG classify pass UPDATEs it
    /// post-filing. Surfacing it here would invite callers to set
    /// it at session creation, which would conflict with the
    /// classify pass and bypass the provenance contract
    /// (Principle 2 — the category, when present, must come from a
    /// dated pass with a recorded model id).
    pub capture_kind: CaptureKind,
}

/// What kind of capture this session represents.
///
/// Persisted to `sessions.capture_kind` (migration 025). Drives the
/// dictation-tail KG filing source-gate: only `KgNote` sessions
/// enqueue into `kg_filing_queue` when `KgGraphEnabled=true`.
///
/// **Wave 1D.3 (mb-0gt6) update.** Text notes DO get a session row
/// with `KgNoteText` (via [`crate::kg::ingest_text::ingest_text_note`]).
/// The session/transcripts write IS the canonical store; the
/// Dictations history page filters out `kg-note-text` rows so they
/// stay KG-only at the UI surface but remain inspectable via the
/// same provenance ladder every other session uses (mode_id,
/// prompt_id, dictionary_snapshot_id, example_set_id). This
/// supersedes ADR 0052 §D3's earlier "synthetic entry id in
/// `kg_filing_queue`" sketch — the simpler design reuses the
/// existing tables instead of adding a parallel write path.
/// (Recorded in `docs/LESSONS.md` 2026-05-30; will be picked up
/// in the Wave 1D.6 seal note on ADR 0052.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureKind {
    /// The pre-1D default; covers every existing session row and
    /// every future PTT / in-app live-mic dictation that wasn't
    /// initiated from the KG screen.
    #[default]
    Dictation,
    /// Audio note initiated from the KG screen (Wave 1D.3). Dual-
    /// writes: standard dictation row + transcripts AND enqueues
    /// for KG filing via the source-gated dictation-tail hook.
    KgNote,
    /// Text-only KG note (Wave 1D.3 / mb-0gt6). Written by
    /// [`crate::kg::ingest_text::ingest_text_note`]: a sessions
    /// row with raw/cleaned/final transcripts all carrying the
    /// user-typed text (no Whisper, no cleanup LLM pass), then
    /// enqueued directly into `kg_filing_queue`. The Dictations
    /// history list filters rows with this `capture_kind` out so
    /// they stay KG-only at the UX layer while remaining auditable
    /// at the row + transcript level.
    KgNoteText,
}

impl CaptureKind {
    /// Canonical DB string. UI / IPC code should round-trip through
    /// this rather than hand-rolling string literals.
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Dictation => "dictation",
            Self::KgNote => "kg-note",
            Self::KgNoteText => "kg-note-text",
        }
    }

    /// Parse a DB string. Unknown values fall back to `Dictation` —
    /// the safe default (same defensive-parse rationale as
    /// `StartMode::parse_db` / `SessionSource::parse_db`). A row that
    /// mysteriously says `"foobar"` is at least not falsely promoted
    /// to `KgNote` (which would cause the source-gate to enqueue it
    /// into the KG filing queue when the toggle was flipped on).
    pub fn parse_db(s: &str) -> Self {
        match s {
            "kg-note" => Self::KgNote,
            "kg-note-text" => Self::KgNoteText,
            _ => Self::Dictation,
        }
    }
}

/// Where the audio for a session originated.
///
/// Persisted to `sessions.source` (migration 018). Drives the Iter 2
/// export-job filter (only `Desktop` and `DesktopImport` get projected
/// out to the synced vault — `MobileInbox` rows would round-trip back
/// to the same vault they came from and create export loops).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionSource {
    /// Live mic capture on the desktop (PTT hold OR in-app Start
    /// button). The PRE-ADR-0046 default; covers every existing row.
    #[default]
    Desktop,
    /// User picked an audio file via the "+ Audio file" desktop
    /// import button (mb-7vyz / ADR 0046 Iter 1).
    DesktopImport,
    /// Audio courier'd in via the iOS Shortcut → synced Obsidian vault
    /// → inbox watcher flow (ADR 0046 Iter 3). Reserved; no callsite
    /// emits this yet.
    MobileInbox,
}

impl SessionSource {
    /// Canonical DB string. Match the values the UI renders against —
    /// see `ui/src/lib/types.ts` `SessionSource`.
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::DesktopImport => "desktop-import",
            Self::MobileInbox => "mobile-inbox",
        }
    }

    /// Parse a DB string. Unknown values default to `Desktop` — the
    /// safe fallback: an unknown source string most likely means a
    /// downgrade ran against a newer-than-expected schema, and the
    /// only honest answer is "some kind of local recording".
    pub fn parse_db(s: &str) -> Self {
        match s {
            "desktop-import" => Self::DesktopImport,
            "mobile-inbox" => Self::MobileInbox,
            _ => Self::Desktop,
        }
    }
}

/// Discriminates the two ADR 0045 dictation start paths.
///
/// Persisted to `sessions.start_mode` (migration 017). Drives the
/// list-pill label in the UI and skips the focus-drift abort path
/// for `InApp` sessions (Mockingbird is the focus the whole time,
/// so the heuristic doesn't apply).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartMode {
    /// Right Alt PTT or any other future keyboard-hook trigger.
    /// Default for legacy rows (pre-migration-017) and for the
    /// happy-path Right-Alt-hold flow.
    #[default]
    Ptt,
    /// Programmatic start via `dictation_start` IPC — e.g. the
    /// in-app Start Dictation button. There is no "target app";
    /// injection is intentionally skipped.
    InApp,
}

impl StartMode {
    /// Canonical DB string. Must match the value the UI renders
    /// against — see `ui/src/lib/types.ts` `StartMode`.
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Ptt => "ptt",
            Self::InApp => "in_app",
        }
    }

    /// Parse a DB string. Unknown values default to `Ptt` (the safe
    /// fallback — a row that mysteriously says "foobar" is at least
    /// not falsely promoted to in-app, which would suppress paste).
    pub fn parse_db(s: &str) -> Self {
        match s {
            "in_app" => Self::InApp,
            _ => Self::Ptt,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Recording,
    Processing,
    #[default]
    Complete,
    Error,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recording => "recording",
            Self::Processing => "processing",
            Self::Complete => "complete",
            Self::Error => "error",
        }
    }

    pub fn parse(s: &str) -> AppResult<Self> {
        match s {
            "recording" => Ok(Self::Recording),
            "processing" => Ok(Self::Processing),
            "complete" => Ok(Self::Complete),
            "error" => Ok(Self::Error),
            other => Err(AppError::Other(format!(
                "invalid session status: {other:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: i64,
    pub uuid: String,
    pub mode_id: i64,
    pub hotkey_pressed: String,
    pub started_at: String,
    pub recording_ended_at: String,
    pub processing_completed_at: Option<String>,
    pub status: SessionStatus,
    pub error_message: Option<String>,
    pub foreground_app: Option<String>,
    pub foreground_window_title: Option<String>,
    pub audio_duration_ms: i64,
    pub audio_blob_path: Option<String>,
    pub prompt_id: Option<i64>,
    pub dictionary_snapshot_id: Option<i64>,
    pub example_set_id: Option<i64>,
    pub stt_latency_ms: Option<i64>,
    pub cleanup_latency_ms: Option<i64>,
    pub injection_latency_ms: Option<i64>,
    /// Canonical injection outcome string. NULL on legacy rows and
    /// on currently-in-flight sessions. See migration 004.
    pub injection_status: Option<String>,
    /// ADR 0045 + mb-tfyp. NOT NULL with DEFAULT 'ptt' on disk
    /// (migration 017) — but kept as the typed enum here so callers
    /// can match without juggling strings.
    pub start_mode: StartMode,
    /// ADR 0046 + mb-jqhw. NOT NULL with DEFAULT 'desktop' on disk
    /// (migration 018). Same defensive-default-on-read pattern as
    /// `start_mode`.
    pub source: SessionSource,
    /// ADR 0052 + mb-pxzk. NOT NULL with DEFAULT 'dictation' on disk
    /// (migration 025). Same defensive-default-on-read pattern as
    /// `start_mode` / `source` — unknown DB strings fall back to
    /// `Dictation` to avoid false KG enqueues.
    pub capture_kind: CaptureKind,
    /// ADR 0052 + mb-oji5. NULL until the KG classify pass fills it
    /// in (only for `KgNote` sessions). The Phase 1C retrieval-by-
    /// category surface is its consumer.
    pub category: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ProcessingCompletion {
    pub completed_at: String,
    pub status: SessionStatus,
    pub stt_latency_ms: Option<i64>,
    pub cleanup_latency_ms: Option<i64>,
    pub injection_latency_ms: Option<i64>,
    /// Canonical injection outcome string (matches
    /// `InjectionOutcome::as_db_str`). `None` means "not applicable"
    /// — e.g. processing failed before the injector ran.
    pub injection_status: Option<String>,
}

/// Insert a new session row. All provenance FKs must point at real
/// rows in their respective tables (mode_id is FK-enforced by SQL;
/// the others are NULLable in SQL but mandatory at this API layer).
pub fn insert(conn: &Connection, new: &NewSession) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO sessions ( \
            uuid, mode_id, hotkey_pressed, started_at, recording_ended_at, \
            status, foreground_app, foreground_window_title, audio_duration_ms, \
            audio_blob_path, prompt_id, dictionary_snapshot_id, example_set_id, \
            start_mode, source, capture_kind \
         ) VALUES \
         (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            new.uuid,
            new.mode_id,
            new.hotkey_pressed,
            new.started_at,
            new.recording_ended_at,
            new.status.as_str(),
            new.foreground_app,
            new.foreground_window_title,
            new.audio_duration_ms,
            new.audio_blob_path,
            new.prompt_id,
            new.dictionary_snapshot_id,
            new.example_set_id,
            new.start_mode.as_db_str(),
            new.source.as_db_str(),
            new.capture_kind.as_db_str(),
            // NOTE: `category` intentionally NOT inserted here. The
            // KG worker fills it via UPDATE after the classify pass
            // completes; an explicit NULL at insert time leaves the
            // column at the DB default (NULL) without coupling the
            // dictation orchestrator to the KG classify result.
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_by_id(conn: &Connection, id: i64) -> AppResult<Option<Session>> {
    fetch_one(conn, "WHERE id = ?1", params![id])
}

pub fn get_by_uuid(conn: &Connection, uuid: &str) -> AppResult<Option<Session>> {
    fetch_one(conn, "WHERE uuid = ?1", params![uuid])
}

pub fn list_recent(conn: &Connection, limit: usize) -> AppResult<Vec<Session>> {
    let sql = format!("{} ORDER BY started_at DESC LIMIT ?1", SELECT_ALL);
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![limit as i64], row_to_session)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn update_processing_complete(
    conn: &Connection,
    id: i64,
    completion: &ProcessingCompletion,
) -> AppResult<()> {
    conn.execute(
        "UPDATE sessions SET \
            processing_completed_at = ?1, \
            status = ?2, \
            stt_latency_ms = ?3, \
            cleanup_latency_ms = ?4, \
            injection_latency_ms = ?5, \
            injection_status = ?6 \
         WHERE id = ?7",
        params![
            completion.completed_at,
            completion.status.as_str(),
            completion.stt_latency_ms,
            completion.cleanup_latency_ms,
            completion.injection_latency_ms,
            completion.injection_status,
            id,
        ],
    )?;
    Ok(())
}

/// mb-v2fa / ADR 0047 §Wave 2.5 -- set `edit_free_within_5min = 1`
/// on a session that just got injected successfully. The orchestrator
/// calls this right after `update_processing_complete` when
/// `InjectionOutcome` is one of the success variants
/// (`Ok` or `OkClipboardNotRestored`). For every other outcome
/// (aborts, failures, in-app, headless ingest) the column stays
/// NULL, which the Insights aggregation reads as "excluded from
/// the metric population".
///
/// This intentionally lives in one place so the "only success
/// injects are eligible" rule is enforced by the caller's match
/// rather than buried in SQL `CASE` logic.
pub fn mark_injected_for_edit_metric(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute(
        "UPDATE sessions SET edit_free_within_5min = 1 WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

/// mb-v2fa / ADR 0047 §Wave 2.5 -- record that the user did an
/// edit-equivalent action (LlmPassCard run, raw copy) on a
/// previously-injected session. Conditional UPDATE:
///
///   * No-op if `edit_free_within_5min` is NULL (the session never
///     injected -- not in the metric population) or already 0
///     (already counted; idempotent).
///   * No-op if `processing_completed_at` is more than 5 min ago.
///     The 5-min observation window is anchored to injection time,
///     not session creation; once it elapses the row's metric
///     value is locked in.
///   * Otherwise flip the column to 0.
///
/// SQLite's `datetime('now', '-5 minutes')` evaluates against
/// UTC, matching the ISO-8601 `Z`-suffixed strings
/// `update_processing_complete` writes via `dictation::now_iso`.
pub fn mark_edit_observed(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute(
        "UPDATE sessions \
         SET edit_free_within_5min = 0 \
         WHERE id = ?1 \
           AND edit_free_within_5min = 1 \
           AND processing_completed_at IS NOT NULL \
           AND datetime(processing_completed_at) >= datetime('now', '-5 minutes')",
        params![id],
    )?;
    Ok(())
}

pub fn update_status_error(conn: &Connection, id: i64, error_message: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE sessions SET status = 'error', error_message = ?1 WHERE id = ?2",
        params![error_message, id],
    )?;
    Ok(())
}

const SELECT_ALL: &str =
    "SELECT id, uuid, mode_id, hotkey_pressed, started_at, recording_ended_at, \
            processing_completed_at, status, error_message, foreground_app, \
            foreground_window_title, audio_duration_ms, audio_blob_path, \
            prompt_id, dictionary_snapshot_id, example_set_id, \
            stt_latency_ms, cleanup_latency_ms, injection_latency_ms, \
            injection_status, start_mode, source, capture_kind, category \
     FROM sessions";

fn fetch_one(
    conn: &Connection,
    where_clause: &str,
    params: impl rusqlite::Params,
) -> AppResult<Option<Session>> {
    let sql = format!("{SELECT_ALL} {where_clause}");
    let mut stmt = conn.prepare(&sql)?;
    let session = stmt.query_row(params, row_to_session).optional()?;
    Ok(session)
}

fn row_to_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    let status_str: String = row.get(7)?;
    let status = SessionStatus::parse(&status_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            7,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            )),
        )
    })?;
    Ok(Session {
        id: row.get(0)?,
        uuid: row.get(1)?,
        mode_id: row.get(2)?,
        hotkey_pressed: row.get(3)?,
        started_at: row.get(4)?,
        recording_ended_at: row.get(5)?,
        processing_completed_at: row.get(6)?,
        status,
        error_message: row.get(8)?,
        foreground_app: row.get(9)?,
        foreground_window_title: row.get(10)?,
        audio_duration_ms: row.get(11)?,
        audio_blob_path: row.get(12)?,
        prompt_id: row.get(13)?,
        dictionary_snapshot_id: row.get(14)?,
        example_set_id: row.get(15)?,
        stt_latency_ms: row.get(16)?,
        cleanup_latency_ms: row.get(17)?,
        injection_latency_ms: row.get(18)?,
        injection_status: row.get(19)?,
        start_mode: {
            // NOT NULL on disk (migration 017 DEFAULT 'ptt'), but be
            // defensive against pre-017 rows in case a downgrade ever
            // re-runs this code against an older schema.
            let s: Option<String> = row.get(20)?;
            s.as_deref().map(StartMode::parse_db).unwrap_or_default()
        },
        source: {
            // NOT NULL on disk (migration 018 DEFAULT 'desktop'), same
            // defensive-default rationale as start_mode above.
            let s: Option<String> = row.get(21)?;
            s.as_deref()
                .map(SessionSource::parse_db)
                .unwrap_or_default()
        },
        capture_kind: {
            // NOT NULL on disk (migration 025 DEFAULT 'dictation'),
            // same defensive-default rationale as start_mode/source.
            let s: Option<String> = row.get(22)?;
            s.as_deref().map(CaptureKind::parse_db).unwrap_or_default()
        },
        category: row.get(23)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    /// Build a NewSession with valid seeded mode_id=1 and freshly-inserted
    /// snapshot + example_set rows so FK constraints pass.
    fn fresh_new_session(conn: &Connection) -> NewSession {
        // Provenance prerequisites via raw INSERTs (we're testing
        // sessions.rs, not dictionary/examples).
        conn.execute(
            "INSERT INTO dictionary_snapshots (term_ids) VALUES ('[]')",
            [],
        )
        .unwrap();
        let snapshot_id = conn.last_insert_rowid();

        conn.execute(
            "INSERT INTO example_sets (mode_slug, example_ids) VALUES ('normal', '[]')",
            [],
        )
        .unwrap();
        let example_set_id = conn.last_insert_rowid();

        // prompt_id from seed (migration 003 inserted version-1 of each mode).
        let prompt_id: i64 = conn
            .query_row(
                "SELECT id FROM prompts WHERE mode_slug='normal' AND version=1",
                [],
                |r| r.get(0),
            )
            .unwrap();

        NewSession {
            uuid: uuid::Uuid::new_v4().to_string(),
            mode_id: 1,
            hotkey_pressed: "Ctrl+Win".into(),
            started_at: "2026-05-15T00:00:00Z".into(),
            recording_ended_at: "2026-05-15T00:00:05Z".into(),
            status: SessionStatus::Recording,
            foreground_app: Some("notepad.exe".into()),
            foreground_window_title: Some("Untitled - Notepad".into()),
            audio_duration_ms: 5000,
            audio_blob_path: None,
            prompt_id,
            dictionary_snapshot_id: snapshot_id,
            example_set_id,
            start_mode: StartMode::Ptt,
            source: SessionSource::Desktop,
            capture_kind: CaptureKind::Dictation,
        }
    }

    #[test]
    fn capture_kind_db_strings_are_stable() {
        // These end up in sessions.capture_kind and become part of
        // the persisted provenance + the dictation-tail KG source-
        // gate's match arm — changing them is a schema break AND a
        // behaviour break.
        assert_eq!(CaptureKind::Dictation.as_db_str(), "dictation");
        assert_eq!(CaptureKind::KgNote.as_db_str(), "kg-note");
        assert_eq!(CaptureKind::KgNoteText.as_db_str(), "kg-note-text");
        assert_eq!(CaptureKind::parse_db("dictation"), CaptureKind::Dictation);
        assert_eq!(CaptureKind::parse_db("kg-note"), CaptureKind::KgNote);
        assert_eq!(
            CaptureKind::parse_db("kg-note-text"),
            CaptureKind::KgNoteText
        );
        // Unknown values fall back to Dictation — the safe default
        // (an unknown string MUST NOT be falsely promoted to KgNote,
        // which would trigger KG enqueue when the toggle was on).
        assert_eq!(CaptureKind::parse_db("bogus"), CaptureKind::Dictation);
        // Default trait yields Dictation.
        assert_eq!(CaptureKind::default(), CaptureKind::Dictation);
    }

    #[test]
    fn insert_and_read_kg_note_capture_kind() {
        let db = Database::open_in_memory().unwrap();
        let mut new = fresh_new_session(&db.conn);
        new.capture_kind = CaptureKind::KgNote;
        let id = insert(&db.conn, &new).unwrap();
        let got = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(got.capture_kind, CaptureKind::KgNote);
        // category is NULL at insert time (worker fills it post-classify).
        assert!(got.category.is_none(), "category must be NULL at insert");
    }

    #[test]
    fn capture_kind_defaults_to_dictation_on_legacy_rows() {
        // Simulate pre-migration-025 rows: bypass the NewSession API
        // and INSERT manually omitting capture_kind. The column's
        // DEFAULT 'dictation' must produce CaptureKind::Dictation on
        // read. Mirrors the source_defaults_to_desktop_on_legacy_rows
        // pattern above.
        let db = Database::open_in_memory().unwrap();
        let new = fresh_new_session(&db.conn);
        db.conn
            .execute(
                "INSERT INTO sessions ( \
                    uuid, mode_id, hotkey_pressed, started_at, recording_ended_at, \
                    status, foreground_app, audio_duration_ms, \
                    prompt_id, dictionary_snapshot_id, example_set_id \
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    "legacy-capture-kind-uuid",
                    new.mode_id,
                    new.hotkey_pressed,
                    new.started_at,
                    new.recording_ended_at,
                    new.status.as_str(),
                    new.foreground_app,
                    new.audio_duration_ms,
                    new.prompt_id,
                    new.dictionary_snapshot_id,
                    new.example_set_id,
                ],
            )
            .unwrap();
        let id = db.conn.last_insert_rowid();
        let got = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(got.capture_kind, CaptureKind::Dictation);
        assert!(got.category.is_none());
    }

    #[test]
    fn session_source_db_strings_are_stable() {
        // These end up in sessions.source and become part of the
        // persisted provenance — changing them is a schema break.
        assert_eq!(SessionSource::Desktop.as_db_str(), "desktop");
        assert_eq!(SessionSource::DesktopImport.as_db_str(), "desktop-import");
        assert_eq!(SessionSource::MobileInbox.as_db_str(), "mobile-inbox");
        assert_eq!(SessionSource::parse_db("desktop"), SessionSource::Desktop);
        assert_eq!(
            SessionSource::parse_db("desktop-import"),
            SessionSource::DesktopImport
        );
        assert_eq!(
            SessionSource::parse_db("mobile-inbox"),
            SessionSource::MobileInbox
        );
        // Unknown values fall back to the safe default.
        assert_eq!(SessionSource::parse_db("bogus"), SessionSource::Desktop);
    }

    #[test]
    fn insert_and_read_desktop_import_source() {
        let db = Database::open_in_memory().unwrap();
        let mut new = fresh_new_session(&db.conn);
        new.source = SessionSource::DesktopImport;
        let id = insert(&db.conn, &new).unwrap();
        let got = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(got.source, SessionSource::DesktopImport);
    }

    #[test]
    fn source_defaults_to_desktop_on_legacy_rows() {
        // Simulate pre-migration-018 rows: bypass the NewSession API
        // and INSERT manually omitting source. Because the column has
        // DEFAULT 'desktop', the read should produce SessionSource::Desktop.
        let db = Database::open_in_memory().unwrap();
        let new = fresh_new_session(&db.conn);
        db.conn
            .execute(
                "INSERT INTO sessions ( \
                    uuid, mode_id, hotkey_pressed, started_at, recording_ended_at, \
                    status, foreground_app, audio_duration_ms, \
                    prompt_id, dictionary_snapshot_id, example_set_id \
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    "legacy-source-uuid",
                    new.mode_id,
                    new.hotkey_pressed,
                    new.started_at,
                    new.recording_ended_at,
                    new.status.as_str(),
                    new.foreground_app,
                    new.audio_duration_ms,
                    new.prompt_id,
                    new.dictionary_snapshot_id,
                    new.example_set_id,
                ],
            )
            .unwrap();
        let id = db.conn.last_insert_rowid();
        let got = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(got.source, SessionSource::Desktop);
    }

    #[test]
    fn start_mode_db_strings_are_stable() {
        // These end up in sessions.start_mode and become part of the
        // persisted provenance — changing them is a schema break.
        assert_eq!(StartMode::Ptt.as_db_str(), "ptt");
        assert_eq!(StartMode::InApp.as_db_str(), "in_app");
        assert_eq!(StartMode::parse_db("ptt"), StartMode::Ptt);
        assert_eq!(StartMode::parse_db("in_app"), StartMode::InApp);
        // Unknown values fall back to the safe default.
        assert_eq!(StartMode::parse_db("bogus"), StartMode::Ptt);
    }

    #[test]
    fn insert_and_read_in_app_start_mode() {
        let db = Database::open_in_memory().unwrap();
        let mut new = fresh_new_session(&db.conn);
        new.start_mode = StartMode::InApp;
        let id = insert(&db.conn, &new).unwrap();
        let got = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(got.start_mode, StartMode::InApp);
    }

    #[test]
    fn start_mode_defaults_to_ptt_on_legacy_rows() {
        // Simulate pre-migration-017 rows: bypass the NewSession API
        // and INSERT manually omitting start_mode. Because the
        // column has DEFAULT 'ptt', the read should still produce Ptt.
        let db = Database::open_in_memory().unwrap();
        let new = fresh_new_session(&db.conn);
        db.conn
            .execute(
                "INSERT INTO sessions ( \
                    uuid, mode_id, hotkey_pressed, started_at, recording_ended_at, \
                    status, foreground_app, audio_duration_ms, \
                    prompt_id, dictionary_snapshot_id, example_set_id \
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    "legacy-uuid",
                    new.mode_id,
                    new.hotkey_pressed,
                    new.started_at,
                    new.recording_ended_at,
                    new.status.as_str(),
                    new.foreground_app,
                    new.audio_duration_ms,
                    new.prompt_id,
                    new.dictionary_snapshot_id,
                    new.example_set_id,
                ],
            )
            .unwrap();
        let id = db.conn.last_insert_rowid();
        let got = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(got.start_mode, StartMode::Ptt);
    }

    #[test]
    fn insert_and_round_trip() {
        let db = Database::open_in_memory().unwrap();
        let new = fresh_new_session(&db.conn);
        let uuid_in = new.uuid.clone();
        let id = insert(&db.conn, &new).unwrap();
        let got = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(got.uuid, uuid_in);
        assert_eq!(got.status, SessionStatus::Recording);
        assert_eq!(got.foreground_app.as_deref(), Some("notepad.exe"));
    }

    #[test]
    fn duplicate_uuid_errors() {
        let db = Database::open_in_memory().unwrap();
        let new = fresh_new_session(&db.conn);
        insert(&db.conn, &new).unwrap();
        let err = insert(&db.conn, &new).unwrap_err();
        assert!(matches!(err, AppError::Sqlite(_)));
    }

    #[test]
    fn get_by_uuid_returns_none_for_missing() {
        let db = Database::open_in_memory().unwrap();
        assert!(get_by_uuid(&db.conn, "no-such-uuid").unwrap().is_none());
    }

    #[test]
    fn list_recent_orders_by_started_at_desc() {
        let db = Database::open_in_memory().unwrap();
        let mut new1 = fresh_new_session(&db.conn);
        new1.started_at = "2026-05-15T00:00:00Z".into();
        insert(&db.conn, &new1).unwrap();

        let mut new2 = fresh_new_session(&db.conn);
        new2.started_at = "2026-05-15T01:00:00Z".into();
        insert(&db.conn, &new2).unwrap();

        let list = list_recent(&db.conn, 10).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].started_at, "2026-05-15T01:00:00Z");
        assert_eq!(list[1].started_at, "2026-05-15T00:00:00Z");
    }

    #[test]
    fn update_processing_complete_sets_latencies_and_injection_status() {
        let db = Database::open_in_memory().unwrap();
        let new = fresh_new_session(&db.conn);
        let id = insert(&db.conn, &new).unwrap();
        update_processing_complete(
            &db.conn,
            id,
            &ProcessingCompletion {
                completed_at: "2026-05-15T00:00:10Z".into(),
                status: SessionStatus::Complete,
                stt_latency_ms: Some(150),
                cleanup_latency_ms: Some(800),
                injection_latency_ms: Some(20),
                injection_status: Some("ok".into()),
            },
        )
        .unwrap();
        let got = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(got.status, SessionStatus::Complete);
        assert_eq!(got.stt_latency_ms, Some(150));
        assert_eq!(got.cleanup_latency_ms, Some(800));
        assert_eq!(got.injection_latency_ms, Some(20));
        assert_eq!(got.injection_status.as_deref(), Some("ok"));
    }

    #[test]
    fn injection_status_persists_aborted_secure() {
        // Provenance check: a secure-input abort must round-trip.
        let db = Database::open_in_memory().unwrap();
        let new = fresh_new_session(&db.conn);
        let id = insert(&db.conn, &new).unwrap();
        update_processing_complete(
            &db.conn,
            id,
            &ProcessingCompletion {
                completed_at: "2026-05-15T00:00:10Z".into(),
                status: SessionStatus::Complete,
                stt_latency_ms: Some(100),
                cleanup_latency_ms: Some(50),
                injection_latency_ms: None, // never tried
                injection_status: Some("aborted_secure".into()),
            },
        )
        .unwrap();
        let got = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(got.injection_status.as_deref(), Some("aborted_secure"));
        assert_eq!(got.injection_latency_ms, None);
    }

    #[test]
    fn injection_status_starts_null_until_processing_completes() {
        let db = Database::open_in_memory().unwrap();
        let new = fresh_new_session(&db.conn);
        let id = insert(&db.conn, &new).unwrap();
        let got = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(got.injection_status, None);
    }

    #[test]
    fn update_status_error_sets_status_and_message() {
        let db = Database::open_in_memory().unwrap();
        let new = fresh_new_session(&db.conn);
        let id = insert(&db.conn, &new).unwrap();
        update_status_error(&db.conn, id, "stt failed: cuda OOM").unwrap();
        let got = get_by_id(&db.conn, id).unwrap().unwrap();
        assert_eq!(got.status, SessionStatus::Error);
        assert_eq!(got.error_message.as_deref(), Some("stt failed: cuda OOM"));
    }

    #[test]
    fn session_status_parse_round_trips() {
        for s in [
            SessionStatus::Recording,
            SessionStatus::Processing,
            SessionStatus::Complete,
            SessionStatus::Error,
        ] {
            assert_eq!(SessionStatus::parse(s.as_str()).unwrap(), s);
        }
        assert!(SessionStatus::parse("BOGUS").is_err());
    }

    /// Helper: read the raw `edit_free_within_5min` value as `Option<i64>`
    /// so tests can distinguish NULL / 0 / 1 explicitly.
    fn read_edit_free(conn: &Connection, id: i64) -> Option<i64> {
        conn.query_row(
            "SELECT edit_free_within_5min FROM sessions WHERE id = ?1",
            params![id],
            |r| r.get::<_, Option<i64>>(0),
        )
        .unwrap()
    }

    #[test]
    fn edit_free_within_5min_defaults_to_null_on_insert() {
        let db = Database::open_in_memory().unwrap();
        let new = fresh_new_session(&db.conn);
        let id = insert(&db.conn, &new).unwrap();
        assert_eq!(
            read_edit_free(&db.conn, id),
            None,
            "new rows must read as NULL until injection succeeds"
        );
    }

    #[test]
    fn mark_injected_for_edit_metric_sets_one() {
        let db = Database::open_in_memory().unwrap();
        let new = fresh_new_session(&db.conn);
        let id = insert(&db.conn, &new).unwrap();
        mark_injected_for_edit_metric(&db.conn, id).unwrap();
        assert_eq!(read_edit_free(&db.conn, id), Some(1));
    }

    #[test]
    fn mark_edit_observed_flips_one_to_zero_within_window() {
        let db = Database::open_in_memory().unwrap();
        let new = fresh_new_session(&db.conn);
        let id = insert(&db.conn, &new).unwrap();
        mark_injected_for_edit_metric(&db.conn, id).unwrap();
        // Anchor processing_completed_at to NOW so we're well inside
        // the 5-min window. SQLite's datetime('now') yields the
        // canonical UTC string the production code writes via
        // dictation::now_iso (modulo seconds precision).
        db.conn
            .execute(
                "UPDATE sessions SET processing_completed_at = datetime('now') WHERE id = ?1",
                params![id],
            )
            .unwrap();
        mark_edit_observed(&db.conn, id).unwrap();
        assert_eq!(read_edit_free(&db.conn, id), Some(0));
    }

    #[test]
    fn mark_edit_observed_is_noop_outside_window() {
        let db = Database::open_in_memory().unwrap();
        let new = fresh_new_session(&db.conn);
        let id = insert(&db.conn, &new).unwrap();
        mark_injected_for_edit_metric(&db.conn, id).unwrap();
        // Age the row 10 minutes -- past the 5-min window.
        db.conn
            .execute(
                "UPDATE sessions SET processing_completed_at = datetime('now', '-10 minutes') \
                 WHERE id = ?1",
                params![id],
            )
            .unwrap();
        mark_edit_observed(&db.conn, id).unwrap();
        assert_eq!(
            read_edit_free(&db.conn, id),
            Some(1),
            "outside the 5-min window the metric stays locked at 1"
        );
    }

    #[test]
    fn mark_edit_observed_is_noop_when_never_injected() {
        // Session was created but never had mark_injected_for_edit_metric
        // called -- e.g. in-app, abort, file import. mark_edit_observed
        // must leave NULL untouched. This is the cross-check that the
        // Insights aggregation can rely on NULL == "not in the
        // population" and not have to filter on injection_status
        // separately.
        let db = Database::open_in_memory().unwrap();
        let new = fresh_new_session(&db.conn);
        let id = insert(&db.conn, &new).unwrap();
        db.conn
            .execute(
                "UPDATE sessions SET processing_completed_at = datetime('now') WHERE id = ?1",
                params![id],
            )
            .unwrap();
        mark_edit_observed(&db.conn, id).unwrap();
        assert_eq!(read_edit_free(&db.conn, id), None);
    }

    #[test]
    fn mark_edit_observed_is_idempotent_after_first_flip() {
        let db = Database::open_in_memory().unwrap();
        let new = fresh_new_session(&db.conn);
        let id = insert(&db.conn, &new).unwrap();
        mark_injected_for_edit_metric(&db.conn, id).unwrap();
        db.conn
            .execute(
                "UPDATE sessions SET processing_completed_at = datetime('now') WHERE id = ?1",
                params![id],
            )
            .unwrap();
        mark_edit_observed(&db.conn, id).unwrap();
        mark_edit_observed(&db.conn, id).unwrap();
        assert_eq!(read_edit_free(&db.conn, id), Some(0));
    }

    #[test]
    fn insert_with_bad_mode_id_errors_via_fk() {
        let db = Database::open_in_memory().unwrap();
        let mut new = fresh_new_session(&db.conn);
        new.mode_id = 99_999;
        let err = insert(&db.conn, &new).unwrap_err();
        assert!(matches!(err, AppError::Sqlite(_)));
    }
}
