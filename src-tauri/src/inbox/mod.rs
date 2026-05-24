//! Mobile-inbox subsystem — ADR 0046 Iter 3.
//!
//! Sibling to [`crate::vault`], which owns the OUTBOUND projection
//! (DB → markdown). This module owns the INBOUND direction: audio
//! files dropped into `<vault>/inbox/` by the iOS Shortcut (Voice
//! Memo → Files → vault) are detected, validated, decoded, and
//! routed through the existing headless-ingest channel so they
//! land in the `sessions` table identically to a desktop import.
//!
//! Wave map (per ADR 0046 §6, refined by
//! `docs/spikes/iter3-sync-layer-findings.md`):
//!
//! - **Wave 3.1** ([`watcher`]) — `notify-debouncer-full` subscriber
//!   plus stability state machine; emits [`watcher::StableInboxFile`]
//!   once two consecutive size-stable reads land.
//! - **Wave 3.2** ([`courier`], Phase B) — single-in-flight processor:
//!   validate → decode → enqueue `HeadlessIngestRequest` → archive
//!   on success / quarantine on failure.
//! - **Wave 3.3** ([`runtime`], Phase C) — `InboxRuntime` lifecycle,
//!   gated by `MobileSyncEnabled` + `VaultPath` (mirrors the existing
//!   [`crate::vault::export_job::VaultRuntime`] gate).
//!
//! The watcher is conceptually a separate flow from the outbound
//! projection — they share the vault root but are otherwise
//! independent — so this lives as a top-level subsystem rather than
//! under `vault::`. Mirrors the existing `dictation` / `meetings` /
//! `activity` split.

// Phase A (Wave 3.1) — file-watcher + stability state machine.
pub mod watcher;

// Phase B (Wave 3.2) — validate / decode / ingest / archive courier.
pub mod courier;

// Phase C (Wave 3.3) — InboxRuntime lifecycle, gated by
// MobileSyncEnabled + VaultPath. Mirrors VaultRuntime's shape.
pub mod runtime;
