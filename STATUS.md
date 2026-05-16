# Mockingbird — STATUS

**Current phase:** Phase 1 — Wave 1 ✅ landed; Waves 2-5 queued for fresh-context iterations
**Last updated:** 2026-05-15 (end of session containing bootstrap + Phase 0 + Phase 1 Wave 1)
**Last successful judge run:** _Phase-0 structural self-check + Phase-1 Wave-1 cargo gate (fmt/clippy/test/check all green), 2026-05-15_
**Cost line (cumulative):** _Track from first /goal run — bootstrap + Phase 0 + Phase 1 Wave 1 in one session; record when LLM judges run._

---

## Phase 0 — Groundwork: ✅ COMPLETE

All 21 Phase 0 tasks (per `docs/phases/phase0.md`) closed in `bd`. Phase tag
`phase-0-complete` applied to the seal commit.

### Wave-by-wave summary

| Wave | Deliverables                                                 | Status |
|------|--------------------------------------------------------------|--------|
| 1    | dirs + `.gitkeep`, `LICENSE` (MIT), `docs/SETTINGS.md` stub, `docs/phases/phase0.md` | ✅ |
| 2    | `lefthook.yml`, `verify-environment.ps1`, `setup-dev.ps1`, ADR `0000-template` + 9 backfill ADRs, 16 slash commands, `.code_puppy/README.md`, toolchain pins (`.npmrc`/`.rustfmt.toml`/`.env.example`), `CONTRIBUTING.md` + `CHANGELOG.md` | ✅ |
| 3    | `assets/icons/mockingbird.svg`, `scripts/generate-icons.ps1`, generated icon set under `src-tauri/icons/` | ✅ |
| 4    | `README.md`, this STATUS.md, judge self-check, commit + tag | ✅ |

### Mid-iteration learnings logged

- `rust-toolchain.toml` is a PIN not an MSRV → removed from the repo;
  MSRV moves to `Cargo.toml [package] rust-version` in Phase 1.
- PowerShell `$Args` is an automatic; don't name a param `$Args`.
- `cargo tauri icon <svg>` Just Works™ — no ImageMagick needed.
- See `docs/LESSONS.md` for the full set (now 7 entries from bootstrap+Phase-0).

---

## Tauri updater public key (carried forward from bootstrap; Phase 1 embeds into `tauri.conf.json`)

```
dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEQ5N0E1MTkzODYzNTBGQTEKUldTaER6V0drMUY2MlNiS2g5anF0Vjl6UEkyODRQTlZlS0FMRjNuNWcvdEpJUC9RRG1QVm5Ja04K
```

Private key at `%USERPROFILE%\.tauri\mockingbird.key` (empty password —
re-encrypt before Phase 7).

---

## Section −1 resolution (carried forward)

| # | Item | Status | Resolution |
|---|------|--------|------------|
| 1 | Project name | ✅ | `Mockingbird` / `mockingbird`. |
| 2 | License | ✅ | MIT shipped this phase (`LICENSE`). |
| 3 | GitHub repo URL | 🟨 DEFERRED | Placeholder OK; resolve pre-Phase-7. |
| 4 | Code-signing cert | 🟨 DEFERRED | ADR 0005 (deferred to Phase 7). |
| 5 | Tauri updater key | ✅ | Generated bootstrap; embedded by Phase 1. |
| 6 | Cloud Claude model strings | 🟨 DEFERRED | Re-verify pre-Phase-4. |
| 7 | DBOS | ✅ DEFERRED | User confirmed. |
| 8 | `extra_models.json` rotation | 🟨 DEFERRED | Empty scaffold; decide pre-Phase-4. |
| 9 | Orchestration model | ✅ | ADR 0002 (no pack agents). |

---

## Blocked / human input needed

- **cmake** not installed → <https://cmake.org/download/>
- **CUDA Toolkit 12.x** (`nvcc`) → <https://developer.nvidia.com/cuda-downloads>
- **ollama** → <https://ollama.com/download>

Phase 0 and Phase 1 can proceed without these. **Phase 2 cannot.**
Install before kicking off `/phase2-goal`.

---

## Phase 1 — Foundation: IN PROGRESS (Wave 1 ✅)

Binding plan: `docs/phases/phase1.md` (planning-agent session 1b10a8, 25 tasks across 5 waves).

### Wave 1 — Decisions, scaffolding, prompt stubs ✅

| File | What it does |
|------|--------------|
| `docs/adr/0004-rusqlite-over-sqlx.md` | ADR: rusqlite (bundled) over sqlx; tauri-plugin-sql dropped |
| `Cargo.toml` (workspace) | Phase-1 deps pinned; `whisper-rs`/`cpal`/`ort`/`enigo` DEFERRED to Phase 2 |
| `src-tauri/Cargo.toml` | Member crate, `staticlib`+`cdylib`+`rlib`, Windows-only `windows` dep |
| `src-tauri/build.rs` | `tauri_build::build()` |
| `src-tauri/tauri.conf.json` | Main window (visible:false), tray, CSP allowing `localhost:11434` for Phase-4 ollama, updater configured (active:false until Phase 7) |
| `src-tauri/src/{main,lib,error}.rs` | Skeleton; `AppError` via thiserror; 2 unit tests pass |
| `src-tauri/src/cleanup/prompts/{normal,verbose,fragment}.md` | Phase-1 stubs (~200 words each, Phase 4 refines) |
| `docs/DATA_MODEL.md` | Reference copy of PLAN §7 |
| `.gitattributes` | Cross-platform line-ending pinning (LF for source, CRLF for .ps1) |

**Cargo quality gate green** (all four):
- `cargo check --workspace` ✅ (cold: 4m07s; rusqlite-bundled compiles SQLite from C)
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ (35s)
- `cargo test --workspace --quiet` ✅ (2/2 unit tests in `error.rs`)
- `cargo fmt --check` ✅ (after dropping `newline_style=Unix` in `.rustfmt.toml`; see LESSONS)

### Waves 2-5 — queued for next iteration

| Wave | Scope | Notes |
|------|-------|-------|
| 2 | Migrations 001/002/003 + runner + integration tests | **Migrations are SEALED forever once `phase-1-complete` tag lands** — wants a fresh full-context iteration. Delegated to `migration-author` project agent. |
| 3 | DB repository modules (7 files in `src-tauri/src/db/`) | Depends on Wave 2 runner. |
| 4 | App shell: logging, settings, tray, commands, app wire | Parallel after Wave 2 runner. |
| 5 | Docs flesh-out, lefthook live-fire verify, judges, seal + tag | Final iteration before phase-1-complete. |

### How to resume Phase 1 Wave 2 in a fresh session

1. `/agent code-puppy`
2. `/phase1-goal`
3. **Required reading for Wave 2** (in this order):
   1. `.code_puppy/AGENTS.md`
   2. `docs/phases/phase1.md` (phase plan)
   3. **`docs/phases/phase1-wave2-brief.md`** ← THIS IS BINDING for Wave 2
   4. `docs/LESSONS.md` (10 entries; search for `[phase-1]` and `migration`)
   5. `bd ready` (Wave 2 tasks `mb-4qg`, `mb-l6d`, `mb-7u9`, `mb-o0d`, `mb-rzf` are top)
4. **Implementation plan, codified in the Wave 2 brief**:
   - **DO NOT re-decide** audit-trigger column projections — the brief has the full SQL for all four `_history_*` tables, extrapolated from PLAN's dictionary-only example, with the dictionary `OLD.enabled` PLAN bug already worked around.
   - **DO NOT re-decide** the runner file layout — the brief specifies `db/mod.rs` + `db/migrations.rs` + `db/prompt_loader.rs` with function signatures.
   - **DO NOT re-decide** the integration-test set — the brief specifies 7 tests with exact assertion counts (trigger_count_is_14, history_prompts == 3 after seed, etc.).
   - **DO** invoke `migration-author` for the three .sql files and the integration tests; the runner is code-puppy's.
5. Wave 2 is **mechanical** at this point. Deviations from the brief require a LESSONS.md note explaining why.
6. **DO NOT tag `phase-1-complete` at end of Wave 2.** The tag seals migrations forever and lands at Wave 5 after DB repos + app shell + judges run.

---

## Judge-run notes

### Phase 1 Wave 1 (2026-05-15)

Mechanically verified (real LLM judges run at phase exit, not per-wave):

- **`build-passes`** (cargo gate): ✅ check + clippy + fmt + test all green.
- **`adr-recorded`**: ADR 0004 present, Status=Accepted, follows 0000-template.md schema.
- **`plan-aligned`** (partial): Cargo.toml deps match PLAN §5 minus the deferred CUDA-coupled crates (documented deviation).
- LLM-judged full pass: at end of Phase 1 Wave 5 per `docs/phases/phase1.md` §C.

### Phase 0 structural self-check (2026-05-15)

Real judges (`phase0-structure`, `adr-format`, `status-initialized`,
`setup-script-runs`) need a separate orchestrator pass that hands the
diff + STATUS.md to a model — not part of this iteration's tool budget.
Instead I verified mechanically:

- `phase0-structure`: dirs + `.code_puppy/` + `.agents/commands/` (16 cmds) all present.
- `agents-md-present`: unchanged from bootstrap.
- `hook-config-valid`: unchanged from bootstrap; 17/17 smoke tests green.
- `judges-seeded`: idempotent merge confirmed in setup-dev run.
- `adr-format`: every ADR file has Status/Context/Decision/Consequences sections.
- `status-initialized`: this file (you are reading it).
- `setup-script-runs`: `verify-environment.ps1` exits 0, `setup-dev.ps1` exits 0.

Full LLM-judged pass: will run on the post-Phase-1 iteration as part
of the regular `/goal` flow.

---

## Notes for the next agent (post context-clear)

1. Read this file first, then `docs/LESSONS.md` (10 entries now — search before
   doing PowerShell, rustfmt, beads, or hook work).
2. PLAN-mockingbird-v2.md and `.code_puppy/AGENTS.md` are binding.
3. `bd ready` shows the queue. Phase 1 Wave 1 is done; Wave 2 tasks
   (`mb-4qg`, `mb-l6d`, `mb-7u9`, `mb-o0d`, `mb-rzf`) are now ready.
4. Phase 1 plan is at `docs/phases/phase1.md`. Wave 2 = migrations,
   delegated to `migration-author` project agent.
5. **Migrations 001-003 are SEALED forever once `phase-1-complete`
   tag lands.** Hook `block-migration-edit-after-phase-1` enforces.
   Triple-check 001/002/003 before that commit + tag.
