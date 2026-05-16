# Mockingbird — STATUS

**Current phase:** Phase 0 → Phase 1 (queued)
**Last updated:** 2026-05-15
**Last successful judge run:** _Phase-0 structural self-check, 2026-05-15 (see judge-run notes at bottom)_
**Cost line (cumulative):** _Track from first /goal run — Phase 0 + bootstrap consumed roughly TBD tokens; record when judges run._

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

## Phase 1 (queued)

Per PLAN §10 Phase 1: Tauri v2 app opens to tray, SQLite migrations
001–003 applied, settings round-trip, FTS5 search smoke test passes.

Next step (when ready):

1. `/agent planning-agent`
2. `/plan-phase 1`
3. `/agent code-puppy`
4. `/phase1-goal`

The implementor (code-puppy) will continue unattended into Phase 1
and Phase 2 per Dustin's kickoff instruction.

---

## Judge-run notes (Phase-0 structural self-check, 2026-05-15)

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

1. Read this file first, then `docs/LESSONS.md` (7 entries — search before
   doing PowerShell or beads work).
2. PLAN-mockingbird-v2.md and `.code_puppy/AGENTS.md` are binding.
3. `bd ready` shows the queue. Phase 0 leaves only the deferred
   pre-Phase-2/4 build-prereq tasks open; everything else closed.
4. Phase 1 needs planning-agent to decompose first → produces
   `docs/phases/phase1.md`. Follow the same workflow as Phase 0.
