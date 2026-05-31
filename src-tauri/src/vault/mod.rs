//! Vault projection subsystem — ADR 0046.
//!
//! Mockingbird is the source of truth; the vault is a *disposable
//! projection* of the canonical SQLite DB into a synced Obsidian
//! folder. This module owns everything between the DB and the
//! filesystem under the user's vault root.
//!
//! Iteration map (per ADR §"Iteration plan"):
//!
//! - **Iter 2 (this PR, in flight):** outbound projection.
//!     - [`layout`] — zone management, idempotent dir creation.
//!     - [`manifest`] — `.mockingbird/manifest.json` read/write +
//!       schema versioning + machine-fingerprint slot.
//!     - [`project`] (Phase B) — pure record → markdown serializer.
//!     - [`export_job`] (Phase C) — reconciliation engine + triggers.
//! - **Iter 3:** inbound watcher for the iOS Shortcut courier.
//! - **Iter 4:** Settings UI for the full Mobile tab + hardening.
//!
//! No file under this module reaches into Phase 3 dictation, Phase
//! MC meeting capture, or Phase 10 activity capture. The export
//! job *reads* from those subsystems' canonical tables via the
//! shared DB handle, but never edits their code paths -- the
//! `sealed-phases-untouched` judge (Wave 5.3) verifies this
//! mechanically.

pub mod entity_pages;
pub mod export_job;
pub mod history;
pub mod kg_layout;
pub mod layout;
pub mod manifest;
pub mod markdown_parser;
pub mod markdown_serializer;
pub mod project;
pub mod watcher;
mod watcher_reconcile;
pub mod writer;
