# ADR-0019: Hotkey conflict detection + F23/F24 fallback ladder

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** Dustin (project lead), code-puppy (implementor), planning-agent

## Context

The default hotkey is **Right Alt** (PLAN §6.1 example). Some users
have remapped Right Alt to Compose, Esc, or AltGr-as-modifier; some
games consume Right Alt for crouch; some accessibility tools remap
it entirely. We need:

1. To detect at startup whether the configured hotkey is "already
   in use" enough that holding it produces wrong behaviour;
2. To offer a fallback that survives on virtually every keyboard +
   every remapper;
3. To let the user override the choice via settings (Phase 5 surfaces
   the UI; Phase 3 reads the keys).

Detection is the hard part. `WH_KEYBOARD_LL` is a chain — multiple
hooks can coexist, and we cannot enumerate other hook owners through
documented APIs. `RegisterHotKey` returns `ERROR_HOTKEY_ALREADY_REGISTERED`
only for that specific API's registry, which is disjoint from our
low-level hook. There is **no perfect "is this hotkey free?" test on
Windows.**

We will use a heuristic + an explicit user-confirmation step (Phase 5)
+ a safe-default fallback ladder.

## Decision

### Startup probe (heuristic)

For the configured hotkey:

- If it is a modifier combination (`Ctrl+Shift+X`, etc.), call
  `RegisterHotKey(HWND_NULL, 0xCAFE, mods, vk)`. If it succeeds, we
  release immediately (`UnregisterHotKey`). If it returns
  `ERROR_HOTKEY_ALREADY_REGISTERED`, we treat it as occupied.
- If it is a raw VK (e.g. Right Alt = `VK_RMENU`), we assume free.
  The low-level hook is shared/chained anyway, so we will still
  receive events. The risk is that *another* hook earlier in the
  chain consumes the keystroke before we see it — undetectable
  without explicit user confirmation.

### Fallback ladder

If the probe says "occupied", we walk:

```text
configured_hotkey  →  Right Alt   (PLAN §6.1 default)
                  →  F23
                  →  F24
                  →  Ctrl+Shift+Space
                  →  USER_PROMPT (Phase 5 wizard)
```

F23 (`VK_F23` = `0x86`) and F24 (`VK_F24` = `0x87`) are deliberately
unmapped on standard layouts and survive virtually every
stenography / gaming-keyboard remapper. They have no native OS
binding on Windows 10/11.

Ctrl+Shift+Space is a last-resort modifier combination: relatively
unlikely to clash with apps, but not a tap-friendly hold target.
The fallback ladder uses it as a stopgap, not a recommendation.

### Resolved binding written to settings

```json
"hotkey": {
  "binding": "VK_RMENU",
  "resolved_from": "default"
}
```

`resolved_from` is one of:

- `"default"` — PLAN default (Right Alt), probe was clean.
- `"conflict_fallback"` — probe failed; we walked the ladder. The
  exact step is in `binding`.
- `"user_override"` — settings.json hand-edit or future Phase-5 UI.

On conflict, emit a tray toast: "⚠️ Right Alt appears taken — falling
back to F23. Open settings to change." Auto-dismiss 8 s.

### Phase-5 wizard hook

A boolean settings flag `hotkey.user_confirmed` defaults `false`.
The Phase-5 first-run wizard surfaces a "test the hotkey" step,
sets `user_confirmed = true` on success, and offers a "pick a
different key" option. Phase 3 does not surface UI; it only writes
the resolved binding and emits the toast.

### Watchdog (Phase 3 includes this)

After the hook is installed, a background timer logs at WARN level
if no `HotkeyEvent::KeyDown` has been received for 5 minutes AND the
user has not toggled "pause dictation". This catches silent unhook
events (ADR 0015 risk #3) and other dead-hook conditions. It is not
a UX surface in Phase 3 — only a log signal for triage. Phase 5 may
upgrade to a tray status indicator.

## Consequences

- **Positive:**
  - Default works for ~98% of users without any prompting.
  - The 2% who have Right Alt remapped get an automatic, non-blocking
    fallback to F23 / F24.
  - Settings provenance is explicit (`resolved_from`).
  - Watchdog catches silent failures.
- **Negative:**
  - Conflict detection for raw VKs is fundamentally a guess. Real
    conflicts (another hook earlier in chain consuming events) are
    only detectable by user confirmation. Documented.
  - F23/F24 require a programmable keyboard or scripting tool to
    bind to; users without one need the Ctrl+Shift+Space step or
    a hand-edited settings.json. Phase 5 surfaces this clearly.
- **Neutral:**
  - The `RegisterHotKey` probe leaves no trace: we register and
    immediately unregister. Other apps cannot observe the probe.

## Alternatives considered

- **`UnhookWindowsHookEx` chain walk via undocumented kernel
  structures.** Available on older Windows; broken on Windows 10/11
  due to KASLR + structure changes. Hostile to maintenance. Rejected.
- **Ship without conflict detection.** Users with Right Alt remapped
  would see "dictation doesn't work" with no diagnostic and would
  uninstall before discovering settings. Rejected.
- **Force user to pick a hotkey on first run.** Maximizes friction
  for the 98% who'd be fine with the default. Phase 5 may move to
  this UX once we have data. Rejected for Phase 3.
- **Use a global mouse-button or chord (Shift+RightClick).** Mouse
  hotkeys are inferior for dictation ergonomics (hand must be on
  mouse). Rejected.

## Cross-references

- PLAN §6.1 (state machine + Right Alt example), §12 #20 (binding:
  low-level hook), §12 #11 (cross-app QA — confirms hotkey
  reachability per app)
- ADR 0015 (low-level keyboard hook — provides the hook handle
  whose silent unhook this ADR's watchdog catches)
- `docs/phases/phase3.md` Wave 3 (`hotkey/windows.rs`)
- Microsoft docs:
  <https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-registerhotkey>
