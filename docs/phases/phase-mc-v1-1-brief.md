# Phase MC v1.1 — Single-Wave Brief

> **Charter:** ADR 0032. **Epic:** `bd: mb-1ir`. **Children:** mb-nig,
> mb-rm7, mb-mom, mb-tn5. **Predecessor:** `phase-mc-complete`
> (sealed 2026-05-22). **Successor tag:** none — closure is via ADR
> 0032 Accepted + STATUS update + epic close.

## Goal

Close the four polish gaps surfaced in the 2026-05-22 post-seal audit,
in a single coherent commit (or one commit per gap if diffs balloon).
Preserve all five Phase MC judges by construction. No new tag, no
re-tagging.

## Files in scope (additive only; nothing in the seal-binding list)

| File | Change |
|---|---|
| `src-tauri/src/meetings/levels.rs` | **NEW.** `compute_dbfs` pure fn + `LevelsState` thread-safe holder |
| `src-tauri/src/meetings/mod.rs` | Add `pub mod levels;` |
| `src-tauri/src/meetings/capture.rs` | `TwinStreamCapture` gains `levels: Arc<LevelsState>` field + `current_levels()` accessor; `owner_thread_loop` calls `levels.update(channel, &buf)` after each drain |
| `src-tauri/src/meetings/lifecycle.rs` | `start_meeting` spawns tick thread (250ms cadence); `finalize_meeting` clears its `running` flag |
| `src-tauri/src/meetings/runtime.rs` | `InFlightMeeting` carries the `Arc<AtomicBool>` tick-running flag + the tick-thread `JoinHandle` |
| `src-tauri/src/meetings/filler_words.rs` | Add `"basically"` to `FILLERS` + one test |
| `ui/src/lib/meetings.ts` | Add `clampMaxDuration` |
| `ui/src/lib/types.ts` | Add `MeetingTickEvent` type |
| `ui/src/meeting_overlay/MeetingOverlay.tsx` | Subscribe to `meeting:tick`; render `<VuBars>` |
| `ui/src/meeting_overlay/MeetingOverlay.module.css` | `.vuBars` + `.vuBar` + `.vuFill` styles (design tokens only) |
| `ui/src/pages/MeetingDetail.tsx` | First-time ephemeral notice in `LlmPassPanel` |
| `ui/src/pages/SettingsMeetingTab.tsx` | New number input for `MeetingMaxDurationSeconds` |
| `ui/src/i18n/en.json` | 4 new strings (notice body, dismiss, maxDuration label, maxDuration help) |
| `docs/LESSONS.md` | Seal-time entry |
| `STATUS.md` | Anchor block update |

**Out of scope (sealed binding list — DO NOT TOUCH):**
`src-tauri/src/hotkey/{state,windows,driver}.rs`, `dictation/**`,
`injection/**`, `recording_window.rs`,
`cleanup/{provider,llm_cleaner,ollama}.rs`,
`db/migrations/00[1-9]*.sql` and `db/migrations/010_*.sql`.

## Type definitions

### Rust (`meetings/levels.rs`)

```rust
//! Per-channel dBFS levels for the live VU display.

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

use crate::meetings::capture::Channel;

/// Floor of the dBFS scale we report. Below this we clamp.
/// -100 dBFS is below the noise floor of any realistic mic.
pub const DBFS_FLOOR: f32 = -100.0;

/// Peak-amplitude dBFS for an i16 PCM buffer.
///
/// * Empty input → `0.0` (treated as "no data yet" sentinel; the UI
///   maps this to a flat bar).
/// * All-zero input → `DBFS_FLOOR`.
/// * Full-scale i16 (`±32768`) → `0.0` dBFS.
///
/// Pure: no allocation, no I/O, deterministic.
pub fn compute_dbfs(samples: &[i16]) -> f32 { /* … */ }

/// Thread-safe holder for the most-recent (mic_db, sys_db) pair.
///
/// Stored as `AtomicI32` (dBFS × 100, rounded) per channel so reads
/// and writes are lock-free. The 0.01 dB granularity is far below
/// visual perception threshold for a VU bar.
pub struct LevelsState {
    mic_cdb: AtomicI32, // dBFS × 100
    sys_cdb: AtomicI32,
}

impl LevelsState {
    pub fn new() -> Arc<Self> { /* … */ }
    pub fn update(&self, channel: Channel, samples: &[i16]) { /* … */ }
    pub fn snapshot(&self) -> (f32, f32) { /* (mic_db, sys_db) */ }
}
```

### Rust (additive on `TwinStreamCapture`)

```rust
pub struct TwinStreamCapture {
    // … existing fields …
    levels: Arc<LevelsState>,
}

impl TwinStreamCapture {
    /// Snapshot of the most-recent per-channel dBFS levels.
    /// Inactive channels report `DBFS_FLOOR`.
    pub fn current_levels(&self) -> (f32, f32) {
        self.levels.snapshot()
    }
}
```

### Tick thread (in `lifecycle.rs`)

```rust
fn spawn_tick_emitter(
    app_handle: tauri::AppHandle,
    uuid: String,
    started_at: std::time::Instant,
    levels: Arc<LevelsState>,
    running: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> { /* 250ms loop, emit meeting:tick */ }
```

### TypeScript (`ui/src/lib/types.ts`)

```ts
/** Payload of the `meeting:tick` event emitted every ~250ms while
 *  a meeting is in flight. `micDb` / `sysDb` are dBFS in [-100, 0];
 *  an inactive channel reports -100. */
export interface MeetingTickEvent {
  uuid: string;
  elapsedMs: number;
  micDb: number;
  sysDb: number;
}
```

```ts
// ui/src/lib/meetings.ts
export const MEETING_MAX_DURATION_MIN_SEC = 60;
export const MEETING_MAX_DURATION_MAX_SEC = 21_600;

/** Clamp the user-entered max-duration to the server-enforced range.
 *  Returns the clamped integer (seconds). NaN/negative → MIN. */
export function clampMaxDuration(input: number): number { /* … */ }
```

## Function signatures (changed-public-surface only)

- `TwinStreamCapture::current_levels(&self) -> (f32, f32)` — NEW
  accessor; no existing fn signature changes.
- All other changes are internal (private helpers, struct fields, UI
  components) and do not appear in the public crate or `ui/src/lib`
  surface.

## Test specs (inputs → expected outputs)

### `meetings/levels.rs::tests`

1. `silence_is_floor` — `compute_dbfs(&[0i16; 1024])` ≈ `DBFS_FLOOR`
   (within 1e-3).
2. `full_scale_is_zero` — `compute_dbfs(&[i16::MAX; 1024])` ≈ `0.0`
   (within 0.01).
3. `half_scale_is_minus_six` — `compute_dbfs(&[i16::MAX / 2; 1024])`
   in `(-6.5, -5.5)`.
4. `empty_input_is_zero_sentinel` — `compute_dbfs(&[])` == `0.0`
   exactly.
5. `levels_state_update_then_snapshot_roundtrip` — after
   `LevelsState::new().update(Channel::Mic, &[i16::MAX; 256])`, the
   snapshot is `(≈0.0, DBFS_FLOOR)`; after a `Channel::Sys` update,
   both channels read their respective values.

### `meetings/capture.rs::tests` (additive)

6. `current_levels_starts_at_floor` — fresh `TwinStreamCapture` (via
   `start_with` + a `StubCapture` that emits nothing yet) reports
   `(DBFS_FLOOR, DBFS_FLOOR)`.

### `meetings/filler_words.rs::tests` (additive)

7. `basically_is_a_filler` — `FILLERS.contains("basically")` is true;
   `okay_is_not_a_filler` (existing) still passes.

### UI vitest (`ui/src/lib/meetings.test.ts` or new file)

8. `clampMaxDuration_floors_below_min` — `clampMaxDuration(0)` ===
   `60`; `clampMaxDuration(-100)` === `60`; `clampMaxDuration(NaN)`
   === `60`.
9. `clampMaxDuration_ceils_above_max` — `clampMaxDuration(100000)`
   === `21600`.
10. `LlmEphemeralNotice_renders_then_dismisses` — vitest with
    `localStorage` stub: first mount with result present → notice
    renders; click "Got it"; remount → notice absent.

**Test count delta:** +10 Rust tests (5 levels + 1 capture + 1
filler) — wait, that's 7. Let me re-count: 5 (levels) + 1 (capture)
+ 1 (filler) = **7 Rust tests**. + 3 vitest = **10 tests total**.
Project test total after: ~574 (564 + 7 Rust + 3 UI).

(The `#[ignore]`'d tick-thread integration smoke from ADR 0032 §4 is
deferred — `lifecycle.rs` already has thread-spawn tests it inherits
from runtime.rs's pattern, and a real tick test needs a Tauri
AppHandle which makes it a #[ignore] anyway. Skipping until/unless it
proves to flake.)

## Deviations from ADR 0032 (with justification)

- **No `#[ignore]`'d tick-thread integration smoke.** ADR 0032 §4
  proposed one. On reflection, exercising it requires either a real
  `tauri::AppHandle` (which makes it an `#[ignore]` test that never
  runs in CI anyway) or a Tauri test harness shim that doesn't
  exist in this codebase. The tick thread is ~12 LoC of pure I/O
  (sleep, atomic-read, emit) and is unit-testable indirectly via
  `LevelsState` snapshot tests + a Drop-flag liveness check. Marginal
  coverage gain doesn't pay for the harness work. If the tick thread
  ever misbehaves in real recording sessions, that's the moment to
  build the harness.
- **`LevelsState` uses `AtomicI32` not `Mutex<(f32, f32)>`.** Cheaper
  on the tick read path (lock-free), and the precision loss
  (0.01 dB) is invisible at VU-bar resolution. Same end-to-end
  behavior, less risk of mutex poisoning across threads.

## Cargo + UI gate checklist (must be green at seal)

- [ ] `cd src-tauri && cargo check --all-targets`
- [ ] `cd src-tauri && cargo clippy --release --all-targets -- -D warnings`
- [ ] `cd src-tauri && cargo fmt --check`
- [ ] `cd src-tauri && cargo test --release --no-run` (LESSONS
  2026-05-17 `0xC0000139` fallback; live-run blocked by environment)
- [ ] `cd ui && npx tsc --noEmit`
- [ ] `cd ui && npx vitest run`
- [ ] `git diff --name-only phase-mc-start..HEAD -- <binding-list>`
  is empty (preserves `mc-dictation-untouched` judge)
- [ ] `grep -n 'OllamaProvider\|LlmCleaner' src-tauri/src/meetings/lifecycle.rs`
  is empty (preserves `mc-no-llm-in-critical-path` judge)

## Seal procedure

1. All gate items green.
2. Commit (descriptive message referencing ADR 0032 + the four bd ids).
3. `bd close mb-nig mb-rm7 mb-mom mb-tn5 mb-1ir`.
4. Append entry to `docs/LESSONS.md` (`2026-05-23 [mc-v1.1]` header).
5. Flip ADR 0032 status `Proposed` → `Accepted`.
6. Update STATUS.md anchor block:
   - Add to `LATERAL EPICS DONE`: "ADR 0032 (MC v1.1 polish, Accepted
     2026-05-23, mb-1ir closed)".
   - Add a `MC v1.1 SEALED (2026-05-23)` block summarizing what
     landed.
7. Final commit ("docs: seal MC v1.1 polish epic — ADR 0032 Accepted").
8. **Do NOT create a git tag.** Phase tags are reserved for PLAN §10.
