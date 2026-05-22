//! Activity capture — sibling subsystem to dictation + meeting capture.
//!
//! Charter: ADR 0036 (sibling-subsystem boundary), ADR 0037 (Command
//! Center is the invocation surface — no chord). Phase 10 Wave 1B
//! ships the **titles-only skeleton**: lifecycle FSM, sessions +
//! events tables (migration 012), foreground-window sampler, and the
//! UI surface (Activity list + ActivityDetail timeline).
//!
//! ## Sibling boundary (ADR 0036)
//!
//! Activity-capture lives in its own module and:
//!
//! 1. **Does not import the dictation surface.** No
//!    `crate::hotkey::*`, `crate::dictation::*`, `crate::injection::*`,
//!    `crate::recording_window::*`. The pre-commit hook
//!    `block-cross-module-coupling` enforces this.
//! 2. **Does not import the meeting surface.** Same hook.
//! 3. **Shares only the audit / db / settings / window_context
//!    infrastructure.** Each is a leaf module with no sibling-
//!    subsystem opinions baked in.
//! 4. **Is invoked from the Command Center only** (Wave 1A's
//!    `command_center` module). There is no separate chord, no
//!    separate tray entry — the user always opens the CC and picks
//!    "Activity".
//!
//! ## Wave 1B contents
//!
//! - [`lifecycle`] — pure-Rust FSM (`Idle → Active → Paused →
//!   Stopped`). Full unit-test coverage of the transition matrix.
//! - [`persist`] — `activity_sessions` + `activity_events` repo.
//! - [`ids`] — UUID-backed id generators (ULID is a future swap).
//! - [`sampler`] — `Sampler` trait + Windows polling impl + stub
//!   for non-Windows.
//! - [`runtime`] — [`ActivityCaptureRuntime`], the clone-cheap
//!   orchestrator that ties the FSM, persist, and sampler together.
//!   `app.manage()`'d at boot.
//!
//! ## What's NOT here (yet)
//!
//! - **Block segmentation + abstractor** (Layer 1 → Layer 2 →
//!   Layer 3 in `mockingbird-activity-capture-plan.md`). Wave 3.
//! - **UIA-rich snapshots** (clipboard, scroll position, structured
//!   page DOM). Wave 2.
//! - **Mic transcription cross-stitch.** Wave 4.
//! - **Screenshots.** Wave 7. Tables exist (`screenshot_enabled` is
//!   a column on `activity_sessions`) but the column is hard-wired
//!   to `0` in inserts.
//! - **Encryption at rest.** Wave 5, gated on ADR 0038.
//! - **FTS5 search.** Wave 3+, gated on having
//!   `activity_blocks.generated_abstract` content to index.
//!
//! ## Cross-wave invariants
//!
//! 1. `activity_events` is RAW. The migration ships an `UPDATE`-
//!    blocking trigger; the persist layer never issues an UPDATE.
//!    Principle 1 of AGENTS.md.
//! 2. `activity_blocks` and `activity_summaries`/markdown are
//!    EDITABLE in v2; v1 leaves the editor unwired.
//! 3. The lifecycle FSM is the single source of truth for legal
//!    transitions. Adding a new input means adding a new
//!    `LifecycleInput` variant + extending the `apply` match —
//!    not bolting on a side door in `runtime.rs`.

pub mod ids;
pub mod lifecycle;
pub mod persist;
pub mod runtime;
pub mod sampler;

pub use lifecycle::{LifecycleEffect, LifecycleInput, LifecycleState, Transition};
pub use persist::{ActivityEventRow, ActivitySessionDetail, ActivitySessionRow, SessionStatus};
pub use runtime::ActivityCaptureRuntime;
pub use sampler::{Sampler, SamplerEvent};
