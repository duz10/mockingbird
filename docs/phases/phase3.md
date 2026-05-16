# Phase 3 — Global hotkey + text injection

**Phase entry tag:** `phase-2-complete` (151/151 tests, GPU verified on RTX 2060, CUDA 12.8)
**Phase exit tag:** `phase-3-complete` (target — adds Hotkey/Injection AppError variants, first end-to-end hotkey→STT→inject flow, no schema changes)
**Planner:** planning-agent-662770
**Implementor:** code-puppy (active agent) with **injection-author** (project JSON agent) scoped to `src-tauri/src/{hotkey,injection,window_context}/` for per-file authoring across Waves 2–4. Wave 1 (decisions/scaffolds) and Wave 5 (judges/seal) stay with code-puppy.
**Estimated iterations:** 4–5

> Binding spec lives in PLAN-mockingbird-v2.md §3 (Layer-3 injection flow), §4 (file layout — sealed), §6.1 (hotkey state machine — verbatim), §12 #10/#11/#15/#17/#18/#20 (binding rules). This doc operationalizes them.

## Overview

Phase 3 makes Mockingbird *usable for the first time*. By the end, the user holds a configured key (default: **Right Alt**), speaks into any focused Windows app, releases — and the transcript appears at the caret. The full pipeline lights up: low-level keyboard hook → audio capture (Phase 2) → VAD trim → Whisper transcribe → secure-input check → clipboard save → paste → clipboard restore → session row persisted with full provenance. The Cleanup LLM step (Phase 4) is a passthrough stub that returns the raw transcript as `final_text`. No UI beyond a tray menu and a toast for secure-input aborts. No new migrations — the `sessions` + `transcripts` tables from migration 001 already accommodate every column we write.

## Pre-flight — ADRs authored in Wave 1

### ADR 0015 — Low-level keyboard hook (WH_KEYBOARD_LL) over tauri-plugin-global-shortcut

`tauri-plugin-global-shortcut` fires once on press; the hold-detection state machine in §6.1 requires **both** key-down AND key-up callbacks so we can distinguish a tap (<80 ms — pass through) from a hold (≥80 ms — start recording). We therefore use `windows-rs` `SetWindowsHookEx(WH_KEYBOARD_LL)` directly. PLAN §12 #20 declares this binding; the ADR cements it and documents the message-pump thread that owns the hook (separate from Tauri's main thread) plus the "no work in the hook callback" rule (post to a channel and return within microseconds — otherwise Windows times us out and silently unhooks us).

### ADR 0016 — Injection strategy: paste-via-clipboard default, SendInput keystroke fallback, per-app override table

Default path is **clipboard paste** (Ctrl+V via SendInput, wrapped in the save/restore dance from ADR 0018). Per ADR 0007 this is "Tier 0" — works across virtually every Win32/Electron/UWP surface. Keystroke fallback (`SendInput` with `KEYEVENTF_UNICODE`) is used when the per-app table says so (terminals where Ctrl+V means SIGINT or "open file"; some games; password managers we want to opt out of entirely → ABORT). The override table is a static `phf` map keyed on process basename (e.g. `WindowsTerminal.exe → Keystroke`, `cmd.exe → Keystroke`, `1Password.exe → Abort`); unknown apps default to Paste. User overrides land in `settings.json` under `injection.app_overrides` (Phase 5 surfaces UI; Phase 3 reads them).

### ADR 0017 — Secure-input guard policy

PLAN §12 #18 is binding: secure-input fields **abort** injection. We detect via `GetGUIThreadInfo(GUI_SECUREINPUT | GUI_CARETBLINKING)` on the focused thread *plus* a class-name allowlist for known-bad windows (UAC consent dialog `$$$Secure UAP Dummy Layout$$$`, Credential UI `CredentialDialogXamlHost`, `Edit` controls with `ES_PASSWORD` style via `GetWindowLong`). On detection we: (1) do not paste, (2) do not write to the clipboard, (3) emit a tray toast "Secure field — transcript discarded", (4) still write the `transcripts(stage='raw')` row (provenance is total per PLAN principle #2) but mark the session `injection_status='aborted_secure'`. Raw audio is *not* persisted in this case (no opt-in audio retention in v1 anyway).

### ADR 0018 — Clipboard save/restore protocol

The four-step dance, wrapped around every paste:

1. `OpenClipboard(NULL)` → snapshot every format we recognize (`CF_UNICODETEXT`, `CF_TEXT`, `CF_HDROP`, `CF_BITMAP`, `CF_HTML`, registered `HTML Format`, registered `image/png`). Large blobs (>4 MB) snapshot by handle reference + length; we restore by re-`SetClipboardData` of the saved `HGLOBAL`.
2. `EmptyClipboard()` → `SetClipboardData(CF_UNICODETEXT, payload)` → `CloseClipboard()`.
3. SendInput Ctrl+V; wait for paste sentinel (poll `GetClipboardSequenceNumber` until it advances or 250 ms timeout — whichever first; keeps us from racing slow apps).
4. Restore every snapshotted format; `CloseClipboard()`.

Edge cases the ADR pins: (a) ownership change mid-paste (another app writes the clipboard between steps 2 and 4) → log, skip restore, surface tray toast "Clipboard changed during dictation — not restored"; (b) non-text formats only (e.g. a screenshot was copied) → snapshot succeeds, restore succeeds, no data lost; (c) clipboard locked by another process → retry 3× with 10 ms backoff, then ABORT with `AppError::Injection("clipboard locked")`; (d) clipboard history (Win+V) is unaffected because we use the standard API surface.

### ADR 0019 — Hotkey conflict detection

At service startup we register a probe: try installing `WH_KEYBOARD_LL` and watch for the configured hotkey for 2 seconds; if any other app reports already-owning the key (we can't directly query; instead we use `RegisterHotKey` as a one-shot test for global-modifier combos and assume free for raw VK codes), surface a first-run-wizard step (Phase 5 polishes; Phase 3 emits a tray toast + logs) offering the fallback ladder: **F23 → F24 → Ctrl+Shift+Space → user picks**. F23/F24 are deliberately unmapped on standard layouts and survive on most stenography/gaming-keyboard remappers. Settings store the resolved binding under `hotkey.binding` with provenance `hotkey.resolved_from = "default" | "conflict_fallback" | "user_override"`.

## Phase 3 Cargo deps (incremental to Phase 2 manifest)

Add to `src-tauri/Cargo.toml`:

```toml
[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [
    "Win32_Foundation",
    "Win32_UI_WindowsAndMessaging",     # SetWindowsHookEx, GetForegroundWindow, GetGUIThreadInfo
    "Win32_UI_Input_KeyboardAndMouse",  # SendInput, KEYBDINPUT, VIRTUAL_KEY
    "Win32_System_DataExchange",        # OpenClipboard / SetClipboardData / GetClipboardData
    "Win32_System_Memory",              # GlobalAlloc / GlobalLock for clipboard HGLOBAL
    "Win32_System_Threading",           # GetCurrentThreadId, message-pump thread
    "Win32_System_ProcessStatus",       # GetModuleBaseNameW for foreground process name
    "Win32_UI_Accessibility",           # IUIAutomation — DEFERRED to Wave 4 if needed for per-app heuristics
] }
phf = { version = "0.11", features = ["macros"] }   # static per-app override table
arboard = "3"                                       # DEFERRED — only if Win32 clipboard ergonomics defeat us in Wave 4; default plan is raw windows-rs
```

**`enigo` stays out** (PLAN §12 #20 commentary). Raw `SendInput` gives us Unicode control (`KEYEVENTF_UNICODE` for the transcript paste-fallback path) and surrogate-pair handling for emoji that `enigo` historically botches.

No `tauri-plugin-global-shortcut`. No `tauri-plugin-clipboard-manager` (the save/restore protocol is too specific). No `@tanstack/*` (hook blocks it anyway).

## AppError carry-forward

Wave 1 adds two new variants to `src-tauri/src/error.rs` (predicted in Phase 2 LESSONS — "AppError variants are added per-module as the modules come online"):

```rust
/// Text-injection failures (clipboard ops, SendInput failures, secure-input abort).
#[error("injection error: {0}")]
Injection(String),

/// Hotkey subsystem failures (hook install, message-pump death, conflict-probe failure).
#[error("hotkey error: {0}")]
Hotkey(String),
```

Wrap `windows::core::Error` as `String` at the construction site, mirroring the `WhisperError → AppError::Stt(e.to_string())` pattern from Phase 2.

## Section 6.1 state machine — binding spec for `hotkey/state.rs`

Verbatim from PLAN §6.1. Implementation MUST be in pure Rust (no Windows API calls) so it's fully unit-testable.

```
IDLE
  └─ on key_down(any mode hotkey, held > 80 ms) → RECORDING(mode)
       (taps < 80 ms ignored — passes through to OS for native shortcuts)

RECORDING(mode)
  ├─ on key_up                    → PROCESSING(mode, audio_buffer)
  ├─ on Escape (< 30s recorded)   → CANCELLED (discard audio)
  ├─ on Escape (≥ 30s recorded)   → CONFIRM_CANCEL toast (3s timeout = continue)
  └─ on duration > 300s           → STOPPED (auto-stop, treat as key_up)

PROCESSING(mode, audio)
  ├─ VAD trim
  ├─ Whisper transcribe        → raw_transcript (immutable on write)
  ├─ Cleanup LLM call(mode)    → cleaned_text  [Phase 4 — stub returns raw for Phase 3]
  ├─ Secure-input check        → ABORT if secure field focused
  ├─ Text inject               → final_text (clipboard saved+restored)
  └─ Persist to DB (atomic)    → IDLE

Edge cases:
- Two mode hotkeys held simultaneously → first wins; second ignored
- Same hotkey re-pressed during PROCESSING → ignored (audio cue if enabled)
- Pause-dictation tray toggle → key_down events no-op until cleared
```

The state machine takes `HotkeyEvent` inputs (`KeyDown { vk, ts }`, `KeyUp { vk, ts }`, `Escape { ts }`, `Tick { ts }`, `PauseToggle { paused }`) and emits `StateAction` outputs (`StartCapture`, `StopCapture`, `DiscardAudio`, `ShowConfirmCancel`, `Inject(text)`, `Persist(record)`). Drives 100% of the §6.1 edge cases as table-driven tests in `state.rs` itself.

## File layout (sealed in PLAN §4 — DO NOT relitigate)

```
src-tauri/src/
├── hotkey/
│   ├── mod.rs               # trait HotkeyListener — yields key_down AND key_up events
│   ├── state.rs             # state machine per §6.1 (pure, unit-testable)
│   ├── windows.rs           # WH_KEYBOARD_LL hook + message-pump thread
│   ├── macos.rs             # todo!() stubs
│   └── linux.rs             # todo!() stubs
├── injection/
│   ├── mod.rs               # trait Injector
│   ├── windows.rs           # SendInput (Unicode + Ctrl+V dispatch)
│   ├── macos.rs             # todo!() stubs
│   ├── linux.rs             # todo!() stubs
│   ├── paste.rs             # clipboard save/restore + paste helper (the ONLY caller of set_clipboard)
│   ├── strategy.rs          # per-app paste vs keystroke choice + override table
│   └── secure_guard.rs      # trait SecureInputGuard + Windows impl
└── window_context/
    ├── mod.rs               # trait WindowContext
    ├── windows.rs           # GetForegroundWindow + GetWindowTextW + GetModuleBaseNameW
    ├── macos.rs             # todo!() stubs
    └── linux.rs             # todo!() stubs
```

Every `mod.rs` defines its trait + cfg-gated `pub use platform::*`. The 600-line cap applies; `injection/windows.rs` and `injection/paste.rs` will press against it — pre-split helpers into sibling files before hitting 500.

## Task waves

Priority key: **P0** blocks the wave; **P1** must ship in the wave; **P2** ships in the wave if cheap, otherwise documents the deferral; **P3** stretch.

**Brief-pattern reminder (cross-wave):** code-puppy authors `docs/phases/phase3-waveN-brief.md` at the end of each wave for N+1. Tracked as implicit deliverables (not separate bd tasks) per the proven Phase 1/2 pattern.

### Wave 1 — Decisions, ADRs, deps, AppError, cross-platform scaffolds (Iteration 1)

| bd-task title (prefix `Phase 3:`) | priority | files |
|-----------------------------------|----------|-------|
| ADR 0015 — WH_KEYBOARD_LL over tauri-plugin-global-shortcut | P0 | `docs/adr/0015-low-level-keyboard-hook.md` |
| ADR 0016 — Injection strategy (paste default, keystroke fallback, per-app table) | P0 | `docs/adr/0016-injection-strategy.md` |
| ADR 0017 — Secure-input guard policy | P0 | `docs/adr/0017-secure-input-guard.md` |
| ADR 0018 — Clipboard save/restore protocol | P0 | `docs/adr/0018-clipboard-save-restore.md` |
| ADR 0019 — Hotkey conflict detection + F23/F24 fallback | P1 | `docs/adr/0019-hotkey-conflict-detection.md` |
| Cargo deps (windows-rs features, phf) + AppError Hotkey/Injection variants + module scaffolds (traits in `mod.rs`, `todo!()` Windows/macOS/Linux stubs per §4) | P0 | `src-tauri/Cargo.toml`, `src-tauri/src/error.rs`, 12 files under `hotkey/`, `injection/`, `window_context/` |

### Wave 2 — Window context + pure state machine + secure guard (Iteration 2)

| bd-task title (prefix `Phase 3:`) | priority | files |
|-----------------------------------|----------|-------|
| `window_context/windows.rs` — `GetForegroundWindow` + `GetWindowTextW` + `GetModuleBaseNameW` → `ForegroundWindow { hwnd, title, process_name, exe_path }` | P0 | `src-tauri/src/window_context/{mod,windows}.rs` |
| `hotkey/state.rs` — full §6.1 state machine, pure Rust, 100% edge-case test coverage (taps <80 ms ignored, Escape pre/post-30 s, 300 s auto-stop, double-mode collision, re-press during processing, pause toggle) | P0 | `src-tauri/src/hotkey/{mod,state}.rs` |
| `injection/secure_guard.rs` — Windows impl: `GetGUIThreadInfo(GUI_SECUREINPUT)` + class-name allowlist + `ES_PASSWORD` style check | P0 | `src-tauri/src/injection/secure_guard.rs` |
| `injection/strategy.rs` — `phf` static per-app override table (Terminal/cmd/PowerShell → Keystroke; 1Password/KeePass → Abort; default → Paste) + user-override merge from settings | P1 | `src-tauri/src/injection/strategy.rs` |
| Unit tests across all four (state machine ≥20 tests; window_context ≥4; secure_guard ≥6; strategy ≥6) | P0 | sibling `#[cfg(test)] mod tests` blocks |

### Wave 3 — Low-level keyboard hook + conflict probe (Iteration 3)

| bd-task title (prefix `Phase 3:`) | priority | files |
|-----------------------------------|----------|-------|
| `hotkey/windows.rs` — `SetWindowsHookEx(WH_KEYBOARD_LL)` on a dedicated message-pump thread; hook callback POSTS to an `mpsc::Sender<HotkeyEvent>` and returns within microseconds (no work in callback per ADR 0015); 80 ms hold-vs-tap discriminator wired through `state.rs` | P0 | `src-tauri/src/hotkey/windows.rs` |
| Integration test via synthetic event injection (no real keyboard) | P0 | `src-tauri/tests/hotkey_integration.rs` |
| Hotkey conflict probe at service startup; on collision: log + tray toast + F23 fallback (Phase-5 wizard hooks deferred but settings key written) | P1 | `src-tauri/src/hotkey/windows.rs`, settings wiring |
| Tray menu integration: "Pause dictation" toggle drives `PauseToggle` event into the state machine | P1 | `src-tauri/src/tray.rs` (existing from Phase 1; extend) |

> Wave 3 invokes **injection-author** for `hotkey/windows.rs` authoring; code-puppy owns the tray wiring and integration test harness.

### Wave 4 — Injection pipeline + end-to-end wiring (Iteration 4 — the heavy wave; HUMAN-IN-LOOP)

| bd-task title (prefix `Phase 3:`) | priority | files |
|-----------------------------------|----------|-------|
| `injection/paste.rs` — clipboard save/restore protocol per ADR 0018; the *only* place in the codebase that calls `SetClipboardData` (hook `block-bare-paste` enforces) | P0 | `src-tauri/src/injection/paste.rs` |
| `injection/windows.rs` — `SendInput` with both Ctrl+V dispatch path and `KEYEVENTF_UNICODE` keystroke-fallback path; surrogate-pair handling for non-BMP characters | P0 | `src-tauri/src/injection/windows.rs` |
| Strategy resolver wiring — `WindowContext` → `InjectionStrategy::resolve(process_name) → {Paste, Keystroke, Abort}` | P0 | `src-tauri/src/injection/strategy.rs` (extend) |
| End-to-end orchestrator in `src-tauri/src/dictation.rs` (new) — wires hotkey state machine → audio capture → VAD → Whisper → secure-input check → injection → DB write. Cleanup LLM is a passthrough stub (`fn cleanup(raw: &str, _mode: Mode) -> String { raw.to_string() }`) per Phase 4 deferral | P0 | `src-tauri/src/dictation.rs`, `lib.rs` |
| DB persistence — session row + raw transcript row, atomic, with provenance fields (`prompt_version`, `dict_snapshot_id`, `injection_strategy`, `injection_status`, `hotkey_binding`, `app_process_name`). Reuses migration 001 tables; **no migration 004**. | P0 | extends `src-tauri/src/db/sessions.rs` from Phase 1 |
| Recording-window stub — non-activating Tauri window (`WS_EX_NOACTIVATE`, `focus: false`, `skipTaskbar: true`) per PLAN §12 #10; shows a 1-line "● recording…" label, no waveform (Phase 5) | P1 | `src-tauri/src/recording_window.rs`, `tauri.conf.json` |
| Cross-app QA matrix run — **requires human at keyboard.** Test in: Notepad, Word, Outlook, Slack, Teams, Chrome (Gmail), VS Code, Claude Desktop, cmd, PowerShell, Windows Terminal, Cursor. Record pass/fail + strategy used per app in `docs/phases/phase3-qa-matrix.md`. | P0 | `docs/phases/phase3-qa-matrix.md` |

> Wave 4 invokes **injection-author** for `paste.rs`, `windows.rs`, and `strategy.rs` extension. The orchestrator (`dictation.rs`) and DB wiring stay with code-puppy. The cross-app matrix is a **human-driven** task — code-puppy can't validate Slack/Teams paste behavior from a CI box.

### Wave 5 — Judges, retrospective, seal (Iteration 5)

| bd-task title (prefix `Phase 3:`) | priority | files |
|-----------------------------------|----------|-------|
| 4 judge cards + JSON entries: `e2e-injection`, `db-provenance`, `clipboard-restored`, `secure-input-respected` | P0 | `docs/judges/phase-3/*.md`, `.code_puppy/judges-template.json` |
| Retrospective in `docs/LESSONS.md` (`[phase-3-retrospective]` tag) + STATUS.md update + bd close all Phase-3 issues | P0 | `docs/LESSONS.md`, `STATUS.md` |
| Cargo gate green (release): `cargo check + clippy --release -D warnings + test --release + fmt --check` → seal commit → `git tag phase-3-complete` | P0 | git |

## Cross-wave invariants

1. **File size hard limit: 600 lines.** `injection/windows.rs` and `injection/paste.rs` will press; pre-split helpers into siblings before hitting 500.
2. **Test density target: ~10 tests per ~500 LoC.** Phase 2 hit this exactly (151 tests / ~5–6 kLoC cumulative; +50 new tests in Phase 2). Phase 3 targets **+45–60 new tests** across ~2,500–3,000 new lines, with heavy weighting toward `hotkey/state.rs` (pure → easy to push to 25+ tests covering every §6.1 edge case).
3. **Cross-platform traits from day one (PLAN §12 #15 binding).** Every new module pairs a trait with `#[cfg(target_os = "windows")]` impl + macOS/Linux `todo!()` stubs. Wave 1 lays all stubs; later waves only flesh out the Windows side.
4. **Raw transcripts immutable (PLAN §12 #3 binding, sealed).** Wave 4's DB write inserts `transcripts(stage='raw')` exactly once per session — no UPDATE path. Hook `enforce-immutable-raw` already vetoes; we just don't write that code.
5. **Clipboard save/restore is the only entry point to the clipboard (PLAN §12 #17 binding).** `injection/paste.rs` is the *sole* file in the workspace allowed to call `SetClipboardData`. Hook `block-bare-paste` warns on violations.
6. **Secure-input check fires BEFORE any clipboard mutation (PLAN §12 #18 binding).** Ordering in `dictation.rs`: `WindowContext::foreground()` → `SecureInputGuard::is_secure()` → if true ABORT before `paste::with_saved_clipboard()` is even entered.
7. **No work in the hook callback (ADR 0015).** The `WH_KEYBOARD_LL` callback posts to a channel and returns; the state machine runs on a worker thread. Violating this kills the hook (Windows unhook timeout) and yields silent dictation failures that are murder to debug.
8. **`tracing` only — no `println!` outside CLI harnesses.** Carried forward from Phase 2.
9. **Brief pattern (LESSONS 2026-05-15).** Code-puppy authors `docs/phases/phase3-waveN-brief.md` at end-of-wave-N for each transition (1→2, 2→3, 3→4, 4→5). Briefs include type definitions, function signatures, test specs with inputs/expected outputs, known risks, deviations from PLAN with justification, and the wave-exit cargo-gate checklist.
10. **The cargo gate is four green lights (LESSONS):** `cargo check + clippy --release -D warnings + test --release + fmt --check`. Clippy MUST be `--release` to reuse the CUDA-built `whisper-rs-sys` artifacts; plain debug clippy triggers a fresh 10-minute cmake build.

## Exit criteria

1. **Functional:** Hold Right Alt (or resolved fallback) in a focused Notepad → speak "hello world" → release → "hello world" appears at the caret. Verified manually by Dustin.
2. **QA matrix:** `docs/phases/phase3-qa-matrix.md` shows pass for all 12 apps in PLAN §12 #11 (Notepad, Word, Outlook, Slack, Teams, Chrome/Gmail, VS Code, Claude Desktop, cmd, PowerShell, Windows Terminal, Cursor). "Pass" = correct text appears at caret AND clipboard is restored to its pre-dictation state. Per-app failures get a per-app override table entry (Keystroke fallback) and re-tested.
3. **Cargo gate:** `cargo check + clippy --release -D warnings + test --release + fmt --check` all four green. ~196–211 tests total (151 from Phase 2 + 45–60 new).
4. **Judges green:** all 4 new judges (`e2e-injection`, `db-provenance`, `clipboard-restored`, `secure-input-respected`) report PASS; all carry-forward judges (`build-passes`, `tests-pass`, `lint-clean`, `adr-recorded`, `plan-aligned`, `status-updated`) still PASS.
5. **ADRs 0015–0019 present** with `Status: Accepted` and cross-references back to PLAN §6.1 / §12 #17 / §12 #18 / §12 #20.
6. **Secure-input abort verified by hand** in at least one real surface (UAC consent prompt or Windows Credential dialog). Tray toast appears; clipboard untouched; session row marked `aborted_secure`.
7. **No migration 004.** If a migration ends up needed, an ADR justifies it and updates the §12 #3 sealing-discipline note.
8. **STATUS.md** updated: current phase = "Phase 4 (queued)"; last-judge-run line populated; blocked-on cleared.
9. **`git tag --list "phase-*"`** includes `phase-3-complete`.

## Risks & mitigations

| # | Risk | Severity | Mitigation |
|---|------|----------|------------|
| 1 | **Secure-input bypass** — we miss a class-name pattern and inject into a password field | **Critical** (PLAN §12 #18 binding) | Class-name allowlist + `GUI_SECUREINPUT` flag + `ES_PASSWORD` style check, three layers in OR. Judge `secure-input-respected` runs against synthetic flags every CI run. Manual test against UAC and CredentialUI before seal. If a real-world miss is found post-seal, hotfix + LESSONS entry. |
| 2 | **Clipboard data loss** — restore step crashes or the snapshot misses a format the user had | **High** | Snapshot every format the system enumerates via `EnumClipboardFormats`, not just a hardcoded list. Restore inside a `Result`-guarded scope with a finally-style `CloseClipboard`. On restore failure, log + tray toast "Clipboard changed during dictation — not restored". E2E judge `clipboard-restored` covers the happy path. |
| 3 | **Keyboard hook starvation** — slow work inside the `WH_KEYBOARD_LL` callback causes Windows to silently unhook us | **High** | ADR 0015 codifies "callback posts to channel, returns in microseconds". The orchestrator runs on a worker thread. Watchdog timer logs if the hook stops delivering events for >5 s while a key is suspected held. |
| 4 | **Per-app strategy bloat** — Terminal-vs-cmd-vs-PowerShell-vs-WSL-vs-... matrix explodes | **Medium** | `phf` static table for the v1 shortlist (12 apps) + user-override map in settings. Defer "auto-learn the right strategy from telemetry" indefinitely (no telemetry per principle #4). Document the manual override path in CONTRIBUTING. |
| 5 | **Anti-cheat / kernel-mode-protected games block SendInput** | **Medium** | Detect known anti-cheat process names (Vanguard, EAC, BattlEye) in the strategy table → mark `Abort` with toast "This app blocks synthetic input". Not a regression — Wispr Flow has the same limitation. |
| 6 | **Focus loss between speak and inject** — user releases the hotkey, Alt-Tabs, then injection lands in the wrong window | **High** | `WindowContext::foreground()` snapshot is taken **twice**: once on `key_down` (recorded in session row) and once on `key_up` immediately before injection. If they differ AND the user hasn't enabled "follow focus", ABORT with toast "Focus changed — transcript copied to clipboard instead" (and the clipboard restore step is skipped — user got the text but loses their old clipboard, surfaced explicitly). |
| 7 | **Conflict probe is a heuristic** — we can't truly enumerate global-hook owners | **Medium** | Document the limitation in ADR 0019. F23/F24 fallback ladder is the real safety net. First-run wizard in Phase 5 surfaces a "test the hotkey" step with explicit user confirmation. |
| 8 | **`windows-rs` 0.58 API churn** — the crate is still pre-1.0 and renames things between minors | **Low** | Pin exact minor in `Cargo.toml`; `Cargo.lock` is committed. If a 0.59 forces an upgrade, treat as its own LESSONS entry and bump in a dedicated commit. |
| 9 | **Surrogate-pair / emoji injection** via `KEYEVENTF_UNICODE` — naive single-WORD-per-input drops non-BMP code points | **Medium** | Encode the transcript as UTF-16, batch surrogate pairs into a single `SendInput` call. Unit test with 🐦, 𝓗𝓮𝓵𝓵𝓸, and CJK fixtures. |
| 10 | **Tap-vs-hold threshold (80 ms) too aggressive on slow typists / too lax on chord users** | **Low** | Threshold is configurable in settings (`hotkey.hold_threshold_ms`, default 80, clamp [40, 250]). Brief notes the default and rationale; Phase 5 surfaces UI. |

## Iteration estimate

**4–5 iterations**, honest reasoning:

| Iteration | Wave | Why this fits in one iteration |
|-----------|------|------------------------------------------------------------|
| 1 | Wave 1 | ADRs + scaffolds + AppError variants. No platform code yet. Comparable to Phase 2 Wave 1 (which sailed). |
| 2 | Wave 2 | Pure Rust state machine + window-context wrappers + secure-guard wrapper. All three are testable without a keyboard hook. The state machine is meaty (≥20 tests) but pure. |
| 3 | Wave 3 | First wave that touches `WH_KEYBOARD_LL`. Real chance of a "the hook silently unhooks itself" mystery costing a half-iteration. Buffer is baked in. |
| 4 | Wave 4 | **Heavy.** Clipboard protocol + SendInput + orchestrator + DB wiring + cross-app QA matrix. The QA matrix alone is a half-day of Dustin's keyboard time. The injection-author agent absorbs the per-file authoring load so code-puppy can focus on the orchestrator and DB. |
| 5 | Wave 5 | Judges + retrospective + seal. Buffer for one 5-attempt-rule escalation (PLAN's hard ceiling) — if any wave above blew through its budget, this wave absorbs the slip. |

**Phase 2 hit 5 iterations including a same-day GPU-re-enable scramble.** Phase 3 has comparable surface area but more user-facing failure modes (anything weird on a Slack paste blocks the seal). 5 iterations is the realistic estimate; **4 only if the cross-app matrix comes back clean on first pass** (it won't — Terminal almost certainly needs the Keystroke fallback, and there'll be at least one surprise app in the list).

## Judge roster at phase exit

| Judge                          | Origin     | Run? | Notes |
|--------------------------------|------------|------|-------|
| `build-passes`                 | Phase 0    | YES  | `cargo build --release` (CUDA backend still in play) |
| `tests-pass`                   | Phase 0    | YES  | `cargo test --release --workspace`, target ~196–211 tests |
| `lint-clean`                   | Phase 0    | YES  | `cargo clippy --release -- -D warnings` + `cargo fmt --check` |
| `adr-recorded`                 | Phase 0    | YES  | ADRs 0015–0019 present, `Status: Accepted` |
| `plan-aligned`                 | Phase 0    | YES  | deliverable checklist vs PLAN §3 + §6.1 + §12 #11/#17/#18/#20 |
| `status-updated`               | Phase 0    | YES  | last-judge-run line populated |
| `agents-md-present`            | Phase 0    | passthrough | — |
| `hook-config-valid`            | Phase 0    | passthrough | — |
| `fts5-smoke`                   | Phase 1    | passthrough | DB layer unchanged |
| `stt-correct`                  | Phase 2    | YES  | Phase-2 fixture still transcribes correctly (regression gate) |
| `cuda-verified`                | Phase 2    | YES  | CUDA path still logs init on model load |
| `perf-stt`                     | Phase 2    | YES  | criterion bench still < 1000 ms / 10 s on RTX 2060 |
| **`e2e-injection`** *(new)*    | Phase 3    | YES  | Fixture WAV + synthetic hotkey events → text appears in Notepad child proc, byte-for-byte match |
| **`db-provenance`** *(new)*    | Phase 3    | YES  | Every Phase-3 session row has non-null provenance columns (PLAN principle #2) |
| **`clipboard-restored`** *(new)*| Phase 3   | YES  | Pre-paste `CF_UNICODETEXT` snapshot equals post-paste snapshot (or both empty) |
| **`secure-input-respected`** *(new)*| Phase 3| YES  | Synthetic `GUI_SECUREINPUT` flag → injection ABORTS, no clipboard mutation, session marked `aborted_secure` |

Four NEW judge prompts authored in Wave 5; cards live under `docs/judges/phase-3/`.

## Out of scope (DEFER to later phases)

- **Cleanup LLM** → Phase 4. Phase 3 ships a passthrough stub (`fn cleanup(raw, _mode) -> raw.clone()`).
- **Recording-window UI polish, waveform, audio meter** → Phase 5. Phase 3 ships the non-activating shell + a "● recording…" text label only.
- **First-run hotkey wizard UI** → Phase 5. Phase 3 ships the F23/F24 fallback logic + a tray toast on conflict.
- **History viewer / data UI** → Phase 6.
- **Real macOS/Linux impls** → Phase 9. Stubs only in Phase 3.
- **Code signing** → Phase 7 (per ADR 0005).
- **Learning loop** → Phase 8.
- **Telemetry of any kind** → never (principle #4).
