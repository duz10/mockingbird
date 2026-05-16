# ADR-0001: `bd` (beads) alongside `STATUS.md` as dual task tracker

- **Status:** Accepted
- **Date:** 2026-05-15
- **Deciders:** Dustin, code-puppy-adeb7b

## Context

PLAN-mockingbird-v2.md was authored assuming `STATUS.md` would be the
only persistent task tracker between Code Puppy iterations. During
bootstrap, Dustin asked whether the project would use `bd` (beads).
Beads is a CLI issue tracker with first-class dependency support and
SQLite/Dolt persistence — a strict superset of `STATUS.md` for the
"what's queued, what's blocked, what's ready" problem.

## Decision

We will use **both**, with non-overlapping roles:

- **`bd`** is the live task queue: ordered by priority, dependency-aware,
  unblocked-work view via `bd ready`. Issues prefixed `mb-`.
- **`STATUS.md`** is the human-readable phase snapshot at iteration
  boundaries. The PLAN, judges, and hooks expect it; do not delete.

End-of-iteration discipline: close completed `bd` issues, create new
ones for discovered work, AND update `STATUS.md`. They are kept in
sync, not merged.

## Consequences

- **Positive:** dependency graph + ready-work view we didn't have; the
  `session-start-briefing` hook surfaces top-of-queue without reading a
  whole markdown file.
- **Negative:** two places to update at iteration end. Mitigated by
  ritual + the `post-commit-status-check` hook warning when STATUS.md
  isn't in a commit.
- **Neutral:** `.beads/` lives in the repo and ships with the project.

## Alternatives considered

- **`STATUS.md` only:** simpler, but no ready-work query, no dependency
  graph, no per-task closure reason history.
- **`bd` only:** would silently break PLAN/judge expectations and lose
  the human-readable phase snapshot.

## Cross-references

- PLAN §11 (iteration boundaries)
- `.code_puppy/AGENTS.md` "Issue Tracking" section
- `docs/LESSONS.md` 2026-05-15 entry on `bd init` interactivity
