# ADR-0027: Double-tap meeting activation via a dedicated meetings message-pump thread

- **Status:** Accepted
- **Date:** 2026-05-20
- **Deciders:** Dustin (project lead), code-puppy (implementor)
- **Phase MC companion to:** ADR 0026 (sibling-subsystem charter)

## Context

Dictation uses a single `WH_KEYBOARD_LL` hook installed by
`hotkey/windows.rs`, owned by a thread spawned by `hotkey/driver.rs`,
and discriminated tap-vs-hold by the state machine in `hotkey/state.rs`.
ADR 0015 §3 binds the callback to "no work in the hook — post the event
to a channel and return immediately." ADR 0019 governs the conflict-
probe fallback ladder (default → F23 → F24 → user-pick) when the chosen
VK is already claimed by another app.

Phase MC needs a **double-tap** activation gesture on a **different**
key (default `VK_PAUSE`, fallback `VK_F23`/`VK_F24` per ADR 0019). The
gesture is fundamentally different from dictation's tap-vs-hold: it
discriminates on the *interval between two key-down events*, not on the
*duration of one key-down-to-key-up* window. The double-tap window is
also configurable separately (`MeetingDoubleTapWindowMs`, default 400 ms,
clamp `[150, 800]`), which would re-litigate the dictation hook's
80 ms tap-vs-hold threshold every time a user tunes it.

**The forcing question**: where does the second hook's message-pump
thread live? Two candidates:

1. **Same thread as the dictation hook** — Windows allows multiple
   `WH_KEYBOARD_LL` hooks system-wide, and they chain via
   `CallNextHookEx`. A single thread could install both hooks and run
   one `GetMessageW` loop dispatching to both callbacks.
2. **A dedicated meetings message-pump thread** — the meetings runtime
   spawns its own thread, calls `SetWindowsHookEx`, and runs its own
   `GetMessageW` loop.

Option 1 is the apparent micro-optimum (one thread instead of two). It
also requires either modifying `hotkey/driver.rs` to expose a "register a
sibling hook on my thread" entry point, or having `meetings/` reach
inside the driver's privates to grab the thread handle — both of which
violate the Phase MC binding list (dictation's `hotkey/driver.rs` is
sealed). Relaxing the binding for this one method would mean
re-asserting the seal each time someone adds a Phase MC follow-up,
authoring a sub-ADR to charter the relaxation, and weakening the
`block-cross-module-coupling-meeting-dictation` hook's strictness.

The cost of option 2 is one Windows thread sitting in `GetMessageW`. On
modern Windows that's a kernel-managed wait — sub-microsecond per
keystroke dispatched, zero CPU while idle. The benefit is **zero
modification of the sealed dictation surface** and a strict coupling
hook that catches future drift mechanically.

## Decision

**Phase MC installs a second `WH_KEYBOARD_LL` system-wide hook on a
dedicated message-pump thread owned by the meetings runtime. The
dictation hook, its driver, and its thread are not touched.**

Concretely:

1. **`meetings/activation.rs` is a pure-Rust state machine** (no Windows
   API calls). It takes `ActivationEvent` inputs (`KeyDown { ts }`,
   `KeyUp { ts }`, `Tick { ts }`, `PauseToggle { paused }`) and emits
   `ActivationAction` outputs (`MeetingToggle { source }`, `Noop`). The
   state diagram is in PLAN §MC.1. Fully unit-testable; target ≥20
   tests covering all edges.
2. **`meetings/runtime.rs` (Wave 3) spawns a `meetings_hook` thread**
   on `MeetingCaptureRuntime::start()`. The thread:
   - Calls `SetWindowsHookExW(WH_KEYBOARD_LL, proc, HINSTANCE(0), 0)`
     to install a parallel system-wide hook.
   - Runs the standard `GetMessageW` / `TranslateMessage` /
     `DispatchMessageW` loop.
   - The hook callback observes ONLY the configured meeting key,
     posts a `KeyDown` / `KeyUp` event to an `mpsc::Sender<ActivationEvent>`,
     and *immediately* returns via `CallNextHookEx` (ADR 0015 §3 honored).
   - On shutdown, posts `WM_QUIT` to itself, exits the message loop,
     and calls `UnhookWindowsHookEx`.
3. **Conflict probe at startup** (Wave 3): if `MeetingHotkey` resolves
   to the same VK as the dictation hotkey, refuse to start the meetings
   hook and surface a tray-toast error. Fallback ladder Pause → F23 →
   F24 → user-pick mirrors ADR 0019.
4. **The dictation hook is uninvolved.** It sees its own configured key
   (default Right Alt). Windows dispatches `WM_KEYBOARD_LL` events to
   *each* installed hook independently; there is no shared state between
   the two installs. The `block-cross-module-coupling-meeting-dictation`
   hook (authored in Wave 1) rejects any diff that imports
   `hotkey::driver` or `hotkey::windows` from inside `meetings/`.

The double-tap window:

- Inputs are the `KeyDown` timestamps from the meetings hook.
- The state machine arms on `KeyDown(meeting_key)`, advances to
  `WAITING_SECOND` on `KeyUp`, and emits `MeetingToggle` if a second
  `KeyDown` arrives within `MeetingDoubleTapWindowMs` of the first
  `KeyUp` (default 400 ms, clamp `[150, 800]`).
- Tick-driven timeouts (>800 ms) reset to `IDLE`.
- Triple-tap is deterministic but doesn't fire twice: the third press
  starts a new `ARMED` state and waits for a fourth.

## Consequences

### Positive

- **Zero modification of sealed dictation surface.** The
  `mc-dictation-untouched` judge passes trivially: the meetings hook
  install lives entirely under `meetings/`.
- **State machine is pure Rust and fully unit-testable.** No Windows
  API surface is needed to drive the tests — feed `ActivationEvent`
  inputs, assert on `ActivationAction` outputs. Target ≥20 tests covers
  single-tap-too-slow, double-tap-too-fast, double-tap-too-slow,
  triple-tap, hold-then-tap, pause-toggle-while-armed, and the edge
  case where the second tap arrives exactly at the boundary.
- **The double-tap window can be re-tuned in isolation** without
  re-litigating dictation's 80 ms tap-vs-hold threshold. Two
  independently-evolvable timing knobs.
- **System-wide multi-hook behavior is well-documented Windows territory.**
  `SetWindowsHookEx` explicitly supports multiple installers per hook
  type, and the chain-via-`CallNextHookEx` contract is the same one the
  dictation hook already honors. No exotic Win32 contortion.

### Negative

- **One extra OS thread.** Sitting in `GetMessageW` is essentially free
  (kernel wait, zero CPU), but it's still one more thread for Process
  Explorer to list and one more thread that has to be torn down cleanly
  on app shutdown.
- **Two hook callbacks dispatched per keystroke system-wide.** Each
  callback returns within microseconds (channel-send + `CallNextHookEx`)
  so the user-visible impact is nil, but a profiler will show two
  callback invocations per keypress instead of one.
- **No shared conflict-detection between the two hooks at install time.**
  If the user manually configures the same VK for dictation and meeting
  (e.g. by editing settings JSON), both hooks will install successfully
  and both will fire on every press. The conflict probe in Wave 3
  catches the *configured* same-VK case at startup; the runtime
  protection against a malformed manual edit is the activation state
  machine's `Noop` branch ignoring meeting activation while the
  dictation state machine reports `Recording`.

### Neutral

- **On Wayland/Linux and macOS** the activation state machine is the
  same pure Rust; the platform hook layer differs. macOS/Linux land in
  the platform-expansion epic as `todo!()` stubs in Wave 1 (binding
  rule §15) and get fleshed out in Phase 9.

## Alternatives considered

- **Install the second hook on the dictation thread by exposing a
  `register_sibling_hook` entry point on `HotkeyDriver`.** Rejected.
  Requires modifying `hotkey/driver.rs`, which violates the Phase MC
  binding list. Would require a sub-ADR ("ADR 0027a") to charter the
  relaxation, plus weakening the coupling hook. The thread savings
  (one fewer `GetMessageW`-blocked thread) are not worth the precedent.

- **Use `RegisterHotKey` (synchronous `WM_HOTKEY` delivery) instead of
  `WH_KEYBOARD_LL`.** Rejected. ADR 0015 explicitly chose `WH_KEYBOARD_LL`
  over `RegisterHotKey` for app-agnostic interception (`WM_HOTKEY` only
  delivers when the target window has focus or the system is configured
  to deliver thread-wide hotkeys — both fragile). For double-tap detection
  specifically, `RegisterHotKey` would deliver only `KeyDown` events
  (good enough for double-tap timing) but would require a hidden window
  to receive `WM_HOTKEY`, which adds its own thread + window-class
  registration — about the same cost as the LL hook. The LL hook also
  generalizes to future "double-press X then hold Y" combos which
  `RegisterHotKey` can't express.

- **Reuse the dictation hook with a wrapping discriminator** (one
  callback observes both keys, dispatches to one of two state machines).
  Rejected. Requires modifying `hotkey/windows.rs` — sealed. Also makes
  the dictation callback do more work (it now has to filter for two
  VKs), violating ADR 0015 §3's spirit even if not its letter.

- **Defer double-tap to a future epic; ship Phase MC with hold-to-record
  meeting capture.** Rejected by Dustin during plan review. Hold-to-record
  for hours-long meetings is hostile UX; the WisprFlow-parity goal
  requires a "start, walk away, come back, stop" gesture.

## Cross-references

- **PLAN:** `docs/phases/phase-meeting-capture.md` §MC.1 (activation
  state diagram), Wave 3 task table (hook install + conflict probe).
- **ADR 0026:** charter — establishes the binding list this ADR honors.
- **ADR 0015:** existing dictation hook — defines the "no work in
  callback" contract this ADR also honors for the new hook.
- **ADR 0019:** hotkey conflict detection — fallback ladder reused.
- **bd issues:** this ADR is `mb-7q0`; install + probe work scheduled
  for Wave 3 (separate bd tasks to be created in Wave 2's brief).

---

_The `adr-format` judge validates this structure exists in every numbered
ADR. Keep section headings stable._
