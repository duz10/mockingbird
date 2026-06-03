# Mockingbird 

> Local-first voice dictation, meeting capture, and a Personal Knowledge
> Engine capture substrate for Windows. Privacy-respecting,
> self-improving, fully on-device.

![release](https://img.shields.io/badge/release-v0.2.0--beta.1-blue)
![platform](https://img.shields.io/badge/platform-Windows%2010%2F11-lightgrey)
![license](https://img.shields.io/badge/license-MIT-green)

>  **Public beta** (`v0.2.0-beta.1`, 2026-06-08). Dictation, Meeting
> Capture, Activity Capture, and the Knowledge Graph capture + vault
> projection substrate are all sealed and shipping. macOS support is
> planned for Phase 9. See [`CHANGELOG.md`](./CHANGELOG.md) for the full
> what-shipped breakdown and [`STATUS.md`](./STATUS.md) for current
> in-flight state.

## What it is

Mockingbird is three things in one binary, layered:

1. **Voice dictation** — push-to-talk via a global hotkey (Right Alt).
   Whisper (CUDA when present, CPU fallback) transcribes; an optional
   local LLM (Ollama) cleans; the result is pasted into the focused
   app via clipboard save-and-restore. Three modes (casual / normal /
   formal), each with versioned prompts and few-shot example sets.
   Secure-input fields abort injection with a toast.

2. **Meeting capture** — chord-toggled long-form recording
   (Right Ctrl + `.`). Captures mic + WASAPI system loopback in
   parallel, chunks both into 30s / 2s-overlap WAVs, transcribes with
   rolling-prompt long-form Whisper, merges into a two-speaker
   Markdown transcript via a deterministic formatter, optionally
   summarizes with an ephemeral LLM pass (not persisted).

3. **Personal Knowledge Engine capture substrate** — captures (audio
   + text notes) flow through a five-pass pipeline (segment → classify
   → extract → extract_entities → normalize) and are projected to an
   Obsidian vault as wiki-linked Markdown with auto-generated Entity /
   Project / Tag stub pages, an `INDEX.md` + `LOG.md` + `Tags/`
   subtree, and a `SCHEMA.md` contract.

The split is intentional and load-bearing (see ADR 0054 and LESSONS
PINNED P14 — the Karpathy/Clark pattern):

- **Mockingbird** is the **capture + first-pass synthesis layer**.
- **The user's chat-LLM** (Claude Code / Cursor / OpenCode / etc.) is
  the **wiki author/maintainer** — it reads `SCHEMA.md` and performs
  Ingest / Query / Lint over the vault.
- **The Obsidian vault** is the **knowledge codebase**.

Mockingbird never speaks to a cloud unless you explicitly opt in
(Unsplash backgrounds or a cloud cleanup LLM). **Zero telemetry. Ever.**

## Prereqs

Run `powershell -File scripts\verify-environment.ps1` to check, or
read the summary:

- **Always required**: Windows 10 / 11, rustc + cargo (≥ 1.77), node +
  npm (≥ 20), git, cargo-tauri, `bd` (beads), WebView2 runtime.
- **For STT**: CMake + CUDA Toolkit 12.8 (`nvcc`) for the CUDA build
  path; CPU-only Whisper works without CUDA but is slower.
- **For local cleanup + KG**: Ollama with `qwen2.5:7b-instruct-q4_K_M`
  pulled (~4.7 GB). `qwen2.5:3b` is a documented tags-only degraded
  mode.

Install URLs for missing tools are surfaced by the verify script.

## Quickstart

```pwsh
# 1. Clone, then run the dev setup
powershell -File scripts\setup-dev.ps1

# 2. Restore the runtime model dir (~12 MB ORT + ~500 MB-2 GB Whisper)
powershell -File scripts\download-onnxruntime.ps1
powershell -File scripts\download-models.ps1

# 3. Run the app (always via the launcher — sets ORT_DYLIB_PATH +
#    prepends CUDA bin to PATH so onnxruntime.dll and cudart64_12.dll
#    load at process start)
powershell -File scripts\run-mockingbird.ps1
```

Runtime model home: `%USERPROFILE%\mockingbird_models\`. Override the
parent via the `MOCKINGBIRD_MODELS_DIR` env var
(`mockingbird_models` is appended).

For the Karpathy/Clark vault flow: point Mockingbird's Settings →
Knowledge Graph → Vault at an Obsidian vault folder, enable
`KgGraphEnabled`, capture a note via the KG capture surface, then let
your chat-LLM read `<vault>/Knowledge Graph/SCHEMA.md` for the
Ingest / Query / Lint contract.

## Repo layout

```
.
├── PLAN-mockingbird-v2.md   # The spine. Re-read only when phase doc silent.
├── STATUS.md                # Current phase + sealed table (slim).
├── CHANGELOG.md             # Keep-a-Changelog format.
├── CONTRIBUTING.md          # Workflow + iteration discipline.
├── LICENSE                  # MIT.
├── .code_puppy/
│   ├── AGENTS.md            # Project rules.
│   ├── settings.json        # Hook engine.
│   ├── agents/              # Project JSON agents.
│   ├── skills/              # Domain skills.
│   └── README.md            # Tour of this directory.
├── docs/
│   ├── PRODUCT-STATE.md     # Durable "what does it do today" reference.
│   ├── adr/                 # Architecture decision records.
│   ├── phases/              # Per-phase binding plans.
│   ├── knowledge-graph/     # KG phase briefs + parity fixtures.
│   ├── judges/              # Per-phase invariant judge specs.
│   ├── mobile/              # iOS Shortcut recipes (Inbox + KG-Inbox).
│   └── LESSONS.md           # Append-only non-obvious findings.
├── scripts/
│   ├── cargo-with-cuda.ps1  # All cargo invocations go through here.
│   ├── run-mockingbird.ps1  # The right way to launch the app.
│   ├── hooks/               # Code Puppy hook implementations.
│   ├── verify-environment.ps1
│   ├── setup-dev.ps1
│   ├── download-onnxruntime.ps1
│   └── download-models.ps1
├── src-tauri/               # Rust + Tauri 2 backend.
│   └── src/
│       ├── audio/ stt/ cleanup/ hotkey/ dictation/
│       ├── injection/ meetings/ activity/ vault/ inbox/
│       └── kg/              # Knowledge Graph library.
├── ui/                      # React 19 + TypeScript + Tailwind v4 frontend.
└── experimental/
    └── kg-validation/       # KG Phase 0/0.5 sandbox (sealed).
```

## How development works

This project is built by [Code Puppy](https://github.com/code-puppy) +
the human (Dustin). Iterations end **green** (fmt + clippy + tests +
gates + STATUS.md updated). The hook engine in
`.code_puppy/settings.json` enforces the unbreakable rules — raw
transcripts immutable, migrations append-only after `phase-1-complete`,
no `@tanstack/*`, no `npm install` without `--ignore-scripts`, no
secrets in commits.

PLAN §10 phases land one at a time with annotated git tags
(`phase-0-complete`, `phase-1-complete`, … `phase-mc-complete`,
`phase-10-complete`). Lateral epics (ADR-chartered) seal via an
Accepted ADR + a STATUS.md "Sealed" row and do **not** carry their
own `phase-*-complete` tag (LESSONS PINNED P5).

Release tags carry the conventional `v<semver>` prefix. The first
publicly-tagged build is `v0.2.0-beta.1`.

For the full workflow story, read
[`.code_puppy/AGENTS.md`](./.code_puppy/AGENTS.md) and
[`CONTRIBUTING.md`](./CONTRIBUTING.md).

## License

[MIT](./LICENSE) © 2026 Dustin Boyd.
