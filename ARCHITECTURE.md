# Architecture

A one-page subsystem map for developers considering a fork. For install
instructions see [`INSTALL.md`](./INSTALL.md). For the build setup see
[`CONTRIBUTING.md`](./CONTRIBUTING.md).

## Top-level shape

Mockingbird is a [Tauri 2](https://tauri.app/) desktop app: a Rust
backend hosting a system-webview frontend written in React 19 +
TypeScript ([WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/)
on Windows, WKWebView on macOS). The Rust side owns audio,
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
|  macOS:   CoreAudio | ScreenCaptureKit | CGEventTap |      |
|           AX API | Keychain | WKWebView | Metal            |
+------------------------------------------------------------+
```

## Subsystems

Each section names the directory and the externally-visible behaviour.
Module-level doc comments explain the WHY in code.

### Capture

`src-tauri/src/audio/`

Microphone input via [`cpal`](https://github.com/RustAudio/cpal). System
audio capture for meeting capture is per-platform: WASAPI loopback on
Windows, ScreenCaptureKit on macOS (`meetings/sck_macos.rs`).
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
   ADR-0024 defaults; later migrations up through 030 carry the tuned
   versions, the per-mode model overrides, and the user prompt
   overrides).
2. **Anthropic Claude** (cloud, opt-in). Direct HTTPS to
   `api.anthropic.com`. API key held in the platform secret store (see
   Secrets below).

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

SQLite via [`rusqlite`](https://github.com/rusqlite/rusqlite). The
database file lives under the per-OS app-data root resolved by
`resolve_app_data_dir`: `%APPDATA%\com.dustin.mockingbird\mockingbird.db`
on Windows and `~/Library/Application Support/com.dustin.mockingbird/mockingbird.db`
on macOS. Migrations are append-only `.sql` files in
`db/migrations/` numbered `001`..`NNN`, embedded into the binary with
`include_str!` and driven by `db/migrations.rs`. Once a migration is in
a tagged release it never changes; corrections ship as a new migration
that fixes forward, and the `block-migration-edit-after-phase-1` hook
enforces that.

The raw-transcript table (`transcripts(stage='raw')`) is immutable.
New facts about an utterance go into a new row; a raw row is never
UPDATEd. This is one of the binding principles for the project, and it
is a real invariant: the `block-raw-transcript-edit` hook in
`scripts/dev/hooks/` scans non-test code for SQL that UPDATEs
`transcripts` at the raw stage and refuses the write, and the
application code never issues such a statement.

What does not exist today is a SQL-layer guard. The header of
`001_initial.sql` says as much: raw-transcript immutability is
enforced by the hook engine rather than by a trigger in that file, and
a belt-and-suspenders trigger was explicitly deferred to a future
migration. The only triggers on `transcripts` are the FTS sync pair
(`transcripts_fts_insert` and `transcripts_fts_delete`).

The pattern is available and already used elsewhere in this schema:
`activity_events_no_update` and `activity_events_no_delete`
(`012_activity_capture.sql`), plus `kg_entity_mentions_no_update` and
`kg_tag_mentions_no_update` (`024_kg_phase_1b.sql`), all abort
mutation at the SQL layer. Adding the equivalent `BEFORE UPDATE`
trigger for raw transcripts in a new migration is a reasonable
hardening step for a fork.

### Injection

`src-tauri/src/injection/`

The paste injector saves the existing clipboard contents (text and
common binary formats), writes the cleanup result to the clipboard,
synthesizes a paste keystroke, waits for the target app to consume it,
then restores the original clipboard. The keystroke is `Ctrl+V` via
`SendInput` on Windows and `Cmd+V` via `CGEvent` on macOS, behind a
common strategy trait. A `SecureInputGuard` queries the foreground
field before injecting (UI Automation on Windows, the AX API on
macOS); if the field reports as a password or otherwise looks like a
credential input, the paste is aborted and a toast surfaces.

### Hotkey

`src-tauri/src/hotkey/`

Global low-level keyboard hook behind a `HotkeyListener` trait:
`SetWindowsHookExW(WH_KEYBOARD_LL, ...)` on Windows, `CGEventTap` on
macOS (which requires the Input Monitoring permission). The listener
runs on its own thread with a dedicated run loop or message pump.
Press-and-hold semantics for dictation; on Windows a chord toggle
(Right Ctrl + `.`) starts meeting capture, while on macOS meeting
capture is driven from the UI and Command Center rather than a chord.
Both are reconfigurable from Settings.

### Dictation orchestrator

`src-tauri/src/dictation/`

State machine that wires hotkey events to capture start/stop, STT, the
cleanup pipeline, and the injector. Emits Tauri events to the recording
overlay window for the live VU meter and transcription status.

### Meeting capture

`src-tauri/src/meetings/`

Twin-stream capture (mic + system audio: WASAPI loopback on Windows,
ScreenCaptureKit on macOS). Per-channel chunked Whisper. Deterministic
two-channel merger. Optional ephemeral LLM summarization
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

Windows-only today. A five-pass pipeline (`kg/passes/`: segment,
classify, extract, extract_entities, normalize) turns a dictation tail
or a hand-typed kg-note into a typed
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

Sibling subsystem, Windows-only today. Polls the foreground window
title and a UI Automation snapshot of the focused element on a
configurable cadence. Snapshots
roll up into per-block summaries via the configured local LLM. Audio
co-capture is optional per block. An exclusion list lets users mark apps
that should never be captured. Output is a per-day PDF and a queryable
SQLite history.

### Secrets

`src-tauri/src/secrets/`

Anthropic and Unsplash API keys go through a `SecretStore` trait with a
per-OS backend. On Windows they are encrypted via
[DPAPI](https://learn.microsoft.com/en-us/windows/win32/seccrypto/cryptprotectdata)
(`CryptProtectData` / `CryptUnprotectData`) and written as opaque
`.dpapi` blobs under `%LOCALAPPDATA%\Mockingbird\secrets\`. On macOS
they live in the login Keychain via `security-framework`. Either way
the key material is bound to the OS user account, so secrets are not
portable across machines or accounts.

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
to JSON columns. The settings UI (`ui/src/pages/Settings.tsx` plus its
per-tab `Settings*Tab.tsx` siblings) is a thin form layer over Tauri
commands that round-trip these structs.

## Cross-platform design

Mockingbird ships on Windows and on macOS 15+ (Apple Silicon). macOS
landed in 0.3.0-beta.1 and is installable from a Homebrew cask as of
0.3.0-beta.3. Dictation and meeting capture are at parity across both
platforms. Activity capture, the Knowledge Graph pipeline, and Mobile
Sync are Windows-only for now.

The architecture isolates every platform-specific concern behind a
trait + `#[cfg(target_os)]` implementation, and the macOS port is the
proof that the seam held: the platform surfaces below each grew a
second backend without the orchestrators above them changing. The
platform-specific surfaces are:

- Hotkey hook (Windows: `WH_KEYBOARD_LL`; macOS: `CGEventTap`; Linux: a
  display-server-specific path).
- Audio capture (`cpal` covers mic on all three; loopback is the hard
  one: WASAPI loopback on Windows, ScreenCaptureKit on macOS, and an
  open question on Linux. See
  [`docs/research/macos-implementation-notes.md`](./docs/research/macos-implementation-notes.md)).
- STT acceleration (CUDA on Windows; Metal on macOS; ROCm or Vulkan on
  Linux).
- Secrets (DPAPI on Windows; Keychain on macOS; Secret Service on
  Linux).
- Paste injection (SendInput on Windows; Quartz event taps on macOS;
  XTest/uinput on Linux).
- Secure-input detection (UI Automation on Windows; AX API on macOS;
  toolkit-specific on Linux).

The `docs/research/macos-implementation-notes.md` document records the
deltas worked through during the macOS port. No Linux build is
pursued, but the same seam is where a forker would start.

## What is intentionally NOT in this app

- Telemetry, analytics, or remote crash reporting. Crashes log to local
  files and stay there.
- Online accounts or sign-in. Mockingbird does not have an identity
  layer; it is scoped to the logged-in OS user by virtue of DPAPI on
  Windows and the login Keychain on macOS.
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
