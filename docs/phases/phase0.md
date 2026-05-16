# Phase 0 — Groundwork

**Phase entry tag:** `bootstrap-complete` (commit `656f558`)
**Phase exit tag:** `phase-0-complete` (target)
**Planner:** planning-agent (session f05422)
**Implementor:** code-puppy-adeb7b
**Estimated iterations:** 1–2

> This is the **binding plan** for Phase 0. The PLAN.md spine + Section
> 10 Phase 0 sub-block remain canonical; this doc operationalizes them.

## Resolved ambiguities (carried forward from planning)

1. **`.agents/commands/*.md` ARE part of Phase 0.** The `phase0-structure`
   judge prompt checks for them (PLAN line ~2293). Stubs for phases 1–8
   point at `docs/phases/phase{N}.md`; Phase 0's slash command is the
   real one.
2. **ADR numbering honors PLAN reservations.** `0004 = rusqlite vs sqlx`
   is reserved for Phase 1; `0005 = code-signing CA` for Phase 7 (we
   stub it with `Status: Deferred`). Backfill ADRs renumbered accordingly
   — see table below.
3. **PLAN §4 toolchain pin files (`.npmrc`, `.rustfmt.toml`, etc.) are
   included in Phase 0.** They sit harmlessly without `Cargo.toml`/
   `package.json` and `.npmrc ignore-scripts=true` MUST be present
   before Phase 1 touches npm (the hook will scream otherwise).

## ADR numbering plan (Phase 0 backfill)

| #    | Topic                                                        | Status              |
|------|--------------------------------------------------------------|---------------------|
| 0000 | Template (Michael Nygard format)                             | n/a                 |
| 0001 | `bd` (beads) alongside `STATUS.md` as dual task tracker      | Accepted            |
| 0002 | Pack agents deprecated; `code-puppy` is the orchestrator     | Accepted            |
| 0003 | No `@tanstack/*` dependencies (Mini Shai-Hulud IOC)          | Accepted            |
| 0004 | **RESERVED** for Phase 1 (rusqlite vs sqlx) — do not write  | —                   |
| 0005 | **PLACEHOLDER** for Phase 7 (code-signing CA)                | Deferred            |
| 0006 | npm `--ignore-scripts` mandatory                             | Accepted            |
| 0007 | Tier-0 clipboard paste as injection default                  | Accepted            |
| 0008 | Prompt versioning: no edits after shipment                   | Accepted            |
| 0009 | Tailwind v4 + `tokens.css` over CSS-in-JS                    | Accepted            |
| 0010 | Raw-transcript immutability (the data-model invariant)       | Accepted            |

## Task waves

### Wave 1 — Specs & directories (parallel-safe)

| id              | title                                | priority |
|-----------------|--------------------------------------|----------|
| `p0-dirs`       | Create empty dirs + `.gitkeep`       | 1        |
| `p0-this-doc`   | Commit this `docs/phases/phase0.md`  | 1        |
| `p0-license`    | `LICENSE` (MIT)                      | 1        |
| `p0-settings-stub` | `docs/SETTINGS.md` empty stub     | 2        |

### Wave 2 — Hooks, env scripts, ADRs, slash commands (parallel-safe after Wave 1)

| id                    | title                                    | priority |
|-----------------------|------------------------------------------|----------|
| `p0-lefthook`         | `lefthook.yml`                           | 1        |
| `p0-verify-env`       | `scripts/verify-environment.ps1`         | 1        |
| `p0-setup-dev`        | `scripts/setup-dev.ps1`                  | 1        |
| `p0-adr-template`     | `docs/adr/0000-template.md`              | 1        |
| `p0-adrs-batch`       | Write the 9 backfill ADRs                | 1        |
| `p0-slash-commands`   | `.agents/commands/*.md`                  | 1        |
| `p0-codepuppy-readme` | `.code_puppy/README.md`                  | 2        |
| `p0-toolchain-pins`   | `.npmrc`, `.rustfmt.toml`, `rust-toolchain.toml`, `.env.example` | 2 |
| `p0-contributing-stub`| `CONTRIBUTING.md` + `CHANGELOG.md`       | 3        |

### Wave 3 — Branding & icons

| id                    | title                                    | priority |
|-----------------------|------------------------------------------|----------|
| `p0-icon-svg`         | `assets/icons/mockingbird.svg`           | 2        |
| `p0-icon-gen-script`  | `scripts/generate-icons.ps1`             | 2        |
| `p0-icon-run`         | Run the icon script once (soft prereq)   | 3        |

### Wave 4 — README, status, seal

| id                    | title                                    | priority |
|-----------------------|------------------------------------------|----------|
| `p0-readme`           | `README.md` skeleton                     | 1        |
| `p0-status-update`    | Update `STATUS.md` to Phase 0 → 1        | 1        |
| `p0-judges-run`       | Run end-of-phase judges                  | 1        |
| `p0-commit-tag`       | Commit + `git tag phase-0-complete`      | 1        |
| `p0-bd-sync`          | Close p0-* in `bd`; seed Phase 1 tasks   | 1        |

## Exit criteria (verbatim from PLAN §10)

> Every deliverable present, `git log --oneline` shows Phase 0 commit, tag `phase-0-complete` exists, STATUS.md says Phase 0 complete, hook engine ran clean on the final commit.

Operationalized:

1. All Wave-1–4 tasks closed in `bd`.
2. All judges in the next section return `complete=True`.
3. `git tag --list "phase-*"` includes `phase-0-complete`.
4. `git status` clean.
5. STATUS.md current-phase set to "Phase 1 (queued)", last-judge-run line present.
6. `cargo fmt`/`clippy`/`test` no-op cleanly (no Cargo.toml yet); lefthook skip-if logic demonstrably works.

## Judges at phase exit

| Judge                | Run? | Notes                                                       |
|----------------------|------|-------------------------------------------------------------|
| `phase0-structure`   | YES  | Primary. Checks §4 dirs + `.code_puppy/` + `.agents/commands/`. |
| `agents-md-present`  | passthrough | Already green from bootstrap; re-verify unchanged.   |
| `hook-config-valid`  | passthrough | Already green from bootstrap; re-verify unchanged.   |
| `judges-seeded`      | passthrough | `seed-judges.ps1` already idempotent-verified.       |
| `adr-format`         | YES  | Validates `0000-template` + 9 backfill ADRs structure.      |
| `status-initialized` | YES  | STATUS.md reflects Phase 0 not Bootstrap.                   |
| `setup-script-runs`  | YES  | Dry-run of `verify-environment.ps1` + `setup-dev.ps1`.      |

## Risks (top 5)

1. **`.agents/commands/` omission** would fail `phase0-structure`. Mitigated by including `p0-slash-commands`.
2. **`lefthook.yml` skip-if logic** is finicky on Windows. Mitigation: use `glob:` patterns matching only `*.rs`/`*.ts`/`*.tsx` (which match nothing at Phase 0 → free no-op).
3. **Icon generation tools may not be installed.** Mitigation: script no-ops with message; Phase 1 regenerates via `cargo tauri init`.
4. **ADR backfill is ~2k words.** Mechanical but not zero-cost. Mitigation: batch-author, cross-link PLAN line numbers.
5. **Pre-Phase-2 build prereqs still missing** (cmake/nvcc/ollama). Not a Phase 0 risk, but flagged for visibility — must be resolved before Phase 2 starts.

## Out of scope (DO NOT do in Phase 0)

- `src-tauri/Cargo.toml`, Rust code (Phase 1)
- `ui/package.json`, React code (Phase 1+)
- `cargo tauri init` (Phase 1)
- Migrations 001–003 (Phase 1)
- `tauri.conf.json` (Phase 1 — wires in the pub key generated during bootstrap)
- DBOS db URL (user-deferred)
- `enable_pack_agents=true` in `puppy.cfg` (pack agents deprecated)
