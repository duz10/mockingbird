//! INDEX.md rebuild + LOG.md append phases (ADR 0054 §D / §E).
//!
//! - [`maybe_rebuild_index_md`]: phase 5b -- full rebuild from DB
//!   after every filing. O(N) over filed entries; fine for a single
//!   user's KG.
//! - [`maybe_append_log_capture`]: phase 5c -- single append-line
//!   per capture event. Crash-safe via atomic temp-sibling rename
//!   inside `vault::log_md`.
//!
//! Both are non-fatal on failure: the entry + queue are already
//! sealed, so an INDEX/LOG glitch can't unwind the filing.
//! Vault-unconfigured ⇒ silent no-op.
//!
//! Split out of `worker.rs` during Wave 1E.7 Part 2 (`mb-5lla`).
//! Behaviour is unchanged.

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::settings::{model::SettingKey, Settings};
use crate::vault::index_md::rebuild_index_md;
use crate::vault::log_md::{append_log_line, LogOp};
use crate::vault::writer::CommitOutcome;

/// Phase 5b -- INDEX.md rebuild from DB.
pub(super) fn maybe_rebuild_index_md(conn: &Arc<Mutex<Connection>>, queue_id: i64, entry_id: i64) {
    let lock = conn.lock();
    let Ok(c) = lock else {
        // Hotfix `mb-wzcj`: warn -> error per gate #2.
        tracing::error!(
            target: "kg::worker",
            queue_id,
            entry_id,
            "db mutex poisoned in maybe_rebuild_index_md; skipping (non-fatal)"
        );
        return;
    };
    let vault_root: Option<std::path::PathBuf> = Settings::new(&c)
        .get::<Option<String>>(SettingKey::VaultPath)
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .map(std::path::PathBuf::from);
    let Some(vault_root) = vault_root else {
        return;
    };
    match rebuild_index_md(&c, &vault_root) {
        Ok(outcome) => tracing::info!(
            target: "kg::worker",
            queue_id,
            entry_id,
            sources = outcome.sources_emitted,
            entities = outcome.entities_emitted,
            projects = outcome.projects_emitted,
            tags = outcome.tags_emitted,
            concepts_preserved = outcome.concepts_preserved,
            "INDEX.md rebuild complete"
        ),
        Err(e) => tracing::error!(
            target: "kg::worker",
            queue_id,
            entry_id,
            error = %e,
            "INDEX.md rebuild failed; next filing will retry (non-fatal; hotfix mb-wzcj)"
        ),
    }
}

/// Phase 5c (ADR 0054 §E, mb-bgpt) -- LOG.md append.
///
/// Subject is derived from the entry filename (slug between the
/// date prefix and the `__id8` suffix). Pulling the title via a
/// fresh DB lookup would round-trip more bytes than the slug is
/// worth for an operations log meant to be skimmed by a human.
pub(super) fn maybe_append_log_capture(
    conn: &Arc<Mutex<Connection>>,
    queue_id: i64,
    entry_id: i64,
    outcome: &CommitOutcome,
) {
    let vault_root_opt: Option<std::path::PathBuf> = {
        let lock = conn.lock();
        let Ok(c) = lock else {
            // Hotfix `mb-wzcj`: warn -> error per gate #2.
            tracing::error!(
                target: "kg::worker",
                queue_id,
                entry_id,
                "db mutex poisoned in maybe_append_log_capture; skipping (non-fatal)"
            );
            return;
        };
        Settings::new(&c)
            .get::<Option<String>>(SettingKey::VaultPath)
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty())
            .map(std::path::PathBuf::from)
    };
    let Some(vault_root) = vault_root_opt else {
        return;
    };

    let subject = log_subject_from_vault_path(&outcome.vault_relative_path);
    match append_log_line(&vault_root, chrono::Utc::now(), LogOp::Capture, &subject) {
        Ok(o) => tracing::info!(
            target: "kg::worker",
            queue_id,
            entry_id,
            log = %o.path.display(),
            line = %o.line.trim_end(),
            "LOG.md append complete"
        ),
        Err(e) => tracing::error!(
            target: "kg::worker",
            queue_id,
            entry_id,
            error = %e,
            "LOG.md append failed; entry already sealed (non-fatal; hotfix mb-wzcj)"
        ),
    }
}

/// Derive a human-skimmable subject from a vault-relative entry
/// path like
/// `Knowledge Graph/Entries/2026-06-15-buy-milk__abcd1234.md`. The
/// 10-char date prefix + 10-char `__id8.md` suffix get stripped so
/// the middle slug is what surfaces in LOG.md.
fn log_subject_from_vault_path(vault_rel: &str) -> String {
    let basename = vault_rel.rsplit('/').next().unwrap_or(vault_rel);
    let stem = basename.strip_suffix(".md").unwrap_or(basename);
    let without_id = match stem.rfind("__") {
        Some(idx) => &stem[..idx],
        None => stem,
    };
    // Strip `YYYY-MM-DD-` prefix if present.
    let trimmed = if without_id.len() > 11
        && without_id
            .chars()
            .take(10)
            .all(|c| c.is_ascii_digit() || c == '-')
        && without_id.chars().nth(10) == Some('-')
    {
        &without_id[11..]
    } else {
        without_id
    };
    if trimmed.is_empty() {
        basename.to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_subject_strips_date_prefix_and_id_suffix() {
        // The canonical writer output shape: YYYY-MM-DD-<slug>__<id8>.md
        assert_eq!(
            log_subject_from_vault_path("Knowledge Graph/Entries/2026-06-15-buy-milk__abcd1234.md"),
            "buy-milk"
        );
    }

    #[test]
    fn log_subject_falls_back_when_path_is_unexpected_shape() {
        // No date prefix, no __id suffix -- return the stem as-is.
        assert_eq!(
            log_subject_from_vault_path("Knowledge Graph/Entries/freeform.md"),
            "freeform"
        );
    }

    #[test]
    fn log_subject_handles_bare_filename_with_no_directory() {
        assert_eq!(
            log_subject_from_vault_path("2026-06-15-x__deadbeef.md"),
            "x"
        );
    }
}
