//! Entity / Project / Tag stub-page generation phases.
//!
//! - [`maybe_generate_stub_pages`]: phase 4b (ADR 0053 §D11 / §D12,
//!   amendment `mb-08za`) -- entity + project stubs from
//!   `result.segment_entities`.
//! - [`maybe_generate_tag_stub_pages`]: phase 5a (ADR 0054 §F,
//!   `mb-bgpt`) -- tag stubs from `result.entries[*].topic_tags`.
//!
//! Both run strictly AFTER the seal + `mark_done` transaction. Each
//! per-slug stub call is independently non-fatal: a write failure is
//! logged and the next slug continues. The pages are write-once
//! (see [`crate::vault::entity_pages`]) so re-firing for an existing
//! slug is a no-op.
//!
//! Split out of `worker.rs` during Wave 1E.7 Part 2 (`mb-5lla`).
//! Behaviour is unchanged.

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::settings::{model::SettingKey, Settings};
use crate::vault::entity_pages::{
    ensure_entity_page, ensure_project_page, ensure_tag_page, StubPageReport,
};
use crate::vault::markdown_serializer::slugify_title;

use super::super::pipeline::PipelineResult;

/// Phase 4b -- entity + project stub pages keyed by entity slug.
///
/// Aggregation: we union (slug, EntityType) across
/// `result.segment_entities` and dedupe by slug. The slug merge
/// rule is OR over `is_project`: a slug seen as both Person AND
/// Project gets a Project stub (we DO want the Project stub if any
/// classification flagged it). The stub is write-once anyway, so a
/// subsequent classification can't retroactively flip a Person stub
/// to a Project stub.
///
/// The slug rule is shared with the serializer via
/// [`crate::vault::markdown_serializer::slugify_title`].
pub(super) fn maybe_generate_stub_pages(
    conn: &Arc<Mutex<Connection>>,
    queue_id: i64,
    entry_id: i64,
    result: &PipelineResult,
) {
    // Snapshot vault root under a short-lived lock. If the toggle
    // flipped off between seal and now, we still write the stubs
    // (the entry is already on disk; matching stubs is the
    // user-friendly behaviour). If the vault root is unset we
    // can't write anything.
    let vault_root_opt: Option<std::path::PathBuf> = {
        let lock = conn.lock();
        let Ok(c) = lock else {
            tracing::warn!(
                target: "kg::worker",
                queue_id,
                entry_id,
                "db mutex poisoned in maybe_generate_stub_pages; skipping"
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
        return; // not configured -- nothing to do
    };

    // Aggregate (slug, is_project) across all surviving segment
    // entity outputs. BTreeMap for deterministic iteration order
    // (eases log-driven debugging + tests).
    let mut by_slug: std::collections::BTreeMap<String, bool> = std::collections::BTreeMap::new();
    for seg in &result.segment_entities {
        for ent in &seg.entities {
            let slug = slugify_title(&ent.name);
            // `slugify_title` always returns non-empty ("untitled"
            // fallback); guard anyway against future contract drift.
            if slug.is_empty() || slug == "untitled" {
                // Skip the all-symbols / empty entity name case;
                // stub for "untitled" would be useless.
                continue;
            }
            let is_project = matches!(ent.entity_type, super::super::passes::EntityType::Project);
            // First-seen wins for entity_type. The OR-merge below
            // means a slug seen as both Person AND Project gets
            // Project (we DO want the Project stub if any
            // classification flagged it). This is the only
            // "merge" semantic; everything else is first-seen.
            by_slug
                .entry(slug)
                .and_modify(|v| *v = *v || is_project)
                .or_insert(is_project);
        }
    }

    let now = chrono::Utc::now();
    let mut entity_created = 0usize;
    let mut entity_already = 0usize;
    let mut project_created = 0usize;
    let mut project_already = 0usize;

    for (slug, is_project) in &by_slug {
        match ensure_entity_page(&vault_root, slug, now) {
            Ok(StubPageReport::Created) => {
                entity_created += 1;
            }
            Ok(StubPageReport::AlreadyExists) => {
                entity_already += 1;
            }
            Err(e) => {
                tracing::warn!(
                    target: "kg::worker",
                    queue_id,
                    entry_id,
                    slug,
                    error = %e,
                    "entity stub generation failed; continuing"
                );
            }
        }
        if *is_project {
            match ensure_project_page(&vault_root, slug, now) {
                Ok(StubPageReport::Created) => {
                    project_created += 1;
                }
                Ok(StubPageReport::AlreadyExists) => {
                    project_already += 1;
                }
                Err(e) => {
                    tracing::warn!(
                        target: "kg::worker",
                        queue_id,
                        entry_id,
                        slug,
                        error = %e,
                        "project stub generation failed; continuing"
                    );
                }
            }
        }
    }

    tracing::info!(
        target: "kg::worker",
        queue_id,
        entry_id,
        entity_created,
        entity_already,
        project_created,
        project_already,
        slug_count = by_slug.len(),
        "stub-page generation complete"
    );
}

/// Phase 5a (ADR 0054 §F, mb-bgpt) -- tag stub-page generation.
///
/// Mirrors [`maybe_generate_stub_pages`] but unions tag slugs across
/// `result.entries[*].topic_tags` instead of entity slugs across
/// `result.segment_entities`. Same non-fatal-per-slug semantics: a
/// failed stub write is logged and the loop continues. The pages
/// are write-once (see [`crate::vault::entity_pages::ensure_tag_page`]),
/// so re-firing for a slug that already has a stub is a no-op.
///
/// Vault root resolution + the "toggle flipped off mid-flight" /
/// "vault unconfigured" early returns are identical to the entity
/// path -- copied (not factored) because the read-locked block is
/// 8 lines and a shared helper would obscure the per-phase log
/// targets that LESSONS values for triage.
pub(super) fn maybe_generate_tag_stub_pages(
    conn: &Arc<Mutex<Connection>>,
    queue_id: i64,
    entry_id: i64,
    result: &PipelineResult,
) {
    let vault_root_opt: Option<std::path::PathBuf> = {
        let lock = conn.lock();
        let Ok(c) = lock else {
            // Hotfix `mb-wzcj`: was `tracing::warn!`. Upgraded to
            // `error!` so non-fatal continues in phases 5a/5b/5c
            // surface in logs/UI without changing the non-fatal
            // semantics — a silent failure here is the worst case
            // (entry sealed, post-seal artifacts missing).
            tracing::error!(
                target: "kg::worker",
                queue_id,
                entry_id,
                "db mutex poisoned in maybe_generate_tag_stub_pages; skipping (non-fatal)"
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

    // Union tag slugs across every Entry the pipeline produced for
    // this dictation. `topic_tags` is already normalized + canonical
    // by `passes::normalize` / `passes::tag_validator`, so we don't
    // re-slugify -- the canonical form IS the slug.
    let mut slugs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for entry in &result.entries {
        for tag in &entry.topic_tags {
            let t = tag.trim();
            if t.is_empty() {
                continue;
            }
            slugs.insert(t.to_string());
        }
    }

    let now = chrono::Utc::now();
    let mut created = 0usize;
    let mut already = 0usize;
    for slug in &slugs {
        match ensure_tag_page(&vault_root, slug, now) {
            Ok(StubPageReport::Created) => created += 1,
            Ok(StubPageReport::AlreadyExists) => already += 1,
            Err(e) => {
                // Hotfix `mb-wzcj`: warn -> error per gate #2.
                tracing::error!(
                    target: "kg::worker",
                    queue_id,
                    entry_id,
                    slug = %slug,
                    error = %e,
                    "tag stub generation failed; continuing (non-fatal)"
                );
            }
        }
    }

    tracing::info!(
        target: "kg::worker",
        queue_id,
        entry_id,
        tag_created = created,
        tag_already = already,
        slug_count = slugs.len(),
        "tag-stub generation complete"
    );
}
