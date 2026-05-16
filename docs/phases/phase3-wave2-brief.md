# Phase 3 — Wave 2 brief

**From:** code-puppy at end of Wave 1
**To:** code-puppy + injection-author for Wave 2
**Entry tag:** Wave 1 cargo gate green (164/164 tests, clippy `--release -D warnings` clean, fmt clean)
**Exit goal:** Wave 2 cargo gate green with the four deliverables below shipped + ≥36 new unit tests.

## What changed in Wave 1 (context Wave 2 needs)

1. **`AppError` has two new variants:** `AppError::Hotkey(String)` and `AppError::Injection(String)`. Wrap `windows::core::Error` as `String` at the construction site (mirrors `AppError::Stt`).
2. **Module skeletons exist with traits already defined** — Wave 2 fills them in. Do NOT change trait shapes without a quick check-in. Specifically:
   - `crate::hotkey::HotkeyListener` + `HotkeyEvent` (in `hotkey/mod.rs`)
   - `crate::injection::Injector` + `InjectionOutcome` (in `injection/mod.rs`)
   - `crate::injection::strategy::InjectionStrategy` (already enum-defined; resolution function lands in Wave 2)
   - `crate::injection::secure_guard::SecureInputGuard` (in `injection/secure_guard.rs`)
   - `crate::window_context::{WindowContext, ForegroundWindow}` (in `window_context/mod.rs`)
3. **Cargo deps to use, not add:**
   - `windows` 0.56 already has every feature we need for Wave 2 (UI_WindowsAndMessaging, UI_Input_KeyboardAndMouse, System_DataExchange, System_Memory, System_Threading, System_ProcessStatus).
   - `phf` 0.11 is on the workspace + `src-tauri/Cargo.toml`. Use `phf::phf_map!` macro for the static override table.
4. **Build env wrapper is mandatory:** every `cargo` call goes through `pwsh scripts/cargo-with-cuda.ps1 <args>`. No inline env setup. See LESSONS 2026-05-17.

## Deliverables (4 modules + ≥36 unit tests)

### 1. `src-tauri/src/window_context/windows.rs` (bd `mb-dl2`, P0)

Replace the `AppError::Other("Wave 2")` stub with a real impl using `windows::Win32::UI::WindowsAndMessaging::*`.

**Function signatures (don't reinvent):**

```rust
impl WindowContext for WinWindowContext {
    fn foreground(&self) -> AppResult<ForegroundWindow> {
        // 1. GetForegroundWindow() -> HWND  (null hwnd -> AppError::Other("no foreground window"))
        // 2. GetWindowTextW(hwnd, &mut buf) -> i32 (chars copied; UTF-16 decode to String)
        // 3. GetWindowThreadProcessId(hwnd, &mut pid)
        // 4. OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid) -> HANDLE
        //    Use PROCESS_QUERY_LIMITED_INFORMATION (NOT PROCESS_QUERY_INFORMATION) — limited
        //    works on protected processes (svchost, csrss); regular query gets ACCESS_DENIED.
        // 5. K32GetModuleBaseNameW(hproc, NULL, &mut buf) -> u32 (chars; decode UTF-16)
        //    Falls back via QueryFullProcessImageNameW if needed for exe_path.
        // 6. CloseHandle(hproc)
        // Return ForegroundWindow { hwnd: hwnd.0 as isize, title, process_name, exe_path }
    }
}
```

**Tests (≥4):**

| Test                                              | Expected                                                                                                            |
|---------------------------------------------------|----------------------------------------------------------------------------------------------------------------------|
| `null_hwnd_yields_other_error`                    | Mocked via direct call when no window has focus — returns `AppError::Other` with "no foreground window".              |
| `foreground_real_window_returns_populated_struct` | On a real CI machine: spawn `notepad.exe` child, snapshot, kill child. Asserts `process_name == "notepad.exe"`.       |
| `wide_string_with_null_terminator_decoded`        | Direct test of the UTF-16 decode helper with a `[u16; N]` fixture ending in `0x0000` mid-buffer.                      |
| `non_ascii_title_round_trips`                     | Synthetic title `"héllo 🐦 world"` survives UTF-16 -> String round trip.                                              |

**Risks:**
- `K32GetModuleBaseNameW` lives in `Win32_System_ProcessStatus` (feature already enabled). Some Rust docs call it `GetModuleBaseNameW` from `psapi.h`; in `windows-rs` 0.56 it's `K32GetModuleBaseNameW`.
- `OpenProcess` returns `HANDLE` that MUST be `CloseHandle`-d — use an RAII guard or explicit Drop. There is `windows::Win32::Foundation::CloseHandle`; idiomatic is a small `OwnedHandle` struct.

### 2. `src-tauri/src/hotkey/state.rs` (bd `mb-pux`, P0) — PURE RUST

The §6.1 state machine. Lives entirely in safe Rust; takes `HotkeyEvent` inputs and emits `StateAction` outputs. Zero OS dependencies — Wave 3 will drive it from `hotkey/windows.rs`.

**Types to introduce:**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyMode {
    Normal,   // default — long press
    Fragment, // future: Shift+Hotkey (don't implement detection yet — single mode in Wave 2)
    Verbose,  // future: Ctrl+Hotkey
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyState {
    Idle,
    PendingHold { vk: u32, since: Instant, mode: HotkeyMode },  // <80 ms — still deciding tap vs hold
    Recording { mode: HotkeyMode, since: Instant },             // hold confirmed
    Processing { mode: HotkeyMode },                            // key released; pipeline running
    ConfirmingCancel { mode: HotkeyMode, since: Instant },      // Escape after 30 s — 3 s prompt
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateAction {
    StartCapture(HotkeyMode),
    StopCapture,
    DiscardAudio,
    ShowConfirmCancel,
    HideConfirmCancel,
    None,
}

pub struct HotkeyStateMachine {
    state: HotkeyState,
    paused: bool,
    hold_threshold: Duration,     // configurable; default 80 ms (clamp [40, 250])
    max_session: Duration,        // 300 s auto-stop
    cancel_threshold: Duration,   // 30 s after which Escape requires confirm
    confirm_timeout: Duration,    // 3 s after which confirm-cancel turns into continue
}

impl HotkeyStateMachine {
    pub fn new(...) -> Self;
    pub fn handle(&mut self, ev: HotkeyEvent) -> StateAction;
    pub fn state(&self) -> &HotkeyState;
}
```

**Test matrix (≥20):**

| Scenario                                              | Expected `StateAction` sequence                               |
|-------------------------------------------------------|---------------------------------------------------------------|
| tap (key_down then key_up within 50 ms)               | `None`, `None` — state returns to Idle (tap passed through)   |
| 80 ms hold (key_down, tick at 80 ms)                  | `None`, `StartCapture(Normal)` — Recording entered            |
| key_up while Recording                                | `StopCapture` — Processing entered                            |
| Escape at 5 s (< 30 s) in Recording                   | `DiscardAudio` — back to Idle                                 |
| Escape at 60 s (>= 30 s) in Recording                 | `ShowConfirmCancel` — ConfirmingCancel entered                |
| Escape twice (confirm cancel within 3 s)              | second Escape -> `DiscardAudio`, back to Idle                 |
| Confirm timeout (3 s without confirm)                 | `HideConfirmCancel` — return to Recording                     |
| 300 s tick in Recording                               | `StopCapture` — auto-stop                                     |
| second key_down while Recording (different mode)      | `None` — ignored; first mode wins                             |
| same key_down while Processing                        | `None` — ignored                                              |
| pause toggle on, then key_down                        | `None` — paused; PendingHold not entered                      |
| pause toggle off, then key_down                       | normal Recording entry                                        |
| Processing returns (manual via `complete_processing`) | back to Idle                                                  |
| custom hold_threshold (40 ms minimum)                 | hold detected sooner                                          |
| custom hold_threshold (250 ms maximum)                | tap-vs-hold delay respected                                   |

Plus error-case tests:
- Tick events without any key state -> `None`.
- key_up without prior key_down -> `None` (idempotent).
- key_up of wrong VK while Recording -> `None` (different key release).

### 3. `src-tauri/src/injection/secure_guard.rs` (bd `mb-tye`, P0)

Replace `NeverSecureGuard` with the real `WinSecureInputGuard`. Keep `NeverSecureGuard` as test infrastructure.

**Implementation outline:**

```rust
pub struct WinSecureInputGuard;

impl SecureInputGuard for WinSecureInputGuard {
    fn is_secure(&self, fg: &ForegroundWindow) -> bool {
        gui_thread_info_says_secure(fg.hwnd) ||
        class_name_in_allowlist(fg.hwnd) ||
        focused_edit_is_password(fg.hwnd)
    }
}

fn gui_thread_info_says_secure(hwnd: isize) -> bool {
    // GetWindowThreadProcessId -> tid
    // GetGUIThreadInfo(tid, &mut gti) -> bool
    // (gti.dwFlags & GUI_SECUREINPUT) != 0
}

const SECURE_CLASSES: &[&str] = &[
    "$$$Secure UAP Dummy Layout$$$",
    "CredentialDialogXamlHost",
    "LockApp",
    "ConsentUI",
];

fn class_name_in_allowlist(hwnd: isize) -> bool {
    // GetClassNameW(hwnd, &mut buf) -> chars; UTF-16 -> String; case-sensitive match in allowlist
}

fn focused_edit_is_password(hwnd: isize) -> bool {
    // GetWindowThreadProcessId -> tid
    // GetGUIThreadInfo(tid) -> gti
    // GetWindowLongPtrW(gti.hwndFocus, GWL_STYLE) & ES_PASSWORD != 0
    // Edit class check: GetClassNameW(gti.hwndFocus) == "Edit"
}
```

**Tests (≥6):**

| Test                                                | Strategy                                                                                  |
|-----------------------------------------------------|-------------------------------------------------------------------------------------------|
| `never_secure_guard_returns_false`                  | already exists from Wave 1 — keep                                                          |
| `allowlist_match_is_case_sensitive_exact`           | direct unit test of `class_name_in_allowlist` with synthetic class names                  |
| `allowlist_miss_returns_false`                      | random class names not in the list                                                        |
| `gui_thread_info_helper_handles_zero_tid`           | Direct: hwnd 0 -> false (no crash)                                                        |
| `password_style_bit_check_detects_es_password`      | Hardcoded style word `0x20` (= ES_PASSWORD) -> true                                       |
| `password_style_bit_check_misses_normal_edit`       | Style word without ES_PASSWORD bit -> false                                               |

### 4. `src-tauri/src/injection/strategy.rs` (bd `mb-7xs`, P1)

Extend the existing enum with the resolver function. Use `phf::phf_map!` for compile-time lookup.

```rust
use std::collections::HashMap;
use phf::phf_map;

static BUILTIN_OVERRIDES: phf::Map<&'static str, InjectionStrategy> = phf_map! {
    "windowsterminal.exe" => InjectionStrategy::Keystroke,
    "cmd.exe"             => InjectionStrategy::Keystroke,
    "powershell.exe"      => InjectionStrategy::Keystroke,
    "pwsh.exe"            => InjectionStrategy::Keystroke,
    "conhost.exe"         => InjectionStrategy::Keystroke,
    "1password.exe"       => InjectionStrategy::Abort,
    "keepass.exe"         => InjectionStrategy::Abort,
    "keepassxc.exe"       => InjectionStrategy::Abort,
    "bitwarden.exe"       => InjectionStrategy::Abort,
    "vanguard.exe"        => InjectionStrategy::Abort,
    "easyanticheat.exe"   => InjectionStrategy::Abort,
    "beservice.exe"       => InjectionStrategy::Abort,
};

pub fn resolve(
    process_name: &str,
    user_overrides: &HashMap<String, InjectionStrategy>,
) -> InjectionStrategy {
    let key = process_name.to_ascii_lowercase();
    if let Some(s) = user_overrides.get(&key) { return *s; }
    if let Some(s) = BUILTIN_OVERRIDES.get(key.as_str()) { return *s; }
    InjectionStrategy::Paste
}
```

**Tests (≥6):**

| Test                                                | Expected                                                |
|-----------------------------------------------------|----------------------------------------------------------|
| `default_strategy_is_paste`                         | already exists from Wave 1 — keep                        |
| `strategy_serializes_lowercase`                     | already exists from Wave 1 — keep                        |
| `terminal_resolves_to_keystroke`                    | `"WindowsTerminal.exe"` -> Keystroke (case-insensitive)  |
| `password_manager_resolves_to_abort`                | `"1Password.exe"` -> Abort                               |
| `unknown_app_falls_back_to_paste`                   | `"randomapp.exe"` -> Paste                               |
| `user_override_wins_over_builtin`                   | user map `{"cmd.exe" -> Paste}` -> Paste (not Keystroke) |
| `user_override_for_unknown_app`                     | user map `{"foo.exe" -> Keystroke}` -> Keystroke         |
| `process_name_lowercased_before_lookup`             | `"WINDOWSTERMINAL.EXE"` -> Keystroke                     |

## Definition of done for Wave 2

1. All four `mb-*` tasks (`mb-dl2`, `mb-pux`, `mb-tye`, `mb-7xs`) closed in bd.
2. Cargo gate green via `pwsh scripts/cargo-with-cuda.ps1 <step>` for: `check`, `clippy --release --all-targets -- -D warnings`, `test --release --workspace`, `fmt --check`.
3. Test count: 164 -> ~200 (+36 new). Target stretches if any of the platform code wants more coverage.
4. `mb-9ir` (Wave 2 cargo gate + Wave 3 brief) closed.
5. Wave 3 brief authored at `docs/phases/phase3-wave3-brief.md` covering: `hotkey/windows.rs` design (RAII handle, message-pump thread, channel-passing), conflict probe, tray pause-toggle wiring, synthetic-event integration test harness.

## Known risks for Wave 2

| # | Risk                                                                    | Mitigation                                                     |
|---|-------------------------------------------------------------------------|---------------------------------------------------------------|
| 1 | `windows-rs` API surface for `GetGUIThreadInfo` returns `BOOL`, not `Result` — easy to misread the return | Use `BOOL.as_bool()` and check inline; explicit pattern        |
| 2 | `OpenProcess` handle leak                                              | RAII `OwnedHandle` wrapper; one tiny struct in `window_context/windows.rs` |
| 3 | State machine drift from §6.1                                          | Quote §6.1 verbatim in module-level doc-comment; test names match scenarios in §6.1 |
| 4 | `phf` 0.11 macro requires `phf` feature `"macros"` — verify Cargo.toml | Already enabled in Wave 1 Cargo.toml — confirm before coding   |

## Brief discipline reminder (LESSONS 2026-05-15)

End-of-Wave-2 work includes authoring `phase3-wave3-brief.md`. Brief MUST include: type signatures, function bodies in pseudocode, test scenarios with inputs / expected outputs, known risks, and the wave-exit cargo-gate checklist. Pattern established in Wave 1; do not skip.
