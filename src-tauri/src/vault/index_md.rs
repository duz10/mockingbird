//! Phase 1E amendment `mb-bgpt` (ADR 0054 ??D) -- **INDEX.md**
//! auto-maintained content catalog.
//!
//! INDEX.md sits at `<vault>/Knowledge Graph/INDEX.md` and is the
//! chat-LLM's catalog for "what does this vault contain?" without
//! having to walk the filesystem on every Query / Lint pass.
//!
//! # Contract (ADR 0054 ??D)
//!
//! - **Five H2 sections** in this fixed order: `Sources`,
//!   `Entities`, `Projects`, `Tags`, `Concepts`.
//! - **Mockingbird owns** the first four sections -- the worker
//!   rebuilds them from DB state after every successful KG filing.
//! - **The chat-LLM owns** the `Concepts` section -- Mockingbird
//!   never touches its body, only preserves whatever bytes are
//!   already there.
//! - **File-wins-against-chat-LLM-edits in Concepts only;
//!   DB-wins-against-user-edits in the first four sections.** A
//!   user hand-edit to Sources / Entities / Projects / Tags is
//!   silently overwritten on the next filing -- the Entries on
//!   disk are the source of truth; INDEX.md is a derived catalog.
//!
//! # Why full rebuild (not incremental diff)
//!
//! Two reasons:
//!
//! 1. **Simpler invariant**: the file is fully determined by the
//!    DB + the existing Concepts section. Two filings on the same
//!    DB state produce byte-identical INDEX.md.
//! 2. **Crash-safety**: an incremental delta engine that crashes
//!    mid-write leaves the catalog in a half-state we'd have to
//!    detect-and-recover. A full rebuild is atomically swapped via
//!    temp-sibling-rename; any crash leaves either the old INDEX
//!    or the new INDEX, never a Frankenstein.
//!
//! The cost (re-scanning all sessions on every filing) is
//! negligible relative to the LLM pipeline that produced the
//! filing in the first place.
//!
//! # Atomic write
//!
//! Bytes are composed in memory, then written to
//! `<INDEX.md>.mb-tmp` and atomically renamed onto the target.
//! Same `.mb-tmp` suffix as `vault::entity_pages::write_atomic`
//! and `vault::writer::write_atomic`, so the future reconcile
//! sweeper that GCs crash-leaked temps catches all three sites.

use crate::error::{AppError, AppResult};
use crate::vault::kg_layout::{kg_root_file_paths, kg_subtree_paths};
use rusqlite::Connection;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// How many `## Sources` rows the worker emits. Capped because the
/// chat-LLM only needs the recent slice to orient on what's new;
/// the full archive is in `Entries/`. Bigger numbers are not wrong,
/// they're just noise.
pub const SOURCES_RECENT_CAP: usize = 64;

/// The five H2 headers, in fixed order. Public so judges and tests
/// can pin against the contract.
/// The `## Sources` H2 header literal -- the recent-filed-Entries section.
pub const H2_SOURCES: &str = "## Sources";
/// The `## Entities` H2 header literal -- the alphabetical canonical-entities section.
pub const H2_ENTITIES: &str = "## Entities";
/// The `## Projects` H2 header literal -- subset of Entities with `entity_type='project'`.
pub const H2_PROJECTS: &str = "## Projects";
/// The `## Tags` H2 header literal -- the alphabetical tag-slug section.
pub const H2_TAGS: &str = "## Tags";
/// The `## Concepts` H2 header literal -- chat-LLM-owned; Mockingbird preserves verbatim.
pub const H2_CONCEPTS: &str = "## Concepts";

/// Render the minimal INDEX.md skeleton seeded at first KG
/// activation by [`crate::vault::kg_layout::bootstrap_kg_root_files`].
///
/// The skeleton has the five H2 section headers with empty bodies +
/// a short header comment explaining the file's role. The first
/// `process_one` after activation will trigger a full rebuild
/// (phase 5b) that replaces the four Mockingbird-owned sections
/// with real catalog content; the Concepts section stays empty
/// until the chat-LLM authors a concept page.
///
/// Pure (no I/O). Deterministic across machines and runs.
pub fn render_skeleton_index_md() -> String {
    let mut out = String::with_capacity(512);
    out.push_str("# INDEX\n");
    out.push('\n');
    out.push_str("<!--\n");
    out.push_str("Auto-maintained content catalog for this Knowledge Graph vault.\n");
    out.push_str("Mockingbird rebuilds Sources / Entities / Projects / Tags after\n");
    out.push_str("every successful KG filing. The Concepts section is chat-LLM-\n");
    out.push_str("owned -- Mockingbird never touches its body. See SCHEMA.md.\n");
    out.push_str("-->\n");
    out.push('\n');
    out.push_str(H2_SOURCES);
    out.push_str("\n\n");
    out.push_str(H2_ENTITIES);
    out.push_str("\n\n");
    out.push_str(H2_PROJECTS);
    out.push_str("\n\n");
    out.push_str(H2_TAGS);
    out.push_str("\n\n");
    out.push_str(H2_CONCEPTS);
    out.push('\n');
    out
}

/// Snapshot of the DB-side facts that drive a full INDEX rebuild.
/// Materialized in one `Connection` borrow so the build is a single
/// short transaction-equivalent, not a series of independent
/// queries each racing the worker's other DB activity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexSnapshot {
    /// Most-recent-first list of filed Entries:
    /// `(started_at, vault_path)`. Capped at
    /// [`SOURCES_RECENT_CAP`]. `vault_path` is vault-relative
    /// POSIX-style as stored in `sessions.vault_path` (migration 026).
    pub sources: Vec<IndexSourceRow>,
    /// Alphabetical entity names from `kg_entities`. Includes
    /// projects (projects ARE entities with `entity_type='project'`)
    /// -- the Projects section is a strict subset.
    pub entities: Vec<String>,
    /// Alphabetical project names from
    /// `kg_entities WHERE entity_type='project'`.
    pub projects: Vec<String>,
    /// Alphabetical unique tag slugs from `kg_tag_mentions`.
    pub tags: Vec<String>,
}

/// One row in the `## Sources` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexSourceRow {
    /// RFC 3339 UTC timestamp from `sessions.started_at` (also the
    /// recent-first sort key). The sessions table has no
    /// `captured_at` column; `started_at` is the canonical capture
    /// time used by the Dictations view + history archive.
    pub started_at: String,
    /// Vault-relative POSIX-style path from `sessions.vault_path`.
    pub vault_path: String,
}

/// Read the DB snapshot that drives a full rebuild. Pure-ish: holds
/// the connection only for the duration of the three queries.
pub fn snapshot_from_db(conn: &Connection) -> AppResult<IndexSnapshot> {
    let sources = load_sources(conn)?;
    let entities = load_entities(conn, /* projects_only = */ false)?;
    let projects = load_entities(conn, /* projects_only = */ true)?;
    let tags = load_tags(conn)?;
    Ok(IndexSnapshot {
        sources,
        entities,
        projects,
        tags,
    })
}

fn load_sources(conn: &Connection) -> AppResult<Vec<IndexSourceRow>> {
    // sessions.vault_path is non-null only for successfully-filed KG
    // entries (migration 026). We rely on the vault_path filename
    // for the displayed title (`YYYY-MM-DD-<slug>__<id8>.md`) --
    // avoids a join through transcripts and matches what the user
    // sees in Obsidian's file tree.
    let mut stmt = conn
        .prepare(
            "SELECT started_at, vault_path \
             FROM sessions \
             WHERE vault_path IS NOT NULL \
             ORDER BY started_at DESC \
             LIMIT ?1",
        )
        .map_err(|e| AppError::Vault(format!("index_md: prepare sources query -- {e}")))?;
    let mut rows = stmt
        .query(rusqlite::params![SOURCES_RECENT_CAP as i64])
        .map_err(|e| AppError::Vault(format!("index_md: query sources -- {e}")))?;
    let mut out = Vec::new();
    while let Some(r) = rows
        .next()
        .map_err(|e| AppError::Vault(format!("index_md: walk sources -- {e}")))?
    {
        let started_at: String = r
            .get(0)
            .map_err(|e| AppError::Vault(format!("index_md: read sources.started_at -- {e}")))?;
        let vault_path: String = r
            .get(1)
            .map_err(|e| AppError::Vault(format!("index_md: read sources.vault_path -- {e}")))?;
        out.push(IndexSourceRow {
            started_at,
            vault_path,
        });
    }
    Ok(out)
}

fn load_entities(conn: &Connection, projects_only: bool) -> AppResult<Vec<String>> {
    let sql = if projects_only {
        "SELECT DISTINCT name FROM kg_entities WHERE entity_type = 'project' ORDER BY LOWER(name) ASC"
    } else {
        "SELECT DISTINCT name FROM kg_entities ORDER BY LOWER(name) ASC"
    };
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| AppError::Vault(format!("index_md: prepare entities query -- {e}")))?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| AppError::Vault(format!("index_md: query entities -- {e}")))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| AppError::Vault(format!("index_md: walk entities -- {e}")))?);
    }
    Ok(out)
}

fn load_tags(conn: &Connection) -> AppResult<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT DISTINCT tag_slug FROM kg_tag_mentions ORDER BY LOWER(tag_slug) ASC")
        .map_err(|e| AppError::Vault(format!("index_md: prepare tags query -- {e}")))?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| AppError::Vault(format!("index_md: query tags -- {e}")))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| AppError::Vault(format!("index_md: walk tags -- {e}")))?);
    }
    Ok(out)
}

/// Compose the full INDEX.md bytes from a DB snapshot and an
/// optional pre-existing Concepts section.
///
/// `existing_concepts_section` is the **verbatim** block that
/// follows the `## Concepts` header in the current on-disk INDEX.md
/// (including the header line itself), or `None` when no INDEX.md
/// existed pre-rebuild. The function emits exactly that block
/// (unmodified) for the Concepts section; if `None`, it emits a
/// bare `## Concepts\n` header so the section anchor is still
/// present for the chat-LLM to find on its next Ingest.
///
/// Pure -- no I/O. Output is LF-only and deterministic given inputs.
pub fn render_index_md(
    snapshot: &IndexSnapshot,
    existing_concepts_section: Option<&str>,
) -> String {
    let mut out = String::with_capacity(8192);
    out.push_str("# INDEX\n");
    out.push('\n');
    out.push_str("<!--\n");
    out.push_str("Auto-maintained content catalog for this Knowledge Graph vault.\n");
    out.push_str("Mockingbird rebuilds Sources / Entities / Projects / Tags after\n");
    out.push_str("every successful KG filing. The Concepts section is chat-LLM-\n");
    out.push_str("owned -- Mockingbird never touches its body. See SCHEMA.md.\n");
    out.push_str("-->\n");
    out.push('\n');

    // ?????? ## Sources ??????
    out.push_str(H2_SOURCES);
    out.push('\n');
    if snapshot.sources.is_empty() {
        out.push('\n');
    } else {
        out.push('\n');
        for src in &snapshot.sources {
            let title = derive_title_from_path(&src.vault_path);
            // `[[<vault-relative path WITHOUT .md>|<title>]]` is the
            // Obsidian-canonical wiki-link form. `vault_path` is
            // already vault-relative + POSIX-style per migration 026.
            let link_target = src.vault_path.trim_end_matches(".md");
            out.push_str(&format!(
                "- {} [[{link_target}|{}]]\n",
                src.started_at,
                escape_pipes(&title)
            ));
        }
        out.push('\n');
    }

    // ?????? ## Entities ??????
    render_alpha_section(&mut out, H2_ENTITIES, &snapshot.entities, "Entities");

    // ?????? ## Projects ??????
    render_alpha_section(&mut out, H2_PROJECTS, &snapshot.projects, "Projects");

    // ?????? ## Tags ??????
    render_alpha_section(&mut out, H2_TAGS, &snapshot.tags, "Tags");

    // ?????? ## Concepts -- chat-LLM owned; preserve verbatim ??????
    match existing_concepts_section {
        Some(block) => {
            // Caller has already trimmed leading whitespace and is
            // handing us the section starting at `## Concepts`. We
            // ensure exactly one trailing newline.
            let trimmed = block.trim_end_matches('\n');
            out.push_str(trimmed);
            out.push('\n');
        }
        None => {
            out.push_str(H2_CONCEPTS);
            out.push('\n');
        }
    }
    out
}

fn render_alpha_section(out: &mut String, header: &str, items: &[String], parent_dir: &str) {
    out.push_str(header);
    out.push('\n');
    if items.is_empty() {
        out.push('\n');
        return;
    }
    out.push('\n');
    for name in items {
        let slug = entity_name_to_slug(name);
        out.push_str(&format!(
            "- [[{parent_dir}/{slug}|{}]]\n",
            escape_pipes(name)
        ));
    }
    out.push('\n');
}

/// Conservative kebab-case slugification that matches what the
/// `vault::entity_pages` writer uses for filenames. ASCII alphanum
/// preserved; everything else folded to `-`; runs collapsed; lower-
/// cased; trimmed of leading/trailing `-`.
///
/// Kept as a local helper rather than depending on a vault-wide
/// "slugify" -- `entity_pages` slugifies at *write* time when
/// emitting the stub, so the stable token in the DB is the
/// post-slugify name. Most names here will already be slug-shaped,
/// but defending against the off-axis case is cheap.
fn entity_name_to_slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Escape `|` and `]` in a wiki-link display alias so the link
/// target/alias separator and link terminator stay unambiguous.
fn escape_pipes(s: &str) -> String {
    s.replace('|', r"\|").replace(']', r"\]")
}

/// Derive a fallback title from a vault-relative path. For
/// `Knowledge Graph/Entries/2026-06-15-buy-milk__abcd1234.md` this
/// yields `buy-milk`. Used when `sessions.transcript_title` is
/// empty.
fn derive_title_from_path(vault_path: &str) -> String {
    let file = vault_path.rsplit(['/', '\\']).next().unwrap_or(vault_path);
    let stem = file.trim_end_matches(".md");
    // Strip `__<id8>` suffix
    let no_id = stem.rsplit_once("__").map(|(a, _)| a).unwrap_or(stem);
    // Strip leading `YYYY-MM-DD-` if present.
    if no_id.len() > 11 && no_id.as_bytes()[10] == b'-' {
        let (date, rest) = no_id.split_at(11);
        if date
            .chars()
            .take(10)
            .all(|c| c.is_ascii_digit() || c == '-')
        {
            return rest.to_string();
        }
    }
    no_id.to_string()
}

/// Extract the verbatim `## Concepts` section (header + body) from
/// an existing INDEX.md, returning everything from the `## Concepts`
/// line through end-of-file. `None` when no such header exists
/// (fresh INDEX.md never seeded; user wiped the section heading;
/// etc).
pub fn extract_concepts_section(existing: &str) -> Option<String> {
    // We search for `\n## Concepts` (newline-anchored) to avoid a
    // pathological header that happens to start the file (the
    // skeleton has a `# INDEX` H1 before the H2s).
    let pat = format!("\n{H2_CONCEPTS}");
    let idx = existing.find(&pat)?;
    Some(existing[idx + 1..].to_string())
}

/// Rebuild INDEX.md from the DB snapshot, preserving any existing
/// Concepts section verbatim, and atomically write the result.
///
/// Caller contract: `vault_root` is the user's vault root (the
/// parent of `Knowledge Graph/`), and the subtree + root files
/// have already been bootstrapped. The function tolerates a
/// missing INDEX.md by treating it as "no Concepts section yet".
pub fn rebuild_index_md(conn: &Connection, vault_root: &Path) -> AppResult<RebuildOutcome> {
    let snapshot = snapshot_from_db(conn)?;
    let paths = kg_root_file_paths(vault_root);

    // Belt-and-braces: ensure the parent `Knowledge Graph/` exists.
    // bootstrap_kg_subtree should have done this already, but a
    // mid-life vault-path relocation could leave us pointed at an
    // empty dir.
    let subtree = kg_subtree_paths(vault_root);
    if !subtree.root.is_dir() {
        return Err(AppError::Vault(format!(
            "rebuild_index_md: KG subtree root missing at {}",
            subtree.root.display()
        )));
    }

    let existing = fs::read_to_string(&paths.index_md).ok();
    let existing_concepts = existing.as_deref().and_then(extract_concepts_section);

    let bytes = render_index_md(&snapshot, existing_concepts.as_deref());
    write_atomic(&paths.index_md, bytes.as_bytes()).map_err(|e| {
        AppError::Vault(format!(
            "rebuild_index_md: atomic write to {} -- {e}",
            paths.index_md.display()
        ))
    })?;

    Ok(RebuildOutcome {
        path: paths.index_md,
        sources_emitted: snapshot.sources.len(),
        entities_emitted: snapshot.entities.len(),
        projects_emitted: snapshot.projects.len(),
        tags_emitted: snapshot.tags.len(),
        concepts_preserved: existing_concepts.is_some(),
    })
}

/// Outcome of a successful [`rebuild_index_md`] call. Counts surface
/// in `tracing::info!` so we can see at a glance what shape the
/// catalog took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildOutcome {
    /// Absolute path of the INDEX.md that was rewritten.
    pub path: PathBuf,
    /// Number of rows emitted under `## Sources`.
    pub sources_emitted: usize,
    /// Number of names emitted under `## Entities`.
    pub entities_emitted: usize,
    /// Number of names emitted under `## Projects` (subset of entities).
    pub projects_emitted: usize,
    /// Number of slugs emitted under `## Tags`.
    pub tags_emitted: usize,
    /// True iff the pre-existing INDEX.md had a `## Concepts` section we preserved verbatim.
    pub concepts_preserved: bool,
}

/// Atomic file write: temp-sibling + rename. Mirrors
/// `vault::entity_pages::write_atomic` and `vault::writer::
/// write_atomic` -- same `.mb-tmp` suffix so the future reconcile
/// sweep that GCs crash-leaked temps catches all three sites.
fn write_atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut tmp = target.as_os_str().to_owned();
    tmp.push(".mb-tmp");
    let tmp_path = PathBuf::from(tmp);
    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, target)?;
    Ok(())
}

#[cfg(test)]
#[path = "index_md_tests.rs"]
mod tests;
