# Contributing to Mockingbird

## Before you start

1. Read [`PLAN-mockingbird-v2.md`](./PLAN-mockingbird-v2.md) — the spine.
2. Read [`.code_puppy/AGENTS.md`](./.code_puppy/AGENTS.md) — binding rules.
3. Read [`STATUS.md`](./STATUS.md) — current phase + open work.

## Dev setup

```pwsh
pwsh ./scripts/setup-dev.ps1
```

The script verifies your toolchain, initializes beads if needed, seeds
project judges, and prints next-step pointers.

## Workflow

This project uses **`bd` (beads)** for live task tracking and
`STATUS.md` for the human-readable phase snapshot. Run `bd ready` to
see unblocked work.

Iterations end **green**:
- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test --quiet`
- `npm run lint && npm test` (when JS files changed)
- `STATUS.md` updated, `bd` synced, `docs/LESSONS.md` appended if anything non-obvious surfaced.

Pre-commit hooks via [`lefthook`](https://github.com/evilmartians/lefthook)
enforce the fast checks; install with `lefthook install` after cloning.

## ADRs

Architectural decisions go in [`docs/adr/`](./docs/adr/) using the
[Michael Nygard format](./docs/adr/0000-template.md). The `adr-format`
judge validates structure end-of-phase.

## Lessons learned

Non-obvious findings go in [`docs/LESSONS.md`](./docs/LESSONS.md).
**Search it before starting work in a new area** — saves real time.

## Reporting issues

Issues live in `bd`. If you're an outside contributor without beads
configured: file a GitHub issue (repo URL TBD pre-Phase-7) and the
maintainer will mirror it into beads.
