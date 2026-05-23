# Judge: sealed-phases-untouched (ADR 0046 Iter 1)

**Target:** the **diff** between commit `d99a4cd` (ADR 0046 Accepted
anchor; vault charter merged but no implementation yet) and `HEAD`
(Iter 1 implementation complete: `0ecfda2` → `0c250e5` → `fcf8008`
→ `f1e4752` → `2a4ea12` → `a004efa`).

**Question:** Did ADR 0046's Iteration 1 implementation stay inside the
authorization boundary defined by ADR 0046 §3 (headless ingest
extraction), §3.1 (SessionsEventBus companion refactor), and §3.2
(orchestrator input-channel topology amendment)?

This judge is the structural analog of `phase-10/sealed-phases-untouched`.
ADR 0046 follows the same posture as ADR 0037: a tightly scoped,
ADR-named carve-out into sealed-Phase-3 dictation code, with everything
else (hotkey FSM, injection, secure-input, meetings, activity, recording
window, cleanup, stt) explicitly fenced off. The judge mechanically
verifies the diff respects the fence.

**Authorization boundary — ADR 0046 §3 + §3.1 + §3.2:**

The following files MAY have changes; every other file under
`src-tauri/src/` MUST be untouched.

**Edited in-place (sealed surfaces — every hunk must trace to a §3.x
authorization):**

| File | Authorization |
|---|---|
| `src-tauri/src/dictation.rs` | §3 (free-fn `resolve_active_mode_from_db` extraction; `complete()` body delegation), §3.1 (`emit_session_saved` helper routes through `SessionsEventBus` at all three persist sites), §3.2 (`run()` two-channel `select!` reshape + new `handle_headless` method) |
| `src-tauri/src/dictation/runtime.rs` | §3.2 footnote (`std::sync::mpsc` → `crossbeam-channel` bridge thread + `headless_ingest_sender()` accessor) |
| `src-tauri/src/db/sessions.rs` | Migration 018 cascade (`NewSession.source` field + `SessionSource` enum + `insert()` SQL plumb) |
| `src-tauri/src/db/migrations.rs` | Registry growth: migration 018 wired in |
| `src-tauri/src/commands/dictation.rs` | New `dictation_import_file` IPC (kickoff prompt) |
| `src-tauri/src/commands/mod.rs` | Registers `dictation_import_file` |
| `src-tauri/src/lib.rs` | `app.manage(runtime.headless_ingest_sender())` so IPC handlers can grab a sender clone |
| `src-tauri/src/audio/mod.rs` | `pub mod decode;` mod-line only |
| `Cargo.toml`, `src-tauri/Cargo.toml`, `Cargo.lock` | New deps: `symphonia`, `crossbeam-channel` |

**New files (cohesion-required additions — listed in §3 / §3.1 / §3.2):**

| File | Authorization |
|---|---|
| `src-tauri/src/dictation/events.rs` | §3.1 — `SessionsEventBus` trait + `RecordingWindow` adapter |
| `src-tauri/src/dictation/ingest.rs` | §3 — `headless_ingest()` + `IngestProvenance` + `IngestDeps` + `persist_ingest()` |
| `src-tauri/src/dictation/ingest_channel.rs` | §3.2 — `HeadlessIngestRequest` + channel helpers + sender type |
| `src-tauri/src/audio/decode.rs` | §4 (referenced from §3.2 footnote) — `decode_to_pcm16_mono_16k` symphonia helper |
| `src-tauri/src/db/migrations/018_session_source.sql` | §2 — `sessions.source` column |

**Test landings (Phase A-D explicit test scope):**

| File | Authorization |
|---|---|
| `src-tauri/tests/db_repos.rs` | Migration 018 cascade in existing integration tests (`source: SessionSource::Desktop` filler) |
| `src-tauri/tests/dictation_orchestrator.rs` | `run()` signature change cascade (now takes two crossbeam receivers) + new `select!` coverage |
| `src-tauri/src/learning/{corrections,eval,runner}.rs` | `NewSession` constructor cascade inside `#[cfg(test)] mod tests` ONLY (mechanical compile-fix from §2's migration) |

**Docs / status / charter (always-authorized):**

| File | Authorization |
|---|---|
| `docs/adr/0046-mobile-extension-via-vault.md` | §3.2 amendment landed in this iteration |
| `docs/LESSONS.md`, `STATUS.md` | Per-iteration scratch (always allowed) |
| `.beads/issues.jsonl`, `.beads/interactions.jsonl` | Bead-DB updates |

**UI (Phase D — `+ Audio file` button):**

| File | Authorization |
|---|---|
| `ui/src/pages/Dictations.tsx` | `+ Audio file` button + filepicker handler |
| `ui/src/lib/tauri.ts` | Typed `dictation_import_file` IPC wrapper |
| `ui/src/lib/types.ts` | `IngestProvenance` / `ImportResult` type defs |

**FORBIDDEN edits (these MUST be untouched — any change is a HARD FAIL):**

Inside `src-tauri/src/dictation.rs`:
- `fn start_capture`
- `fn discard`
- `fn signal_pipeline_complete`
- The entire `pub mod pipeline { ... }` block
- The `SessionState` struct definition
- All existing tests at the bottom of the file (`#[cfg(test)] mod tests`)

Entire subsystems (zero diff allowed under these paths):
- `src-tauri/src/hotkey/**`
- `src-tauri/src/meetings/**`
- `src-tauri/src/activity/**`
- `src-tauri/src/injection/**`
- `src-tauri/src/window_context/**`
- `src-tauri/src/secrets/**`
- `src-tauri/src/cleanup/**` (used as a library; no implementation edits)
- `src-tauri/src/stt/**`
- `src-tauri/src/recording_window.rs` (the `SessionsEventBus` impl
  per §3.1 lives in the NEW `dictation/events.rs`, NOT here — see
  Note A below)

Permanent sealed surfaces:
- Migrations 001-017 (all `.sql` files) — modification-free
- `transcripts(stage='raw')` — no new `UPDATE` statements

### Note A — `SessionsEventBus` impl placement

ADR §3.1 specifies that `RecordingWindow` is the PTT-path
`SessionsEventBus` impl, "a thin wrapper that delegates to the existing
inherent method; no logic moves." The kickoff prompt suggested this
might land via a one-line `impl SessionsEventBus for RecordingWindow`
inside `recording_window.rs`. The implementation chose the cleaner
alternative explicitly endorsed by §3.1's "lives under `dictation/`
rather than under `recording_window.rs` because the trait belongs to
the dictation domain" rule: the impl lives in
`src-tauri/src/dictation/events.rs` alongside the trait definition,
via a `use crate::recording_window::RecordingWindow;` import. This
results in **zero diff** to `recording_window.rs`, which is strictly
stronger than the kickoff prompt's allowance. PASS.

## The judge's task

1. **Read the diff** (provided inline below or attached as
   `git diff d99a4cd..HEAD`).
2. **Classify each touched file** as AUTHORIZED (in the tables above)
   or UNAUTHORIZED (not in any table).
3. **Classify each touched region of `dictation.rs`** as AUTHORIZED (one
   of the §3 / §3.1 / §3.2 refactor patterns) or FORBIDDEN (touches one
   of `start_capture` / `discard` / `signal_pipeline_complete` /
   `pub mod pipeline` / `SessionState` / the test module body).
4. **Output a verdict** in the format below.

## Mechanical sanity-checks (supplement the LLM grader)

Run these and report results alongside the LLM verdict. ANY non-empty
output is a flag for the LLM grader to investigate:

1. **Forbidden subsystems must show zero diff:**
   ```powershell
   git diff --stat d99a4cd..HEAD -- `
     src-tauri/src/hotkey `
     src-tauri/src/meetings `
     src-tauri/src/activity `
     src-tauri/src/injection `
     src-tauri/src/window_context `
     src-tauri/src/secrets `
     src-tauri/src/stt `
     src-tauri/src/cleanup `
     src-tauri/src/recording_window.rs
   ```
   Expected: empty.

2. **No `UPDATE` against `stage='raw'`:**
   ```powershell
   git diff d99a4cd..HEAD -- src-tauri/src/ | `
     Select-String -Pattern 'UPDATE transcripts|UPDATE .* stage'
   ```
   Expected: empty (or matches only in comments / `stage='cleaned'`).

3. **Migrations 001-017 byte-identical (`--diff-filter=M`):**
   ```powershell
   git diff --diff-filter=M --name-only d99a4cd..HEAD -- `
     "src-tauri/src/db/migrations/0[01][0-9]_*.sql"
   ```
   Expected: empty. (Migration 018 is a NEW file and is filtered out
   by `-M`.)

4. **`dictation.rs` forbidden-function bodies byte-identical:** spot-check
   that no hunk in `git diff d99a4cd..HEAD -- src-tauri/src/dictation.rs`
   targets line ranges inside `start_capture`, `discard`,
   `signal_pipeline_complete`, `pub mod pipeline`, `SessionState`, or
   the test module. Use the HEAD-side function line ranges from
   `Select-String -Pattern '^\s*fn '` to bound the check.

5. **Link surface clean** (LESSONS P2 fallback — live exec blocked on
   this box):
   ```powershell
   powershell -File scripts\cargo-with-cuda.ps1 test --release --no-run
   ```
   Expected: zero errors. (This is in the end-of-iteration gate
   already; surface the previous run's result if rerunning is
   redundant.)

## Verdict format

```
## Verdict: PASS | FAIL
## Authorized edits observed: <list>
## Forbidden edits observed: <list, ideally empty>
## Reasoning: <2-4 paragraphs explaining the classification of each
              touched file, with special attention to any region of
              `dictation.rs` that strictly speaking touches a function
              on the FORBIDDEN list but does so only via a §3.1
              single-emit-point refactor>
## Confidence: <0-100%>
```

## On failure

- **DO NOT close `mb-thmd`.**
- **DO NOT commit the verdict.**
- Stop and surface the specific unauthorized edit(s). The Iter 1
  implementation either needs a rollback OR an ADR §3.x amendment to
  authorize the unforeseen edit.
- If criterion 1 (forbidden subsystems) trips: the Iter 1
  implementation reached outside its boundary; revert the cross-cut
  edit. ADR 0046 explicitly forbids meetings / activity / injection /
  hotkey / secure-input / recording-window changes.
- If criterion 2 (raw transcripts) trips: rip out the `UPDATE` and
  append a new `stage='cleaned'` row instead. The
  `block-mutate-raw-transcripts` hook should also have caught it at
  write time.
- If criterion 3 (sealed migrations) trips: the `block-sealed-migration-edits`
  hook should have refused; file a P1 bead against the hook engine.
- If criterion 4 (forbidden function bodies) trips: revert the body
  changes; the function name is on the FORBIDDEN list for a reason.

## Cross-references

- **Structural template:** `docs/judges/phase-10/sealed-phases-untouched.md`.
- **Authorization precedent:** ADR 0037 §Boundary (Command Center
  surgical edits to sealed Dictation / Meeting Capture).
- **Most recent Phase-3 amendment precedent:** ADR 0045 (programmatic
  dictation start/stop — same `complete()`-adjacent posture).
- **Binding principles touched by the boundary:** Principle 5 (layers
  are replaceable) and the `phase-{N}-complete` tag convention.
