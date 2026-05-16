# Mockingbird — STATUS

**Current phase:** Bootstrap (Section 0.5)
**Last updated:** 2026-05-15
**Last successful judge run:** _(none yet — bootstrap is pre-judge)_
**Cost line (cumulative):** _(track from first /goal run)_

---

## Bootstrap iteration progress (Section 0.5, 17 steps)

| # | Step | Status | Notes |
|---|------|--------|-------|
| 1 | Confirm project name | ✅ | `Mockingbird` (display) / `mockingbird` (slug). PLAN.md title updated. |
| 2 | Verify Code Puppy env | ✅ | Agents: code-puppy, agent-creator, helios, planning-agent, qa-kitten. No pack agents (deprecated). |
| 3 | DBOS | ✅ DEFERRED | User explicitly deferred — not required for solo workflow. |
| 4 | Cloud LLM availability | 🟨 DEFERRED | Re-verify before Phase 4. Section 5 strings remain canonical. |
| 5 | Build prereqs | 🟨 PARTIAL | See **Blocked-on** below. |
| 6 | WebView2 runtime | ✅ | 148.0.3967.54. |
| 7 | `.code_puppy/AGENTS.md` | ✅ | From Appendix A + bd integration appended. |
| 8 | `.code_puppy/settings.json` | ✅ | 9 hooks across PreToolUse/PostToolUse/SessionStart/Stop. |
| 9 | Hook scripts | ✅ | 9 scripts + shared `_lib.py` + smoke test green (17/17). |
| 10 | Mint 5 project JSON agents | ✅ | Via agent-creator: migration-author, injection-author, ui-author, prompt-tuner, learning-loop-author. |
| 11 | 6 project skills | ✅ | data-model, injection-recipes, supply-chain, quality, prompts, design-tokens. |
| 12 | Seed judges | ✅ | `.code_puppy/judges-template.json` + `scripts/seed-judges.ps1` (idempotent, verified). |
| 13 | Tauri updater key pair | ✅ | At `~/.tauri/mockingbird.key{,.pub}`. **BACK UP THE PRIVATE KEY.** Public key recorded below. |
| 14 | Confirm Section −1 items | ✅ | See **Section −1 resolution** below. |
| 15 | Initialize STATUS.md | ✅ | This file. |
| 16 | Initial commit + tag | ✅ | Commit `80c14d8`, tag `bootstrap-complete`. |
| 17 | Hand off summary | ✅ | Surfaced to Dustin in chat. Bootstrap iteration sealed.

---

## Tauri updater public key (for Phase 0 `tauri.conf.json`)

```
dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEQ5N0E1MTkzODYzNTBGQTEKUldTaER6V0drMUY2MlNiS2g5anF0Vjl6UEkyODRQTlZlS0FMRjNuNWcvdEpJUC9RRG1QVm5Ja04K
```

The private key lives at `%USERPROFILE%\.tauri\mockingbird.key` and is
**unencrypted (empty password)** so the build flow doesn't need a
password vault yet. Re-encrypt before Phase 7.

---

## Section −1 resolution

| # | Item | Status | Resolution |
|---|------|--------|------------|
| 1 | Project name | ✅ | `Mockingbird` / `mockingbird`. |
| 2 | License | ✅ | MIT (PLAN default). Adds `LICENSE` in Phase 0. |
| 3 | GitHub repo URL | 🟨 DEFERRED | Placeholder `TODO(repo-url)` in tauri.conf.json + README. Resolve before Phase 7. |
| 4 | Code-signing cert | 🟨 DEFERRED | Phase 7 + ADR 0005. |
| 5 | Tauri updater key | ✅ | Generated this iteration (step 13). |
| 6 | Cloud Claude model strings | 🟨 DEFERRED | Re-verify before Phase 4. |
| 7 | DBOS | ✅ DEFERRED | User confirmed. |
| 8 | `extra_models.json` rotation | 🟨 DEFERRED | Empty scaffold; decide before Phase 4. Default = single Anthropic key for Phases 0–3. |
| 9 | Orchestration model | ✅ | No pack agents (deprecated). code-puppy is the orchestrator. |

---

## Blocked / human input needed

- **cmake** not installed. Needed for Phase 2 (whisper.cpp build).
  Install: <https://cmake.org/download/>
- **nvcc / CUDA Toolkit 12.x** not installed. Needed for Phase 2 GPU
  acceleration. Install: <https://developer.nvidia.com/cuda-downloads>
- **ollama** not installed. Needed for Phase 4 (local cleanup LLM).
  Install: <https://ollama.com/download>

All three are pre-Phase-2/4 install tasks; bootstrap proceeds without
them. Action: install before kicking off Phase 2.

---

## Phase 0 (next iteration)

Not yet decomposed. Will be planned via `/agent planning-agent` →
`/plan-phase 0` per kickoff instructions. After bootstrap commit lands,
the human will run that flow and then `/agent code-puppy` →
`/phase0-goal`.

---

## Notes for the next agent (post context-clear)

1. Read this file first.
2. Read `docs/LESSONS.md` — there are 5 entries from bootstrap that will
   save you time.
3. Run `bd ready` to see what's queued. Bootstrap leaves only the
   commit+tag (mb-6c6) and handoff (mb-b6g) open; everything else closed.
4. `bd prime` will give you the workflow brief.
5. PLAN-mockingbird-v2.md and `.code_puppy/AGENTS.md` are the binding
   docs. Read them before doing anything.
