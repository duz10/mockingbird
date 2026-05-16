# Mockingbird 🐦

> Local-first, system-wide voice dictation for Windows. A privacy-respecting,
> self-improving replacement for Wispr Flow.

![phase](https://img.shields.io/badge/phase-0%20groundwork-blue)
![build](https://img.shields.io/badge/build-pre--alpha-lightgrey)
![license](https://img.shields.io/badge/license-MIT-green)

> ⚠️ **Pre-alpha.** Phase 0 has just landed (repo scaffolding + CI hygiene).
> No runnable app yet. See [STATUS.md](./STATUS.md) for current phase.

## What it is

- **Local-first STT** via whisper.cpp (CUDA when available, CPU fallback).
- **Local-first cleanup LLM** via Ollama (cloud Claude as opt-in per mode).
- **Three modes**: Default, Email, Code, Casual — each with its own
  versioned prompt and few-shot example set.
- **Total provenance**: raw transcripts are immutable; every cleanup
  records the exact prompt version, dictionary snapshot, and example
  set used.
- **No telemetry. Ever.**

Platform support: **Windows 10/11 v1**, macOS as planned Phase 9.

## Quickstart

```pwsh
# 1. Clone, then run the dev setup
pwsh ./scripts/setup-dev.ps1

# 2. Read the binding docs
#    - PLAN-mockingbird-v2.md   (the spine)
#    - .code_puppy/AGENTS.md    (project rules)
#    - STATUS.md                (current phase / blocked-on)

# 3. See what's queued
bd ready
```

## Prereqs

Run `pwsh scripts/verify-environment.ps1` to check. Summary:

- **Required now**: rustc + cargo (≥1.77), node + npm (≥20), git, python,
  cargo-tauri, `bd` (beads), WebView2 runtime.
- **Phase 2+**: cmake, CUDA Toolkit (`nvcc`) — for whisper.cpp CUDA build.
- **Phase 4+**: ollama — for local cleanup LLM.

Install URLs for missing tools are surfaced by the script.

## Repo layout

```
.
├── PLAN-mockingbird-v2.md   # The spine. Read first.
├── STATUS.md                # Current phase snapshot.
├── CONTRIBUTING.md          # Workflow + iteration discipline.
├── CHANGELOG.md             # Keep-a-Changelog format.
├── LICENSE                  # MIT.
├── .code_puppy/             # AI coding agent config (binding).
│   ├── AGENTS.md            # Project rules.
│   ├── settings.json        # Hook engine.
│   ├── agents/              # Project JSON agents.
│   ├── skills/              # Domain skills.
│   └── README.md            # Tour of this directory.
├── .agents/commands/        # Slash command stubs for /goal workflow.
├── docs/
│   ├── adr/                 # Architecture decision records.
│   ├── phases/              # Per-phase binding plans.
│   ├── LESSONS.md           # Append-only non-obvious findings.
│   └── SETTINGS.md          # (Phase 1+) typed setting keys.
├── scripts/
│   ├── hooks/               # Code Puppy hook implementations.
│   ├── verify-environment.ps1
│   ├── setup-dev.ps1
│   ├── seed-judges.ps1
│   └── generate-icons.ps1
├── assets/icons/            # Source SVG.
├── src-tauri/icons/         # Generated icon set (Phase 0 placeholder).
└── (Phase 1+)
    ├── src-tauri/           # Rust + Tauri backend.
    └── ui/                  # React + Tailwind v4 frontend.
```

## How development works

This project is built by [Code Puppy](https://github.com/code-puppy) +
the human (Dustin). Iterations end **green** (fmt + clippy + tests +
STATUS.md updated). The hook engine in `.code_puppy/settings.json`
enforces the unbreakable rules — raw transcripts immutable, migrations
append-only after Phase 1, no `@tanstack/*`, no `npm install` without
`--ignore-scripts`, no secrets in commits.

Phases land one at a time with annotated git tags
(`bootstrap-complete`, `phase-0-complete`, `phase-1-complete`, …).
Each phase has a binding plan at `docs/phases/phase{N}.md`.

For the full workflow story, read
[`.code_puppy/AGENTS.md`](./.code_puppy/AGENTS.md) and
[`CONTRIBUTING.md`](./CONTRIBUTING.md).

## License

[MIT](./LICENSE) © 2026 Dustin Boyd.
