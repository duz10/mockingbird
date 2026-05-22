# Judge: sealed-phases-untouched (Phase 10)

**Target:** the **diff** between `phase-mc-complete` and `HEAD`,
scoped to the Dictation subsystem
(`src-tauri/src/dictation/*`, `src-tauri/src/injection/*`,
`src-tauri/src/cleanup/{provider,llm_cleaner,ollama}.rs`), the
Meeting Capture subsystem
(`src-tauri/src/meetings/{capture,long_form_stt,formatter,merge,chunker}.rs`,
the `meetings/runtime.rs` twin-stream wiring,
`src-tauri/src/audio/capture.rs`), the `transcripts` table
raw-immutability trigger, and the sealed migrations 001-014.

**Authorization boundary:** ADR 0037 §Boundary explicitly authorized
**surgical** edits to:

- The hotkey infrastructure for `Right Ctrl + Space` (Command Center
  chord) and any chord-collision probe extensions.
- `meetings/lifecycle.rs` + `commands/meetings.rs` to take a
  `started_from = "command_center" | "legacy_chord"` parameter at
  start, and to call back into the Command Center on stop.
- Tray menu additions (legacy MC chord toggle, command_center_chord
  Settings row).
- Tauri capabilities migration (continuation of ADR 0035).

Anything **outside** that boundary that has changed since
`phase-mc-complete` is a seal violation.

**Question:** Did Phase 10's commits (`phase-mc-complete..HEAD`)
stay within ADR 0037's authorization boundary? Specifically:

- (a) Is the Dictation module's public API surface (function
  signatures + public types in `dictation/mod.rs`,
  `dictation/dictation.rs`, `injection/*`) **unchanged**?
- (b) Is the Meeting Capture pipeline's *shape* unchanged — i.e.
  twin-stream capture into `LongFormStt::run_long_form`,
  per-channel stitching, then `merge_two_channels`, then the
  deterministic `formatter::format` pass, with no new LLM call
  inserted into the critical recording-to-canonical-transcript path?
- (c) Has the `transcripts` table raw-immutability invariant
  (Principle 1) been preserved — no new `UPDATE` statements
  introduced against `stage = 'raw'` rows anywhere in the diff?
- (d) Have migrations 001-014 stayed byte-identical after their
  respective phase-seal tags? (Migration 011 is sealed at
  `phase-mc-complete`; migrations 012-014 are sealed at the
  Phase 10 Wave 1B / Wave 2 / Wave 4 tags-or-equivalents — see
  STATUS.md "Sealed" table for the live list. Migration 015 is the
  one Wave 5 was authorized to ship and is NOT in scope here.)

**Rationale:** This is the structural analog of Phase MC's
`mc-dictation-untouched` judge. Phase 10 took the unusual step of
authorizing *named, scoped* edits to sealed surfaces via ADR 0037 —
because the Unified Recording Command Center genuinely needs to
plumb a "started-from" signal into Dictation and Meeting Capture for
SessionCard rendering. That authorization is the carve-out, not a
license to refactor. If a Phase 10 contributor reaches into
`dictation/dictation.rs::complete()` to "just clean up the paste
path while I'm here", we've punched a hole in the seal the
dictation judges no longer protect — and the next ADR-charter cycle
becomes "everything is open, all the time", which is precisely the
posture Principle 5 ("layers are replaceable") + the
`phase-{N}-complete` tag convention are designed to prevent.

This is a **LLM-graded** judge. The static `git diff` portions are
mechanical; the "did the diff stay within authorization boundary"
verdict is judgment. Structural template: this file mirrors
`docs/judges/phase-mc/mc-dictation-untouched.md`. Use that file as
the structural reference if any criterion below is ambiguous.

**Pass criteria — ALL of:**

1. **No Dictation public-API drift since `phase-mc-complete`:**

   ```powershell
   git diff phase-mc-complete..HEAD -- `
     src-tauri\src\dictation\ `
     src-tauri\src\injection\ `
     src-tauri\src\cleanup\provider.rs `
     src-tauri\src\cleanup\llm_cleaner.rs `
     src-tauri\src\cleanup\ollama.rs
   ```

   LLM-graded output. Pass iff every diff hunk is one of:

   - A **comment-only** change (doc-comment additions, `// SAFETY:`
     annotations, line-comment edits).
   - A **whitespace / formatting** change (rustfmt run; no
     semantic delta).
   - A **`pub fn` signature** change that ADR 0037 §Boundary
     explicitly authorizes (the `started_from` plumbing for
     `dictation::start_with_*`). If you see a `pub fn` signature
     change in a function NOT named in ADR 0037, FAIL with the
     hunk + the unauthorized function name.

   Bonus (eyeball): no new module added to `src-tauri/src/dictation/`
   that didn't exist at `phase-mc-complete`. Listing addition is
   only OK for `dictation/paste_payload.rs` (sealed in `stable-alpha-v0.1`,
   pre-dates Phase 10) — verify by checking `git log
   src-tauri/src/dictation/paste_payload.rs | tail -1` shows a
   commit BEFORE `phase-mc-complete`.

2. **No Meeting Capture pipeline-shape drift since
   `phase-mc-complete`:**

   ```powershell
   git diff phase-mc-complete..HEAD -- `
     src-tauri\src\meetings\capture.rs `
     src-tauri\src\meetings\long_form_stt.rs `
     src-tauri\src\meetings\formatter.rs `
     src-tauri\src\meetings\merge.rs `
     src-tauri\src\meetings\chunker.rs `
     src-tauri\src\meetings\filler_words.rs `
     src-tauri\src\audio\capture.rs
   ```

   LLM-graded. Pass iff every diff hunk is one of:

   - Comment-only / whitespace.
   - An `audio::capture.rs` extension authorized by ADR 0037
     (specifically, the WASAPI loopback capture being parameterized
     so `activity/audio.rs` can wrap it without duplicating).
     Audio capture sharing was charter-approved.
   - A `meetings/lifecycle.rs` or `meetings/runtime.rs` edit
     plumbing the Command Center stop callback (NOT in the file
     list above — those are the authorized surfaces).

   **Hard FAIL conditions:**
   - Any new `OllamaProvider::new(...)` construction in
     `capture.rs` / `long_form_stt.rs` / `formatter.rs` / `merge.rs`
     (LLM in critical path — supersedes ADR 0026 Principle).
   - Any `use crate::cleanup::*` newly added to those files.
   - Any deletion of an `assert!` / `debug_assert!` in `formatter.rs`
     that asserts determinism or fixpoint property.
   - Any change to `formatter::format`'s signature.

3. **`transcripts` table raw-immutability still holds (`stage='raw'`
   never mutated):**

   ```powershell
   git diff phase-mc-complete..HEAD -- src-tauri\src\ |
     Select-String -Pattern 'UPDATE transcripts|UPDATE .* stage'
   ```

   LLM-graded. Pass iff every match is in a comment line (e.g. a
   docstring naming the invariant) OR is a `stage='cleaned'` /
   `stage='dictation'` UPDATE (the cleaned-text and dictation-stage
   rows are mutable per the original schema). If a real UPDATE
   against `stage='raw'` is introduced, FAIL — and the
   `block-mutate-raw-transcripts` hook should have caught it at
   write time; this judge is belt-and-suspenders.

4. **Sealed migrations untouched since their respective seal tags:**

   ```powershell
   git diff phase-mc-complete..HEAD -- `
     src-tauri\src\db\migrations\001_*.sql `
     src-tauri\src\db\migrations\002_*.sql `
     src-tauri\src\db\migrations\003_*.sql `
     src-tauri\src\db\migrations\004_*.sql `
     src-tauri\src\db\migrations\005_*.sql `
     src-tauri\src\db\migrations\006_*.sql `
     src-tauri\src\db\migrations\007_*.sql `
     src-tauri\src\db\migrations\008_*.sql `
     src-tauri\src\db\migrations\009_*.sql `
     src-tauri\src\db\migrations\010_*.sql `
     src-tauri\src\db\migrations\011_*.sql `
     src-tauri\src\db\migrations\012_*.sql `
     src-tauri\src\db\migrations\013_*.sql `
     src-tauri\src\db\migrations\014_*.sql
   ```

   Expected output: **empty**. Migrations 001-010 sealed at
   `phase-4-complete`, 011 sealed at `phase-mc-complete`, 012-014
   sealed at their Wave-level commits (`7333a98` / `9155f40` /
   `e3f90db`). Migration 015 is Wave 5's authorized addition and is
   NOT in this list. If any 001-014 file shows in the diff, FAIL —
   and the `block-sealed-migration-edits` hook should also have
   caught it.

5. **Dictation + Meeting Capture test suites still link clean:**

   ```powershell
   powershell -File scripts\cargo-with-cuda.ps1 test --release --lib `
     --no-run -- dictation:: cleanup:: injection:: meetings::
   ```

   Per LESSONS P2, link-only proof is the sanctioned fallback when
   live exec is blocked on this box. Pass iff `--no-run` reports
   zero errors. If link FAILs (e.g. a sealed trait surface was
   changed and a Phase 10 caller no longer matches), the diff has
   silently broken a sealed contract via a transitive change — even
   if criteria 1-4 didn't flag it textually.

**On failure:**

- **Block the `phase-10-complete` tag.** This is the broadest
  invariant in the Wave 6 slate; it's the one judge that catches
  "death by a thousand surgical edits".
- If criterion 1 surfaces an unauthorized `dictation/` change:
  revert the change. If it's load-bearing for Phase 10, draft a
  successor ADR before re-applying. The judges don't accept
  "but I needed it" — they accept ADR-Accepted authorization.
- If criterion 2 surfaces an LLM-in-critical-path introduction:
  this is a Principle 2 violation. The work belongs in the
  optional-LLM-pass path (mirroring `meetings/llm_pass.rs`), NOT
  in the canonical-transcript pipeline.
- If criterion 3 surfaces a real `UPDATE` against `stage='raw'`:
  rip it out. Append to `transcripts(stage='cleaned')` instead. The
  raw row is the audit log; mutating it destroys provenance.
- If criterion 4 surfaces a migration edit: the hook should have
  vetoed; if it didn't, the hook is the bug. File a P1 bead
  against the hook engine.
- If criterion 5 surfaces a link failure: the API drift is real and
  criterion 1 or 2 missed it. Trace the link error back to the
  changed signature.

**Last run (Wave 6.A dry-run):** _TBD — see Wave 6.A dispatch
report. The LLM-graded portion isn't run during dry-run (no
judges-engine harness invoked); the static `git diff` portions
ARE run and reported below in the dispatch summary._
