# Changelog

All notable changes to Mockingbird are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this project
follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

_Nothing yet._

## [0.2.0-beta.2] - 2026-06-16

Second beta iteration on the v0.2.0 line. Dictionary UI improvements
plus a CI infrastructure fix. No data migrations, no schema changes;
upgrade is a straight installer-over-installer replacement.

### Fixed

- **Dictionary: "Add term" UI is now reachable when the dictionary is
  empty.** Previously the empty-state card replaced the add form
  entirely, so first-time users had no path to create their first
  entry without seeding it from somewhere else.

### Added

- **Dictionary: add a term directly from any dictation.** A new
  "Add term to dictionary" action surfaces on every dictation; the
  modal pre-fills app context from the source dictation so the new
  entry's scope is set without manual hunting.
- **Dictionary: entries now group by canonical term, with misspelling
  variants shown as chips.** Editing a group lets you add, remove, or
  rename variants in one place instead of managing each spelling as a
  standalone row.

### Infrastructure

- **CI: pinned the Windows runner to `windows-2022`.** GitHub rolled
  `windows-latest` forward to ship Visual Studio 18 (2026), which
  collided with the cached `target/` from earlier Visual Studio 17
  (2022) builds and broke the `whisper-rs-sys` CMake step. Pinning the
  runner matches the dev box's VS 2022 toolchain and folds
  `matrix.os` into the cargo cache key so a future runner image bump
  auto-invalidates rather than poisoning the cache.

## [0.2.0-beta.1] - 2026-06-08

First public beta release. Local-first voice dictation, meeting capture,
and knowledge graph capture for Windows. Everything runs on-device.
Zero telemetry, zero cloud calls unless you explicitly opt in.

### Features

- **Push-to-talk dictation** via a configurable global hotkey
  (Right Alt by default). Press to record, release to paste into the
  focused app. Three cleanup modes (casual, normal, formal) plus an
  on-demand Compress transform.
- **Local Whisper STT** via whisper.cpp. CUDA acceleration when an
  NVIDIA GPU is present, CPU fallback otherwise.
- **Silero VAD** via ONNX Runtime for gating Whisper and trimming
  silence at chunk boundaries.
- **Optional cleanup** through a pluggable provider trait. Ships with
  Ollama (local, recommended default) and Anthropic Claude (cloud,
  opt-in, bring your own API key).
- **Meeting capture mode.** Chord toggle (Right Ctrl + `.`) starts a
  long-form session capturing microphone and system audio in parallel.
  Whisper runs over rolling 30-second windows with 2-second overlap.
  Two-channel transcripts are merged into a single Markdown document.
  Optional ephemeral LLM summarization that is never persisted.
- **Knowledge graph capture** with optional Obsidian vault projection.
  Captures are parsed into typed entries with open-vocabulary tags and
  typed entity references, then projected into a deterministic vault
  subtree as wiki-linked Markdown files. Reverse-watcher reconciles
  edits made directly in Obsidian back into the local database.
- **Activity capture** sibling subsystem. Polls the foreground window,
  rolls up snapshots into per-block summaries via your configured local
  LLM, optionally captures audio per block, exports a per-day PDF.
  Configurable exclusion list lets you mark apps that should never be
  captured (password managers, banking apps, anything you want).
- **Mobile capture** via a synced Obsidian vault. Drop a voice memo
  into the vault inbox from an iOS Shortcut on your phone; Mockingbird
  picks it up, transcribes it, classifies it, projects it.
- **Clipboard save and restore** around every paste. Your existing
  clipboard contents are preserved after each dictation paste.
- **Secure-input field detection.** Paste injection is aborted with a
  toast when the focused field reports itself as a password or other
  secure input. Your password manager is safe.
- **Encrypted API key storage** via Windows DPAPI tied to your Windows
  user account. Both the optional Anthropic key and the optional
  Unsplash key are protected this way.
- **Zero telemetry.** No analytics. No crash reporting that phones home.
  Crashes log to local files only.
- **Unified Recording Command Center.** Single front door for dictation
  and meeting capture, plus an activity tile.

### Known limitations

- **Beta software.** Expect rough edges. Please file issues.
- **Windows only** as a shipped build. macOS support exists as a
  source-build path with deltas documented in
  [`docs/research/macos-implementation-notes.md`](./docs/research/macos-implementation-notes.md);
  no signed Mac installer ships in the beta.
- **Not code-signed.** The MSI is unsigned. Windows SmartScreen will
  warn on first launch; this is normal for independent open-source
  Windows apps. Click "More info" then "Run anyway".
- **Test runner ABI mismatch on the canonical Windows dev box.**
  `cargo test --release` exits with `STATUS_ENTRYPOINT_NOT_FOUND`
  (0xc0000139) during the test runner load sequence. The test binaries
  themselves link clean (which validates types, traits, and the link
  surface) and the shipping app binary is unaffected. Test execution
  works fine on Linux and on the throwaway-crate workaround documented
  in CONTRIBUTING.md.
- **Knowledge graph requires Ollama.** The KG pipeline expects a local
  `qwen2.5:7b-instruct-q4_K_M` (or comparable) running on Ollama.
  Smaller models degrade gracefully to a tags-only mode.
- **Cross-device sync delegated to your vault.** Mockingbird does not
  synchronize its SQLite database across machines. Cross-device
  knowledge sharing happens through your Obsidian vault (or any other
  folder sync you use).
- **Reverse-watcher full sweep deferred.** The reverse-watcher
  reconciles individual Obsidian edits at roughly 3 seconds median
  latency. A nightly full-vault sweep is planned but not in this
  release.

### Security

- All data at rest is on your local disk under
  `%LOCALAPPDATA%\Mockingbird\`. Protected by standard Windows ACLs;
  relies on BitLocker for offline encryption if you have it enabled.
- API keys encrypted via Windows DPAPI, tied to your Windows user
  account, never stored in plaintext.
- Updater wired but disabled by default in the beta. Signing pubkey
  generated; opt-in toggle will surface in a future release.
- npm dependencies installed with `--ignore-scripts` to defend against
  postinstall-script supply-chain attacks.

## [0.1.0] - never shipped publicly

Internal milestone marker. The first publicly tagged build is
`0.2.0-beta.1` above.
