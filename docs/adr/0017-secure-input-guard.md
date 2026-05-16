# ADR-0017: Secure-input guard policy

- **Status:** Accepted (amended 2026-05-17 Wave 2 — see "Update" below)
- **Date:** 2026-05-17
- **Deciders:** Dustin (project lead), code-puppy (implementor), planning-agent

## Context

PLAN §12 #18 is binding: **secure-input fields abort injection.** A
user holds the hotkey while focused on a UAC consent prompt, a login
dialog, or a password field; the pipeline must not paste, must not
write the user's password to the clipboard (even momentarily), and
must not leave a trace in the dictation history except as a
provenance record that says "aborted, secure field".

Windows offers no single API for "is the focused control secure?".
We compose three signals:

1. **`GetGUIThreadInfo(GUI_SECUREINPUT)`** — the OS sets this flag on
   the thread of the foreground window during UAC consent prompts
   and similar trusted-path UI.
2. **Class-name allowlist** — specific window classes are known to be
   secure (UAC's `$$$Secure UAP Dummy Layout$$$`, Credential UI's
   `CredentialDialogXamlHost`, Windows Hello's `LockApp`).
3. **`ES_PASSWORD` window style** — classic Win32 password edit
   controls (`Edit` class) carry this style. Detectable with
   `GetWindowLongPtrW(hwnd, GWL_STYLE) & ES_PASSWORD`. Modern Edge /
   Chrome render password fields without a real HWND (they're inside
   the WebView2 child process), so this signal is sufficient but not
   necessary.

Any one signal triggering must abort. They are OR-combined.

## Decision

`SecureInputGuard::is_secure(&ForegroundWindow) -> bool` returns true
if **any** of:

1. `GetGUIThreadInfo` on the foreground window's thread returns flags
   with `GUI_SECUREINPUT` (`0x00000040`) set.
2. The foreground window's class name (lowercased) matches the static
   allowlist:
   - `$$$secure uap dummy layout$$$` (UAC consent UI)
   - `credentialdialogxamlhost` (Modern Credential UI)
   - `lockapp` (Windows Hello / lock screen — defensive)
   - `consentui` (legacy UAC name, present on Windows 10)
3. The currently-focused child window (`GetGUIThreadInfo.hwndFocus`,
   which may differ from the foreground HWND) is an `Edit` control
   AND `GWL_STYLE & ES_PASSWORD != 0`.

On `true`:

- **Do not call `OpenClipboard` at all.** The transcript text never
  reaches the clipboard, even transiently.
- **Do not call `SendInput`.** No keystroke synthesis.
- **Persist the session row** with `injection_status =
  "aborted_secure"` and the raw transcript intact (provenance is total
  — PLAN principle #2). The transcript is still queryable in the
  history viewer (Phase 6); the user can see "I tried to dictate this
  into a password field — here's what I said, but it went nowhere".
- **Emit a tray toast:** "🔒 Secure field detected — transcript not
  pasted." Auto-dismiss after 5 s.
- **Do not persist captured audio.** PLAN §12 also forbids raw-audio
  retention without opt-in; we are doubly forbidden here.

The guard runs inside `dictation.rs` **before** the orchestrator
touches the clipboard or calls `Injector::inject(...)`. The ordering
is enforced by code structure, not by trust: there is no path through
the orchestrator that reaches `paste::with_saved_clipboard(...)`
without `SecureInputGuard::is_secure(...)` having returned `false`
first.

## Consequences

- **Positive:**
  - Defence in depth: three orthogonal signals OR-combined.
  - Zero clipboard exposure on secure fields.
  - Provenance preserved for user review.
  - Cheap — three Win32 calls, all of them O(1).
- **Negative:**
  - WebView2-hosted password fields (Edge, modern Chrome, Electron
    apps using BrowserView, every login form on the modern web) DO
    NOT trip `ES_PASSWORD` because the underlying HWND is a generic
    WebView surface, not a Win32 password edit. We rely on the user
    to enable the per-app override (`Abort` strategy for known
    password-manager processes) and on `GUI_SECUREINPUT` for
    OS-trusted paths. Documented prominently in CONTRIBUTING.
  - Class-name allowlist will drift as Microsoft renames internal
    classes. Maintenance burden documented; LESSONS entry on any
    confirmed miss.
- **Neutral:**
  - The check requires a `WindowContext::foreground()` snapshot
    immediately before injection — already required by ADR 0016's
    strategy resolver, so no extra cost.

## Alternatives considered

- **`GUI_SECUREINPUT` only.** Misses classic Win32 password fields
  outside trusted-path UI (e.g. legacy domain login dialogs).
  Rejected.
- **`ES_PASSWORD` only.** Misses UAC + Credential UI. Rejected.
- **UI Automation `IsPassword` property.** Beautiful in theory; rare
  in practice (only well-instrumented apps set it). Useful as a Tier-3
  signal in a future ADR — not Phase-3 scope.
- **Block always when no foreground HWND.** Too aggressive; legitimate
  scenarios (UWP "no foreground" transient states) would fail open
  to abort. Rejected.
- **Heuristic: focused control's text matches password-mask regex.**
  We have no access to control text — would need accessibility APIs
  that themselves require trust. Rejected as security theatre.

## Update — 2026-05-17 (Wave 2 implementation)

While wiring `injection/secure_guard.rs` in Wave 2, code-puppy went to
look up the `GUI_SECUREINPUT` flag value in `windows-rs` 0.56 and
discovered the constant doesn't exist — not in the crate and not in
the official Win32 SDK (`winuser.h`). The full set of
`GUITHREADINFO_FLAGS` is `GUI_CARETBLINKING` (0x1), `GUI_INMOVESIZE`
(0x2), `GUI_INMENUMODE` (0x4), `GUI_SYSTEMMENUMODE` (0x8),
`GUI_POPUPMENUMODE` (0x10). The original ADR conflated this with
macOS's `IsSecureEventInputEnabled()` (a real API).

**Amendment:** Signal 1 (`GUI_SECUREINPUT` check) is dropped. Signals
2 (class-name allowlist) and 3 (`ES_PASSWORD` on focused edit) remain
and are sufficient because:

- UAC consent prompts run on a separate **secure desktop** that our
  process cannot enumerate. During an active UAC prompt,
  `GetForegroundWindow()` returns NULL, which already causes
  `WindowContext::foreground()` (Wave 2) to error before any
  injection path is reached. The same applies to the Windows Hello
  PIN prompt, BitLocker recovery, and `Ctrl+Alt+Del`.
- The Credential UI (`CredentialDialogXamlHost`) runs on the normal
  desktop and is caught by signal 2 (class-name allowlist).
- Win32 password edits (legacy domain login dialogs, classic apps)
  are caught by signal 3 (`ES_PASSWORD`).
- WebView2 / modern-web password fields remain the documented gap
  ("Negative" §). Mitigation: per-app `Abort` overrides via ADR 0016
  for `1password.exe`, `bitwarden.exe`, etc.

Net effect on behaviour: zero regressions versus the originally-
imagined three-signal design. Two signals OR-combined still gives
defence in depth for every concrete attack scenario.

Follow-up: Section "Alternatives considered" reference to
`GUI_SECUREINPUT only` is now historical context — left in place so
future readers can see the reasoning trail without rewriting history.

## Cross-references

- PLAN §3 (Layer-3 injection), §12 #18 (binding: secure-input abort),
  §12 principle #2 (provenance is total — explains why we still
  persist the raw transcript on abort)
- ADR 0016 (injection strategy — strategy table also has Abort
  entries for password managers; this ADR is the orthogonal "OS-level
  trusted-path" guard)
- ADR 0010 (raw-transcript immutability — abort writes the raw row
  exactly once, no UPDATE path)
- `docs/phases/phase3.md` Wave 2 (`injection/secure_guard.rs`),
  Wave 5 judge `secure-input-respected`
- Microsoft docs: <https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-getguithreadinfo>
