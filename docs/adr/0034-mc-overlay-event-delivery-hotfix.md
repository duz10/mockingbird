# ADR 0034 — Meeting overlay event-delivery hotfix (mb-z5y)

- **Status:** Accepted
- **Date:** 2026-05-23
- **Supersedes:** none. Lateral hotfix on sealed Phase MC (charter on
  top of ADR 0033 which already shipped the surrounding wiring).
- **Beads issue:** `mb-z5y` (P1 bug).

## Context

ADR 0033 ("MC chord collision + UI wires hotfix", git `a4e0ec3`) shipped
the meeting-overlay listener wiring, `force_show_for_recording`, and
the close (`×`) button. The user reported on 2026-05-23 from a binary
verifiably built **after** that hotfix:

1. Clicking **Start recording** on the main Meetings page starts the
   timer ticking (so `recordingUuid` was set via the optimistic
   post-IPC update) and the overlay window appears.
2. **But** the overlay stays in CHOOSE mode (source picker + "Start a
   meeting" button) instead of flipping to the RECORDING pill.
3. The **Stop** button in the main window stays disabled — the
   `startingOrStopping` latch never clears.
4. The **`×`** dismiss button on the overlay does not respond.

All four symptoms collapse to one root cause: the
`meeting:state="started"` event emitted by
`meetings::lifecycle::emit_state` never reaches the React listeners in
either webview. Both `Meetings.tsx::handleStart` and
`MeetingOverlay.tsx`'s mount-time listener depend on that event to
clear `startingOrStopping` and flip the overlay mode respectively. The
overlay window's `×`-button issue is collateral: the overlay UI is
fundamentally stuck on its initial render (`mode === "choose"`,
`busy === "starting"`) so the user's clicks land on a non-interactive
state.

ADR 0033's premise was correct (the wiring exists). The miss was
ordering: in `commands::meetings::meeting_start` the lifecycle path
called `start_meeting()` (which fires `emit_state("started")` on a
broadcast `app_handle.emit`) **before** `force_show_for_recording`.
The overlay window's `visible: false` initial state appears to make
the webview effectively dormant for broadcast `emit` delivery in
Tauri 2.1.x — the listener is registered (the JS ran at app boot) but
the broadcast event is dropped on the hidden webview.

That's also consistent with the silent failure: `emit_state` used
`let _ = self.app_handle.emit(...)`, so a returned `Err` (if any)
would be invisible.

## Decision

A four-layer defensive fix, all small + local; no architecture
changes. Listed in order of "would alone be sufficient":

1. **Swap show/start order in `meeting_start` IPC.** Call
   `force_show_for_recording` **before** `start_meeting()`, so the
   overlay's webview is unambiguously in the event-receive path when
   the lifecycle path emits `meeting:state="started"`. On `start_meeting`
   error, hide the overlay again so we don't strand a blank pill.

2. **Belt-and-suspenders `emit_to` re-broadcast.** After the
   lifecycle path's broadcast emit, the IPC command also calls
   `app_handle.emit_to(MEETING_OVERLAY_LABEL, "meeting:state", payload)`
   targeting the overlay window specifically. Listeners are idempotent
   (`setMode("recording")` twice is a no-op), so the duplicate is
   harmless. This catches any future regression in broadcast emit
   reaching hidden-or-just-shown webviews.

3. **Frontend defensive clear in `Meetings.tsx::handleStart`.** The
   handler already optimistically sets `recordingUuid` after the IPC
   await resolves; symmetrically, it now also clears
   `startingOrStopping` on success. The listener still clears it
   too — both paths idempotent. The Stop button enables as soon as
   the IPC roundtrip completes, with no dependency on event delivery.

4. **Observability in `emit_state`.** Replace
   `let _ = self.app_handle.emit(...)` with a matched
   `tracing::debug!` (Ok) / `tracing::warn!` (Err) — mirrors the
   pattern already in `dictation::recording_window::emit_state`.
   Future regressions in event delivery surface in logs immediately
   instead of going undiagnosed until live-fire.

## Consequences

- **+:** The Stop button + overlay mode now respond on the
  hottest path (#1, #3) AND the most fragile path (#2). Even if
  Tauri 2.x changes broadcast semantics again, three of the four
  layers stand independently.
- **+:** Cross-window event delivery is now observable. Future
  regressions of this exact class produce a `warn!` log line; the
  silent-failure precondition that hid this bug is removed.
- **−:** The IPC handler now has slightly more code (overlay-hide on
  error path; `emit_to` re-broadcast). Net ~25 lines.
- **−:** No deterministic test for the new ordering — testing IPC
  command ordering against a real `MeetingRuntimeShared` requires a
  full Tauri test harness which this project doesn't have. The
  ordering is documented in code comments referencing this ADR and
  the LESSONS entry, and the manual repro from `mb-z5y` is the
  acceptance gate.
- **=:** Nothing changes for the chord-driven path
  (`handle_toggle` → `show_overlay` → user clicks Start in overlay
  → overlay's own `handleStart` fires `meetings.start`). That path
  already had the overlay shown before any emit and the overlay's
  own handler optimistically flips to RECORDING.

## Sealed by

- This ADR (Accepted).
- Bug `mb-z5y` closed in the same iteration.
- Commit message references both.

## Cross-references

- ADR 0033 — the predecessor wiring that this hotfix builds on (NOT
  superseded; ADR 0033's claims remain accurate post-this-fix).
- LESSONS 2026-05-23 — incident debrief covering both the bug and
  the AGENTS.md session-start over-correction it surfaced.
- AGENTS.md § "Permanently sealed" — Phase MC stays sealed; this is
  a lateral hotfix on the already-sealed phase, not a re-execution.
