# Architecture

A one-page subsystem map for developers considering a fork. For install
instructions see [`INSTALL.md`](./INSTALL.md). For the build setup see
[`CONTRIBUTING.md`](./CONTRIBUTING.md).

## Top-level shape

Mockingbird is a [Tauri 2](https://tauri.app/) desktop app: a Rust
backend hosting a [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/)
frontend written in React 19 + TypeScript. The Rust side owns audio,
STT, storage, the global hotkey hook, paste injection, and all
filesystem IO. The TS side owns the UI, settings forms, and overlay
windows. They talk over Tauri's `invoke` boundary using typed commands
defined in `src-tauri/src/commands/`.

```
+------------------------------------------------------------+
|  React 19 + TS + Tailwind v4 (ui/)                         |
|   Pages: App, Insights, History, Dictionary, Modes,        |
|          Meetings, Activity, KG, Settings, Command Center  |
|   Overlays: recording, meeting_overlay, command_center     |
+----------------------- invoke -----------------------------+
|  Rust backend (src-tauri/src/)                             |
|   audio | stt | cleanup | hotkey | dictation | injection   |
|   meetings | activity | kg | vault | inbox | secrets       |
|   settings | db | learning | window_context | commands     |
+----------------------- syscalls ---------------------------+
|  Windows: WASAPI | WH_KEYBOARD_LL | SendInput | DPAPI |    |
|           UI Automation | WebView2 | CUDA (optional)       |
+------------------------------------------------------------+
```

## Subsystems

Each section names the directory and the externally-visible behaviour.
Module-level doc comments explain the WHY in code.

### Capture

`src-tauri/src/audio/`

Microphone input via [`cpal`](https://github.com/RustAudio/cpal). System
audio capture via WASAPI loopback (Windows-specific) for meeting capture.
Native-format input is resampled through [`rubato`](https://github.com/HEnquist/rubato)
to the 16 kHz mono signal Whisper expects. Capture is a streaming pull
model: a thread feeds rolling chunks into a tokio channel consumed by
STT.

### Speech to text

`src-tauri/src/stt/`

[whisper.cpp](https://github.com/ggerganov/whisper.cpp) via the
[`whisper-rs`](https://github.com/tazz4843/whisper-rs) crate. CUDA
backend is compiled in via the `cuda` feature when the build wrapper
detects a CUDA Toolkit install; otherwise the binary falls back to CPU
inference. Voice activity detection runs in parallel using
[Silero VAD](https://github.com/snakers4/silero-vad) loaded as an ONNX
model via [`ort`](https://github.com/pykeio/ort). VAD output gates
Whisper invocations and trims leading/trailing silence.

For meeting capture, the audio stream is partitioned into 30-second
chunks with 2-second overlap, fed to Whisper with a rolling
`initial_prompt` carrying the previous chunk's tail. The merger uses
the overlap window to deduplicate transcript text deterministically (no
LLM in the critical path).

### Cleanup (optional)

`src-tauri/src/cleanup/`

Pluggable provider trait. Two providers ship today:

1. **Ollama** (local, default). HTTP to `http://localhost:11434`. Per-mode
   versioned prompts loaded from the database (migration 010 seeds the
   defaults; migrations 019-021 hold the current tuned versions).
2. **Anthropic Claude** (cloud, opt-in). Direct HTTPS to
   `api.anthropic.com`. API key encrypted via DPAPI.

Three dictation modes (`casual`, `normal`, `formal`) plus an on-demand
`Compress` transform. Each mode picks a provider, a model, a prompt
version, and a per-pass system header. A length-ratio shrink fallback
catches LLM runaway. Short utterances bypass cleanup entirely (latency
guard).

A fork wanting to swap in OpenAI, Gemini, llama.cpp, or any other LLM
implements the `CleanupProvider` trait, adds itself to the provider
enum, and wires a settings UI tab. No other subsystem needs changes.

### Storage

`src-tauri/src/db/`

SQLite via [`rusqlite`](https://github.com/rusqlite/rusqlite). Database
file lives at `%LOCALAPPDATA%\Mockingbird\mockingbird.db`. Migrations are
append-only Rust functions in `db/migrations/` numbered `001`..`NNN`.
Once a migration is in a tagged release it never changes; corrections
ship as a new migration that fixes forward.

The raw-transcript table (`transcripts(stage='raw')`) is immutable by
trigger; a `BEFORE UPDATE` trigger raises `SQLITE_CONSTRAINT` on any
attempt to mutate raw rows. This is one of the binding principles for
the project and is enforced at the SQL layer (not relying on
application discipline).

### Injection

`src-tauri/src/injection/`

The paste injector saves the existing clipboard contents (text and
common binary formats), writes the cleanup result to the clipboard,
issues a synthesized `Ctrl+V` via `SendInput`, waits for the target app
to consume it, then restores the original clipboard. A `SecureInputGuard`
queries the foreground field's UI Automation properties before injecting;
if the field reports `IsPassword` or otherwise looks like a credential
input, the paste is aborted and a toast surfaces.

### Hotkey

`src-tauri/src/hotkey/`

Global low-level keyboard hook via `SetWindowsHookExW(WH_KEYBOARD_LL, ...)`.
The hook runs on its own thread with a dedicated message pump.
Press-and-hold semantics for dictation; chord toggles (Right Ctrl + `.`)
for meeting capture. Both are reconfigurable from Settings.

### Dictation orchestrator

`src-tauri/src/dictation/`

State machine that wires hotkey events to capture start/stop, STT, the
cleanup pipeline, and the injector. Emits Tauri events to the recording
overlay window for the live VU meter and transcription status.

### Meeting capture

`src-tauri/src/meetings/`

Twin-stream capture (mic + WASAPI loopback). Per-channel chunked Whisper.
Deterministic two-channel merger. Optional ephemeral LLM summarization
that is never persisted (the summary is rendered into the overlay then
dropped). On-disk artefacts are session-scoped chunk WAVs (deleted on
finalize unless audio retention is enabled) plus the merged Markdown
transcript in SQLite. The merge path is deterministic and audited:
lossless stitching across the 2-second overlap, correct two-channel
interleaving, no LLM in the critical path, and the dictation pipeline
is left untouched by meeting capture (binding invariants of the
subsystem).

### Knowledge graph capture

`src-tauri/src/kg/` and `src-tauri/src/vault/`

A five-pass pipeline (segment, classify, extract, extract_entities,
normalize) turns a dictation tail or a hand-typed kg-note into a typed
knowledge entry with open-vocabulary tags and typed entity references.
Entries persist to SQLite (`kg_entities`, `kg_canonical_tags`,
`kg_entity_mentions`, `kg_tag_mentions`, `kg_filing_queue`) via the async
filing worker, which is crash-recoverable via a `pending`/`done`/`failed`
state machine.

The vault projector writes a deterministic Markdown subtree to
`<vault>/Knowledge Graph/` with `Entries/`, `Entities/`, `Projects/`,
`History/`, and `Inbox/` directories. Each generated file carries a
SHA-256 content hash for change detection. A reverse-watcher reconciles
user edits made directly in Obsidian back into SQLite using the file as
the source of truth on conflict.

The companion `SCHEMA.md` shipped into the vault documents the nine
knowledge shapes (`source`, `note`, `concept`, `entity`, `project`,
`question`, `decision`, `reference`, `observation`) and is the contract
a user's chat-LLM consumes when doing Ingest, Query, or Lint passes
against the vault.

### Activity capture

`src-tauri/src/activity/`

Sibling subsystem. Polls the foreground window title and a UI Automation
snapshot of the focused element on a configurable cadence. Snapshots
roll up into per-block summaries via the configured local LLM. Audio
co-capture is optional per block. An exclusion list lets users mark apps
that should never be captured. Output is a per-day PDF and a queryable
SQLite history.

### Secrets

`src-tauri/src/secrets/`

Anthropic and Unsplash API keys are encrypted via
[Windows DPAPI](https://learn.microsoft.com/en-us/windows/win32/seccrypto/cryptprotectdata)
(`CryptProtectData` / `CryptUnprotectData`) and stored as opaque blobs in
the database. The encryption key is derived from the Windows user
account, so secrets are not portable across machines or accounts.

### Inbox courier

`src-tauri/src/inbox/`

Watches a configurable Obsidian inbox folder for new audio drops
(typically dropped there by an iOS Shortcut). New files are transcribed
through the normal STT pipeline, classified, projected to the vault, and
the source file is moved to a processed subfolder.

### Settings store

`src-tauri/src/settings/`

Typed settings facade over the database. Settings groups (`dictation`,
`meetings`, `kg`, `appearance`, etc.) are versioned structs serialized
to JSON columns. The settings UI in `ui/src/pages/Settings` is a thin
form layer over Tauri commands that round-trip these structs.

## Cross-platform design

Mockingbird is Windows-only today. The architecture, however, isolates
every platform-specific concern behind a trait + `#[cfg(target_os)]`
implementation. The platform-specific surfaces are:

- Hotkey hook (Windows: `WH_KEYBOARD_LL`; macOS: `CGEventTap`; Linux: a
  display-server-specific path).
- Audio capture (`cpal` covers mic on all three; loopback is the
  hard one, see [`docs/research/macos-implementation-notes.md`](./docs/research/macos-implementation-notes.md)).
- STT acceleration (CUDA on Windows; Metal on macOS; ROCm or Vulkan on
  Linux).
- Secrets (DPAPI on Windows; Keychain on macOS; Secret Service on
  Linux).
- Paste injection (SendInput on Windows; Quartz event taps on macOS;
  XTest/uinput on Linux).
- Secure-input detection (UI Automation on Windows; AX API on macOS;
  toolkit-specific on Linux).

The `docs/research/macos-implementation-notes.md` document spells out
the deltas for a macOS port. The maintainer does not pursue macOS
distribution, but the door is wide open for forkers.

## What is intentionally NOT in this app

- Telemetry, analytics, or remote crash reporting. Crashes log to local
  files and stay there.
- Online accounts or sign-in. Mockingbird does not have an identity
  layer; it is per-Windows-user by virtue of DPAPI.
- Bundled Whisper or LLM models. Users supply their own (the download
  scripts pull from public sources at install time).
- A/B experimentation, remote feature flags, or any other server-driven
  configuration.
- A plugin or extension API. The codebase is the extension surface;
  fork it.
- An auto-update channel by default. The Tauri updater is wired but
  off in the beta. Users may opt in once the post-beta releases are
  signed.

## Acknowledgements

Built solo with substantial AI coding assistance.
