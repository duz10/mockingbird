//! Phase 1E amendment `mb-08za` (ADR 0053 §D11 / §D12) —
//! auto-generated Entity + Project stub pages.
//!
//! On every successful KG filing, the worker iterates the entry's
//! unique entity slugs and idempotently creates a stub `.md` at
//! `Knowledge Graph/Entities/<slug>.md` (and, for entities the
//! pipeline classifies as `EntityType::Project`, also at
//! `Knowledge Graph/Projects/<slug>.md`). This module owns the
//! write-once-then-leave-alone contract.
//!
//! # Contract (write-once, user-owns-thereafter)
//!
//! - Stub is written ONCE, on first mention. The detection mechanism
//!   is purely existence-based: `Entities/<slug>.md` exists on disk
//!   ⇒ skip stub generation entirely. No content hash; no schema
//!   upgrades that mutate user-edited stubs.
//! - After creation, Mockingbird NEVER overwrites the file. The user
//!   may freely rewrite the body (add notes, add their own queries,
//!   change the title) — Mockingbird will not touch it.
//! - Atomic write via temp-sibling + rename (same discipline as
//!   `vault::writer::commit_entry_to_vault`). A crash mid-write
//!   leaves a `.mb-tmp` sibling that a future call simply overwrites.
//! - Canonical-form bytes: LF only, deterministic frontmatter field
//!   order, single trailing newline. The 1E.5 reverse-watcher's
//!   hash-based loop-prevention depends on byte stability of any
//!   file Mockingbird wrote.
//!
//! # Why a sibling module to `vault::writer` (not an extension of it)
//!
//! `vault::writer` owns the **DB-coupled** two-phase commit for
//! entry projection (writes the `Entries/<...>.md` file, then seals
//! `sessions.vault_file_hash` and the reconcile signature). Entity
//! and Project stubs are DB-decoupled:
//! there is no `kg_entities.vault_file_hash` column, no per-stub
//! session row, no reconcile signature. Keeping the two
//! responsibilities in separate modules makes the "DB-coupled vs
//! DB-decoupled" boundary visible to future readers and lets the
//! stub generator stay synchronous + lock-free (no `Connection`
//! parameter).
//!
//! # Cross-platform path discipline
//!
//! All path assembly via [`PathBuf::join`] off
//! [`kg_subtree_paths`]'s `entities` / `projects` PathBufs. The
//! literal `Entities/` + `Projects/` constants live in `kg_layout`,
//! so a typo in this module can't drift the path off the
//! bootstrap'd subtree.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::error::{AppError, AppResult};

use super::kg_layout::kg_subtree_paths;

/// Outcome of an `ensure_*_page` call. Distinguishes "stub was
/// missing and is now present" from "stub was already present;
/// left untouched" so the caller (the KG worker) can log the right
/// level (`info!` vs `trace!`) and a future judge can mechanically
/// count writes-per-filing.
///
/// Both variants are success — neither is an error condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StubPageReport {
    /// The stub file did not exist; Mockingbird wrote it.
    Created,
    /// The stub file already existed; Mockingbird left it untouched.
    /// This is the write-once invariant in action.
    AlreadyExists,
}

/// Distinguishes the two stub shapes so the file body / frontmatter
/// can branch on a single value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StubKind {
    Entity,
    Project,
}

impl StubKind {
    fn frontmatter_type(self) -> &'static str {
        match self {
            Self::Entity => "entity",
            Self::Project => "project",
        }
    }
}

/// Slug rule for the dispatch boundary: same alphabet as
/// `markdown_serializer::slugify_title` — lowercase ASCII alphanum
/// plus `-`. The caller is expected to have pre-slugified; this
/// helper is a defensive belt-and-suspenders check so a buggy
/// caller can't smuggle a `..` or a `/` past us into the path
/// join. Length cap matches `SLUG_MAX_LEN = 50` upstream.
const MAX_SLUG_LEN: usize = 50;

fn validate_slug(slug: &str) -> AppResult<()> {
    if slug.is_empty() {
        return Err(AppError::Vault(
            "entity_pages: slug must not be empty".to_string(),
        ));
    }
    if slug.len() > MAX_SLUG_LEN {
        return Err(AppError::Vault(format!(
            "entity_pages: slug exceeds max length {MAX_SLUG_LEN}: {slug:?}"
        )));
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(AppError::Vault(format!(
            "entity_pages: slug must be lowercase ASCII kebab-case: {slug:?}"
        )));
    }
    if slug.starts_with('-') || slug.ends_with('-') {
        return Err(AppError::Vault(format!(
            "entity_pages: slug must not start or end with '-': {slug:?}"
        )));
    }
    Ok(())
}

/// Idempotently create the entity stub page for `slug` under
/// `<vault_root>/Knowledge Graph/Entities/<slug>.md`.
///
/// - `slug` must be a pre-slugified ASCII kebab-case identifier (as
///   produced by `vault::markdown_serializer::slugify_title`). The
///   helper rejects malformed input rather than silently
///   sanitizing — keeping caller responsibility for slug derivation
///   explicit.
/// - `created_at` is stamped into the frontmatter. Passed in (not
///   `Utc::now()` inside) so unit tests are deterministic.
///
/// Returns:
///
/// - `Ok(StubPageReport::AlreadyExists)` if `Entities/<slug>.md`
///   already exists. NEVER overwrites — the write-once contract.
/// - `Ok(StubPageReport::Created)` if the stub was just written.
/// - `Err(AppError::Vault)` for slug-validation failures, I/O
///   failures (permission denied, parent missing despite bootstrap,
///   atomic-rename failure, etc.). The error message names the
///   offending path for actionable toasts.
pub fn ensure_entity_page(
    vault_root: &Path,
    slug: &str,
    created_at: DateTime<Utc>,
) -> AppResult<StubPageReport> {
    ensure_stub_page(vault_root, slug, created_at, StubKind::Entity)
}

/// Idempotently create the project stub page for `slug` under
/// `<vault_root>/Knowledge Graph/Projects/<slug>.md`. Same
/// write-once contract as [`ensure_entity_page`]; see that doc for
/// details.
pub fn ensure_project_page(
    vault_root: &Path,
    slug: &str,
    created_at: DateTime<Utc>,
) -> AppResult<StubPageReport> {
    ensure_stub_page(vault_root, slug, created_at, StubKind::Project)
}

fn ensure_stub_page(
    vault_root: &Path,
    slug: &str,
    created_at: DateTime<Utc>,
    kind: StubKind,
) -> AppResult<StubPageReport> {
    validate_slug(slug)?;

    let subtree = kg_subtree_paths(vault_root);
    let target_dir = match kind {
        StubKind::Entity => subtree.entities,
        StubKind::Project => subtree.projects,
    };
    let target_path = target_dir.join(format!("{slug}.md"));

    // Existence probe FIRST — write-once contract. We never read
    // the existing file (no hash, no schema check); presence on
    // disk is the entire signal.
    if target_path.exists() {
        return Ok(StubPageReport::AlreadyExists);
    }

    // Bootstrap was idempotent at toggle-on time, but a user who
    // deleted the parent folder mid-session shouldn't lose the
    // stub. `create_dir_all` is a no-op on the happy path.
    fs::create_dir_all(&target_dir).map_err(|e| {
        AppError::Vault(format!(
            "entity_pages: failed to ensure parent dir {} -- {}",
            target_dir.display(),
            e
        ))
    })?;

    let body = render_stub(kind, slug, created_at);
    write_atomic(&target_path, body.as_bytes()).map_err(|e| {
        AppError::Vault(format!(
            "entity_pages: atomic write to {} failed -- {}",
            target_path.display(),
            e
        ))
    })?;

    tracing::info!(
        target: "kg::vault_stubs",
        kind = kind.frontmatter_type(),
        slug,
        path = %target_path.display(),
        "\u{2728} stub page created"
    );

    Ok(StubPageReport::Created)
}

/// Render the canonical-form bytes for a stub page. Pure; no I/O.
///
/// Frontmatter shape (per ADR 0053 §D11 / §D12 as amended):
///
/// ```yaml
/// ---
/// id: <slug>
/// type: entity            # or "project"
/// schema_version: 1
/// created_at: 2026-06-06T10:00:00Z
/// aliases: []             # entity only
/// status: active          # project only
/// ---
/// ```
///
/// Body: a level-1 heading with the slug + a Dataview query block
/// filtering all entries that link to this page via the `entities`
/// frontmatter field.
fn render_stub(kind: StubKind, slug: &str, created_at: DateTime<Utc>) -> String {
    let mut out = String::with_capacity(512);
    out.push_str("---\n");
    out.push_str(&format!("id: \"{slug}\"\n"));
    out.push_str(&format!("type: \"{}\"\n", kind.frontmatter_type()));
    out.push_str("schema_version: 1\n");
    out.push_str(&format!(
        "created_at: \"{}\"\n",
        created_at.format("%Y-%m-%dT%H:%M:%SZ")
    ));
    match kind {
        StubKind::Entity => out.push_str("aliases: []\n"),
        StubKind::Project => out.push_str("status: \"active\"\n"),
    }
    out.push_str("---\n");
    out.push('\n');
    out.push_str(&format!("# {slug}\n"));
    out.push('\n');
    // Dataview block: the entity wiki-link can appear in two
    // shapes in an entry's `entities:` frontmatter list — the
    // post-1E.5-polish pipe-alias form `[[Entities/<slug>|<slug>]]`
    // (current) and the pre-1E.5-polish bare form
    // `[[Entities/<slug>]]` (Wave 1E.2/3/4 amendment, written by
    // earlier Mockingbird builds). The `any(...) => contains(...)`
    // predicate matches BOTH so existing on-disk entries continue
    // to surface on the stub page after the polish lands. Pure
    // presentation; the DB-side representation is always the bare
    // slug. Project pages reuse the same predicate — navigating
    // from either Entity page or Project page lands the user on
    // the same set of entries (project entities are a strict
    // subset of all entities).
    out.push_str("```dataview\n");
    out.push_str("TABLE category, type, status, captured_at\n");
    out.push_str("FROM \"Knowledge Graph/Entries\"\n");
    out.push_str(&format!(
        "WHERE any(entities, (e) => contains(e, \"[[Entities/{slug}]]\") OR contains(e, \"[[Entities/{slug}|\"))\n"
    ));
    out.push_str("SORT captured_at DESC\n");
    out.push_str("```\n");
    out
}

/// Atomic file write: temp-sibling + rename. Same shape as
/// `vault::writer::write_atomic` (private there; we re-implement
/// rather than promote-and-share because the two call sites have
/// different surrounding lock discipline and merging them would
/// silently couple their concurrency contracts). The temp suffix
/// `.mb-tmp` is the same so `reconcile_vault`-style sweeps can
/// recognize crash-leaked temps from either writer.
fn write_atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut tmp = target.as_os_str().to_owned();
    tmp.push(".mb-tmp");
    let tmp_path = PathBuf::from(tmp);
    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, target)?;
    Ok(())
}

#[cfg(test)]
#[path = "entity_pages_tests.rs"]
mod tests;
