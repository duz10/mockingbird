# Judge: clipboard-restored (Phase 3)

**Target:** `src-tauri/src/injection/paste.rs` (`with_saved_clipboard`, `snapshot`, `restore`), ADR 0018

**Question:** When the paste-via-clipboard injector runs the four-step clipboard dance (snapshot → write injection text → simulate Ctrl+V → restore snapshot), does the user's original clipboard contents come back unchanged?

**Rationale:** ADR 0018 commits Mockingbird to **invisible** clipboard usage. The user holds the hotkey, speaks, releases — the transcript appears, and their previous clipboard (a URL, a copied paragraph, a password from a password manager) is still there if they hit Ctrl+V five seconds later. Any regression here is a privacy + UX disaster: the user thinks they're pasting their old contents and instead pastes the last dictation. Worse, a regression that "almost works" (restores most formats but loses HTML, say) is invisible until a user reports lost data. This judge nails down the round-trip with a sentinel — provably restored before/after — and runs it on every CI gate (pure analysis) plus on-demand against the live Win32 clipboard (`#[ignore]`).

**Pass criteria — ALL of:**

1. **Pure sequence-analysis tests (CI):**

   ```powershell
   pwsh scripts/cargo-with-cuda.ps1 test --release --workspace --lib `
     -- injection::paste::tests
   ```

   The pure layer (`SequenceAnalysis::classify`, `encode_utf16_nul`, `decode_utf16_nul`, snapshot-struct round-trips) must pass without touching the OS clipboard. These run in CI.

2. **Live clipboard round-trip (manual, `#[ignore]`):**

   ```powershell
   pwsh scripts/cargo-with-cuda.ps1 test --release --workspace --lib `
     -- injection::paste::tests::live_snapshot_then_write_then_restore_preserves_text `
     --ignored --exact
   ```

   Test body (already in `paste.rs`):
   - Plants a sentinel string into the live clipboard via `paste::write_unicode_text`.
   - Runs `with_saved_clipboard("injected payload", || Ok(()))`.
   - Calls `paste::snapshot()` and asserts the sentinel is back, byte-for-byte.

3. **No new `#[allow(...)]` on the paste module:**

   Eyeball the diff of `injection/paste.rs` against the previous tag. Any newly-introduced `#[allow]` (especially `dead_code`, `unused`) is a yellow flag that something got silently disabled to make the test pass. The Wave-5 retrospective should call any such addition out explicitly.

**Why the live test is `#[ignore]`:**

- CI machines may have empty / unusable clipboards (headless runners).
- A live clipboard test colliding with the developer's actual clipboard contents during a parallel `cargo test` run would be a terrible debugging experience.
- The pure sequence-analysis tests + the four-step state machine are themselves heavily covered by unit tests (`paste::tests::sequence_*`, `paste::tests::classify_*`), so CI confidence is high even without the live test running every push.

**On failure:**

- **Block the `phase-3-complete` tag** if either the pure tests OR the live test fails.
- If the live test fails with `expected sentinel == got <payload>`: `with_saved_clipboard`'s restore step ran but didn't actually copy bytes back — likely an early-return after the paste guard timed out. Check `restore_from_snapshot`.
- If the live test fails with `expected sentinel == got <empty>`: the clipboard owner died between snapshot and restore (Tier 1 fallback per ADR 0018 should fire — check `OkClipboardNotRestored`).
- If sequence-analysis tests fail: the win32 `GetClipboardSequenceNumber` accounting drifted; verify against the `SequenceAnalysis` truth table.

**Last run:** _Wave 5 — **GREEN (CI portion)**. Pure sequence-analysis tests pass in CI. The `#[ignore]`d live test was last run manually during Wave 4.9 and passed (receipt in `docs/LESSONS.md` Wave-4.9 entry). The live test stays `#[ignore]` per the rationale above; it is **required** to run + pass before tagging `phase-3-complete`._
