# Mockingbird

Local-first voice dictation, meeting capture, and a personal knowledge engine. Everything runs on your machine. Zero telemetry.

**Platforms.** Windows 10/11 has a downloadable installer (below). **macOS 15+ on Apple Silicon** runs voice dictation and meeting capture (both with local cleanup) at parity — it's a from-source build, no installer yet; see [macOS install](./INSTALL.md#macos-apple-silicon-source-build). Activity capture, the Knowledge Graph, and Mobile Sync are Windows-only for now on the Mac build.

![release](https://img.shields.io/badge/release-v0.2.0--beta.2-blue)
![platform](https://img.shields.io/badge/platform-Windows%2010%2F11%20%7C%20macOS%2015%2B%20(Apple%20Silicon)-lightgrey)
![license](https://img.shields.io/badge/license-MIT-green)

## Maintainer Statement

This is a reference implementation. I built it for my own daily use and published it freely. Pull requests are welcomed but not prioritized. If you want a feature or fix that I haven't shipped, the best path is to fork the project and build it yourself. The architecture is documented and the code is MIT licensed. I reserve the right to archive this repository (read-only, forks remain free) if maintenance burden exceeds my capacity.

## What it does

Mockingbird is three capabilities in one app.

1. **Voice dictation.** Push-to-talk via a global hotkey (Right Alt by default). Whisper transcribes locally; an optional local LLM tidies the result; the text is pasted into whichever app has focus, with your previous clipboard contents preserved. Three modes (casual, normal, formal) let you pick how much cleanup you want.
2. **Meeting capture.** Chord-toggled long-form recording. Microphone and system audio are captured in parallel, transcribed by Whisper in rolling 30-second windows, and merged into a two-speaker Markdown transcript. Optional ephemeral LLM summarization is supported and never persisted.
3. **Personal knowledge engine capture.** Fast-note workflow that projects captures to an Obsidian vault as wiki-linked Markdown, with auto-generated entity and tag pages. Your chat-LLM of choice (Claude Code, Cursor, etc.) reads the vault and helps you maintain it.

Everything stays on your machine unless you explicitly opt in to a cloud surface (Claude API for cleanup, or Unsplash for ambient backgrounds). The app has no analytics, no crash reporting, no "phone home" of any kind.

### Mobile capture

Capture quick text or voice notes from your iPhone and have them land in Mockingbird on your PC automatically. The flow uses any sync provider that mirrors a folder between your phone and your desktop (iCloud Drive, OneDrive, Dropbox, Google Drive, etc.). You drop a note in the synced inbox folder from an iOS Shortcut, the sync provider mirrors it to your PC, and Mockingbird's vault watcher routes it into the knowledge graph (or, for audio, transcribes it through the dictation pipeline). See [`docs/mobile/`](./docs/mobile/) for the iOS Shortcut recipes for both routes.

## Quick install

The fastest path:

1. Open the [Releases](../../releases) page and pick the MSI for your hardware:
   - `Mockingbird-Setup-x.y.z.msi` (about 50 MB download, about 80 MB installed) for CPU-only transcription on any 64-bit Windows machine.
   - `Mockingbird-CUDA-Setup-x.y.z.msi` (about 250 MB download, about 770 MB installed) for fast NVIDIA GPU transcription. Bundles the CUDA runtime libraries so no separate Toolkit install is needed; only an NVIDIA driver is required and that ships with every NVIDIA GPU.
2. Run it. On first launch SmartScreen may warn that the app is unsigned. Click "More info" then "Run anyway".
3. The first launch downloads a Whisper model (about 500 MB to 2 GB depending on which variant you pick in Settings).
4. Press Right Alt and start talking. Release to paste into the focused app.

See [`INSTALL.md`](./INSTALL.md) for the standard and from-source paths, including the optional Ollama integration for local LLM cleanup, and [`PREREQS.md`](./PREREQS.md) for the full hardware and OS requirements. **On macOS?** Follow the [macOS (Apple Silicon, source build)](./INSTALL.md#macos-apple-silicon-source-build) walkthrough instead — the Quick install above is Windows-only.

## How it works (high level)

- **Speech to text.** [whisper.cpp](https://github.com/ggerganov/whisper.cpp) via the `whisper-rs` crate. CUDA acceleration when an NVIDIA GPU is available; CPU fallback otherwise.
- **Voice activity detection.** Silero VAD running on ONNX Runtime, used to gate Whisper and trim leading/trailing silence.
- **Optional cleanup.** Pluggable provider. [Ollama](https://ollama.com/) running locally (default and recommended) or the Anthropic Claude API (opt-in, BYO key, stored via Windows DPAPI / macOS Keychain).
- **Storage.** SQLite under `%LOCALAPPDATA%\Mockingbird\`. Migrations are append-only.
- **Shell.** [Tauri 2](https://tauri.app/) with a React 19 + TypeScript frontend in WebView2.

## Build your own / fork it

The architecture is intentionally pluggable: the cleanup LLM, the STT engine, and each capture surface are isolated behind traits or modules. Common forking targets:

- Swap the local LLM for `llama.cpp`, OpenAI, Gemini, or anything else.
- Swap Whisper for Groq, Deepgram, faster-whisper, etc.
- Add a new capture surface (clipboard history, focus sessions, etc.).
- Port to macOS (see [`docs/research/macos-implementation-notes.md`](./docs/research/macos-implementation-notes.md)).
- Port to Linux (no research doc yet, but the cross-platform abstraction pattern is in place).

Start with [`ARCHITECTURE.md`](./ARCHITECTURE.md) for the subsystem map, then [`CONTRIBUTING.md`](./CONTRIBUTING.md) for build instructions.

## Repo layout

```
.
├── README.md
├── INSTALL.md                # 3-tier install walkthrough
├── PREREQS.md                # system requirements
├── ARCHITECTURE.md           # subsystem map for forkers
├── SECURITY.md               # vulnerability reporting + security model
├── PRIVACY.md                # data flow guarantees
├── CONTRIBUTING.md           # fork guide
├── CODE_OF_CONDUCT.md        # Contributor Covenant 2.1
├── CHANGELOG.md
├── LICENSE                   # MIT
├── Cargo.toml / Cargo.lock   # workspace manifest
├── lefthook.yml              # git hooks config
├── .env.example
├── assets/
│   └── icons/                # source SVG
├── docs/
│   ├── mobile/               # iOS Shortcut recipes
│   └── research/             # forker enablement notes (macOS port, etc.)
├── scripts/
│   ├── run-mockingbird.ps1   # launches the app with the right env
│   ├── setup-dev.ps1         # one-shot dev environment setup
│   ├── verify-environment.ps1
│   ├── download-models.ps1   # fetches the Whisper GGUF
│   ├── download-onnxruntime.ps1
│   ├── model-manifest.json
│   └── dev/                  # maintainer-only tooling
├── src-tauri/                # Rust backend
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── build.rs
│   ├── icons/
│   ├── capabilities/
│   ├── benches/
│   ├── examples/
│   ├── tests/
│   └── src/
│       ├── audio/            # mic capture + WASAPI loopback
│       ├── stt/              # whisper-rs + Silero VAD
│       ├── cleanup/          # optional LLM cleanup providers
│       ├── hotkey/           # global hotkey hook
│       ├── dictation/        # dictation state machine
│       ├── injection/        # clipboard save/restore + paste
│       ├── meetings/         # two-channel capture + merge
│       ├── activity/         # foreground polling + summarization
│       ├── kg/               # knowledge graph pipeline
│       ├── vault/            # Obsidian vault projection
│       ├── inbox/            # mobile capture courier
│       ├── secrets/          # DPAPI-wrapped key storage
│       ├── settings/         # typed settings store
│       ├── db/               # SQLite migrations + repos
│       ├── learning/         # empirical prompt tuning loop
│       ├── command_center/
│       ├── window_context/
│       ├── commands/         # Tauri command handlers
│       └── bin/              # support binaries
└── ui/                       # React 19 + TypeScript + Tailwind v4
    ├── package.json
    ├── vite.config.ts
    ├── playwright.config.ts
    ├── public/fonts/
    ├── tests/                # Playwright specs
    └── src/
        ├── App.tsx
        ├── main.tsx
        ├── pages/            # main app pages
        ├── components/       # shared components
        ├── design/           # tokens + primitives
        ├── i18n/
        ├── lib/
        ├── routes/
        ├── command_center/
        ├── meeting_overlay/
        └── recording/        # recording overlay
```

## Tech stack

- [Tauri 2](https://tauri.app/) (Rust backend + WebView2 frontend)
- Rust (edition 2021, MSRV 1.77)
- React 19 + TypeScript (strict mode) + Tailwind v4
- SQLite via `rusqlite`
- [whisper.cpp](https://github.com/ggerganov/whisper.cpp) via [`whisper-rs`](https://github.com/tazz4843/whisper-rs)
- [Silero VAD](https://github.com/snakers4/silero-vad) via [ONNX Runtime](https://onnxruntime.ai/)
- Optional: [Ollama](https://ollama.com/) for local cleanup
- Optional: Anthropic Claude API for cloud cleanup

## License

[MIT](./LICENSE). Copyright (c) 2026 Dustin Boyd.

## Acknowledgements

This project would not exist without:

- [whisper.cpp](https://github.com/ggerganov/whisper.cpp) and the Whisper model family from OpenAI.
- [Ollama](https://ollama.com/) and the open-weight LLMs it makes easy to run locally.
- [Silero VAD](https://github.com/snakers4/silero-vad).
- [Tauri](https://tauri.app/), [Vite](https://vitejs.dev/), and the broader Rust and React ecosystems.
- [Obsidian](https://obsidian.md/) for inspiring the vault-as-knowledge-codebase pattern.
