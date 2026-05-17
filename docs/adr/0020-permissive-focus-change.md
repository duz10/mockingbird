# ADR 0020 — Focus-change handling: permissive with provenance

**Status:** Accepted (Wave 4.9, 2026-05-17)
**Supersedes:** the focus-loss abort behaviour established by ADR 0016
("Injection strategy table") in its `decide_injection` precedence list.
**Related:** ADR 0017 (secure-input guard), ADR 0018 (clipboard
save/restore), ADR 0010 (raw transcripts are immutable).

## Context

The Wave-4 orchestrator captured a `ForegroundWindow` snapshot at both
hotkey key-down (`fg_keydown`) and key-up (`fg_keyup`). If the two
process names differed (case-insensitive), `decide_injection` returned
`AbortFocusChanged` and the session was persisted with
`injection_status = 'aborted_focus_changed'` — no paste, no toast,
silent drop.

Field use under the Wave-4 PoC surfaced two failure modes:

1. **The "alt-tab workflow" is normal.** Real users routinely begin
   a dictation in one window (the notes app where they were just
   reading) and finish it in another (the chat app or compose box
   they actually want to type into). The previous behaviour
   discarded those dictations silently — every one was a
   user-visible bug.
2. **The safety justification was weak.** The original ADR-0016
   reasoning was "don't paste into the wrong window." But the
   *actual* dangerous case — leaking dictated text into a password
   field — is independently handled by the secure-input guard
   (ADR 0017), which inspects `fg_keyup` and aborts regardless of
   what `fg_keydown` was. Focus-change abort was preventing a
   class of "wrong recipient" errors that, in practice, were
   user-intended and not security-sensitive.

Wisprflow's commercial product (the benchmark Mockingbird targets)
follows the permissive model: text lands wherever you released the
hotkey, period.

## Decision

`decide_injection` no longer treats focus change as an abort condition.

Concretely:

- The `InjectionDecision::AbortFocusChanged` variant is **removed**.
- `decide_injection` always proceeds to strategy resolution against
  `fg_keyup.process_name`.
- `fg_keydown` is still captured + still passed to `decide_injection`
  for *logging* purposes: when `fg_keydown.process_name !=
  fg_keyup.process_name` (case-insensitive) we emit a `tracing::info!`
  with both names so the focus change is visible in audit logs.
- The `InjectionOutcome::AbortedFocusChanged` enum variant + its DB
  string `"aborted_focus_changed"` are **kept**. Pre-4.9 user
  databases contain rows with that status, and removing the variant
  would break the `CHECK` constraint on `sessions.injection_status`.
  The variant is now legacy: no code path in the default pipeline
  emits it. A future opt-in "strict focus" mode could re-enable
  emission without a schema change.
- The secure-input guard (ADR 0017) continues to run on `fg_keyup`
  **before** strategy resolution. Permissive focus change does not
  weaken the secure-input invariant in any way.

## Why this is safe

| Concern | Mitigation |
|---|---|
| Dictation lands in a password field after focus change | Secure-input guard catches it on `fg_keyup` (ADR 0017) |
| Per-app abort list bypassed by focus change | `decide_injection` resolves strategy on `fg_keyup.process_name`, so `1Password.exe` etc. still aborts |
| Lost provenance — which app *did* receive the paste? | Session row's `foreground_app` + `foreground_window_title` are populated from `fg_keyup`; the `fg_keydown` divergence is logged at `info` level |
| User pastes into the "wrong" window by accident | Inherent risk of any voice-injection tool. The same hand that triggers the alt-tab is the same hand that releases the hotkey — agency lies with the user |

## Consequences

- Sessions that previously ended in `aborted_focus_changed` will now
  end in `complete` with a normal `injection_status` (e.g. `ok`,
  `aborted_secure`, `aborted_user_opt_out`).
- Wave-5 telemetry (when introduced) can use the focus-change log
  line to measure how often users alt-tab during dictation — a
  useful signal for UX work without being a blocker.
- The strategy-resolution test matrix shrinks by one branch (no
  more focus-change abort) but gains two new tests covering the
  permissive behaviour: focus change into a normal app proceeds,
  focus change into a secure-input app still aborts via the guard.

## Out of scope

- A user-facing "strict focus" toggle. Not building it until a real
  user requests it (YAGNI).
- Per-app focus-strictness overrides. Same reasoning.
