# `.code_puppy/` — project configuration for Code Puppy

This directory is the durable, version-controlled config for the
Code Puppy AI coding agent running on this repo. It survives context
clears; agents read it at session start.

## Map

| Path                       | Purpose                                             |
|----------------------------|-----------------------------------------------------|
| `AGENTS.md`                | Project rules (binding). Read first every iteration. |
| `settings.json`            | Hook engine config — 9 hooks across 4 events.       |
| `judges-template.json`     | Per-project judges merged into `~/.code_puppy/judges.json` via `scripts/seed-judges.ps1`. |
| `extra_models.json`        | Round-robin LLM config (empty until pre-Phase-4).   |
| `agents/`                  | Project JSON agents (specialist sub-agents).        |
| `skills/`                  | Project-specific skills auto-activated by topic.    |

## Agents in `agents/`

| File                        | Specialty                                          |
|-----------------------------|----------------------------------------------------|
| `migration-author.json`     | SQLite migrations (Phase 1+).                      |
| `injection-author.json`     | Per-app text-injection recipes (Phase 3+).         |
| `ui-author.json`            | React 19 + Tailwind v4 (Phase 5+).                 |
| `prompt-tuner.json`         | Cleanup LLM prompt versioning (Phase 4+).          |
| `learning-loop-author.json` | Correction → few-shot pipeline (Phase 6+).         |

Invoke from another agent via `invoke_agent(agent_name=..., user_prompt=...)`.

## Skills in `skills/`

| Name              | When to activate                                   |
|-------------------|----------------------------------------------------|
| `data-model`      | Touching `src-tauri/src/db/` or any migration.     |
| `injection-recipes`| Touching `src-tauri/src/injection/`.              |
| `supply-chain`    | Adding/upgrading any npm or Cargo dependency.      |
| `quality`         | Writing/refactoring code or closing an iteration.  |
| `prompts`         | Editing cleanup-LLM prompts.                       |
| `design-tokens`   | Touching `ui/src/design/` or building UI.          |

Skills carry detailed do/don't lists scoped to the area. Auto-activated
by the framework based on file paths and intent; can also be activated
explicitly via `activate_skill(skill_name=...)`.

## Hooks (configured in `settings.json`)

Six block + two warn + one informational hook fire across:
- `PreToolUse` on edit/create/delete/replace tools
- `PreToolUse` on shell commands
- `PostToolUse` on shell commands
- `SessionStart`
- `Stop`

Hook scripts live in `scripts/hooks/` (not here — they're general
project infrastructure, not agent config).

## Judges

After every iteration, the framework runs the judges configured
in `~/.code_puppy/judges.json`. Run
`pwsh scripts/seed-judges.ps1` after pulling to make sure your local
machine has the latest project judges. The script is idempotent.

## What is NOT here

- Hook script implementations → `scripts/hooks/`
- The PLAN spine → `PLAN-mockingbird-v2.md` at repo root
- Per-iteration state → `STATUS.md` at repo root
- Lessons learned → `docs/LESSONS.md`
- Phase docs → `docs/phases/`
