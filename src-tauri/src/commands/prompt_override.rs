//! Per-mode user PROMPT-override IPC (ADR 0067).
//!
//! Lets the macOS Modes screen show + edit the cleanup prompt for a
//! dictation mode. The shipped prompt bodies live in the immutable,
//! migration-seeded `prompts` table (append-only per ADR 0008); a user
//! edit is stored SEPARATELY in `mode_prompt_overrides` (migration 030),
//! so the shipped defaults stay the source of truth and revert is a
//! simple DELETE. This mirrors the model-override layer (ADR 0066) one
//! dimension over: model -> prompt.
//!
//! ## Precedence (dictation time)
//!
//!   user prompt override  >  macOS tier substitution (`normal_small`)
//!                          >  mode default (`prompts` latest version)
//!
//! i.e. if the user has authored their own prompt, it is used VERBATIM
//! and the small-model tier substitution is skipped (an explicit choice
//! beats the heuristic). With no override, behaviour is exactly today's
//! on every platform. The dictation-time injection happens only at the
//! macOS effective-model seam (`dictation/runtime_cleaner.rs`, behind
//! `#[cfg(target_os = "macos")]`), so Windows never reads this table and
//! its cleanup path is byte-identical.
//!
//! These commands are cross-platform-safe: the table exists everywhere
//! (the migration runs everywhere) but is only ever WRITTEN by the
//! isMac-gated Modes prompt editor.

use serde::Serialize;
use tauri::State;

use crate::commands::{into_err, lock_db, AppStateHandle};
use crate::db::prompts;

/// What the Modes prompt editor needs to render truthfully.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectivePromptDto {
    /// The shipped default prompt body for the mode — the latest version
    /// in the immutable `prompts` table. This is what a Revert restores.
    pub default_body: String,
    /// The shipped default's version (e.g. `5`), for a "shipped default
    /// vN" label.
    pub default_version: i64,
    /// The body that will be edited / is active: the user override if one
    /// exists, else `default_body`.
    pub effective_body: String,
    /// Whether a user override row exists for this mode (drives the
    /// "Custom" vs "Shipped default" badge + the Revert affordance).
    pub is_overridden: bool,
}

/// Read the user's per-mode prompt override, if any.
fn read_override(db: &State<'_, AppStateHandle>, slug: &str) -> Result<Option<String>, String> {
    let conn = lock_db(db)?;
    conn.query_row(
        "SELECT prompt_body FROM mode_prompt_overrides WHERE mode_slug = ?1",
        [slug],
        |r| r.get::<_, String>(0),
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(into_err(other)),
    })
}

/// Resolve the effective prompt for a mode, for the editor.
///
/// `default_body` / `default_version` are the shipped latest prompt for
/// the mode; `effective_body` is the override if present, else the
/// default. Mirrors the dictation-time precedence for the editor's
/// view (minus the macOS tier substitution, which is an internal
/// small-model hardening the editor deliberately does not surface — the
/// user edits the mode's canonical prompt).
#[tauri::command]
pub fn get_effective_prompt(
    db: State<'_, AppStateHandle>,
    slug: String,
) -> Result<EffectivePromptDto, String> {
    let (default_body, default_version) = {
        let conn = lock_db(&db)?;
        let prompt = prompts::get_latest_for_mode(&conn, &slug)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("no shipped prompt for mode {slug:?}"))?;
        (prompt.body, prompt.version)
    };
    let override_body = read_override(&db, &slug)?;
    let is_overridden = override_body.is_some();
    let effective_body = override_body.unwrap_or_else(|| default_body.clone());

    Ok(EffectivePromptDto {
        default_body,
        default_version,
        effective_body,
        is_overridden,
    })
}

/// Persist a user prompt override for a mode (upsert). Used verbatim at
/// dictation time, bypassing the small-model tier substitution.
///
/// Rejects an empty/whitespace-only body — an empty prompt would make
/// the cleanup call meaningless; the user should Revert instead.
#[tauri::command]
pub fn set_mode_prompt_override(
    db: State<'_, AppStateHandle>,
    slug: String,
    body: String,
) -> Result<(), String> {
    if body.trim().is_empty() {
        return Err("prompt body must not be empty (use Revert to restore the default)".into());
    }
    let conn = lock_db(&db)?;
    conn.execute(
        "INSERT INTO mode_prompt_overrides (mode_slug, prompt_body) VALUES (?1, ?2) \
         ON CONFLICT(mode_slug) DO UPDATE SET prompt_body = excluded.prompt_body, \
         updated_at = strftime('%s', 'now')",
        rusqlite::params![slug, body],
    )
    .map_err(into_err)?;
    tracing::info!(mode = %slug, "set per-mode prompt override (ADR 0067)");
    Ok(())
}

/// Clear a mode's prompt override -> revert to the shipped default.
#[tauri::command]
pub fn clear_mode_prompt_override(
    db: State<'_, AppStateHandle>,
    slug: String,
) -> Result<(), String> {
    let conn = lock_db(&db)?;
    conn.execute(
        "DELETE FROM mode_prompt_overrides WHERE mode_slug = ?1",
        [&slug],
    )
    .map_err(into_err)?;
    tracing::info!(mode = %slug, "cleared per-mode prompt override -> shipped default (ADR 0067)");
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Logic-level coverage of the override read/write SQL against a real
    //! migrated in-memory DB. The `#[tauri::command]` wrappers are thin;
    //! the table semantics are what matter (upsert + revert + verbatim).

    use crate::db::{prompts, Database};
    use rusqlite::Connection;

    fn upsert(conn: &Connection, slug: &str, body: &str) {
        conn.execute(
            "INSERT INTO mode_prompt_overrides (mode_slug, prompt_body) VALUES (?1, ?2) \
             ON CONFLICT(mode_slug) DO UPDATE SET prompt_body = excluded.prompt_body, \
             updated_at = strftime('%s', 'now')",
            rusqlite::params![slug, body],
        )
        .unwrap();
    }

    fn read(conn: &Connection, slug: &str) -> Option<String> {
        conn.query_row(
            "SELECT prompt_body FROM mode_prompt_overrides WHERE mode_slug = ?1",
            [slug],
            |r| r.get::<_, String>(0),
        )
        .ok()
    }

    #[test]
    fn no_override_means_shipped_default_is_used() {
        let db = Database::open_in_memory().unwrap();
        assert!(read(&db.conn, "normal").is_none());
        // Sanity: a shipped default exists to fall back to.
        assert!(prompts::get_latest_for_mode(&db.conn, "normal")
            .unwrap()
            .is_some());
    }

    #[test]
    fn upsert_then_revert_roundtrips() {
        let db = Database::open_in_memory().unwrap();
        upsert(&db.conn, "normal", "My custom prompt v1");
        assert_eq!(
            read(&db.conn, "normal").as_deref(),
            Some("My custom prompt v1")
        );
        // Upsert again — replaces, does not duplicate (PK).
        upsert(&db.conn, "normal", "My custom prompt v2");
        assert_eq!(
            read(&db.conn, "normal").as_deref(),
            Some("My custom prompt v2")
        );
        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM mode_prompt_overrides WHERE mode_slug = 'normal'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "upsert must not duplicate the PK row");
        // Revert.
        db.conn
            .execute(
                "DELETE FROM mode_prompt_overrides WHERE mode_slug = 'normal'",
                [],
            )
            .unwrap();
        assert!(read(&db.conn, "normal").is_none());
    }

    #[test]
    fn shipped_prompts_table_is_never_mutated_by_an_override() {
        let db = Database::open_in_memory().unwrap();
        let before = prompts::get_latest_for_mode(&db.conn, "normal")
            .unwrap()
            .unwrap();
        upsert(&db.conn, "normal", "totally different");
        let after = prompts::get_latest_for_mode(&db.conn, "normal")
            .unwrap()
            .unwrap();
        // The shipped default is unchanged — the override lives in a
        // separate table (the load-bearing immutability guarantee).
        assert_eq!(before.body, after.body);
        assert_eq!(before.version, after.version);
    }
}
