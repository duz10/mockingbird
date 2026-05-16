# Phase 3 — Wave 3 brief

**From:** code-puppy at end of Wave 2
**To:** code-puppy for Wave 3
**Entry tag:** Wave 2 cargo gate green (213/213 tests, clippy `-D warnings` clean, fmt clean)
**Exit goal:** Wave 3 cargo gate green with the low-level keyboard hook on a dedicated OS thread, the §6.1 state machine driven by a real `Tick` cadence, hotkey conflict probe (ADR 0019), tray pause-toggle wiring, and ≥10 new tests.

## Context Wave 3 inherits

1. **`HotkeyStateMachine` is pure Rust.** Wave 2 fully wired §6.1 (Idle → PendingHold → Recording → Processing + ConfirmingCancel + 300 s ceiling + pause toggle). Wave 3 only needs to *drive* it with real events from a real hook on a real cadence.
2. **`HotkeyListener` trait + `HotkeyEvent` enum** are stable from Wave 1. `WinKeyboardHook` is the stub Wave 3 fills.
3. **`mb-rne` LESSONS entry 2026-05-17 Finding 2** — `CMAKE_BUILD_PARALLEL_LEVEL=4` is baked into `scripts/cargo-with-cuda.ps1`. Don't touch.

## Deliverables

### 1. `src-tauri/src/hotkey/windows.rs` (bd `mb-7mp`, P0)

**Replace the stub** `WinKeyboardHook` with a real `WH_KEYBOARD_LL` hook that:

- Spawns a dedicated `mockingbird-hotkey` OS thread on `install()`.
- That thread calls `SetWindowsHookEx(WH_KEYBOARD_LL, callback, hinstance, 0)` then runs a message pump (`GetMessageW` / `TranslateMessage` / `DispatchMessageW`).
- The hook callback (`LowLevelKeyboardProc`) does the absolute minimum work — read `WPARAM` (WM_KEYDOWN / WM_KEYUP / WM_SYSKEYDOWN / WM_SYSKEYUP), read `LPARAM` (KBDLLHOOKSTRUCT.vkCode), build a `HotkeyEvent`, `tx.try_send(ev)` — and returns `CallNextHookEx(...)` IMMEDIATELY. ADR 0015 is binding: any work in the callback risks the 300 ms watchdog timeout and silent unhook by Windows.
- The hook filters: only emit events for the configured VK; ignore everything else (zero-copy on `CallNextHookEx`).
- `uninstall()` posts `WM_QUIT` to the hook thread, joins it, and `UnhookWindowsHookEx`s the handle. RAII: a `Drop` impl on the hook thread's owned hook handle ensures unhook happens even on panic.
- The hook handle is stored in a thread-local on the hook thread — `SetWindowsHookEx` returns `HHOOK` only on the thread that called it; passing it across threads is undefined.

**`mpsc::Sender<HotkeyEvent>` is `!Sync` but `Send`.** Pass it INTO the hook thread by `move`. The callback must access it via a thread-local, NOT a `static Mutex` — the latter introduces a lock that blocks the OS callback and trips the watchdog.

**Test strategy:**
- A `#[ignore]` integration test that installs the hook on the current thread (not a spawned one — simpler for testing), synthesises `WM_KEYDOWN` via `SendInput`, peeks the channel, asserts receipt. Run with `cargo test -- --ignored`.
- A unit test on the *filter* helper — given a `KBDLLHOOKSTRUCT` and a configured VK, does the helper produce the right `HotkeyEvent` variant? Pure, no OS calls. ≥5 cases.
- A unit test on the thread-life-cycle helper — `install()` is idempotent, `uninstall()` is idempotent, double-`install()` doesn't spawn two threads.

### 2. State-machine driver thread (bd `mb-vrl`, P0)

Currently the state machine has no `Tick` source. Wave 3 adds a `StateDriver` that:

- Owns the `HotkeyStateMachine` + the `mpsc::Receiver<HotkeyEvent>` from the hook + a `tokio::sync::mpsc::Sender<StateAction>` to the orchestrator.
- Runs a loop with `recv_timeout(Duration::from_millis(20))`:
  - If a `HotkeyEvent` arrived, hand it to `machine.handle(ev)`.
  - Otherwise (timeout), synthesise a `HotkeyEvent::Tick { at: Instant::now() }` and hand it to `machine.handle(...)`.
  - Any returned `StateAction::None` is dropped; other variants are sent to the orchestrator channel.
- 20 ms cadence gives ≥4 ticks inside the 80 ms `hold_threshold` (sufficient resolution) and 15 ticks inside the 300 ms LL-hook watchdog.

Tests: pure-Rust harness with an in-process channel pair, manually-driven clock. ≥5 cases covering: tick cadence triggers `StartCapture`, real `HotkeyEvent` interleaves with synthetic ticks, channel closure exits the loop cleanly.

### 3. Hotkey conflict probe (bd `mb-cef`, P1)

ADR 0019: at startup (after the hook is installed but before the orchestrator runs), synthesise a `KEYEVENTF_KEYUP` + `KEYEVENTF_KEYDOWN` for the configured VK via `SendInput`, wait 50 ms, check whether our channel received it. If not, the OS or another app is filtering it — fall back through the chain.

API shape:
```rust
pub fn probe(listener: &mut dyn HotkeyListener) -> AppResult<u32>
//                                                   ^^ working VK
```

Tries the configured VK first, then `VK_F23` (0x86), then `VK_F24` (0x87), then `Ctrl+Shift+Space` (treated specially — `VK_SPACE` with both modifiers in the LL-hook callback's KBDLLHOOKSTRUCT.flags). Returns the first that survives the round trip; returns `AppError::Hotkey` if none do.

Tests: mock the listener with a channel that selectively echoes or drops events; assert the probe returns the right VK or escalates correctly. ≥4 cases.

### 4. Tray pause-toggle (bd `mb-q9e`, P2)

The hotkey-state machine already accepts `HotkeyEvent::PauseToggle { paused }`. Wave 3 wires this from the tray UI:

- Add a `paused: AtomicBool` to the orchestrator (Wave 4 owns it, but Wave 3 sets it up).
- The tray menu emits a Tauri command `set_paused(bool)`.
- The command flips the atomic AND sends `HotkeyEvent::PauseToggle` into the state-driver channel.

Wave 3 lands the command + the atomic + the channel send; Wave 4 wires it into the dictation orchestrator. ≥2 tests.

### 5. Watchdog log (bd: roll into `mb-7mp`)

ADR 0015 §3: every 5 minutes, log "hook alive, VK=0xXX, last event Xms ago" via `log::info!`. Reuses the state-driver tick loop — every 250th tick (5 min @ 20 ms cadence), check + log. No new test required; covered by the structured-log assertion in Wave 5's judge.

## Definition of done for Wave 3

1. All four `mb-*` tasks closed in bd: `mb-7mp`, `mb-vrl`, `mb-cef`, `mb-q9e`.
2. Cargo gate four-green via `pwsh scripts/cargo-with-cuda.ps1 <step>`.
3. Test count: 213 → ~230+ (≥16 new).
4. `mb-9ir` Wave 2 task closed (this brief is its main deliverable).
5. Wave 4 brief authored at `docs/phases/phase3-wave4-brief.md` covering: `injection/paste.rs` clipboard save/restore, `injection/windows.rs` SendInput orchestrator, recording-window stub, DB persistence rows, cross-app QA matrix.

## Known risks for Wave 3

| # | Risk | Mitigation |
|---|------|------------|
| 1 | `SetWindowsHookEx` returns `HHOOK` thread-affined; using it on another thread crashes | Keep all hook ops on the spawned thread; communicate via `mpsc` only |
| 2 | LL-hook timeout: callback must return in 300 ms or Windows silently unhooks | Move ALL logic out of the callback; only `try_send` + `CallNextHookEx` |
| 3 | `mpsc::Sender::try_send` can fail under backpressure if the consumer stalls | Use a bounded channel sized 256 events; on full, log + drop (NOT block — would trip the watchdog). LESSONS-worthy if it bites in QA. |
| 4 | `WM_QUIT` posting requires the target thread's TID; capture it on hook-thread spawn | Hook thread sends its TID back via a oneshot before entering its message pump |
| 5 | Synthetic events for the conflict probe trigger our OWN hook → false positive | Tag synthetic events via `KBDLLHOOKSTRUCT.flags & LLKHF_INJECTED`; filter them out of normal user-event handling but USE them for the probe |
| 6 | Tests that install a real hook leak hooks if they panic | Wrap the test body in `catch_unwind` and force `uninstall()` in the cleanup branch |

## Brief discipline reminder

End-of-Wave-3 includes authoring `phase3-wave4-brief.md`. Same pattern as Wave 1→2 and Wave 2→3 briefs.

## Hand-off pause point reminder

Wave 4 is the **stop-before-this-runs-without-the-user** wave per the original task brief — it covers the cross-app injection checklist that needs Dustin at the keyboard testing Notepad / VSCode / Terminal / browsers / a password manager. code-puppy should land Wave 3, commit, and **stop**. Resume Wave 4 only on explicit "go" from the user.
