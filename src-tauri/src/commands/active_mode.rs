//! Active-mode IPC: lets the Modes page pick which transcription
//! mode the orchestrator uses for upcoming dictations.
//!
//! ## Model
//!
//! "Active mode" = the slug of the mode the orchestrator resolves at
//! the START of every new dictation session. Stored in the
//! `settings` table under [`ACTIVE_MODE_KEY`] — single source of
//! truth, no in-memory cache to invalidate. The orchestrator reads
//! it fresh each session (one indexed-PK lookup, negligible cost),
//! which means a `set_active_mode` call takes effect on the NEXT
//! Right-Alt hold without any restart, signalling, or refcount
//! dance.
//!
//! ## Why only transcription modes
//!
//! Mockingbird has two mode classes:
//!
//! - **Transcription modes** — `normal`, `verbose`, `fragment`. These
//!   own the Right-Alt hotkey: exactly ONE is active at a time, and
//!   the active one defines the cleanup prompt for whatever the user
//!   dictates next.
//! - **AI command modes** — `rewrite`, `expand`, `summarize`. These
//!   act on already-existing text (clipboard / selection) and are
//!   invoked by their own hotkeys when enabled. They are NOT
//!   candidates for the active-mode setting.
//!
//! `set_active_mode` rejects any slug outside [`TRANSCRIPTION_SLUGS`]
//! to prevent the UI from accidentally pointing Right-Alt at, say,
//! `summarize` (which has no audio input concept).
//!
//! The allowlist is a const slice because the set is small + stable.
//! If we add a fourth transcription mode in Phase 6, this list grows
//! by one entry — no schema change required.

use serde::Serialize;
use tauri::State;

use crate::commands::{into_err, lock_db, AppStateHandle};

/// Settings-table key under which the active transcription-mode slug
/// is stored. Public so the orchestrator can read the same constant.
pub const ACTIVE_MODE_KEY: &str = "dictation.active_mode_slug";

/// Default active mode when the settings row is missing (fresh install
/// or first run after the migration that introduced this setting).
pub const DEFAULT_ACTIVE_MODE: &str = "normal";

/// Slugs eligible to be set as the active transcription mode.
/// AI command modes are intentionally excluded — see module docs.
pub const TRANSCRIPTION_SLUGS: &[&str] = &["normal", "verbose", "fragment"];

/// Response shape for [`get_active_mode`].
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveMode {
    /// Currently-active transcription mode slug.
    pub slug: String,
}

#[tauri::command]
pub fn get_active_mode(db: State<'_, AppStateHandle>) -> Result<ActiveMode, String> {
    let conn = lock_db(&db)?;
    let slug: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [ACTIVE_MODE_KEY],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| DEFAULT_ACTIVE_MODE.into());
    Ok(ActiveMode { slug })
}

#[tauri::command]
pub fn set_active_mode(
    db: State<'_, AppStateHandle>,
    slug: String,
) -> Result<(), String> {
    // Allowlist check FIRST — before we touch the DB. Returns a
    // clear error the UI can surface; no half-applied state.
    if !TRANSCRIPTION_SLUGS.contains(&slug.as_str()) {
        return Err(format!(
            "slug '{slug}' is not a transcription mode (allowed: {})",
            TRANSCRIPTION_SLUGS.join(", ")
        ));
    }
    let conn = lock_db(&db)?;
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [ACTIVE_MODE_KEY, slug.as_str()],
    )
    .map_err(into_err)?;
    Ok(())
}
