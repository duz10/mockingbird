# ADR-0016: Injection strategy — paste default, keystroke fallback, per-app overrides

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** Dustin (project lead), code-puppy (implementor), planning-agent

## Context

Layer 3 of the pipeline (PLAN §3) is "type the cleaned transcript into
the focused app". We have three plausible mechanisms:

1. **Clipboard paste** — put the text on the clipboard, send Ctrl+V.
   Works in 95% of Win32 / Electron / UWP / Chrome / .NET apps. PLAN
   §3 names this **Tier 0** and ADR 0007 already pinned it as the
   default.
2. **Synthetic keystrokes** — `SendInput` with `KEYEVENTF_UNICODE`,
   one virtual key per character. Slower; preserves the user's
   clipboard untouched; works in apps that intercept Ctrl+V for
   non-paste actions (terminals, some games, vim normal-mode editors).
3. **UI Automation `TextPattern.SetValue`** — semantic injection,
   no synthetic input at all. Beautiful in theory; in practice the
   focused control rarely implements `TextPattern` correctly outside
   of accessibility-flagged dialogs. PLAN §3 marked this as
   Tier 2 / aspirational.

A single mechanism cannot serve every focused app. Windows Terminal,
`cmd.exe`, and PowerShell ISE all interpret Ctrl+V differently (or
not at all in raw consoles). Password managers should **never**
receive injected text from a dictation pipeline. Anti-cheat games
silently swallow `SendInput`.

## Decision

We will use **paste as the default, keystroke as the fallback, abort
for opt-out apps**, resolved by a per-app override table keyed on
process basename.

### Strategy enum

```rust
pub enum InjectionStrategy {
    /// Clipboard paste — Ctrl+V via SendInput, wrapped in the ADR-0018
    /// save/restore dance. Default for unknown apps.
    Paste,
    /// Per-character SendInput with KEYEVENTF_UNICODE. Clipboard
    /// untouched. Used in terminals + apps explicitly overriding.
    Keystroke,
    /// Do not inject. Emit tray toast. Persist raw transcript with
    /// `injection_status = "aborted_user_opt_out"`. Used for password
    /// managers + anti-cheat games.
    Abort,
}
```

### Resolution function

```rust
fn resolve(process_basename: &str, user_overrides: &HashMap<String, InjectionStrategy>)
    -> InjectionStrategy
```

User overrides win. Fallback to the static built-in table. Fallback to
`Paste`.

### Built-in table (v1)

Keys are case-insensitive process basenames (Windows is
case-insensitive for filesystem paths; we lowercase before lookup).

| Process basename            | Strategy   | Why |
|-----------------------------|------------|-----|
| `WindowsTerminal.exe`       | Keystroke  | Ctrl+V opens settings in some profiles; raw conhost paste is unreliable |
| `cmd.exe`                   | Keystroke  | Legacy conhost — Ctrl+V is a no-op unless QuickEdit-Insert is on |
| `powershell.exe`            | Keystroke  | Same as cmd |
| `pwsh.exe`                  | Keystroke  | PowerShell 7+ — same console host story |
| `conhost.exe`               | Keystroke  | Standalone console host |
| `1Password.exe`             | Abort      | Password field — never dictate into it |
| `KeePass.exe`               | Abort      | Same |
| `KeePassXC.exe`             | Abort      | Same |
| `Bitwarden.exe`             | Abort      | Same |
| `Vanguard.exe`              | Abort      | Riot anti-cheat |
| `EasyAntiCheat.exe`         | Abort      | Generic anti-cheat |
| `BEService.exe`             | Abort      | BattlEye |

Anything else: `Paste`.

### User overrides

Settings key `injection.app_overrides` is a JSON map:

```json
"injection": {
  "app_overrides": {
    "alacritty.exe": "keystroke",
    "obs64.exe": "abort"
  }
}
```

Phase 5 will surface UI for this. Phase 3 reads the map at startup +
on settings change.

### Storage implementation

Static built-in: `phf::Map<&'static str, InjectionStrategy>` —
zero-allocation compile-time lookup. The full table lives in
`src-tauri/src/injection/strategy.rs`.

## Consequences

- **Positive:**
  - Works out-of-the-box in ~95% of apps (Notepad, Word, Outlook,
    Slack, Teams, Gmail-in-Chrome, VS Code, Claude Desktop).
  - Terminal users get Keystroke automatically.
  - Password-manager users get explicit protection by default.
  - User has full control via settings.
- **Negative:**
  - `phf` adds a build-time codegen step (`phf_codegen` crate or
    `phf::phf_map!` macro — both ~zero runtime cost; modest compile
    cost).
  - Per-app table will need maintenance as apps ship updates that
    change their input-handling behaviour. Documented in
    CONTRIBUTING; users can override locally.
  - Two code paths (Paste / Keystroke) means two test matrices.
    Mitigated by sharing the `SendInput` substrate.
- **Neutral:**
  - We learn nothing about which strategy worked unless the user
    tells us — no telemetry (principle #4). The user-override map IS
    the feedback loop.

## Alternatives considered

- **Paste-only.** Fails in Windows Terminal + cmd. Rejected.
- **Keystroke-only.** Slow (~50–100 chars/s observed on `SendInput`
  Unicode injection). Mangles auto-correct + IME state on many apps.
  Wispr Flow uses paste-default — strong prior. Rejected.
- **UI Automation default.** Sparse impl across apps; falls through
  to paste/keystroke anyway. Deferred to Phase 8+ as a "Tier 2 add".
- **Auto-learn strategy per app from telemetry.** No telemetry
  (principle #4). User overrides are the manual analogue. Rejected
  in principle.

## Cross-references

- PLAN §3 (Layer 3 — three tiers; Tier 0 = paste), §12 #17 (binding:
  clipboard save/restore), §12 #18 (binding: secure-input abort),
  §12 #20 (binding: low-level hook)
- ADR 0007 (Tier-0 clipboard paste default — predecessor)
- ADR 0017 (secure-input guard — orthogonal: even Paste strategy
  aborts if the focus is a secure field)
- ADR 0018 (clipboard save/restore protocol — Paste's substrate)
- `docs/phases/phase3.md` Wave 2 + Wave 4
