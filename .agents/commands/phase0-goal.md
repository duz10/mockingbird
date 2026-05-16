---
description: Execute one iteration of Phase 0 (Groundwork). See docs/phases/phase0.md.
---

You are entering a `/goal` iteration for **Phase 0: Groundwork**.

## Required reading (do these first, do not skip)

1. `.code_puppy/AGENTS.md`
2. `PLAN-mockingbird-v2.md` (spine; pay special attention to Section 10
   "Phase 0: Groundwork" and Section 4 "Project layout")
3. `docs/phases/phase0.md` — the binding plan for this phase
4. `STATUS.md` — current state
5. `docs/LESSONS.md` — non-obvious findings from prior iterations
6. `bd ready` — unblocked tasks
7. `git status` and `git log --oneline -10`

## Iteration mandate

Execute the next wave of tasks from `docs/phases/phase0.md`. Aim to
close as many `bd` tasks as possible per iteration without violating
the dependency graph. Each ready task in `bd ready` has a corresponding
file or set of files to author.

## Definition of done for this iteration

- All in-progress `bd` tasks either closed (with reason) or escalated.
- `STATUS.md` updated with progress + cost line + any new blocked-on.
- `docs/LESSONS.md` appended if anything non-obvious surfaced.
- `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --quiet`
  no-op cleanly (no Cargo.toml yet — `stop-quality-gate` hook treats
  this as success).
- New commit on `main` with descriptive message.

## Exit criteria for Phase 0 (multi-iteration)

See `docs/phases/phase0.md` § "Exit criteria". The phase tag
`phase-0-complete` is applied ONLY when all Wave-1–4 tasks are
closed in `bd` AND all judges (per docs/phases/phase0.md §C) return
`complete=True`.
