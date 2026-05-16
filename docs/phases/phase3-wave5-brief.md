# Phase 3 — Wave 5 brief

**From:** code-puppy at end of Wave 4
**To:** code-puppy for Wave 5 (judge authoring + Phase 3 seal)
**Entry tag:** Wave 4 cargo gate green (303/303 tests + 7 ignored, clippy `-D warnings` clean, fmt clean) AND Dustin's QA-matrix pass on `mb-up3`.
**Exit goal:** 4 judges authored + JSON entries + run green, retrospective written, Phase 3 sealed with the `phase-3-complete` tag.

## Context Wave 5 inherits

1. **Full dictation pipeline lives.** `audio → VAD → STT → cleanup → strategy resolve → secure-input + focus-loss → inject → DB persist` is wired in `dictation.rs::DictationOrchestrator` with full unit coverage of the pure decision layer (`dictation::pipeline::decide`).
2. **Migration 004 lands** the `injection_status` column on `sessions`. Canonical values match `InjectionOutcome::as_db_str()`.
3. **Pure-vs-OS split** is the pattern Wave 5 judges should mirror — judges that exercise pure functions should run in CI; judges that exercise OS surfaces should be `#[ignore]` + manual.
4. **bd open tasks**: `mb-up3` (Dustin's QA matrix; blocks `mb-idy`) and `mb-idy` (this brief's main work).

## Deliverables

### 1. Four judges (bd `mb-idy`, P0)

Per Phase 2's pattern, each judge is a `.code_puppy/judges/<name>/JUDGE.md` + entry in `.code_puppy/judges.json`. The 4 judges to author:

#### `e2e-injection`
Pure: assemble in-memory mock `AudioCapture`, mock `SpeechToText` (returns "hello world"), `PassthroughCleaner`, mock `Injector` that records the call, mock `WindowContext` (returns notepad.exe), `NeverSecureGuard`. Drive a `StateAction::StartCapture(Normal)` + `StopCapture` through `DictationOrchestrator::run`. Assert: injector received `("hello world", Paste)`, DB row exists with `injection_status = "ok"`.

Test target: `cargo test --release --workspace dictation::tests::happy_path_proceeds_with_paste -- --exact`. Already in the codebase as a unit test; the judge wraps it for retrospective accounting.

#### `db-provenance`
Pure: every session row written has non-NULL `prompt_id`, `dictionary_snapshot_id`, `example_set_id`, `hotkey_pressed`, `started_at`, `recording_ended_at`. Implemented as a SQL assertion run against a fresh DB after orchestrator round-trips:

```sql
SELECT COUNT(*) FROM sessions
WHERE prompt_id IS NULL OR dictionary_snapshot_id IS NULL
   OR example_set_id IS NULL OR hotkey_pressed IS NULL
   OR started_at IS NULL OR recording_ended_at IS NULL;
-- expected: 0
```

#### `clipboard-restored`
**Live, marked `#[ignore]`.** Plant a sentinel ("SENTINEL FOR PHASE 3 SEAL") via `paste::write_unicode_text`, then run `with_saved_clipboard("injected payload", || Ok(()))`, then call `paste::snapshot()` and assert the sentinel is back. Already covered by `paste::tests::live_snapshot_then_write_then_restore_preserves_text`; judge wraps it.

#### `secure-input-respected`
Pure: hand-craft a `WinSecureInputGuard`-style mock that returns `true` for a specific `process_name`. Drive the orchestrator's pipeline. Assert: injector is **never called** (recorded by the mock injector having zero calls) and DB row exists with `injection_status = "aborted_secure"`.

### 2. Phase 3 retrospective (bd `mb-idy` cont.)

`docs/phases/phase3.md` already exists from the planning agent. Wave 5 appends a "## Retrospective" section covering:

- **What went well**: pure-vs-OS split made WH_KEYBOARD_LL + clipboard testable without flaky live tests; ADR-first discipline caught the GUI_SECUREINPUT mistake before it shipped; 303 tests with zero failures at the gate is the highest test:LOC ratio in the codebase so far.
- **What went sideways**: ADR 0017 had to be amended in-flight (signal that didn't exist). Wave 2 brief's `mb-7mp/vrl/cef/q9e` IDs didn't match real bd IDs (cosmetic; brief discipline TODO: query bd before naming).
- **Surprises**: `windows-rs 0.56` HWND is `isize` not pointer — surfaces in any future upgrade. Migration 004 OK'd by Dustin after originally being out-of-scope per brief — provenance trumped the "no new migrations" misreading of ADR 0010.
- **Carry-forward to Phase 4**: LLM cleaner replaces `PassthroughCleaner` (Cleaner trait already in place). Audio metadata (sample rate, duration, blob path) gets persisted properly (Wave 4 left these as 0/None placeholders).

### 3. `phase-3-complete` git tag

After judges run green + retrospective lands:

```bash
git tag -a phase-3-complete -m "Phase 3 sealed. Dictation pipeline live."
```

## Definition of done for Wave 5

1. 4 judges in `.code_puppy/judges/` + `.code_puppy/judges.json`.
2. All 4 judges run green.
3. `mb-idy` closed; `mb-up3` closed (Dustin marks it after QA pass).
4. Retrospective appended to `docs/phases/phase3.md`.
5. `phase-3-complete` tag pushed.
6. STATUS.md flipped to "Phase 4 ready" + reset block-on section.

## Known risks for Wave 5

| # | Risk | Mitigation |
|---|------|------------|
| 1 | Mock `AudioCapture` may not satisfy `!Send` constraints if we run the judge cross-thread | Run all judges single-threaded on the test runner's thread. |
| 2 | Live `clipboard-restored` judge collides with the user's actual clipboard in CI | Marked `#[ignore]` — only runs locally. CI runs the pure-version (sequence-analysis tests). |
| 3 | `db-provenance` SQL might pass on a DB with zero rows | Insert at least one session row in the test setup before running the assertion. |
| 4 | The retrospective grows the phase doc beyond 600 lines | If it does, split into `phase3.md` + `phase3-retro.md`. Don't compress the lessons. |

## After Wave 5

Phase 4 (LLM cleanup integration) is unblocked. The handover to that phase's planning agent should highlight:
- Cleaner trait shape (`fn clean(&mut self, raw: &str, mode_slug: &str) -> AppResult<String>`) is stable.
- Prompts live in `src-tauri/src/cleanup/prompts/*.md` (already in repo; loaded but not yet used).
- Phase 4 implementor writes `LlmCleaner` impl + wires it into `DictationOrchestrator::new` via dependency injection.
