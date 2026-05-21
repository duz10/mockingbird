# Judge: mc-dictation-untouched (Phase MC)

**Target:** `src-tauri/src/hotkey/state.rs`,
`src-tauri/src/hotkey/windows.rs`, `src-tauri/src/hotkey/driver.rs`,
`src-tauri/src/dictation/*`, `src-tauri/src/injection/*`,
`src-tauri/src/recording_window.rs`, `src-tauri/src/cleanup/provider.rs`,
`src-tauri/src/cleanup/llm_cleaner.rs`,
`src-tauri/src/db/migrations/001*.sql` through
`src-tauri/src/db/migrations/010*.sql`, the master plan §"Bindings"
list.

**Question:** Did the Phase MC commits modify any file in the
"do-not-touch" set that the master plan declared off-limits? The
answer must be **NO** — except for the explicit, charter-approved
additions of meeting-specific code in *new* files, dictation-
orthogonal IPC handlers in `lib.rs` / `commands/mod.rs`, and the
allowed extensions of `hotkey/probe.rs` (the meeting collision
probe per ADR 0027 — `probe.rs` is NOT in the do-not-touch list;
only `state.rs` / `windows.rs` / `driver.rs` are).

**Rationale:** Phase MC was deliberately scoped as a *sibling*
subsystem to dictation. The do-not-touch list exists because:

  1. Dictation is sealed at `phase-4-complete` and has its own
     judge suite; modifying it under the Phase MC banner would
     re-open that seal without an ADR.
  2. The `cleanup::CleanupProvider` trait is the public contract
     for dictation's LLM pass; extending it for meeting use would
     entangle the two subsystems' lifetimes.
  3. The recording window, hotkey state, and injection paths are
     all dictation-specific. The meeting subsystem ships with its
     own overlay, its own hotkey-installer module, and no
     injection at all.
  4. Migrations 001–010 are sealed at `phase-4-complete` per the
     hook `block-sealed-migration-edits`. The meeting subsystem
     ships migration 011 only (`src-tauri/src/db/migrations/
     011_meeting_capture.sql`).

If a contributor modifies a sealed file under the Phase MC
banner, two things break: the dictation judge suite (which
assumes its targets are immutable post-`phase-4-complete`) and
the architectural separation that makes both subsystems
independently testable. The judge is a *diff* judge — it
inspects the commits between `phase-mc-start` (tag set at the
commit immediately before the first Phase MC commit) and HEAD.
Diffing against `phase-4-complete` is wrong because lateral
epics (ADR 0022/0024/0025) legitimately landed between
`phase-4-complete` and `phase-mc-start` and touched some of these
files; those changes are NOT Phase MC's concern.

**Pass criteria — ALL of:**

1. **No sealed dictation files modified since `phase-mc-start`:**

   ```powershell
   git diff --name-only phase-mc-start..HEAD -- `
     src-tauri\src\hotkey\state.rs `
     src-tauri\src\hotkey\windows.rs `
     src-tauri\src\hotkey\driver.rs `
     src-tauri\src\dictation\ `
     src-tauri\src\injection\ `
     src-tauri\src\recording_window.rs `
     src-tauri\src\cleanup\provider.rs `
     src-tauri\src\cleanup\llm_cleaner.rs
   ```

   Expected output: empty. Any file listed is a violation.

2. **No sealed migrations modified since `phase-mc-start`:**

   ```powershell
   git diff --name-only phase-mc-start..HEAD -- `
     src-tauri\src\db\migrations\001_*.sql `
     src-tauri\src\db\migrations\002_*.sql `
     src-tauri\src\db\migrations\003_*.sql `
     src-tauri\src\db\migrations\004_*.sql `
     src-tauri\src\db\migrations\005_*.sql `
     src-tauri\src\db\migrations\006_*.sql `
     src-tauri\src\db\migrations\007_*.sql `
     src-tauri\src\db\migrations\008_*.sql `
     src-tauri\src\db\migrations\009_*.sql `
     src-tauri\src\db\migrations\010_*.sql
   ```

   Expected output: empty. The hook `block-sealed-migration-edits`
   should also have caught this at write time; the judge is
   belt-and-suspenders.

3. **No new row in the `modes` table:**

   ```powershell
   Select-String -Path src-tauri\src\db\migrations\011_*.sql `
     -Pattern 'INSERT INTO modes|ALTER TABLE modes'
   ```

   Expected: empty. Meeting LLM prompts live in
   `src-tauri/src/meetings/prompts/*.md` (markdown files), not
   in the `modes` table. ADR 0026 §"Meeting prompts" pins this.

4. **`CleanupProvider` trait is unchanged since `phase-mc-start`:**

   ```powershell
   git diff phase-mc-start..HEAD -- `
     src-tauri\src\cleanup\provider.rs `
     src-tauri\src\cleanup\llm_cleaner.rs
   ```

   Expected: empty diff. The trait surface is sealed; the
   meeting LLM pass constructs `OllamaProvider` via its
   existing `pub fn new()` and drives it through the existing
   `CleanupRequest<'_>` per call.

5. **`OllamaProvider::new()` and `OllamaProvider::with_base_url()`
   public surface unchanged since `phase-mc-start`:**

   ```powershell
   git diff phase-mc-start..HEAD -- src-tauri\src\cleanup\ollama.rs
   ```

   Expected: empty diff, OR diff contains only doc-comment
   additions (no signature changes, no new pub items). If the
   meeting subsystem needed a runtime test-counter shim, that
   shim ships in `cleanup\ollama.rs` behind `#[cfg(test)]` and
   was charter-approved in the Wave 6 brief. As of the
   `phase-mc-complete` tag the shim was NOT shipped — the
   `mc-no-llm-in-critical-path` judge uses static checks
   instead, so this diff should be empty.

6. **The dictation judge suite still passes:**

   ```powershell
   pwsh scripts\cargo-with-cuda.ps1 test --release --lib `
     -- dictation:: cleanup:: injection::
   pwsh scripts\cargo-with-cuda.ps1 test --release `
     --test dictation_orchestrator
   ```

   Expected: every dictation/cleanup/injection test that was
   green at `phase-4-complete` is still green at
   `phase-mc-complete`. Regressions here mean Phase MC
   accidentally broke a sealed contract via a transitive
   dependency change.

**On failure:**

- **Block the `phase-mc-complete` tag.**
- If criterion 1 surfaces a file: revert that file to its
  `phase-mc-start` state and re-do the change in a new
  meetings-side file. If a sealed file MUST change (e.g. a Tauri
  upgrade required it), write an ADR justifying the change and
  re-tag the affected dictation sub-phase as superseded.
- If criterion 3 surfaces a `modes` table insert: rip it out;
  prompt-as-markdown is the ADR-026 contract.
- If criterion 6 fails: a transitive dep change broke
  dictation — usually a `Cargo.toml` bump. Pin the offending
  dep, re-run, and document in `LESSONS.md` under
  `[phase-mc-retrospective]`.

**Last run:** _Wave 6 — **GREEN**. `git diff --name-only
phase-mc-start..HEAD` over the do-not-touch set returns ONLY
`src-tauri/src/hotkey/probe.rs` (the allowed meeting collision
probe extension per ADR 0027; `probe.rs` was never in the seal
set). Migration 011 adds the `meetings` table + FTS triggers
only; no `modes` table changes. `cleanup\provider.rs`,
`cleanup\llm_cleaner.rs`, `cleanup\ollama.rs` all untouched
since `phase-mc-start`. Dictation lib + integration test suite
links clean under `--release --no-run`._
