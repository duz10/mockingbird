#![allow(missing_docs)]
// Method-level docs are the API; this module file
// exports trait shapes that get fleshed out per
// wave. Phase MC Wave 1 scaffolds only.

//! Meeting Capture — sibling subsystem to dictation (ADR 0026).
//!
//! This module is the entry point for the meeting-recording feature.
//! It owns:
//!   - chord activation (Right Ctrl + M; ADR 0027), via a dedicated
//!     meetings message-pump thread that installs a SECOND
//!     `WH_KEYBOARD_LL` hook;
//!   - twin-stream audio capture (mic + system loopback; ADR 0028);
//!   - long-form chunked Whisper transcription (ADR 0029, ADR 0030);
//!   - a pure deterministic formatter (no LLM in the critical path);
//!   - persistence to the new `meeting_sessions` / `meeting_transcripts`
//!     tables (migration 011);
//!   - an optional, off-critical-path LLM pass that constructs a fresh
//!     `OllamaProvider` per call via its existing public constructor
//!     (no extension of the `CleanupProvider` trait).
//!
//! ## Binding rules (cross-wave)
//!
//! See `docs/phases/phase-meeting-capture.md` §"Cross-wave invariants"
//! for the full list. Highlights:
//!
//!   1. **Dictation surface is sealed.** No edits to `hotkey/state.rs`,
//!      `hotkey/windows.rs`, `hotkey/driver.rs`, `dictation/`,
//!      `injection/`, `recording_window.rs`, `cleanup/provider.rs`, or
//!      `cleanup/llm_cleaner.rs`. The pre-commit hook
//!      `block-cross-module-coupling-meeting-dictation` enforces this.
//!   2. **No LLM in the critical recording-to-canonical-transcript
//!      path.** The deterministic [`formatter`] IS the canonical pass.
//!      [`llm_pass`] is opt-in, runs post-persist, and its output is
//!      explicitly not persisted to DB.
//!   3. **`modes` table is not touched.** Meeting LLM prompts live as
//!      markdown files in `meetings/prompts/`, not in the DB.
//!   4. **`SpeechToText::transcribe` is sealed.** Phase MC adds a new
//!      additive method `transcribe_segments` (Wave 2 / ADR 0030).
//!      Dictation calls the original; meeting calls the new one.
//!   5. **Cross-platform from day one.** Every new module pairs a
//!      Windows impl with `todo!()` macOS/Linux stubs behind
//!      `#[cfg(target_os = "windows")]`.
//!
//! ## Wave map
//!
//!   - **Wave 1 (this commit):** scaffolds, types, trait signatures,
//!     no logic. `cargo check` / `cargo test --lib` stay green.
//!   - **Wave 2:** [`activation`], [`formatter`], [`filler_words`],
//!     [`chunker`] — the pure modules, fully unit-tested.
//!   - **Wave 3:** [`loopback_windows`], [`capture`],
//!     [`long_form_stt`], real chord hook install + conflict probe.
//!   - **Wave 4:** [`persist`], [`runtime`], [`llm_pass`], [`export`],
//!     [`overlay`], Tauri commands, UI.
//!   - **Wave 5:** tray toggle, settings UI, save-as-file dialogs,
//!     polish.
//!   - **Wave 6:** judges, retrospective, seal.

pub mod activation;
pub mod capture;
pub mod chunker;
pub mod clipboard;
pub mod export;
pub mod filler_words;
pub mod formatter;
pub mod hotkey_installer;
pub mod llm_pass;
pub mod long_form_stt;
pub mod overlay;
pub mod persist;
pub mod repo;
pub mod runtime;

#[cfg(target_os = "windows")]
pub mod loopback_windows;

// Re-export the most commonly-used surface items so call-sites can
// `use crate::meetings::{MeetingSource, MeetingStatus, TimedSegment};`
// without reaching through three module path segments.
pub use capture::MeetingSource;
pub use long_form_stt::TimedSegment;
pub use persist::MeetingStatus;
pub use repo::{MeetingDetail, MeetingMatch, MeetingSummary};
pub use runtime::MeetingCaptureRuntime;
