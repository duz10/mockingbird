# Phase 1 Wave 5 — Implementation brief (finalizer)

> Wave 5 is the **phase-1 finalizer**. Mostly docs + verification +
> cleanup; seals the phase with the `phase-1-complete` tag at the
> end. After this lands, **migrations 001-003 become permanent** (the
> `block-migration-edit-after-phase-1` hook fires once the tag exists).

## Tasks in scope

| bd id    | Deliverable                              | Approx. lines | Type      |
|----------|------------------------------------------|---------------|-----------|
| `mb-65j` | `docs/CONTRIBUTING.md` flesh-out         | ~200          | docs      |
| `mb-20w` | `docs/SETTINGS.md` (binding)             | ~150          | docs      |
| `mb-6op` | Lefthook live-fire on a real commit      | —             | verify    |
| `mb-l07` | End-of-phase judge prompts/cards         | ~3 cards      | docs      |
| `mb-6ph` | Phase 1 retrospective in LESSONS.md      | ~100 lines    | docs      |
| `mb-3pn` | Re-enable `missing_docs` with real docs  | many small edits | code   |
| `mb-dhi` | Seal commit + `phase-1-complete` tag     | git op        | git       |

## Cross-cutting decisions

### 1. Order matters — do `mb-3pn` (docs polish) BEFORE `mb-dhi` (tag)

The `phase-1-complete` tag should land on a tree where every public
item carries appropriate docs. Otherwise the very next phase's first
edit eats the lint debt.

### 2. `missing_docs` re-enablement is fine-grained

Don't blanket-doc every field — that creates noise. Strategy:
- Add module-level `//!` doc to every `pub mod` (most have one already).
- Add doc comment to every `pub fn` / `pub struct` / `pub enum`.
- For self-documenting fields like `pub id: i64`, `pub created_at: String`,
  use struct-level `#[allow(missing_docs)]` IF most fields are
  self-evident; otherwise doc them individually.
- Document every enum variant only when the name isn't already a doc
  (e.g. `Stage::Raw` — "Direct STT output. Immutable." is value-add;
  `SessionStatus::Recording` — name is the doc).

Add `#![warn(missing_docs)]` back to `lib.rs` once clippy is clean.

### 3. Judges are markdown cards, not code

Wave 5 ships *judge prompt cards* in `docs/judges/phase-1/`. Each card
has: a name, a target file or pattern, a YES/NO question, and a
brief rationale. The Wiggum dispatcher reads these and runs them
through a model. Phase 1 needs 3 judges (per `docs/phases/phase1.md`):
- `rusqlite-vs-sqlx.md` — "Does the code use rusqlite, never sqlx?"
- `fts5-smoke.md` — "Does FTS5 actually return hits for inserted text?"
- `no-pack-agents.md` — "Are pack agents (pack-leader, bloodhound) absent from agent invocations?"

These are decision-support, not gates. `phase-1-complete` tags
regardless of judge color, but a RED card should land in LESSONS.

### 4. SETTINGS.md is BINDING now

Wave 4 shipped 8 setting keys. `docs/SETTINGS.md` documents every key
with: type, default, semantics, who reads it, who writes it. Future
key additions update this file in the same PR.

### 5. Lefthook live-fire = real commit, not dry run

Make a tiny, real commit (e.g., touch a doc file) and observe each
hook firing. Capture which ones fired, in what order, and on what
files. If any hook misbehaves, fix it before tagging. Output goes
into LESSONS.md as the canonical "what the hooks look like in
practice" record.

### 6. Phase 1 retrospective is structured

Format in LESSONS.md:
```
## 2026-05-15 [phase-1] RETROSPECTIVE — Foundation phase complete

### Delivered
- (bullet list of what shipped per wave)

### Test count
- N tests, M assertions, all green

### What worked
- (the brief pattern, etc.)

### What surprised us
- (real surprises encountered)

### What we deferred
- (intentional out-of-scope items, with the phase that owns them)

### Carry-forward for Phase 2+
- (any patterns/hooks/decisions that future phases inherit)
```

### 7. The tag commit is empty + atomic

Final commit message: `chore(phase-1): seal phase-1-complete`. The
commit should be empty (`git commit --allow-empty`) so the tag
marker is unambiguous. Then `git tag phase-1-complete` on that commit.

---

## Module 1: `docs/CONTRIBUTING.md` (~200 lines)

Structure:

```markdown
# Contributing to Mockingbird

## Prerequisites
- Rust 1.77+ (rustup default stable)
- Node 18+ (Phase 5 onward for the UI)
- Windows 10/11 for full local testing
- `bd` (the task tracker) — install with `npm i -g …` or pull from this repo's tools

## First-time setup
```bash
git clone <repo>
cd mockingbird
# Cold cargo check takes ~4 min because rusqlite-bundled compiles SQLite from C.
cargo check --workspace
```

## The workflow
1. Pick a `bd ready` task
2. Read `STATUS.md` and `docs/phases/phaseN.md`
3. If a wave-specific brief exists at `docs/phases/phaseN-waveM-brief.md`, it's BINDING — read before authoring code
4. Run the quality gate locally before committing:
   - `cargo fmt`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace`
5. Commit. Lefthook runs the same gates pre-push.
6. Update STATUS.md if you closed a wave.
7. Append to `docs/LESSONS.md` if you hit anything non-obvious.

## Standing rules
- **Raw transcripts are immutable.** No `update_raw`, no `UPDATE transcripts WHERE stage='raw'`. Hook scans.
- **Provenance is total.** Sessions always carry `prompt_id`, `dictionary_snapshot_id`, `example_set_id`.
- **Layers are replaceable.** No SDK lock-in in repo code; abstractions live at module boundaries.
- **No telemetry.** Period.
- **No `@tanstack/*`.** UI uses Zustand + custom hooks.
- **No `npm install/ci` without `--ignore-scripts`.** Post-install scripts are the supply-chain attack surface.
- **Clipboard save/restore around every paste.** No exceptions.
- **Secure-input fields abort injection.** Detect and fail fast.
- **Migrations append-only after `phase-1-complete`.** Hook blocks edits to 001-003.

## Conventions
- Rust style: rustfmt default; clippy `-D warnings` is the bar.
- File limit: 600 lines per file.
- Errors: typed `AppError` in `error.rs`, `?` everywhere.
- Tests: unit tests in `#[cfg(test)] mod tests` at the bottom of each file; integration tests in `src-tauri/tests/`.
- Migrations: append-only after Phase 1; new migrations get the next number.

## When stuck
- 5-attempt rule: if you've tried something 5 times and it's not working, stop and escalate. Don't push to 10.
- Write an ADR draft for any architectural decision.
- Ask a focused question via `ask_user_question` for any unresolved PLAN decision.

## Sub-agents
- `planning-agent` — phase decomposition
- `qa-kitten` — Playwright UI verification
- `helios` — build missing dev tools
- `agent-creator` — mint new project JSON agents
- `migration-author`, `injection-author`, `ui-author`, `prompt-tuner`, `learning-loop-author` — minted in Phase 0

## Pack agents (DEPRECATED)
Pack agents (pack-leader, bloodhound, etc.) are deprecated in Code Puppy. Do not invoke them.
```

## Module 2: `docs/SETTINGS.md` (~150 lines)

```markdown
# Mockingbird settings reference

All settings live in the `settings` table with shape `(key TEXT PRIMARY KEY, value TEXT NOT NULL)`. Values are JSON-encoded TEXT — booleans round-trip as `true`/`false`, integers as `42`, etc.

Access via `mockingbird_lib::settings::Settings` (typed) or the Tauri commands `get_setting` / `set_setting` (from the UI).

## Key registry

| Key | Type | Default | Owner phase | Notes |
|-----|------|---------|-------------|-------|
| `autostart_enabled` | bool | `false` | Phase 1 | Start at boot. Phase 4 wires the OS registration. |
| `log_level` | string | `"info"` | Phase 1 | One of `trace`/`debug`/`info`/`warn`/`error`. Picked up by tracing-subscriber on next start. |
| `theme` | string | `"system"` | Phase 5 | `system` | `light` | `dark`. UI theme. |
| `reduced_motion` | bool | `false` | Phase 5 | Disable animations. |
| `sound_feedback` | bool | `true` | Phase 5 | Play recording-start/-stop beep. |
| `claude_api_key_ref` | string \| null | `null` | Phase 4 | Reference (NOT the secret) to the Windows Credential Manager entry. |
| `audio_retention_days` | int | `30` | Phase 5 | Days to keep audio blob files. 0 = forever, -1 = never store. |
| `learning_enabled` | bool | `true` | Phase 8 | Run the learning loop on a schedule. |

## Adding a new setting

1. Add a variant to `SettingKey` in `src-tauri/src/settings/model.rs`.
2. Add it to `as_str`, `try_parse`, `default_value`, `all`.
3. Add a row to this table.
4. If it affects existing behavior, drop a note in LESSONS.md.
5. (Phase 5+) Surface in the Settings UI.

## Corruption handling

If a stored value isn't valid JSON, `Settings::get_raw` returns the default and logs a warn. The corrupted row stays in place until the next `set`. Phase 6 may add a self-repair job; Phase 1 prefers visibility over silent fixes.
```

## Module 3: Judge cards (~3 files in `docs/judges/phase-1/`)

```markdown
<!-- docs/judges/phase-1/rusqlite-vs-sqlx.md -->
# Judge: rusqlite-vs-sqlx (Phase 1)

**Target:** `src-tauri/**/*.rs`, `Cargo.toml`

**Question:** Is rusqlite the SQLite driver, with sqlx and tauri-plugin-sql absent from all dependency lists and source code?

**Rationale:** ADR 0004 chose rusqlite over sqlx for synchronous semantics, smaller dep tree, and explicit transaction control. Drift back to sqlx would mean redoing the migration runner and every repo module.

**Pass criteria:** `grep -E '(^|[^a-zA-Z])sqlx([^a-zA-Z]|$)' src-tauri Cargo.toml` returns zero matches AND `Cargo.lock` does not contain `sqlx` as a top-level package.

**On failure:** Note in LESSONS.md, file a `bd` task to roll back the offending change.
```

```markdown
<!-- docs/judges/phase-1/fts5-smoke.md -->
# Judge: fts5-smoke (Phase 1)

**Target:** `src-tauri/src/db/search.rs`, `src-tauri/src/db/migrations/001_initial.sql`

**Question:** After inserting a raw transcript, does `search::smoke_test_count` return ≥ 1 for a query word that appears in that transcript?

**Rationale:** FTS5 wiring (virtual table + triggers in migration 001) is opaque; a smoke test prevents silent regressions where the trigger is dropped or the FTS5 module fails to load.

**Pass criteria:** `cargo test --workspace -- search::tests::search_finds_inserted_raw_transcript` passes.

**On failure:** Block the phase-1-complete tag. FTS5 not working is a Phase 1 deliverable miss.
```

```markdown
<!-- docs/judges/phase-1/no-pack-agents.md -->
# Judge: no-pack-agents (Phase 1)

**Target:** `.code_puppy/AGENTS.md`, commit history of session transcripts

**Question:** Are pack agents (pack-leader, bloodhound, etc.) absent from all agent invocations in this phase?

**Rationale:** Pack agents are deprecated in Code Puppy per the project owner's standing rule for this iteration. Drift back would mean adopting an unmaintained orchestration pattern.

**Pass criteria:** `grep -ri 'pack-leader\|bloodhound' .code_puppy/ docs/ src-tauri/` returns only entries explicitly documenting the deprecation.

**On failure:** Note in LESSONS, audit recent invocations.
```

## Module 4: Phase-1 retrospective in LESSONS.md (~100 lines)

Use the template in §6 above. Sections:
- **Delivered** — list each wave with file count + test count + commit sha
- **Test count** — final 101 (Wave 4) + however many Wave 5 ends with
- **What worked** — wave-specific briefs (3-for-3 first-run wins after the brief pattern took root), AppError aggregator, `Database::open_in_memory` for tests, typed `SettingKey` registry
- **What surprised us** — `#[cfg(test)]` doesn't cross crate boundaries, SQL UNIQUE+NULL semantics, CURRENT_TIMESTAMP 1-second granularity, `missing_docs` lint hostile to repo fields, the bd CLI didn't gripe about my parallelism
- **What we deferred** — Mockall trait abstractions (Wave 4 didn't need them); pack agents (deprecated); DBOS (Section 0.5 step 3); operator-aware FTS5 parsing (Phase 6); audio retention enforcement (Phase 5); real example ranking (Phase 8); cross-window injection checklist (Phase 3 requires human)
- **Carry-forward for Phase 2+** — brief pattern is now standard; LESSONS.md is the durable institutional memory; provenance-is-total enforcement at the API layer; AppError aggregator; the Sealed-vs-NOT-yet-sealed migration distinction

## Module 5: Re-enable `missing_docs`

Approach:
1. Add `#![warn(missing_docs)]` back to `src-tauri/src/lib.rs`.
2. Run `cargo clippy --workspace --all-targets -- -D warnings`.
3. For each warning:
   - If the item already has a doc comment but clippy missed it: ignore (probably a clippy bug, doc it more explicitly).
   - If the item is a self-documenting field in a struct that's mostly self-doc'd: `#[allow(missing_docs)]` on the field.
   - If the item is a real API surface (fn, struct, enum, variant with non-obvious meaning): write a one-line doc.
4. Iterate until clean.

This is the most tedious task in Wave 5. Budget ~30 minutes of edit + recompile cycles.

## Module 6: Lefthook live-fire

```bash
# Make a tiny real edit to a doc file.
echo "" >> docs/CONTRIBUTING.md
git add docs/CONTRIBUTING.md
git commit -m "chore(phase-1): lefthook live-fire test"
# Observe which hooks fired, in what order, on what files.
# Capture output to LESSONS.md.
git push  # or revert if push isn't desired pre-tag
```

If a hook misbehaves: fix it, repeat. If it fires correctly: note in
LESSONS what each hook did.

## Module 7: Seal commit + tag

```bash
# After all other tasks are green and committed:
git commit --allow-empty -m "chore(phase-1): seal phase-1-complete"
git tag phase-1-complete
# Optionally push the tag — depends on user's preference.
```

## Wave 5 exit checklist

- [ ] `docs/CONTRIBUTING.md` shipped
- [ ] `docs/SETTINGS.md` shipped (8 keys documented)
- [ ] 3 judge cards in `docs/judges/phase-1/`
- [ ] Phase-1 retrospective in LESSONS.md
- [ ] `#![warn(missing_docs)]` re-enabled; clippy clean
- [ ] Lefthook live-fire run documented in LESSONS
- [ ] `cargo test --workspace` still green
- [ ] All 7 Wave-5 bd tasks closed
- [ ] STATUS.md: Phase 1 COMPLETE
- [ ] Empty seal commit + `phase-1-complete` tag
- [ ] **STOP before Phase 2.** Phase 3 needs the user; Phase 2 can roll
      unattended but user reads the seal commit first.

## Known risks

1. **Re-enabling `missing_docs` will cascade warnings.** Budget time for
   the cycle. If it spirals, drop the warn but commit a `bd` task for
   the next phase to revisit.
2. **The judge cards aren't actually executed in Wave 5** — they're
   prompt-ready artifacts. Wiggum will invoke them. If Wiggum isn't
   set up yet, document the cards as "ready when Wiggum is".
3. **Lefthook may not be configured.** Bootstrap installed it; Wave 5
   verifies. If lefthook isn't set up at all, drop a `bd` task and skip
   the live-fire — don't add scope here.

## Out of scope for Wave 5 (Phase 2+)

- Anything related to STT (Phase 2)
- Cross-process audio capture (Phase 2)
- Injection logic (Phase 3 — needs human)
- UI (Phase 5)
- API key Credential Manager wiring (Phase 4)
- Learning-loop scheduling (Phase 8)
