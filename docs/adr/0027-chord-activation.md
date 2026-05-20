# ADR-0027: Chord activation (Right Ctrl + M) via a dedicated meetings message-pump thread

- **Status:** Accepted
- **Date:** 2026-05-20
- **Deciders:** Dustin (project lead), code-puppy (implementor)
- **Phase MC companion to:** ADR 0026 (sibling-subsystem charter)

## Context

Dictation uses a single `WH_KEYBOARD_LL` hook installed by
`hotkey/windows.rs`, owned by a thread spawned by `hotkey/driver.rs`,
and discriminated **tap-vs-hold** on Right Alt by the state machine in
`hotkey/state.rs`. ADR 0015 §3 binds the callback to "no work in the
hook — post the event to a channel and return immediately." ADR 0019
governs the conflict-probe fallback ladder when the chosen VK is
already claimed.

Phase MC needs an activation gesture that is **distinguishable from
dictation** and **keeps the user's hand near the same modifier zone**
so muscle memory transfers cleanly between the two features. The
gesture is also a **toggle** (press to start a meeting, press again to
stop) rather than a hold.

Three gesture families were considered seriously:

1. **Single-key double-tap** (e.g. double-tap Pause/Break). One key,
   timing-window discrimination. Hand moves away from the dictation
   modifier zone. Requires a multi-state timing-window state machine.
2. **Right Alt + M chord** (modifier reuse). Maximum muscle-memory
   continuity with dictation. But Right Alt is *already* the dictation
   hold-to-talk key — every meeting trigger would fire a phantom
   dictation session because the dictation hook arms on Right Alt
   down. The clean fix (have the meeting hook signal the dictation hook
   to discard) requires modifying `hotkey/state.rs`, which is **sealed
   by the Phase MC binding list**.
3. **Right Ctrl + M chord** (adjacent modifier). One key over from
   Right Alt on every standard keyboard layout. The user's right hand
   sits in the same neighborhood. Disjoint VK sets from dictation
   (dictation: `VK_RMENU`; meeting modifier: `VK_RCONTROL`). Zero
   cross-talk between the two hooks.

The forcing question for the implementation side is identical to what
ADR 0027's original double-tap framing asked: **where does the meeting
hook's message-pump thread live?** Two options:

- **Same thread as the dictation hook.** Apparent micro-optimum (one
  thread instead of two). Requires modifying `hotkey/driver.rs` to
  expose a "register a sibling hook" entry point — which is sealed by
  the binding list. Relaxing the binding for this single method
  requires a sub-ADR and weakens the
  `block-cross-module-coupling-meeting-dictation` hook.
- **Dedicated meetings message-pump thread.** Costs one Windows thread
  sitting in `GetMessageW` (kernel-managed wait, sub-microsecond
  per dispatched keystroke, zero CPU idle). Buys zero modification of
  the sealed dictation surface and a strict coupling hook.

## Decision

**Phase MC installs a second `WH_KEYBOARD_LL` system-wide hook on a
dedicated message-pump thread owned by the meetings runtime. The hook
listens for a configurable modifier + main-key chord (default
`VK_RCONTROL` + `VK_M`). The dictation hook, its driver, and its thread
are not touched.**

Concretely:

1. **`meetings/activation.rs` is a pure-Rust state machine** (no
   Windows API calls). It takes `ActivationEvent` inputs (`ModifierDown
   { ts }`, `ModifierUp { ts }`, `MainKeyDown { ts }`, `MainKeyUp
   { ts }`, `Tick { ts }`, `PauseToggle { paused }`) and emits
   `ActivationAction` outputs (`MeetingToggle { source }`, `Noop`).
   The state diagram is in PLAN §MC.1. Three states: `IDLE`,
   `MOD_HELD`, `MAIN_PRESSED`. Fully unit-testable; target ≥20 tests.
2. **The chord fires once per main-keydown-while-modifier-held.**
   Windows auto-repeats held keys after ~500 ms; the `MAIN_PRESSED`
   state suppresses re-fires until main-keyup. Releasing and
   re-pressing main while the modifier stays held fires the chord
   again. This makes hold-the-chord-for-five-seconds a single
   meeting-toggle event, not spam.
3. **`meetings/runtime.rs` (Wave 3) spawns a `meetings_hook` thread**
   on `MeetingCaptureRuntime::start()`. The thread:
   - Calls `SetWindowsHookExW(WH_KEYBOARD_LL, proc, HINSTANCE(0), 0)`
     to install a parallel system-wide hook.
   - Runs the standard `GetMessageW` / `TranslateMessage` /
     `DispatchMessageW` loop.
   - The hook callback observes ONLY the configured modifier and
     main-key VKs (filtered against `KBDLLHOOKSTRUCT::vkCode`), posts
     the typed event to an `mpsc::Sender<ActivationEvent>`, and
     *immediately* returns via `CallNextHookEx` (ADR 0015 §3 honored).
   - On shutdown, posts `WM_QUIT` to itself, exits the message loop,
     and calls `UnhookWindowsHookEx`.
4. **Configurable modifier and main-key.** Two settings
   (`MeetingHotkeyModifier`, `MeetingHotkeyKey`) — both VK strings;
   modifier clamped to `{RCtrl, LCtrl, RAlt, LAlt, RShift, LShift,
   RWin, LWin}`, main-key any non-modifier VK. Default chord is
   `VK_RCONTROL` + `VK_M`.
5. **Conflict probe at startup** (Wave 3): if `MeetingHotkeyModifier`
   resolves to the same VK as the dictation hotkey, refuse to start
   the meetings hook and surface a tray-toast error. Fallback ladder
   `RCtrl+M` → `RCtrl+F13` → `RCtrl+F14` → user-pick.
6. **The dictation hook is uninvolved.** It sees `VK_RMENU` (Right
   Alt). The meeting hook sees `VK_RCONTROL` + `VK_M`. Windows
   dispatches `WM_KEYBOARD_LL` events to *each* installed hook
   independently; there is no shared state between the two installs.
   The `block-cross-module-coupling-meeting-dictation` hook (authored
   in Wave 1) rejects any diff that imports `hotkey::driver` or
   `hotkey::windows` from inside `meetings/`.

## Consequences

### Positive

- **Zero modification of sealed dictation surface.** The
  `mc-dictation-untouched` judge passes trivially: the meetings hook
  install and chord state machine live entirely under `meetings/`.
- **Disjoint VK observability between the two hooks.** Dictation
  watches `VK_RMENU`; meeting watches `VK_RCONTROL` + `VK_M`. No
  callback in either hook ever fires on the other's keys. No phantom
  dictation sessions from meeting triggers, no spurious meeting
  toggles from dictation holds.
- **Muscle-memory continuity.** Right Ctrl sits immediately adjacent
  to Right Alt on every standard keyboard. The user's right hand
  doesn't move; only the finger choice changes.
- **Pure-Rust state machine, fully unit-testable.** No Windows API
  surface is needed to drive the tests — feed `ActivationEvent`
  inputs, assert on `ActivationAction` outputs. The chord state
  machine is *simpler* than the double-tap alternative — three states,
  no timing windows — so the 20-test target is comfortable.
- **Auto-repeat suppression is explicit and tested.** Hold-the-chord
  fires once and only once until main-keyup; the test set covers it.

### Negative

- **One extra OS thread.** Sitting in `GetMessageW` is essentially
  free (kernel wait, zero CPU), but it's still one more thread for
  Process Explorer to list and one more thread to tear down cleanly on
  app shutdown.
- **Two hook callbacks dispatched per keystroke system-wide.** Each
  returns within microseconds (channel-send + `CallNextHookEx`) so the
  user-visible impact is nil, but a profiler will show two callback
  invocations per keypress instead of one.
- **Right Ctrl + M may collide with app-level shortcuts.** Some IDEs
  use Ctrl + M for line-wrap toggle, paragraph-break navigation, etc.
  Users encountering a collision rebind via Settings. The conflict
  probe only catches the dictation collision (we can't enumerate every
  app's shortcut table).
- **No per-side modifier distinction in the conflict probe v1.** The
  conflict probe compares VK codes directly; a user who configures the
  meeting modifier to `VK_LCONTROL` while a different app's hotkey is
  registered at `VK_CONTROL` (either-side) might not catch the
  collision. This is acceptable for v1 — the user's empirical fix
  (rebind in Settings) is one click.

### Neutral

- **The `Tick` input on `ActivationEvent` is unused** by the chord
  state machine (no timing windows). It's kept on the input enum for
  symmetry with future gesture state machines that *do* need ticks
  (a hypothetical "long-press meeting key for source picker" mode),
  and so the integration glue can dispatch a single event type to all
  gesture detectors.
- **On Wayland/Linux and macOS** the chord state machine is the same
  pure Rust; the platform hook layer differs. macOS/Linux land in the
  platform-expansion epic as `todo!()` stubs in Wave 1 (binding rule
  §15) and get fleshed out in Phase 9.

## Alternatives considered

- **Right Alt + M chord (reuse the dictation modifier).** Rejected.
  Right Alt is the dictation hold-to-talk key; every meeting trigger
  would also fire a brief dictation session (Right Alt-down arms the
  dictation hook, M-down doesn't disarm it, Right Alt-up triggers
  dictation processing). The clean fix requires modifying
  `hotkey/state.rs` to expose a "cancel pending arm" entry point —
  violates the binding list. Right Ctrl is one key over and avoids
  the problem entirely.

- **Single-key double-tap (e.g. double-tap Pause/Break).** Rejected
  after review. Hand moves away from the right-modifier zone,
  breaking muscle-memory transfer from dictation. Also requires a
  timing-window state machine with `WAITING_SECOND` / tick-driven
  timeout logic — strictly more complex than the three-state chord
  machine. The user's request was to keep the gesture in the same
  hand position as dictation; the chord does that, the double-tap
  doesn't.

- **Install the second hook on the dictation thread via a sibling-
  hook entry point on `HotkeyDriver`.** Rejected. Requires modifying
  `hotkey/driver.rs`, which violates the Phase MC binding list.
  Would require a sub-ADR ("ADR 0027a") to charter the relaxation,
  plus weakening the coupling hook. The thread savings (one fewer
  `GetMessageW`-blocked thread) are not worth the precedent.

- **Use `RegisterHotKey` (synchronous `WM_HOTKEY` delivery) instead
  of `WH_KEYBOARD_LL`.** Rejected. ADR 0015 chose `WH_KEYBOARD_LL`
  over `RegisterHotKey` for app-agnostic interception (`WM_HOTKEY`
  only delivers when the target window has focus or the system is
  configured to deliver thread-wide hotkeys). For chord detection
  specifically, `RegisterHotKey` would deliver only the chord-down
  event (which is what we want) and would require a hidden window to
  receive `WM_HOTKEY`. About the same cost as the LL hook. The LL
  hook generalizes to future "modifier + main + qualifier" three-key
  combos which `RegisterHotKey` can't express.

- **Reuse the dictation hook with a wrapping discriminator.**
  Rejected. Requires modifying `hotkey/windows.rs` — sealed. Also
  makes the dictation callback do more work (filter for additional
  VKs and chord state), violating ADR 0015 §3's spirit even if not
  its letter.

## Cross-references

- **PLAN:** `docs/phases/phase-meeting-capture.md` §MC.1 (chord
  activation state diagram), Wave 3 task table (hook install +
  conflict probe), Wave 5 (settings UI surface for modifier + main-
  key picker).
- **ADR 0026:** charter — establishes the binding list this ADR
  honors.
- **ADR 0015:** existing dictation hook — defines the "no work in
  callback" contract this ADR also honors for the new hook.
- **ADR 0019:** hotkey conflict detection — fallback ladder reused
  (adapted for the modifier + main-key pair).
- **bd issues:** this ADR is `mb-7q0`. Install + probe work
  scheduled for Wave 3 (separate bd tasks to be created in Wave 2's
  brief).

---

_The `adr-format` judge validates this structure exists in every numbered
ADR. Keep section headings stable._
