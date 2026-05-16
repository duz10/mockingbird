# Phase 3 — Wave 4 brief

**From:** code-puppy at end of Wave 3
**To:** code-puppy + injection-author for Wave 4 (👋 Dustin reads this first)
**Entry tag:** Wave 3 cargo gate green (244/244 tests + 4 ignored, clippy `-D warnings` clean, fmt clean)
**Exit goal:** end-to-end dictation pipeline: hotkey → audio → VAD → STT → cleanup → secure-input check → inject → DB row, persisted with full provenance. ≥30 new tests. Cross-app QA matrix exercised by Dustin (see §"QA matrix" below).

## ⚠️ This is the wave where you'll be at the keyboard

Waves 1-3 were all things code-puppy could verify by itself. Wave 4
glues the pipeline together AND demands a real cross-app injection
matrix that only a human can run (Notepad, VSCode, Terminal, browser,
password manager, etc.). The wave plan deliberately puts the
mechanical work first so when Dustin sits down, all that's left is
"open the apps in order, hold the hotkey, type a sentence, verify."

Each row in the QA matrix should take <30 seconds. Total wall-clock
human time: ~20 minutes.

## Context Wave 4 inherits

1. **All five `hotkey/` modules are wired**: `state` (pure §6.1, 26
   tests), `windows` (real WH_KEYBOARD_LL), `driver` (20 ms tick
   cadence), `probe` (ADR 0019 fallback chain), `pause` (tray toggle).
2. **All four `injection/` and `window_context/` foundations are
   wired**: `strategy::resolve()`, `secure_guard::WinSecureInputGuard`,
   `window_context::WinWindowContext`, plus stub `paste`, `windows`,
   non-Windows `linux`/`macos` shims.
3. **Cargo gate is green at 244/244.** Build wrapper
   `scripts/cargo-with-cuda.ps1` is the single entry point.
4. **Phase 1 SQLite schema migrations land 001-003.** Wave 4 must NOT
   add migration 004 — see ADR 0010 (raw immutability + migrations
   append-only after Phase 1) and PLAN §12 #5.

## Deliverables

### 1. `src-tauri/src/injection/paste.rs` (bd `mb-cm3`, P0)

**The clipboard owner.** PLAN §12 #19 binding: this file is the ONLY
caller of `SetClipboardData` / `OpenClipboard` / `EmptyClipboard` /
`CloseClipboard` in the entire repo. The `scripts/hooks/warn-bare-
clipboard-set.py` hook enforces the convention; clippy/rust-analyzer
do not yet.

**Public API (sketch):**

```rust
pub struct ClipboardSnapshot {
    /// CF_UNICODETEXT contents at snapshot time. None if clipboard was
    /// empty or held no text format (image, files, custom). ADR 0018
    /// §3 scopes restore to text only; non-text data survives the
    /// dance because we don't EmptyClipboard between snapshot and
    /// restore — we just SetClipboardData(CF_UNICODETEXT, ...) which
    /// preserves other formats.
    text: Option<Vec<u16>>,
}

pub fn snapshot() -> AppResult<ClipboardSnapshot> { … }
pub fn restore(snap: ClipboardSnapshot) -> AppResult<()> { … }

/// Save → write → paste (Ctrl+V via SendInput) → 30 ms guard → restore.
///
/// On any error in the middle, the Drop on the snapshot guard restores
/// the original clipboard. Idempotent: a re-entrant call from within
/// the closure is a programming bug and panics in debug.
pub fn with_saved_clipboard<F>(text: &str, f: F) -> AppResult<()>
where F: FnOnce() -> AppResult<()>
{ … }
```

**Why a closure-passing API:** the orchestrator owns the "send Ctrl+V"
step but not the clipboard save/restore. Passing `f` lets us hold the
restore in `Drop` (RAII) so even a panic from the injection step
restores the original clipboard. This is the central trick that makes
"clipboard save/restore around every paste" (PLAN §12 #4 / ADR 0018)
unmissable.

**Tests:** unit tests on `snapshot()/restore()` round trip with
synthetic strings (≥6 cases including empty, very long, mixed
script, emoji-bearing, lone surrogate, embedded NUL). Live test
ignored by default. Total ≥8.

### 2. `src-tauri/src/injection/windows.rs` (bd `mb-x7i`, P0)

**The keystroke + paste injector.** Replaces the Wave 1 stub.

```rust
impl Injector for SendInputInjector {
    fn inject(
        &self,
        text: &str,
        strategy: InjectionStrategy,
    ) -> AppResult<InjectionOutcome> {
        match strategy {
            InjectionStrategy::Abort => Ok(InjectionOutcome::AbortedSecure),
            InjectionStrategy::Paste => paste::with_saved_clipboard(text, |
                send_ctrl_v()
            ),
            InjectionStrategy::Keystroke => send_unicode_keystrokes(text),
        }
    }
}
```

**`send_ctrl_v`** synthesises Ctrl+V via `SendInput` with two
`KEYBDINPUT` entries (down) + two (up). Use `VK_CONTROL` (0x11) +
`VK_V` (0x56). 30 ms `Sleep` between clipboard write and SendInput
per ADR 0018 §4.

**`send_unicode_keystrokes`** iterates the string's `chars()` —
for each char, build `INPUT` entries with `KEYEVENTF_UNICODE` set and
`wScan = u16` from the BMP code point. Surrogate-pair handling:
if `c as u32 > 0xFFFF`, emit two INPUTs (high surrogate + low
surrogate). `SendInput` is documented to accept this pattern since
Vista.

**Tests:** ≥5. Most coverage is at the wrapper level (paste.rs +
windows.rs together). Keep `send_unicode_keystrokes` testable via a
pure helper that converts `&str → Vec<INPUT>`-shaped events; the
SendInput call is a one-line shim.

### 3. `src-tauri/src/dictation.rs` (bd `mb-8cd`, P0)

**The orchestrator.** End-to-end:

```rust
pub struct DictationOrchestrator {
    state_actions: Receiver<StateAction>,
    audio: Arc<dyn AudioCapture>,
    vad: Arc<dyn VoiceActivityDetector>,
    stt: Arc<dyn SpeechToText>,
    cleaner: Arc<dyn Cleaner>,    // Phase 2 surface
    injector: Arc<dyn Injector>,
    window_ctx: Arc<dyn WindowContext>,
    secure_guard: Arc<dyn SecureInputGuard>,
    db: Arc<DbPool>,
}

impl DictationOrchestrator {
    pub fn run(self) -> AppResult<()> {
        for action in self.state_actions.iter() {
            match action {
                StateAction::StartCapture(mode) => self.start(mode)?,
                StateAction::StopCapture => self.complete()?,
                StateAction::DiscardAudio => self.discard()?,
                StateAction::ShowConfirmCancel => self.show_confirm()?,
                StateAction::HideConfirmCancel => self.hide_confirm()?,
                StateAction::None => unreachable!("filtered by driver"),
            }
        }
        Ok(())
    }
}
```

**`complete()`** is the critical path:

```rust
fn complete(&self) -> AppResult<()> {
    let audio = self.audio.stop_and_take()?;
    let fg_keydown = self.fg_at_keydown.lock().unwrap().take();  // captured on StartCapture
    let fg_keyup = self.window_ctx.foreground()?;
    let strategy = strategy::resolve(&fg_keyup.process_name, &self.user_overrides);
    let raw_audio = self.vad.trim(audio)?;
    let raw_text = self.stt.transcribe(&raw_audio)?;
    let cleaned = self.cleaner.clean(&raw_text)?;

    // SECURE-INPUT GUARD — last gate before clipboard.
    if self.secure_guard.is_secure(&fg_keyup) {
        return self.persist_aborted_secure(raw_text, fg_keydown, fg_keyup);
    }

    // FOCUS-LOSS DOUBLE-SNAPSHOT (ADR 0016 §7).
    if fg_keydown.as_ref().map(|fg| &fg.process_name)
        != Some(&fg_keyup.process_name)
    {
        return self.persist_aborted_focus_changed(raw_text, fg_keydown, fg_keyup);
    }

    let outcome = self.injector.inject(&cleaned, strategy)?;
    self.persist_session(raw_text, cleaned, outcome, fg_keydown, fg_keyup)
}
```

**Tests:** integration test in `tests/dictation_e2e.rs` using
in-memory test doubles for every trait — exercises the happy path +
all three error branches (focus-loss, secure-input, inject-fail).
≥6 cases.

### 4. `src-tauri/src/injection/strategy_wiring.rs` (bd `mb-3yn`, P0)

Wave 2 landed `strategy::resolve()` as a pure function. Wave 4 wires
it into the orchestrator with the foreground-process probe + focus-
loss double-snapshot. New file, not extending `strategy.rs`, to keep
the pure resolver pure.

Tests ≥4.

### 5. `src-tauri/src/db/sessions.rs` extensions (bd `mb-vs3`, P0)

Persist the full session row + raw + cleaned + injection_status +
fg_keydown + fg_keyup. The schema already exists from Phase 1 (PLAN
§7 migration 001/002/003); Wave 4 only writes rows. **No migration
004.** ADR 0010 binding.

Tests ≥4.

### 6. `src-tauri/src/recording_window.rs` (bd `mb-uhk`, P1)

**Stub** — non-activating window that the orchestrator shows on
`StartCapture` and hides on `StopCapture` / `DiscardAudio`. Just the
Rust side; the React UI inside the window lands in Phase 5.

Tests ≥2 (open/close idempotence).

## Definition of done for Wave 4

1. All six `mb-*` tasks closed in bd: `mb-cm3`, `mb-x7i`, `mb-8cd`, `mb-3yn`, `mb-vs3`, `mb-uhk`.
2. Cargo gate four-green via `pwsh scripts/cargo-with-cuda.ps1 <step>`.
3. Test count: 244 → ~275+ (≥30 new).
4. Cross-app QA matrix (below) executed by Dustin; each row passes.
5. `mb-?` Wave 4 close + Wave 5 brief authored.

## QA matrix (Dustin runs these — order matters)

| # | App                       | Expected strategy | Expected behaviour                           |
|---|---------------------------|-------------------|----------------------------------------------|
| 1 | Notepad                   | Paste             | Text appears at cursor; original clipboard preserved. |
| 2 | VSCode (any file open)    | Paste             | Same as Notepad.                             |
| 3 | Windows Terminal (cmd)    | Keystroke         | Each char appears as if typed; no Ctrl+V.    |
| 4 | Windows Terminal (PowerShell) | Keystroke     | Same as cmd.                                 |
| 5 | Chrome address bar        | Paste             | URL-ish text appears in address bar.         |
| 6 | Chrome regular page input | Paste             | Text appears in input field.                 |
| 7 | 1Password unlock dialog   | Abort             | Tray toast: "🔒 ..."; no clipboard activity.|
| 8 | Bitwarden unlock dialog   | Abort             | Same as 1Password.                           |
| 9 | UAC consent prompt        | (preempted)       | Hotkey doesn't even fire (separate desktop).|
| 10 | Login dialog with ES_PASSWORD edit | (preempted) | Tray toast: "🔒 ..."; no paste.        |
| 11 | Empty foreground (alt-tab transient) | (transient) | No-op + log line; no crash.         |
| 12 | Focus loss mid-hold       | (focus_changed)   | Toast: "Focus changed — transcript saved but not pasted"; DB row exists with `aborted_focus_changed`. |

Cross-app row #12 is the trickiest — instructions: hold the hotkey
while focused on Notepad, alt-tab to a different app mid-speech,
release. The transcript should be saved with `injection_status =
aborted_focus_changed` and NO paste should happen anywhere.

## Known risks for Wave 4

| # | Risk | Mitigation |
|---|------|------------|
| 1 | Race: `StateAction::StopCapture` arrives at orchestrator after audio has already been re-allocated for next session | The audio buffer is owned by the orchestrator-side `CpalCapture`; `stop_and_take()` is exclusive. Verify with a stress test that fires 10 rapid taps. |
| 2 | Clipboard race on slow apps: 30 ms guard might not be enough on heavily-loaded systems | Add a configurable guard (default 30 ms, max 200 ms); document in settings.toml. |
| 3 | `SendInput` blocks while another LL-hook is processing | Out of our control (ADR 0015 risk). Document; nothing to mitigate without UIPI elevation. |
| 4 | Surrogate-pair Unicode injection garbles emoji | Test specifically with a 🐦 in the transcribed text via a mock STT. |
| 5 | Recording window steals focus → injection lands in OUR app | Use `WS_EX_NOACTIVATE` + `WS_EX_TOOLWINDOW` (tauri-specific config) when creating the window. |

## Brief discipline reminder

End-of-Wave-4 includes authoring `phase3-wave5-brief.md`. Wave 5 is
the judge-write + retrospective + `phase-3-complete` tag wave; same
pattern as Phase 2 Wave 5.

## Wave 5 preview (so Wave 4 implementor knows what's coming)

4 judges to author:
- `hotkey-hold-detect` — verify 80 ms hold-vs-tap discrimination
- `injection-cross-app-matrix` — qa-kitten runs the matrix above
- `secure-input-respected` — synthetic-secure-window test
- `clipboard-roundtrips` — never observably mutates clipboard
