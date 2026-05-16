//! State machine for the hotkey pipeline (PLAN §6.1).
//!
//! **Empty in Wave 1.** The full §6.1 implementation lands in Wave 2
//! (bd `mb-pux`) — pure Rust, ≥20 table-driven unit tests covering
//! every edge case (tap <80 ms ignored, Escape pre/post-30 s, 300 s
//! auto-stop, double-mode collision, re-press during processing,
//! pause toggle). This file exists in Wave 1 only so the module
//! tree compiles + the `HotkeyListener` trait surface in `mod.rs`
//! references a real path.
//!
//! See `docs/phases/phase3.md` Wave 2 task `mb-pux` and ADR 0015
//! (which mandates that this state machine runs on a worker thread,
//! not in the OS hook callback).
