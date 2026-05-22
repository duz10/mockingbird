# ADR 0035 — MC v1.2 Stable Alpha (capabilities migration + cancel + rename + auto-title + WASAPI loopback fix)

- **Status:** Accepted
- **Date:** 2026-05-24
- **Supersedes:** none. Builds on ADR 0034 (mb-z5y overlay event-delivery
  hotfix). ADR 0034 stays Accepted — its claims remain correct
  post-this-ADR; this is a follow-on epic, not a replacement.
- **Beads issues:** `mb-mc12-caps`, `mb-mc12-cancel`, `mb-mc12-rename`,
  `mb-mc12-title`, `mb-mc12-loopback`, `mb-mc12-pingdebug` (all closed
  in this iteration).
- **Stable-alpha tag:** `stable-alpha-v0.1` lands on the commit that
  seals this ADR — first user-visible Mockingbird build with a stable
  meeting subsystem.

## Context

ADR 0034 (commit `646e7ba`) hot-fixed the meeting-overlay event delivery
by changing emit ordering + adding a belt-and-suspenders `emit_to`
re-broadcast. That worked, but live-fire after that ship surfaced six
related issues that the next iteration cleaned up together:

1. **Tauri capabilities config was empty.** The reason `listen()`,
   `emit_to()`, and `window.hide()` were unreliable from the overlay
   webview wasn't just the show/emit race ADR 0034 fixed — it was
   *also* that no `capabilities/default.json` existed, so the
   Tauri 2.x permission system silently no-op'd those calls on
   non-main windows. `invoke()` of `#[tauri::command]` handlers
   still worked (different permission path), which is exactly why the
   bug looked like an event-delivery race instead of a missing-perm
   problem.

2. **No way to cancel an in-flight meeting.** Stop saves; the only
   "discard" path was the drop-time `Interrupted` finalizer triggered
   by an app crash. Users who started a meeting by mistake had no
   clean way out — they had to Stop (which persists) and then Delete.

3. **No way to rename a saved meeting.** All meetings rendered as
   their start timestamp. After a few weeks of use the History page
   becomes a wall of dates indistinguishable from each other.

4. **No automatic title.** Even with rename, the *default* title
   needed to be more useful than the timestamp. A short
   deterministic heuristic over the formatted transcript ("first 5
   substantive words of the first speaker-stripped paragraph") gives
   a quick-glance label without any LLM call.

5. **WASAPI loopback config-discovery diverged from `probe_sources`.**
   `CpalCapture::build_stream` called `default_input_config()`
   unconditionally, which fails on render devices with "requested
   stream type is not supported." `probe_sources()` already had the
   right branch (uses `default_output_config()` for loopback per
   ADR 0031). Without the same branch in `build_stream`, two-channel
   meetings actually failed to start on some hardware — masked in
   testing because cpal sometimes accepted the wrong config and
   produced silent or garbled audio that survived to the
   "transcript is empty" branch.

6. **No forensic observability into JS listener firing.** ADR 0034
   added `tracing::debug!`/`warn!` to the Rust emit side, but the JS
   listener side was inferred from observable UI symptoms. To
   distinguish "emit landed but React listener didn't fire" from
   "React listener fired but state update raced something else"
   needed a JS-side beacon.

## Decision

One coherent epic, six small additions:

1. **Add `src-tauri/capabilities/default.json`** declaring
   `core:default` permissions for the `main`, `recording`, and
   `meeting_overlay` windows. Without this file, Tauri 2.x's
   permission system is empty by default — events and window control
   silently no-op on the secondary webviews. This is the *real*
   root cause of the mb-z5y class of bugs; ADR 0034's fixes also
   work but are belt-and-suspenders on top of this.

2. **Add `meeting_cancel` IPC** wired to
   `MeetingRuntimeShared::cancel_meeting()`. The runtime path mirrors
   the early teardown of `finalize_meeting` (stop tick emitter → stop
   capture → join long-form) then **deletes the on-disk chunk
   directory** rather than persisting. Emits
   `meeting:state=cancelled`. The PLAN Principle 1 (raw immutability)
   invariant is not violated — nothing in `transcripts(stage='raw')`
   has been written at the point this runs; we're cleaning up the
   pre-DB scratch space the user told us to throw away. The overlay
   `×` button now wires to `meetings.cancel()` while RECORDING and
   `meetings.overlayHide()` while CHOOSE/READY.

3. **Add `meeting_rename` IPC** wired to `repo::rename_meeting()`.
   Empty / whitespace-only titles coerce to `None` (which then
   defaults to the auto-derived title at render time). Idempotent on
   missing uuids — clicking rename on a row that just got deleted
   does not toast a scary error. The `meeting_sessions.title` column
   is mutable; this does *not* violate PLAN Principle 1 because the
   session header row is not a `raw`-stage transcript.

4. **Add `meetings/title.rs`** — pure heuristic title-derivation.
   Picks `merged` channel preferentially, falls back to mic then
   sys. Strips `**You:**` / `**Other(s):**` markdown speaker labels.
   ≤ 5 words, ≤ 60 chars, UTF-8-safe capitalization, skips
   pure-filler paragraphs ("...", single-letter fragments).
   ~25 unit tests including unicode, apostrophes, hyphens, and
   realistic two-speaker merged-formatter output. Wired in
   `lifecycle::build_persist_request`. No I/O, no DB, no clock —
   deterministic and runs in O(words-in-first-paragraph) at
   meeting-finalize time.

5. **Fix `CpalCapture::build_stream`** to branch on
   `DeviceSource::{Input, Loopback}` — Input uses
   `default_input_config()`, Loopback uses `default_output_config()`.
   This is what `probe_sources` already does and what cpal's WASAPI
   backend actually requires (it transparently flips on
   `AUDCLNT_STREAMFLAGS_LOOPBACK` when `build_input_stream` is called
   on a render device — but the *config-discovery* call still has to
   target the device's native output format). See ADR 0031 for the
   loopback backend background.

6. **Add `meeting_overlay_hide` IPC** as a belt-and-suspenders Rust
   path for window hide. JS-side `getCurrentWindow().hide()`
   silently no-ops on Win32 when called synchronously from a button
   onClick handler — routing through Rust uses the AppHandle's
   window registry and works from any context. Same pattern as the
   post-`done` Rust fallback hide.

7. **Add `meeting_debug_listener_ping` IPC** as a forensic beacon.
   React listeners in `Meetings.tsx` and `MeetingOverlay.tsx` call
   this from inside their `meeting:state` callbacks. Rust side
   only logs. Gives hard evidence of JS listener firing for the
   next time this class of bug shows up. **Marked for removal**
   once a Wave-6-style judge can assert event delivery without it.

## Consequences

- **+:** Mockingbird now has a complete meeting subsystem at user-
  level UX parity with what was promised in the PLAN: start, pause,
  stop, cancel, rename, search, delete, export. The "stable alpha"
  designation is now defensible.
- **+:** The capabilities migration closes the *real* root cause of
  the mb-z5y bug class. Future cross-window event/window-control
  bugs become much rarer; when they do appear, the new
  `meeting_debug_listener_ping` beacon makes them debuggable.
- **+:** Two-channel meetings now reliably start on hardware where
  cpal previously refused. ADR 0031's invariants are honored
  end-to-end (was: only honored in `probe_sources`).
- **+:** Auto-derived titles + rename make the meetings history list
  scannable rather than a wall of dates. Pure module, easy to bump
  the heuristic later if needed.
- **−:** `meeting_debug_listener_ping` is intentionally scoped for
  removal. Tracking it as `mb-mc12-pingdebug` with a "follow-up
  cleanup" tag. Not a blocker for sealing — having it in place is a
  net positive for the next event-delivery bug, *if* it ever shows
  up again.
- **−:** No automated test for `meeting_cancel` end-to-end — same
  Tauri-test-harness gap noted in ADR 0034. The manual repro
  (start meeting → click `×` → confirm no DB row + no chunk dir
  remains) is the acceptance gate for this iteration.
- **=:** PLAN Principle 1 (raw immutability), Principle 2 (provenance
  totality), and Principle 4 (no telemetry) all remain honored.
  Cancel deletes pre-raw scratch; rename mutates a non-raw column;
  the debug-ping logs only to the local file logger.

## Sealed by

- This ADR (Accepted).
- Beads `mb-mc12-*` closed in the same iteration.
- Commit `<filled-in-at-commit-time>` and the
  `stable-alpha-v0.1` git tag mark the seal.

## Cross-references

- ADR 0031 — WASAPI loopback backend (referenced by the capture.rs
  fix; loopback semantics inherited unchanged).
- ADR 0033 — chord collision + UI wires hotfix (predecessor to
  ADR 0034).
- ADR 0034 — mb-z5y overlay event-delivery hotfix (this ADR's
  immediate predecessor; ADR 0034's belt-and-suspenders fixes
  remain in place alongside the deeper capabilities fix here).
- LESSONS 2026-05-24 — incident debrief covering the capabilities
  miss + the divergence between `build_stream` and `probe_sources`.
- AGENTS.md § "Permanently sealed" — Phase MC stays sealed; this is
  the ADR-chartered lateral epic vehicle (per ADR 0032 / 0033 / 0034
  precedent), not a re-execution of the sealed phase.
