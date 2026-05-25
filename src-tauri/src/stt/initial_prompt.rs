//! DB-aware wrapper that builds a Whisper `initial_prompt` from the
//! live dictionary table.
//!
//! ## Why this lives in its own module
//!
//! [`crate::stt::prompt_builder`] is deliberately pure — it takes
//! borrowed [`DictionaryView`]s and returns a scored, packed prompt
//! string. It knows nothing about SQLite, mutexes, or the rest of
//! the runtime. That purity is what lets it carry the property-test
//! battery in its own module.
//!
//! Both production call sites (PTT live mic in
//! [`crate::dictation::DictationOrchestrator::complete`] and headless
//! file ingest in [`crate::dictation::ingest::headless_ingest`]) need
//! the same "lock the DB, snapshot dictionary, build prompt" recipe.
//! Putting that recipe inside the pure prompt-builder module would
//! couple it to `rusqlite` and the `db::dictionary` row shape;
//! putting it inside [`crate::dictation`] would duplicate a tiny bit
//! of plumbing between the live and headless paths. Hence a sibling
//! module under `stt::` — same parent crate as the function it wraps,
//! one tiny seam between purity and runtime.
//!
//! ## Failure mode
//!
//! Returns [`None`] on every error path:
//!
//!   * DB mutex poisoned
//!   * `dictionary::list_all` returns an error
//!   * dictionary is empty
//!   * pure builder returns `None` (e.g. all entries scored
//!     identically to zero, or the token cap is undercut by even the
//!     shortest term)
//!
//! `None` is the safe failure: Whisper transcribes without bias,
//! which is exactly the prior (Phase-4-TODO) behaviour. Logging is
//! `tracing::warn!` on the unexpected paths (mutex / query) and
//! `tracing::debug!` on the expected ones (empty dict, no prompt
//! produced).

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::stt::prompt_builder::{build_prompt, DictionaryView, PromptBuilderInput};

/// Snapshot the dictionary table and build a Whisper `initial_prompt`.
///
/// `foreground_app` is the process basename of the currently-focused
/// window (e.g. `"chrome.exe"`). When `Some(...)`, dictionary entries
/// whose `app_context` matches get an [`APP_MATCH_BOOST`] multiplier
/// inside the pure builder. Pass `None` for the headless-ingest path,
/// where there is no target app — entries score on
/// recency × frequency only.
///
/// [`APP_MATCH_BOOST`]: crate::stt::prompt_builder
pub fn build_from_db(db: &Arc<Mutex<Connection>>, foreground_app: Option<&str>) -> Option<String> {
    let conn = match db.lock() {
        Ok(g) => g,
        Err(_) => {
            tracing::warn!("whisper-initial-prompt: db mutex poisoned; skipping initial_prompt");
            return None;
        }
    };
    let entries = match crate::db::dictionary::list_all(&conn) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                error = ?e,
                "whisper-initial-prompt: dictionary lookup failed; skipping initial_prompt"
            );
            return None;
        }
    };
    drop(conn);

    if entries.is_empty() {
        tracing::debug!("whisper-initial-prompt: empty dictionary; no initial_prompt");
        return None;
    }

    // Materialize the borrow targets first so the views can borrow
    // into `entries` without lifetime gymnastics at the call site.
    let views: Vec<DictionaryView<'_>> = entries
        .iter()
        .map(|e| DictionaryView {
            term: &e.term,
            canonical: e.canonical.as_deref(),
            use_count: e.use_count,
            last_used_at: e.last_used_at.as_deref(),
            app_context: e.app_context.as_deref(),
        })
        .collect();

    build_prompt(&PromptBuilderInput {
        dictionary: &views,
        foreground_app,
        recent_transcripts: &[],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::db::dictionary::{self, NewDictionaryEntry};
    use crate::db::migrations;

    /// Open a fresh in-memory DB with all migrations applied, wrapped
    /// in `Arc<Mutex<...>>` so it shape-matches what
    /// `DictationOrchestrator::db` looks like at runtime.
    fn in_memory_db() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("pragma fk");
        migrations::apply_all(&conn).expect("apply migrations");
        Arc::new(Mutex::new(conn))
    }

    #[test]
    fn empty_dictionary_returns_none() {
        let db = in_memory_db();
        assert!(build_from_db(&db, None).is_none());
        assert!(build_from_db(&db, Some("chrome.exe")).is_none());
    }

    #[test]
    fn populated_dictionary_produces_some_prompt_containing_canonical_term() {
        let db = in_memory_db();
        {
            let conn = db.lock().expect("lock db");
            dictionary::insert(
                &conn,
                &NewDictionaryEntry {
                    term: "mockingbird".into(),
                    canonical: Some("Mockingbird".into()),
                    source: "manual".into(),
                    confidence: Some(1.0),
                    app_context: None,
                },
            )
            .expect("insert dict entry");
        }

        let prompt = build_from_db(&db, None).expect("prompt produced");
        // The pure builder writes `canonical` (when present) into the
        // packed string. Asserting on substring is enough to prove
        // the wiring landed — the pure prompt_builder module owns the
        // assembly-format invariants.
        assert!(
            prompt.contains("Mockingbird"),
            "prompt should contain the canonical form, got: {prompt:?}"
        );
    }

    #[test]
    fn app_context_boost_is_honoured_via_pure_builder() {
        // Two entries with identical use_count; the one whose
        // app_context matches the supplied foreground_app should sort
        // ahead of the other in the packed prompt (recency tiebreak
        // is identical because both rows are inserted in the same
        // millisecond window).
        let db = in_memory_db();
        {
            let conn = db.lock().expect("lock db");
            dictionary::insert(
                &conn,
                &NewDictionaryEntry {
                    term: "zeta".into(),
                    canonical: None,
                    source: "manual".into(),
                    confidence: None,
                    app_context: None,
                },
            )
            .expect("insert zeta");
            dictionary::insert(
                &conn,
                &NewDictionaryEntry {
                    term: "alpha".into(),
                    canonical: None,
                    source: "manual".into(),
                    confidence: None,
                    app_context: Some("code.exe".into()),
                },
            )
            .expect("insert alpha");
        }

        let prompt = build_from_db(&db, Some("code.exe")).expect("prompt produced");
        let alpha = prompt.find("alpha").expect("alpha present");
        let zeta = prompt.find("zeta").expect("zeta present");
        assert!(
            alpha < zeta,
            "app-context match should sort ahead; prompt was: {prompt:?}"
        );
    }
}
