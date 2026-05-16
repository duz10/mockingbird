# Mockingbird — STATUS

**Current phase:** Phase 1 — Waves 1 + 2 ✅ landed; Waves 3-5 queued
**Last updated:** 2026-05-15 (Phase 1 Wave 2 iteration)
**Last successful judge run:** _Wave-2 cargo gate green (fmt/clippy/test/check) + 15/15 tests pass including 7/7 cross-crate integration tests, 2026-05-15_
**Cost line (cumulative):** _Track from first /goal run — bootstrap + Phase 0 + Phase 1 Waves 1+2 across two sessions; record when LLM judges run._

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

## Phase 1 — Foundation: IN PROGRESS (Waves 1 + 2 ✅; Waves 3-5 queued)

Binding plan: `docs/phases/phase1.md` (planning-agent session 1b10a8, 25 tasks across 5 waves).

### Wave 2 — Migrations + runner + integration tests ✅

| File | What it does | Lines |
|------|--------------|-------|
| `src-tauri/src/db/migrations/001_initial.sql` | Core tables + FTS5 per PLAN §7 verbatim (BEGIN/COMMIT, PRAGMA WAL+FK) | 174 |
| `src-tauri/src/db/migrations/002_audit_triggers.sql` | All 4 `_history_*` tables + **12 audit triggers** (4 tables × INSERT/UPDATE/DELETE) extrapolated per Wave 2 brief | 186 |
| `src-tauri/src/db/migrations/003_seed_modes.sql` | Seed 3 prompts + 3 modes with `__PROMPT_*_BODY__` tokens + `(SELECT id FROM prompts ...)` sub-selects | 37 |
| `src-tauri/src/db/mod.rs` | `Database::open(path)` + `::open_in_memory()` + `pub fn apply_migrations()` shim + PRAGMA gating + `integrity_check` + `foreign_key_check` | ~115 |
| `src-tauri/src/db/migrations.rs` | Runner with `include_str!` + `schema_version` idempotency + 3 inline unit tests | ~110 |
| `src-tauri/src/db/prompt_loader.rs` | Token substitution + SQL-quote escaping + 3 unit tests | ~80 |
| `src-tauri/tests/db_migrations.rs` | 7 integration tests (schema_version=3, tables present, **14 triggers**, seeded data with audit fired, audit UPDATE before/after, FTS5 round-trip, idempotency via the shim) | 188 |
| `src-tauri/src/lib.rs` | Wired `pub mod db;` + `.setup()` opens DB at `%APPDATA%/Mockingbird/mockingbird.db` | edit |
| `src-tauri/src/error.rs` | Added `Sqlite(#[from] rusqlite::Error)` variant | edit |

**Cargo quality gate all four green:**
- `cargo check --workspace` ✅ (warm 5.5s)
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ (15.7s)
- `cargo test --workspace` ✅ — **15/15** (5 unit + 3 unit + 7 cross-crate integration)
- `cargo fmt --check` ✅ (after auto-fmt)

**Delegation worked:** migration-author authored all 4 SQL/test files; code-puppy authored the runner + lib.rs wiring + error variant. Zero 5-attempt escalations. 15/15 tests pass first run.

### Wave 1 — Decisions, scaffolding, prompt stubs ✅ (commit `8e70d7c`)

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

### Waves 3-5 — queued

| Wave | Scope | bd tasks |
|------|-------|----------|
| 3 | DB repository modules: transcripts, search, sessions, prompts, dictionary, examples, audit | `mb-7oi mb-4f8 mb-9pn mb-91x mb-d5z mb-z4k mb-344` |
| 4 | App shell: logging (rotation + PII scrub), settings (typed facade), tray (placeholder menu), commands (#[tauri::command]s), app wire | `mb-uo1 mb-7si mb-yof mb-8og mb-nk5 mb-mpv` |
| 5 | Docs flesh-out, lefthook live-fire verify, end-of-phase judges, seal commit + `phase-1-complete` tag | `mb-65j mb-20w mb-6op mb-l07 mb-6ph mb-3pn mb-dhi` |

**Note:** migrations 001-003 are **NOT YET SEALED**. The tag
`phase-1-complete` lands at end of Wave 5 after all phase deliverables
are green and judges pass. Until then, fixes to 001-003 are permitted
(hook `block-migration-edit-after-phase-1` checks tag existence).

### How to resume Phase 1 Wave 3 in a fresh session

1. `/agent code-puppy`
2. `/phase1-goal`
3. **Required reading for Wave 3** (in this order):
   1. `.code_puppy/AGENTS.md`
   2. `docs/phases/phase1.md` (phase plan)
   3. **`docs/phases/phase1-wave3-brief.md`** ← if it exists, BINDING; if not, code-puppy should write one at start of iteration before authoring DB repos
   4. `docs/LESSONS.md` (now 14 entries; the brief-pattern lesson is the meta-lesson worth honoring)
   5. `bd ready` (Wave 3 tasks `mb-7oi mb-4f8 mb-9pn mb-91x mb-d5z mb-z4k mb-344` are top)
4. **What Wave 3 implements:** repository modules over the migrations Wave 2 sealed-in-spirit. Each module is a thin typed wrapper around `rusqlite::Connection` for one table family. **No `update_raw` method on transcripts** (hook scans for it). Mockall trait boundaries so unit tests don't need a real DB. `db::search.rs` is on the FTS5 smoke-test critical path.
5. **Wave 3 is NOT migrations**, so no SEAL concerns. But the brief-pattern from Wave 2 worked beautifully (15/15 tests first try) — apply it again: write `docs/phases/phase1-wave3-brief.md` end-of-Wave-3 OR start-of-Wave-3 with the function signatures and test specs for each repo module.
6. **DO NOT tag `phase-1-complete` at end of Wave 3.** Tag lands at Wave 5 after DB repos + app shell + judges run.

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
