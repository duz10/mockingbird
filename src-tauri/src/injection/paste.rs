//! Clipboard save/restore protocol per ADR 0018.
//!
//! **Stub in Wave 1.** The four-step dance (snapshot every format
//! via `EnumClipboardFormats` → write payload → SendInput Ctrl+V →
//! restore) lands in Wave 4 (bd `mb-cm3`). This file is also the
//! **only** location in the workspace permitted to call
//! `SetClipboardData` — PLAN §12 #17 binding. The shell-side hook
//! `scripts/hooks/warn-bare-clipboard-set.py` flags violations.
//!
//! Wave-4 contract preview:
//! ```ignore
//! pub fn with_saved_clipboard<F>(
//!     payload: &str,
//!     paste_fn: F,
//! ) -> AppResult<PasteOutcome>
//! where
//!     F: FnOnce() -> AppResult<()>;
//! ```
//!
//! See `docs/phases/phase3.md` Wave 4 + ADR 0018 for the snapshot
//! scope, sequence-number sentinel, and failure handling.
