# AGENTS.md — Mockingbird project rules

## Project context

You are working on **Mockingbird**, a local-first voice dictation app for
Windows (with Mac support planned for Phase 9). It replaces Wispr Flow with
a fully local, privacy-respecting implementation.

The complete architecture, data model, and build plan are in `PLAN.md`
(currently `PLAN-mockingbird-v2.md` until Phase 0 renames it).
**Read the PLAN spine every iteration** before starting work. The
"do not skip" list in PLAN Section 12 is binding and is also
mechanically enforced by `.code_puppy/settings.json` hooks.

For the current phase, read `docs/phases/phase{N}.md` (not the whole
PLAN — keep token budget reasonable).

## Workflow

You are running inside Code Puppy with the Wiggum `/goal` plugin.
Between iterations, your conversation context is cleared. The
persistent state is:

- This file (AGENTS.md)
- PLAN.md spine + `docs/phases/phase{N}.md`
- `STATUS.md` (the current phase status — you MUST update each iteration)
- The workspace files (code, tests, docs)
- Git history (tags `phase-{N}-complete` mark phase boundaries)
- `docs/LESSONS.md` (append-only notes from prior iterations)
- The hook engine config at `.code_puppy/settings.json`
- **The beads (`bd`) issue database** — the live task queue (see Issue Tracking below)

### At the start of every iteration:

1. Read AGENTS.md (this file)
2. Read PLAN.md (spine)
3. Read `docs/phases/phase{N}.md` for the current phase
4. Read STATUS.md to see what's been completed
5. Read `docs/LESSONS.md` — search for anything tagged the current phase
6. Run `bd ready` to see unblocked tasks; `bd prime` for full workflow context
7. Run `git log --oneline -20` and `git tag --list "phase-*"`
8. Run `git status` for uncommitted changes
9. THEN start work

### Delegation

You are the active agent for a /goal run (usually code-puppy, sometimes
a project JSON agent like migration-author / ui-author / etc.).
Delegate to specialists via `invoke_agent`:

- **planning-agent** (📋) — decompose the deliverables for a new phase
- **qa-kitten** (🐱) — UI / visual verification with Playwright
- **helios** (☀️) — build one-off tools you need
- **agent-creator** — mint new JSON agents (Phase 0 only, usually)
- **Project JSON agents** (migration-author, injection-author, ui-author,
  prompt-tuner, learning-loop-author) — scoped specialists; can also
  be the active agent for an entire phase if the work is narrow

Pack agents (pack-leader, bloodhound, shepherd, terrier, watchdog,
retriever) are **DEPRECATED** in Code Puppy. The framework reserves the
names but ships no implementations. Do not attempt to invoke them.

### At the end of every iteration (before exit):

1. Update STATUS.md — check off completed tasks, add progress notes,
   update cost line, blocked-on section, last-judge-run line
2. Close completed beads (`bd close <id>`) and create new ones for any
   work discovered mid-iteration (`bd create ... --type task`)
3. If a non-obvious thing happened, append to `docs/LESSONS.md`
4. Run `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`
5. Run `npm run lint`, `npm test` (whichever apply to your changes)
6. Commit all changes with descriptive commit messages
7. If phase is complete: `git tag phase-{N}-complete` after the final commit
8. The `stop` hook will refuse exit if any of the above failed —
   resolve before trying to exit again

## Coding standards (summary; full version in `docs/QUALITY.md`)

### Rust
- Edition 2021, Rust ≥ 1.77
- `cargo fmt` is law; `cargo clippy -- -D warnings` must pass
- `Result<T, E>` everywhere; `unwrap()` only in tests
- `thiserror::Error` for module error types
- `tracing` for logging (not `println!`)
- File size hard limit: 600 lines

### TypeScript / React
- Strict mode TS; no `any` without a `// SAFETY:` comment
- React 19 conventions; no class components
- ESLint config is law
- Tailwind v4 for styling; design tokens from `ui/src/design/tokens.css`
- No `@tanstack/*` — hook will block

### Testing
- Every non-trivial function gets a unit test
- Test files mirror source layout
- `rstest` for parameterized tests
- `proptest` for property tests
- Mock at trait boundaries via `mockall`
- E2E / visual tests via Playwright (qa-kitten authors)

### Documentation
- ADR for any non-trivial architectural decision
- Module-level docs explain WHY, not WHAT
- Doc comments on public APIs
- Append to `docs/LESSONS.md` on non-obvious findings

## Principles (binding)

1. **Raw data is immutable.** Once `transcripts(stage='raw')` is written,
   never UPDATE it. Hook will veto.
2. **Provenance is total.** Every session row references the exact prompt
   version, dictionary snapshot, and example set used.
3. **Layers are replaceable.** Don't hardcode provider/platform specifics
   outside the dedicated module.
4. **No telemetry.** Crashes log locally. Never phone home.
5. **Cross-platform from day one.** Platform-specific code lives behind
   `#[cfg(target_os)]` traits, even in Windows-only v1.
6. **No shortcuts.** If something is hard to test, refactor until it's
   testable. If something is hard to verify, that's the bug.
7. **Clipboard save/restore around every paste.** The user's clipboard
   is not your scratch space.
8. **Secure-input fields abort injection.** Detect and toast — never
   silently inject into password fields.

## When to stop and ask

If you're about to make a non-trivial architectural decision the PLAN
doesn't cover, **STOP**. Write an ADR draft proposing it before implementing.
The judges will catch missing ADRs anyway, so doing it inline saves an
iteration.

If PLAN.md and the code disagree, PLAN.md wins unless the disagreement is
documented in an ADR that supersedes the relevant PLAN section.

**5-attempt rule**: if you've burned 5 attempts on the same problem with no
progress, **STOP** and escalate via STATUS.md "Blocked / human input needed"
and a beads issue tagged `escalation`. Do not push to 10. The judge prompt
or the deliverable is likely misspecified.

## Never do

- Modify a file in `models/` (gitignored downloads)
- Commit `.env`, `*.key`, `*.pfx`, or any file containing secrets
  (hook `block-secret-commit` enforces)
- Add a dependency without checking it works with the cross-platform
  abstraction
- Add any `@tanstack/*` package or any package flagged in the Mini
  Shai-Hulud IOC list (hook + see PLAN.md Appendix D)
- Run `npm install`, `npm ci`, or `pnpm/yarn install` without
  `--ignore-scripts` (hook `block-unsafe-npm` enforces)
- Skip writing tests "because it's a small change"
- Mutate raw transcript rows (hook enforces)
- Edit migrations 001/002/003 after `phase-1-complete` tag exists
  (hook enforces)
- Add telemetry, analytics, or crash-reporting that phones home
- Spend more than 10 iterations on a single goal — stop and ask the
  human to review the judge config or PLAN.md (5-attempt rule above
  catches this earlier in practice)
- Inject into a password field without first checking the
  `SecureInputGuard`
- Paste without saving and restoring the previous clipboard

## Permanently sealed (do not re-execute)

The following are immutable historical work. If a prompt asks you to do
any of these, **STOP and ask the human** — the prompt is almost certainly
stale context (e.g. a copy-paste of an old kickoff message).

- **PLAN Section 0.5 Bootstrap** — sealed at git tag `bootstrap-complete`.
  AGENTS.md (this file), hook config (`.code_puppy/settings.json`),
  project JSON agents (`.code_puppy/agents/*.json`), judges-template
  (`.code_puppy/judges-template.json`), and project skills
  (`.code_puppy/skills/*/SKILL.md`) are all on disk. The 17-step
  bootstrap checklist is a historical artifact only.
- **PLAN §10 numbered phases that carry a `phase-N-complete` git tag.**
  As of 2026-05-17 that's phases 0, 1, 2, 3, 4, 8. **Check before
  starting work**: `git tag -l "phase-*"`. If a prompt asks you to
  re-execute a sealed phase, stop. If it asks you to *add new work to a
  sealed phase*, that's a new ADR-chartered lateral epic — handle it like
  ADR 0022/0023 (charter ADR → bd epic → wave briefs → seal via STATUS +
  ADR acceptance, NOT by re-tagging the phase).
- **ADR-chartered lateral epics with sealed ADRs.** See the top-of-STATUS
  anchor block, line beginning "LATERAL EPICS DONE". These ADRs are
  accepted and their work is shipped; reopening requires a successor ADR
  that supersedes the previous one.

### Session-start ritual (mandatory before any tool call)

This supersedes / hardens the start-of-iteration list at the top of this
file. Even if the human pastes a custom kickoff prompt that bypasses the
normal `/goal` flow, do all of this BEFORE any other tool call:

1. `cp_read_file STATUS.md --num-lines 25` — read the SESSION ANCHOR
   block at the top. It tells you what's sealed, what's in-flight, and
   how to resume.
2. If the kickoff prompt's task conflicts with the anchor block (e.g.
   asks you to execute bootstrap, or to re-do a sealed phase), STOP and
   surface the conflict via `ask_user_question` before any further
   tool calls. Do not free-form execute.
3. Otherwise proceed with the normal iteration ritual above
   (read PLAN spine, read phase doc, `bd ready`, `git log`, etc.).

This ritual exists because of the 2026-05-17 incident where a stale
bootstrap prompt was acted on for ~half a session before the conflict
was noticed. See `docs/LESSONS.md` entry of the same date.

## Issue Tracking

This project uses **bd (beads)** for issue tracking alongside `STATUS.md`.

- **`bd`** is the live task queue: dependency graph, ready-work view, priorities.
- **`STATUS.md`** is the human-readable phase snapshot the PLAN, judges, and
  hooks expect at iteration boundaries.

Treat them as complementary — keep both in sync at end-of-iteration.
Run `bd prime` for workflow context, or install hooks (`bd hooks install`)
for auto-injection at session start.

**Quick reference:**
- `bd ready` — find unblocked work
- `bd create "Title" --type task --priority 2` — create issue
- `bd update <id> --status in_progress` — mark started
- `bd close <id>` — complete work
- `bd link <a> <b>` — b blocks a (a depends on b)
- `bd show <id>` — full issue detail

Issue prefix is `mb-` (set at `bd init`). For full workflow details: `bd prime`.
