# Phase 3 — Wave 4.9 brief

**From:** Dustin's hands-on QA matrix on Wave 4 (`mb-up3`).
**To:** code-puppy for the three P0 provenance/clipboard bugs that
surfaced before Wave 5 (judge authoring) starts.
**Entry tag:** Wave 4 cargo gate green (303/303 tests + 7 ignored,
clippy `-D warnings` clean, fmt clean) AND Dustin's QA-matrix run.
**Exit goal:** Bugs A/B/C green + ADR 0020 accepted + the cargo
gate stays green (≥306 tests). No new public API surface. No
schema changes.

## Why a "Wave 4.9" at all

The QA matrix found three issues that would corrupt or hide
provenance — i.e. break the spine of the app — if Wave 5's judges
ran against them. Better to fix them now than to author judges
that codify the broken behaviour:

| ID | Symptom | Cause |
|---|---|---|
| A  | `transcripts` table never populated; only `sessions` had rows | `persist_complete` skipped raw/cleaned/final inserts entirely |
| B  | `sessions.foreground_app` always empty string | `K32GetModuleBaseNameW` requires `PROCESS_QUERY_INFORMATION + PROCESS_VM_READ`, we open with `PROCESS_QUERY_LIMITED_INFORMATION` |
| C  | Clipboard restored to dictated text instead of pre-dictation contents | `SequenceAnalysis::classify` baselined off `seq_before_set`; `EmptyClipboard + SetClipboardData` advanced seq by 2, classifier expected +1, restore was skipped + then ran with stale snapshot |

A fourth issue was a design question rather than a bug: under the
focus-loss double-snapshot, alt-tab during dictation silently
discarded the session. Dustin's call was permissive — see
ADR 0020.

## Deliverables

### Bug A — transcripts persistence (provenance)

- `db::transcripts::insert_{raw,cleaned,final}` called from
  `dictation::persist_complete` on every successful + injection-
  attempted session.
- Added `Cleaner::model_name()` trait method (default
  `"passthrough"`) so the cleaned row records what produced it.
  Phase 4's LLM cleaner overrides with its model id.
- Final transcript row is conditional: written only when the
  injection outcome is `Ok` or `OkClipboardNotRestored`. Aborted
  sessions still get raw + cleaned (provenance is total) but no
  final (nothing was injected).
- `persist_complete` refactored to take a
  `PersistCompleteParams<'a>` struct; previous 8-arg form pushed
  past clippy's limit once raw/cleaned/injected/model were
  added, and the struct makes the call site self-documenting.
- Failure of a single transcript insert is logged but non-fatal —
  the session-row status update must always complete so the
  state machine moves out of Processing.

### Bug B — process_name from exe_path

- Dropped `K32GetModuleBaseNameW` + its import entirely.
- Added pure helper `basename_from_path(&str) -> Option<String>`
  using `std::path::Path::file_name` (handles `\\?\` long-path
  prefix, mixed separators, empty input).
- `process_name` is now derived from `exe_path` (which already
  works at our access level via `QueryFullProcessImageNameW`).
- 4 new unit tests on the helper + the live-foreground test now
  asserts internal consistency
  (`process_name == basename(exe_path)`) when `exe_path` is
  populated.

### Bug C — clipboard sequence baseline

- `SequenceAnalysis::classify` now takes `seq_after_set` (measured
  *after* `write_unicode_text` returns) instead of `seq_before_set`.
  This eliminates the brittle dependency on whether
  `EmptyClipboard + SetClipboardData` advances seq by 1 or 2 on
  the current Windows build.
- Acceptable post-paste deltas tightened: `0` (target read-only,
  the common case) or `+1` (target also wrote). `+2 or more` →
  diverged → skip restore.
- Dropped `wait_for_paste_sentinel` polling loop entirely.
  Replaced with a fixed `PASTE_CONSUME_GRACE` sleep (30 ms) before
  the post-paste seq measurement. Reasoning: read-only paste
  never advances seq, so the poll could only ever time out for
  the common case — burning the full 250 ms timeout instead of
  finishing in 30. The fixed sleep is deterministic + bounded.
- Test names + bodies updated to match the new semantics; added
  a `sequence_diverged_when_seq_went_backwards` defensive case.

### Bug 4 (design call, not a code bug) — permissive focus change

- See ADR 0020. `InjectionDecision::AbortFocusChanged` variant
  removed; legacy `InjectionOutcome::AbortedFocusChanged` enum
  variant + DB string retained for backward DB compatibility
  (the schema's CHECK constraint still lists it).
- `decide_injection` now logs focus changes at `info` level and
  proceeds into the key-up app.
- Secure-input guard (ADR 0017) continues to run on `fg_keyup`,
  unchanged — focus change doesn't weaken the password-field
  invariant.
- Pipeline tests updated: `focus_change_proceeds_into_keyup_app_per_adr_0020`
  + `focus_change_into_secure_input_still_aborts` replace the
  old `focus_loss_aborts_when_processes_differ` test.

## Gate

- `cargo test --lib --skip live_` → 121+ passed, 0 failed
  (live-foreground ignored test still skipped by default).
- `cargo build --lib` → clean.
- `cargo clippy --workspace --all-targets -- -D warnings` →
  expected clean (re-run by Wave 5 before judge authoring).
- `cargo fmt --check` → expected clean.

## Out of scope for 4.9

- Phase-4 LLM cleaner. The new `Cleaner::model_name()` default
  is `"passthrough"`; the LLM impl overrides it when it lands.
- Per-app "strict focus" toggle. Not building it without a real
  user request (YAGNI).
- Schema migrations. None needed — all three fixes are
  Rust-side. Migrations remain SEALED per Phase 1's tag.

## Handoff to Wave 5

The judge work can resume unchanged. The `e2e-injection` judge
sketch in `phase3-wave5-brief.md` now has a strictly better
spec: it can additionally assert that
`SELECT COUNT(*) FROM transcripts WHERE session_id = ? AND stage IN ('raw','cleaned','final') = 3`
for any Ok outcome, and `= 2` for any Aborted outcome.
