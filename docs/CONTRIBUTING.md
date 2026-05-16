# Contributing to Mockingbird

Local-first voice dictation for Windows. Welcome aboard. 🐦

## Prerequisites

- **Rust 1.77+** (`rustup default stable`).
- **Node 18+** — needed from Phase 5 onward for the UI; safe to defer.
- **Windows 10/11** — full local testing path. Other platforms compile
  but the OS surface (tray, hotkeys, injection) is Windows-first.
- **`bd`** — task tracker. Install per the upstream repo; we use it
  for per-wave task tracking.

## First-time setup

```bash
git clone <repo>
cd mockingbird

# COLD cargo check takes ~4 minutes the first time you build.
# `rusqlite-bundled` compiles SQLite (~150k lines of C) from source.
# It's a one-time cost; incremental builds are seconds.
cargo check --workspace
```

After the cold build, the quality gate runs in well under a minute:

```bash
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## The workflow

1. **Pick a task.** `bd ready` lists what's queued.
2. **Read the context.** Open `STATUS.md` and the current phase plan
   in `docs/phases/phaseN.md`.
3. **Check for a wave brief.** If
   `docs/phases/phaseN-waveM-brief.md` exists, it's **binding** —
   read it cover to cover before writing code. The brief specifies
   types, signatures, tests, and known risks for the wave. Wave-
   specific briefs have shipped 100% first-run test pass rates for
   Phase 1 Waves 2–4 (see `docs/LESSONS.md`).
4. **Run the gate locally.** Don't push code that doesn't pass
   `cargo fmt --check`, `cargo clippy -D warnings`, and
   `cargo test --workspace`.
5. **Commit.** Lefthook runs the same gates pre-push as a backstop.
6. **Update `STATUS.md`** when you close a wave or phase.
7. **Append to `docs/LESSONS.md`** anything non-obvious you hit.

## Standing rules

These are project-wide invariants. Hooks enforce most of them.

- **Raw transcripts are immutable.** No `update_raw`, no `upsert_raw`,
  no `UPDATE transcripts WHERE stage = 'raw'`. The
  `block-raw-transcript-edit` hook scans non-test code.
- **Provenance is total.** Sessions always carry `prompt_id`,
  `dictionary_snapshot_id`, `example_set_id`. They're nullable in
  SQL but mandatory at the API layer (`NewSession` uses `i64`, not
  `Option<i64>`).
- **Layers are replaceable.** No SDK lock-in inside repo code;
  abstractions live at module boundaries.
- **No telemetry.** Period.
- **No `@tanstack/*`.** UI uses Zustand + custom hooks.
- **No `npm install/ci` without `--ignore-scripts`.** Post-install
  scripts are the supply-chain attack surface.
- **Clipboard save/restore around every paste.** No exceptions.
- **Secure-input fields abort injection.** Detect via OS APIs, fail
  fast.
- **Migrations append-only after `phase-1-complete`.** The hook
  `block-migration-edit-after-phase-1` enforces this once the tag
  exists.
- **Cross-platform from day one.** Avoid Windows-only code outside
  the OS-surface modules.

## Conventions

- **Style:** rustfmt default. `clippy -D warnings` is the bar.
- **File size limit:** 600 lines per file. Split or refactor if you
  blow past.
- **Errors:** typed `AppError` in `src-tauri/src/error.rs`. Add
  variants with `#[from]` when a new module brings its own error
  type. `?` everywhere.
- **Tests:** unit tests in `#[cfg(test)] mod tests` at the bottom of
  each module. Integration tests in `src-tauri/tests/*.rs`. Use
  `Database::open_in_memory()` for fast DB-backed tests.
- **Migrations:** append-only after Phase 1. New migrations get the
  next number.
- **Tracing:** `tracing::info!`/`warn!`/`error!` etc. Daily-rotated
  logs land at `%APPDATA%\Mockingbird\logs\`. PII scrubbing runs
  on the byte stream.

## When you're stuck

- **5-attempt rule:** if you've tried the same thing five times and
  it's not working, **stop and escalate** — note in STATUS.md's
  `blocked-on` section, ask the user, or take a different
  approach. Don't push to ten attempts.
- **Architectural decisions:** write an ADR draft in `docs/adr/` and
  ask via `ask_user_question`. Don't guess.

## Sub-agents

| Agent | Use for |
|-------|---------|
| `planning-agent` | Phase decomposition |
| `qa-kitten` | Playwright UI verification |
| `helios` | Build missing dev tools |
| `agent-creator` | Mint new project JSON agents |

Phase 0 minted these project agents (each owns a slice of the
codebase):

- `migration-author` — SQL migrations
- `injection-author` — Phase 3 cross-app injection
- `ui-author` — Phase 5 UI components
- `prompt-tuner` — Phase 4 cleanup prompt iteration
- `learning-loop-author` — Phase 8 self-improvement

## Deprecated patterns

**Pack agents (`pack-leader`, `bloodhound`, etc.) are deprecated in
Code Puppy and not used in this project.** Per upstream confirmation.
The Wave 5 judge `no-pack-agents` enforces this.

## The brief pattern (highly recommended)

End every multi-iteration wave by writing
`docs/phases/phaseN-waveM-brief.md` with the next wave's full
context: types, function signatures, test specs, known risks,
deviations from PLAN with reasons. The cost is one iteration's
context-budget; the payoff has been ~100% first-run test pass rates
through Phase 1.

See `docs/phases/phase1-wave2-brief.md` through `phase1-wave5-brief.md`
for the canonical examples.
