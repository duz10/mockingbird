# ADR-0015: Low-level keyboard hook over tauri-plugin-global-shortcut

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** Dustin (project lead), code-puppy (implementor), planning-agent

## Context

Phase 3 needs a global hotkey that fires on **both key-down and
key-up** — the §6.1 state machine distinguishes a tap (<80 ms — pass
through to OS) from a hold (≥80 ms — start recording). PLAN §12 #20 is
binding on this point.

`tauri-plugin-global-shortcut` (built atop
`global-hotkey` / `RegisterHotKey`) only fires once — on press —
and consumes the key entirely. There is no upstream key-up callback,
no "long-press" affordance, and `RegisterHotKey` is exclusive: holding
the key down does not repeat-fire reliably, and we can never observe
the release. That kills both the tap-pass-through behaviour and the
hold-duration discriminator.

Windows offers exactly one supported API that gives us both
transitions for arbitrary keys: `SetWindowsHookEx(WH_KEYBOARD_LL)`.
It is a system-wide low-level hook that receives every keystroke
(down + up + injected vs real) and can choose to consume or pass each
event. The cost is a strict performance contract.

## Decision

We will install our hotkey hook via `SetWindowsHookEx(WH_KEYBOARD_LL)`
directly through `windows-rs`. Specifically:

1. **A dedicated OS thread owns the hook** — separate from Tauri's
   main thread. The hook runs only while that thread's message-pump
   (`GetMessageW` loop) is alive. Phase 3 names this thread
   `mockingbird-hotkey`.
2. **The hook callback (`LowLevelKeyboardProc`) does no work.** It:
   - reads the `KBDLLHOOKSTRUCT`,
   - constructs a `HotkeyEvent` value,
   - `try_send`s on an `mpsc::Sender<HotkeyEvent>`,
   - returns `CallNextHookEx(...)` within microseconds.
   Any synchronous work inside the callback (DB writes, logging beyond
   `tracing::trace!`, audio capture, anything that allocates beyond a
   small enum) risks the Windows hook-timeout. The OS silently
   unhooks slow callbacks, and we'd see dictation "stop working" with
   no diagnostic.
3. **The state machine runs on a worker thread** that consumes the
   `mpsc::Receiver<HotkeyEvent>`. That thread can freely do timer
   bookkeeping, spawn audio capture, etc.
4. **The hook handle is owned by an RAII guard.** `Drop` calls
   `UnhookWindowsHookEx`. We never leak the handle across process
   exit; Windows reclaims it on thread death anyway, but explicit
   teardown keeps reload paths (settings change → rebind hotkey)
   clean.
5. **No `tauri-plugin-global-shortcut`** in `Cargo.toml`. No
   `global-hotkey` crate either.

## Consequences

- **Positive:**
  - Full tap-vs-hold state machine works correctly.
  - Hotkey passes through to the OS on taps (PLAN §6.1 requirement —
    Right Alt taps should still surface AltGr / native AltGr-key
    behaviour on European layouts).
  - We can selectively block/forward modifier-aware events.
  - Standard Windows API — no third-party hotkey daemon, no UAC
    elevation needed.
- **Negative:**
  - Low-level hooks are easy to misuse. Slow callbacks get silently
    unhooked; the next dictation attempt does nothing. We mitigate
    with the watchdog timer in ADR 0019.
  - Hooks are per-thread-message-loop. Tauri's main thread owns the
    UI loop; we cannot install the hook there without competing with
    Tauri's own messages. Hence the dedicated thread.
  - Anti-cheat / kernel-mode-protected processes may bypass user-mode
    hooks (Valorant, EAC, BattlEye). PLAN §3 / ADR 0016 documents the
    Abort fallback for these via the per-app strategy table.
- **Neutral:**
  - Low-level hooks slightly increase per-keystroke latency
    system-wide (microseconds). Imperceptible to users; measurable
    only with `xperf`.

## Alternatives considered

- **`tauri-plugin-global-shortcut` (RegisterHotKey).** No key-up
  callback. No hold detection. Consumes the key entirely. **Rejected
  — fails the §6.1 contract.**
- **`device_query` / polling at 60 Hz.** Misses fast taps; chews CPU;
  loses tap-vs-hold timing precision (16 ms granularity). Rejected.
- **Raw Input API (`RegisterRawInputDevices`).** Works in a window
  procedure; Mockingbird's main window is hidden and would need to
  surface a hidden message-only window. Net complexity is higher than
  `WH_KEYBOARD_LL` and we'd still need a dedicated thread. Rejected.
- **DirectInput.** Deprecated; XInput-only stack on modern Windows.
  Rejected.
- **`rdev` crate.** Wraps `WH_KEYBOARD_LL` but adds an async layer
  and pulls in `winapi 0.3`, conflicting with our `windows-rs 0.56`
  surface. Rejected — write the 80 lines ourselves.

## Cross-references

- PLAN §3 (Layer-3 injection — entry point), §6.1 (state machine),
  §12 #20 (binding: low-level hook)
- ADR 0016 (injection strategy — downstream of hook events)
- ADR 0019 (hotkey conflict probe — installs at same lifetime)
- `docs/phases/phase3.md` Wave 3 (`hotkey/windows.rs`)
- Microsoft docs: <https://learn.microsoft.com/windows/win32/winmsg/lowlevelkeyboardproc>
