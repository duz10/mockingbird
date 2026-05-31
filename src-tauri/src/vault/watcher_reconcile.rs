//! Reverse-watcher reconciliation core (Wave 1E.5 / `mb-qwfy`).
//!
//! Pure file → DB reconciliation logic, factored out of
//! [`crate::vault::watcher`] so the threading runtime + the
//! reconciler stay below the 600-line module cap. Tests live at
//! the bottom of this module; nothing here touches the
//! `notify-debouncer-full` runtime, so unit tests run without
//! spinning up an OS watch.
//!
//! See the parent module docs for the loop-prevention contract,
//! routing table, and lifecycle rules — this module just
//! implements the per-event handler that's invoked once a path
//! has been debounced.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::AppResult;
use crate::kg::store::entities::upsert_entity;
use crate::kg::store::mentions::{insert_entity_mention, insert_tag_mention};
use crate::vault::kg_layout::{
    KG_ENTITIES_NAME, KG_ENTRIES_NAME, KG_HISTORY_NAME, KG_INBOX_NAME, KG_PROJECTS_NAME,
};
use crate::vault::markdown_parser::parse_entry;
use crate::vault::markdown_serializer::slugify_title;
use crate::vault::project::sha256_hex;

// --------------------------------------------------------------------
// Public outcome type
// --------------------------------------------------------------------

/// Public outcome of one watcher event. Surfaced for tests +
/// observability logs; the watcher itself doesn't branch on the
/// value beyond emitting `tracing::info!` / `tracing::warn!`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileOutcome {
    /// Path was outside the Entries subdir (Entities/Projects/
    /// History/Inbox/whatever). Watcher dropped the event silently.
    PathIgnored,
    /// File no longer exists at the time we tried to read it
    /// (deleted between debouncer fire + handler run).
    FileMissing,
    /// File's SHA-256 matched `sessions.vault_file_hash` — this is
    /// our own writer's loop-back; no DB write.
    LoopPrevented,
    /// Frontmatter or body parse failed. Logged + skipped.
    ParseFailed,
    /// No `sessions` row matches the file's `id:` — orphan file
    /// (user moved it from another vault, or the DB was reset).
    NoMatchingSession,
    /// All the right conditions met; mention rows + hash refreshed.
    Reconciled {
        /// `sessions.id` of the row we updated.
        session_id: i64,
        /// Number of tag mentions inserted (after dedupe + trimming).
        tag_count: usize,
        /// Number of entity mentions inserted.
        entity_count: usize,
    },
}

// --------------------------------------------------------------------
// Pure path-filter logic (unit-testable without FS or notify)
// --------------------------------------------------------------------

/// Classify a path under the KG subtree into the subset of work
/// the watcher cares about.
///
/// The watcher receives debouncer events for everything under
/// `<vault>/Knowledge Graph/` (recursive). We filter HERE rather
/// than at the OS layer so the filter logic stays unit-testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathClass {
    /// `Entries/*.md` — the only paths we reconcile to the DB.
    EntryMarkdown,
    /// `Entities/*.md` — user-owned stub pages; ignore.
    EntityPage,
    /// `Projects/*.md` — user-owned stub pages; ignore.
    ProjectPage,
    /// Anything under `History/` — forensic archive; ignore.
    HistoryArtifact,
    /// Anything under `Inbox/` — KG-Inbox courier's concern; ignore.
    InboxArtifact,
    /// Anything else under the KG subtree (or not under it at all).
    Unrelated,
}

impl PathClass {
    /// True if a path of this class should trigger reconciliation.
    /// Currently only [`PathClass::EntryMarkdown`] qualifies.
    pub fn is_actionable(self) -> bool {
        matches!(self, Self::EntryMarkdown)
    }
}

/// Classify `path` relative to `kg_root` (the
/// `<vault>/Knowledge Graph/` directory). Pure path math; no FS
/// I/O. Returns [`PathClass::Unrelated`] for anything outside the
/// KG root.
pub fn classify_path(path: &Path, kg_root: &Path) -> PathClass {
    let Ok(rel) = path.strip_prefix(kg_root) else {
        return PathClass::Unrelated;
    };
    let mut comps = rel.components();
    let Some(first) = comps.next() else {
        return PathClass::Unrelated;
    };
    let first_str = match first.as_os_str().to_str() {
        Some(s) => s,
        None => return PathClass::Unrelated,
    };
    let ext_is_md = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.eq_ignore_ascii_case("md"))
        .unwrap_or(false);

    match first_str {
        KG_ENTRIES_NAME => {
            if ext_is_md {
                PathClass::EntryMarkdown
            } else {
                PathClass::Unrelated
            }
        }
        KG_ENTITIES_NAME => PathClass::EntityPage,
        KG_PROJECTS_NAME => PathClass::ProjectPage,
        KG_HISTORY_NAME => PathClass::HistoryArtifact,
        KG_INBOX_NAME => PathClass::InboxArtifact,
        _ => PathClass::Unrelated,
    }
}

// --------------------------------------------------------------------
// Reconciliation (pure-ish; takes `&Connection` + `&Path` so tests
// don't need a debouncer)
// --------------------------------------------------------------------

/// Reconcile one event against the DB.
///
/// `path` must be an absolute path; `kg_root` is the
/// `<vault>/Knowledge Graph/` directory (used for path
/// classification). The connection is held across the read +
/// transaction; caller takes the lock.
///
/// Returns `Ok(ReconcileOutcome)` for every case the watcher should
/// log + move on from (the watcher MUST stay alive through bad
/// input — see module docs). The only `Err` case is a database
/// error mid-transaction, which the watcher logs as a warning AND
/// continues.
pub fn reconcile_entry_file(
    path: &Path,
    kg_root: &Path,
    conn: &Connection,
) -> AppResult<ReconcileOutcome> {
    // 1. Path classification — fast-path everything that isn't an
    // Entries/*.md edit.
    match classify_path(path, kg_root) {
        PathClass::EntryMarkdown => {}
        PathClass::EntityPage
        | PathClass::ProjectPage
        | PathClass::HistoryArtifact
        | PathClass::InboxArtifact
        | PathClass::Unrelated => return Ok(ReconcileOutcome::PathIgnored),
    }

    // 2. Read file bytes. A missing file (user deleted, or
    // debouncer raced a rename) is a clean ignore.
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ReconcileOutcome::FileMissing);
        }
        Err(e) => {
            tracing::warn!(
                target: "vault::watcher",
                path = %path.display(),
                error = %e,
                "failed to read entry file; skipping"
            );
            return Ok(ReconcileOutcome::FileMissing);
        }
    };
    let content = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => {
            tracing::warn!(
                target: "vault::watcher",
                path = %path.display(),
                "entry file is not valid UTF-8; skipping"
            );
            return Ok(ReconcileOutcome::ParseFailed);
        }
    };
    let file_hash = sha256_hex(content.as_bytes());

    // 3. Parse YAML to extract the `id:` field (the entry UUID).
    let parsed = match parse_entry(&content) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(
                target: "vault::watcher",
                path = %path.display(),
                error = %e,
                "entry file failed to parse; skipping"
            );
            return Ok(ReconcileOutcome::ParseFailed);
        }
    };

    // 4. Find the matching `sessions` row by entry_id (the UUID).
    let session: Option<(i64, Option<String>)> = conn
        .query_row(
            "SELECT id, vault_file_hash FROM sessions WHERE entry_id = ?1 LIMIT 1",
            params![&parsed.entry.id],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?)),
        )
        .optional()?;
    let (session_id, recorded_hash) = match session {
        Some(s) => s,
        None => {
            tracing::info!(
                target: "vault::watcher",
                path = %path.display(),
                entry_id = %parsed.entry.id,
                "no matching session row; treating as orphan file"
            );
            return Ok(ReconcileOutcome::NoMatchingSession);
        }
    };

    // 5. Loop-prevention: if the file hash matches what we recorded
    // for the last Mockingbird-originated write, this is our own
    // change-of-attention bouncing back through the OS watch. Skip.
    if recorded_hash.as_deref() == Some(file_hash.as_str()) {
        tracing::debug!(
            target: "vault::watcher",
            path = %path.display(),
            session_id,
            "hash match — loop-back from own write, ignoring"
        );
        return Ok(ReconcileOutcome::LoopPrevented);
    }

    // 6. Reconcile mention rows + record the new hash. Single
    // transaction so a partial failure doesn't leave us with empty
    // mention tables + a stale hash.
    let now_iso = chrono::Utc::now().to_rfc3339();
    let tag_count = parsed.entry.tags.len();
    let entity_count = parsed.entry.entities.len();
    {
        // `unchecked_transaction` avoids the typed `Transaction`
        // wrapper's mut-borrow on `conn`; we manage commit/rollback
        // explicitly. The wrapper would also work but burns a
        // mutable borrow that callers may not be able to give us.
        let tx = conn.unchecked_transaction()?;

        // Delete-and-reinsert — the `kg_entity_mentions_no_update`
        // + `kg_tag_mentions_no_update` triggers (migration 024)
        // block UPDATE but allow DELETE for exactly this case.
        tx.execute(
            "DELETE FROM kg_tag_mentions WHERE entry_id = ?1",
            params![session_id],
        )?;
        tx.execute(
            "DELETE FROM kg_entity_mentions WHERE entry_id = ?1",
            params![session_id],
        )?;

        // Re-insert tags at segment_idx=0. The slug is already in
        // the YAML form the rest of the pipeline uses; pass through
        // verbatim. Skipping empties defensively.
        for slug in &parsed.entry.tags {
            let trimmed = slug.trim();
            if trimmed.is_empty() {
                continue;
            }
            insert_tag_mention(&tx, session_id, 0, trimmed, &now_iso)?;
        }

        // Re-insert entities. Match-by-slug against existing
        // `kg_entities` rows so a previously-extracted Person /
        // Organization / Project keeps its type. Falls back to
        // type="object" for slugs the DB has never seen — that's
        // the canonical generic bucket the pipeline uses for
        // ambiguous nouns.
        for slug in &parsed.entry.entities {
            let trimmed = slug.trim();
            if trimmed.is_empty() {
                continue;
            }
            let entity_id = find_or_create_entity_by_slug(&tx, trimmed, &now_iso)?;
            insert_entity_mention(&tx, session_id, entity_id, 0, trimmed, &now_iso)?;
        }

        // Re-record the hash so the next watcher fire on our own
        // re-projection (if any) gets matched on step 5 above.
        tx.execute(
            "UPDATE sessions SET vault_file_hash = ?1 WHERE id = ?2",
            params![file_hash, session_id],
        )?;

        tx.commit()?;
    }

    tracing::info!(
        target: "vault::watcher",
        path = %path.display(),
        session_id,
        tag_count,
        entity_count,
        had_checkbox = parsed.had_checkbox,
        checkbox_overrode_yaml = parsed.checkbox_overrode_yaml,
        "reconciled user edit to DB"
    );

    Ok(ReconcileOutcome::Reconciled {
        session_id,
        tag_count,
        entity_count,
    })
}

/// Look up an existing `kg_entities` row whose name slugifies to
/// `slug`; if none found, create a fresh row with the slug as the
/// canonical name and type=`object`.
///
/// O(N) scan over `kg_entities`. N is bounded by the user's
/// vocabulary (tens to low hundreds in realistic Mockingbird
/// usage) so this is fine for v1; promote to an indexed slug
/// column if a future profiler surfaces it. Documented in the
/// 1E.5 follow-up bead `mb-qwfy` resolution.
fn find_or_create_entity_by_slug(conn: &Connection, slug: &str, now_iso: &str) -> AppResult<i64> {
    let mut stmt = conn.prepare("SELECT id, name FROM kg_entities")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        if slugify_title(&name) == slug {
            return Ok(id);
        }
    }
    drop(rows);
    drop(stmt);
    // Fresh slug — upsert into the `object` bucket. Type loss is
    // acceptable (file is the source of truth; the user has
    // already lost the original type by editing the file).
    upsert_entity(conn, slug, "object", &[], now_iso)
}

// --------------------------------------------------------------------
// Unit tests — pure classify_path logic. Reconciler integration
// tests live in `src-tauri/tests/vault_watcher.rs`.
// --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::kg_layout::KG_SUBTREE_ROOT_NAME;
    use std::path::PathBuf;

    fn kg_root() -> PathBuf {
        PathBuf::from("/vault").join(KG_SUBTREE_ROOT_NAME)
    }

    #[test]
    fn classify_entries_md_is_actionable() {
        let p = kg_root()
            .join("Entries")
            .join("2026-06-15-foo__abc12345.md");
        assert_eq!(classify_path(&p, &kg_root()), PathClass::EntryMarkdown);
        assert!(classify_path(&p, &kg_root()).is_actionable());
    }

    #[test]
    fn classify_entities_md_is_ignored() {
        let p = kg_root().join("Entities").join("feta-cheese.md");
        assert_eq!(classify_path(&p, &kg_root()), PathClass::EntityPage);
        assert!(!classify_path(&p, &kg_root()).is_actionable());
    }

    #[test]
    fn classify_projects_md_is_ignored() {
        let p = kg_root().join("Projects").join("phase-1e.md");
        assert_eq!(classify_path(&p, &kg_root()), PathClass::ProjectPage);
    }

    #[test]
    fn classify_history_sidecar_is_ignored() {
        let p = kg_root()
            .join("History")
            .join("2026-06")
            .join("session-uuid.json");
        assert_eq!(classify_path(&p, &kg_root()), PathClass::HistoryArtifact);
    }

    #[test]
    fn classify_history_audio_is_ignored() {
        let p = kg_root()
            .join("History")
            .join("2026-06")
            .join("session-uuid.m4a");
        assert_eq!(classify_path(&p, &kg_root()), PathClass::HistoryArtifact);
    }

    #[test]
    fn classify_inbox_audio_is_ignored() {
        let p = kg_root().join("Inbox").join("memo.m4a");
        assert_eq!(classify_path(&p, &kg_root()), PathClass::InboxArtifact);
    }

    #[test]
    fn classify_entries_non_md_is_unrelated() {
        // A user dropping a .txt into Entries/ shouldn't trigger
        // a reconcile; we only act on .md.
        let p = kg_root().join("Entries").join("notes.txt");
        assert_eq!(classify_path(&p, &kg_root()), PathClass::Unrelated);
    }

    #[test]
    fn classify_path_outside_kg_is_unrelated() {
        let p = PathBuf::from("/vault").join("other.md");
        assert_eq!(classify_path(&p, &kg_root()), PathClass::Unrelated);
    }

    #[test]
    fn classify_unknown_subdir_is_unrelated() {
        // Future subdir we don't recognize: treat as unrelated.
        let p = kg_root().join("Vault Cache").join("foo.md");
        assert_eq!(classify_path(&p, &kg_root()), PathClass::Unrelated);
    }

    #[test]
    fn classify_md_extension_case_insensitive() {
        let p = kg_root().join("Entries").join("Foo.MD");
        assert_eq!(classify_path(&p, &kg_root()), PathClass::EntryMarkdown);
    }
}
