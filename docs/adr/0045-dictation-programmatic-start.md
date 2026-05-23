# ADR 0045: Dictation programmatic start/stop

- **Status:** Accepted
- **Date:** 2026-05-28
- **Supersedes / Amends:** ADR 0037 §4 ("Mode picker & SessionCard UX")
  for the Dictation kind only — the `NoProgrammaticStart` clause that
  made the Command Center's Dictation tile a pure "or hold Right Alt"
  teaching surface no longer applies. Push-to-talk via Right Alt is
  unchanged.

## Context

ADR 0037 §4 codified two distinct UX paths for the three Command
Center tiles:

- **Meeting** and **Activity**: tile pick → programmatic
  `dispatch_start` IPC → `Launching{kind}` → `ShowingSessionCard{kind}`.
- **Dictation**: tile pick → silently dismiss the Command Center,
  showing the user the "or just hold Right Alt" hint. The
  orchestrator's `DispatchOutcome::NoProgrammaticStart` variant was
  the explicit mechanism: the engine recursed with
  `CcInput::Dismiss` instead of `RuntimeReplied`.

The reasoning at the time was that dictation is fundamentally a
push-to-talk gesture and any other start path would just be a worse
PTT. That stance has aged into two concrete problems:

1. **mb-ytex (the silent-dismiss UX bug).** When a user opens the
   Command Center and picks Dictation, the window silently closes
   with no recording started. Even with the "or hold Right Alt" hint,
   this looks like a broken click — the same affordance worked for
   Meeting and Activity tiles, but Dictation does *nothing visible*.
   The UX-completionist read: every tile in a picker should produce
   the same shape of result (a live session card), even if the
   underlying mechanism differs.

2. **Hands-free start.** Some users (accessibility scenarios, voice
   warm-ups while typing, multi-monitor setups where Right Alt is
   awkward) want to start a dictation session without holding the
   PTT key for the duration of the utterance. Today the only path
   is "hold the key the whole time"; there's no way to say "go" and
   "stop" as two discrete events.

We are NOT removing push-to-talk. Right Alt PTT remains the canonical
low-friction path. We are adding a second, supplementary start mode.

## Decision

### Two start modes, one session schema

Dictation now supports two start modes:

- **(a) Push-to-talk (PTT)** — UNCHANGED. The Win32 low-level
  keyboard hook fires `HotkeyEvent::KeyDown` on Right Alt press; the
  state machine enters `PendingHold` → (80 ms threshold) →
  `Recording`. Release fires `KeyUp` → `Processing` → finalize. This
  is the original ADR 0037 §4 path and the only path users have
  today.

- **(b) Programmatic start/stop** — NEW. Two new Tauri IPC commands
  (`dictation_start`, `dictation_stop`) inject synthetic
  `HotkeyEvent::KeyDown` / `HotkeyEvent::KeyUp` events on the same
  hotkey-event channel the OS hook feeds. The state machine cannot
  distinguish synthetic from OS-sourced events — both paths land in
  the same FSM state, produce the same `StartCapture` /
  `StopCapture` `StateAction`s, drive the same orchestrator, and
  emit the same `dictation:state` events to the UI.

Session rows produced by mode (a) and mode (b) are schema-identical.
We are **not** persisting a `start_mode` discriminator on the session
row. Rationale: the FSM is the integration boundary, and both modes
look like the same gesture from the FSM's POV. Adding a field would
require plumbing source-of-truth through the state machine for no
downstream consumer's benefit. If a future analytics need arises
(e.g. "how many users adopt programmatic start"), it can be added as
a non-breaking column then.

### Stop semantics: explicit only, no auto-stop

Mode (b) sessions run until an explicit `dictation_stop` IPC arrives.
There is **no** silence-based auto-stop, no max-duration cap (beyond
whatever existing per-session limits the orchestrator already
enforces), no fallback to "release on Right Alt".

Rationale:

- Mode (a)'s natural stop signal is the user releasing the key — a
  bodily gesture they're already making. Mode (b)'s natural stop
  signal is the user clicking Stop — a deliberate gesture.
  Auto-stopping on silence would mean the recording can end without
  the user asking it to, which is exactly the surprise that
  Wispr-style "smart" stops were rejected in favor of PTT in the
  first place.
- Meeting capture is already explicit-stop (no silence-based
  shutdown) and that's the right precedent for any non-bodily start
  gesture.
- A "Stop after 60s of silence" toggle is trivial to add later if
  real users ask for it. YAGNI says don't pre-bake it.

The user has three independent paths to stop a mode-(b) session:

1. Click "Stop Dictation" on the Dictations page (the button that
   started it).
2. Click Stop on the Command Center's `ShowingSessionCard{Dictation}`
   (the same surface that stops Meeting and Activity sessions).
3. Click Stop on the recording-pill overlay (Esc-cancel works
   identically to the PTT path, just like for the Meeting overlay).

### Orchestrator engine change

`command_center::drive::DispatchOutcome::NoProgrammaticStart` is
removed. The engine now has a single non-error success branch
(`Replied { success: bool }`), and Dictation's `TauriEffects::
dispatch_start` calls `DictationRuntime::start()` and returns
`Replied { success: <ok> }` like Meeting and Activity do.

This is a small simplification — one fewer variant, one fewer
dispatch arm, one fewer special case in the test matrix. The
`path3_dictation_tile_pick_dismisses_and_emits_closed` test is
replaced by `path3_dictation_tile_pick_lands_on_sessioncard`,
asserting the same shape as the Meeting and Activity equivalents.

### CC Dictation tile copy

The tile's "or just hold Right Alt" hint stays — Right Alt PTT is
still the primary fast-path and discoverability matters. The
behavior on tile pick changes from "dismiss" to "start a dictation
session via the programmatic path", which is what every other tile
already does.

### `mb-ytex` is closed by this work

The silent-dismiss symptom is resolved by the same change that
introduces programmatic start. The bead's resolution should
reference this ADR + the implementation commit.

## Consequences

### Positive

- **UX consistency.** All three tiles in the mode picker now produce
  a `SessionCard`-shaped result. No more "did I click that?" moment.
- **Accessibility / hands-free.** A user who can't comfortably hold
  Right Alt for the duration of an utterance has a viable start
  path.
- **Simpler engine.** `NoProgrammaticStart` was a one-off branch
  whose only purpose was to express "dismiss instead of starting".
  Removing it shrinks the orchestrator's variant matrix and lets
  every dispatched start follow the same shape.
- **Closes mb-ytex without a separate fix.**

### Negative

- **Two paths to stop a session that the user didn't visibly start.**
  If the user starts via the button and then forgets about it, the
  recording continues until they hit Stop. The recording-pill
  overlay is the user-visible "this is recording" signal; if the
  user has dismissed it, they could in theory forget. Mitigation:
  the pill is non-dismissable (only Stop closes it).
- **`dictation_stop` while no session is active is a silent no-op.**
  An over-eager UI could call `dictation_stop` after the session
  has already finalized via Esc-cancel; we treat this as idempotent
  (inject `KeyUp` regardless; the state machine ignores `KeyUp` in
  `Idle`). Logging at `debug` so we can spot it during dev.

### Neutral

- **No DB schema change.** Sessions remain schema-identical across
  modes.
- **Right Alt PTT untouched.** Zero risk of regressing the original
  fast-path.
- **No new windows.** Capabilities file unchanged.

## Alternatives considered

### A. Run dictation in a separate "always-on hold-toggle" mode

Add a `HotkeyMode::Toggle` variant where the state machine starts
recording on the first KeyDown and stops on the next KeyDown
(instead of on KeyUp). The PTT path would default to the existing
behavior; the programmatic path would route through Toggle.

**Rejected** because: it splits the FSM into two effectively-parallel
modes, doubles the test matrix, and complicates the state diagram
for no clear benefit. The synthetic-event approach reuses the
existing FSM verbatim — KeyDown to start, KeyUp to stop, same as a
real key hold.

### B. Add a `Mode (programmatic)` variant alongside `Normal` / `Fragment` / `Verbose`

Persist start mode on the session row. Surface it in the Dictations
list as a badge.

**Rejected** because: YAGNI. No downstream consumer (analytics,
billing, prompt selection) needs to distinguish. The session row is
behaviorally identical; the start gesture is forgotten the moment
recording begins. We can add the column non-breakingly later if a
need surfaces.

### C. Leave NoProgrammaticStart in place; add a separate "non-CC programmatic" entry point that bypasses the FSM

Build a second path through the orchestrator that bypasses the state
machine entirely.

**Rejected** because: two integration boundaries for the same
operation is the textbook DRY violation. Whichever path landed
second would inevitably miss invariants the first path enforces
(e.g. secure-input guard, pause-handle gating). Synthetic events on
the existing channel is the single integration point.

## Non-Goals

- Persisting `start_mode` on the session row (revisit if analytics
  needs it).
- Auto-stop on silence (revisit if users ask for it).
- A separate "toggle" hotkey that does the same thing as the button
  (button + Right Alt PTT cover the surface area we care about).
- Mode-(b) keyboard shortcut from the Dictations page (the button
  is enough; a global hotkey would compete with the Right Alt PTT).

## Cargo gate (binding per LESSONS P1)

- `… cargo-with-cuda.ps1 check`
- `… cargo-with-cuda.ps1 clippy --release -- -D warnings`
- `… cargo-with-cuda.ps1 fmt --check`
- `… cargo-with-cuda.ps1 test --release --no-run`
- Throwaway-crate live tests for `command_center::drive` if any
  test logic changes there (LESSONS P2).

## Cross-references

- ADR 0037 — original Command Center charter; this ADR amends §4.
- mb-ddfx — implementing bead.
- mb-ytex — silent-dismiss bug; absorbed and closed by mb-ddfx.
- LESSONS PINNED P1, P2 — Windows cargo wrapper + throwaway-crate
  for testing wired modules.
