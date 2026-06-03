# Changelog

All notable changes to Mockingbird will be documented here. Format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

_Nothing yet._

## [0.2.0-beta.1] - 2026-06-08

First public beta. Local-first voice dictation, meeting capture, and the
Karpathy/Clark Personal Knowledge Engine capture substrate, all running
on-device on Windows. Zero telemetry, zero cloud calls unless the user
explicitly opts in (Unsplash background image or a cloud cleanup LLM).

### Headline

- **Dictation** — push-to-talk via Right Alt; Whisper (CUDA when present,
  CPU fallback) + optional local Ollama cleanup; clipboard-paste injection
  with secure-input field detection; three cleanup modes (casual / normal /
  formal) plus an on-demand Compress transform.
- **Meeting Capture** — chord-toggled (Right Ctrl + `.`) long-form
  recording of mic + WASAPI system loopback in parallel; 30s / 2s-overlap
  chunked long-form Whisper with rolling prompts; deterministic two-channel
  merge; optional ephemeral LLM summarization (not persisted).
- **Knowledge Graph capture + vault projection** — captures (audio + text
  notes) flow through a five-pass pipeline (segment → classify → extract →
  extract_entities → normalize) and are projected to an Obsidian vault as
  wiki-linked Markdown with auto-generated Entity / Project / Tag stub
  pages, an `INDEX.md` + `LOG.md` + `Tags/` subtree, and a `SCHEMA.md`
  contract the user's chat-LLM consumes for Ingest / Query / Lint.
- **Activity Capture** sibling subsystem — foreground polling + idle
  tracking + UIA snapshots → LLM block summarization → optional per-block
  audio transcription, with retention sweep, crash recovery, PDF export,
  and capture-time exclusion.
- **Unified Recording Command Center** — single front door for Dictation
  and Meeting Capture (also surfaces Activity tile).

### Added (by sealed phase / epic)

- **Phase 0** — Rust + Tauri 2 scaffold, hook engine, CI hygiene.
- **Phase 1** — SQLite schema (migrations 001-003), repo layer, typed
  settings store.
- **Phase 2** — Audio capture via `cpal` (mic + native-config-driven
  resampling through `rubato`), `whisper-rs` CUDA STT with CPU fallback,
  Silero VAD via ORT.
- **Phase 3** — Global hotkey hook (`WH_KEYBOARD_LL`), dictation
  orchestrator FSM, `SecureInputGuard` (password-field abort + toast),
  clipboard save/restore-around-paste injection.
- **Phase 4** — Cleanup provider trait + Ollama provider + per-mode
  versioned prompt loader.
- **Phase 8** — Full UI sprint: App / Insights / History / Dictionary /
  Modes / Settings pages + recording overlay.
- **Phase MC** (Meeting Capture) — chord activation, twin-stream capture,
  long-form chunked Whisper, deterministic formatter, two-channel merge,
  ephemeral LLM pass, overlay UI, Meetings + MeetingDetail pages, five
  invariant judges.
- **Phase 10** (Activity Capture) — 22 modules under
  `src-tauri/src/activity/`, migrations 012-015, six invariant judges,
  PDF export via `printpdf`, Unified Recording Command Center.
- **ADR 0022** — Three-mode cleanup pipeline (casual / normal / formal).
- **ADR 0023** — Design Language v1 (warm-earth Liquid Glass + Fraunces).
- **ADR 0024** — Empirical mode-prompt tuning + migration 010.
- **ADR 0025** — Optional Unsplash ambient background (opt-in BYO-key).
- **ADR 0032** — MC v1.1 polish (VU meters, LLM-ephemeral notice,
  MaxDuration UI).
- **ADR 0033 / 0034 / 0035** — MC chord-collision + overlay event-delivery
  + Stable Alpha hotfixes (auto-derived meeting title; loopback
  config-discovery fix; meeting_cancel / meeting_rename / meeting_overlay_hide).
- **ADR 0036 / 0037 / 0040-0044** — Activity Capture charter, command
  center, abstractor pipeline, audio layer, retention cascade,
  exclusion list, PDF export.
- **ADR 0045** — Dictation programmatic start/stop (in-app record button
  on Dictations page; CC tile).
- **ADR 0046** — Mobile extension via synced Obsidian vault: `+ Audio
  file` desktop import, deterministic Markdown projection of dictation +
  meeting history to `<vault>/history/`, inbox courier for iOS-Shortcut
  voice memos, Mobile Sync settings tab, nested-vault detection wizard,
  iOS Shortcut recipe docs.
- **ADR 0047** — Cleanup pipeline refinement: per-pass system headers,
  length-ratio shrink fallback, Whisper `initial_prompt` from user
  dictionary, `DictationCleanupLevel` dial (None / Light / Medium / High),
  LLM-skip-on-short-utterance, opt-in Q5_K_M with VRAM gating, on-demand
  Compress transform, `edit_free_within_5min` instrumentation.
- **ADR 0048** — Knowledge Graph Phase 0 validation methodology
  (corpus + harness + scorer + 6 invariant judges + REPORT.md).
- **ADR 0049** — Knowledge Graph Phase 0.5 + v1 architectural pivot:
  SCHEMA.md as portable contract, entity extraction as 5th pipeline pass,
  qwen2.5:7b-instruct-q4_K_M pinned, two-field entry schema
  (open-vocab `tags:` + typed `entities:`), opt-in graph guarantee.
- **ADR 0049 / Phase 1A** — KG pipeline graduates from sandbox to
  `src-tauri/src/kg/` as a callable library; 32/32 bit-identical parity
  gate vs seed-42 fixture.
- **ADR 0050 / Phase 1B** — KG persistence layer: migration 024
  (`kg_entities`, `kg_canonical_tags`, `kg_entity_mentions`,
  `kg_tag_mentions`, `kg_filing_queue`, two concept-page VIEWs, two
  immutability triggers); async filing worker with crash recovery + reap;
  source-gated dictation-tail enqueue.
- **ADR 0051 / Phase 1C** — KG retrieval UX + activation toggle +
  concept modal: Settings KG tab, failed-filings UX with idempotent
  retry, Dictations retrieval surface (entity / tag / free-text /
  per-row chip strip / filing-state pills), click-to-open concept modal,
  graph-off-UI Playwright invariant.
- **ADR 0052 / Phase 1D** — Source-gated filing + first-class KG screen:
  migration 025 (`capture_kind`), 3-gate cascade (outcome → source →
  toggle), Dictation NEVER auto-files regardless of toggle (raw data
  immutable principle), KG promoted to sidebar destination with 5-band
  dashboard, KG audio + text capture surfaces, Settings KG tab expansion
  (Vault / Vocabularies / ProcessingMode / Dual-write + Obsidian launcher).
- **ADR 0053 / Phase 1E** — KG vault projection layer: deterministic
  Markdown projection to `<vault>/Knowledge Graph/` (Entries / Entities /
  Projects / History / Inbox subtree), auto-generated Entity + Project
  stub pages with wiki-link aliases, SHA-256 content addressing in the
  manifest, two-phase commit, History archive of per-session JSON
  sidecars + audio, reverse-watcher reconciling Obsidian edits back into
  SQLite (~3s p50; SHA-256 loop-prevention; file-wins on conflict).
- **ADR 0054 / Phase 1E** — Personal Knowledge Engine substrate
  (Karpathy/Clark pattern): two-agent role separation — Mockingbird is
  the capture + first-pass synthesis layer, the user's chat-LLM is the
  wiki author/maintainer, the Obsidian vault is the knowledge codebase.
  Nine knowledge shapes (`source`, `note`, `concept`, `entity`, `project`,
  `question`, `decision`, `reference`, `observation`). `SCHEMA.md` /
  `INDEX.md` / `LOG.md` / `Tags/` subtree bootstrapped + maintained
  idempotently. KG-Inbox courier (sibling of ADR 0046 inbox) for iOS
  Shortcut + desktop drag-and-drop drops. iOS Shortcut recipe for KG-Inbox
  documented. Joint-seal with ADR 0053 (Wave 1E.9 four judges all green:
  `kg-reverse-watcher-loop-prevention`, `kg-file-wins-on-conflict`,
  `kg-subtree-bootstrap-idempotent`, `kg-serializer-golden-roundtrip`).
- **Design System v1** (bead-only) — glass-tier semantic tokens, canonical
  sticky-sidebar scroll convention, full `100vh` → `100dvh` sweep, native
  form-control polish.

### Changed

- Default cleanup mode quality target raised: `qwen2.5:7b-instruct-q4_K_M`
  is now the pinned local model for KG-aware operation; `qwen2.5:3b`
  remains a documented tags-only degraded mode.
- Casual mode repointed to `qwen2.5:7b-instruct-q4_K_M` (migration 021).
- Cleanup temperature standardized to 0.2 across casual / normal / formal /
  meetings (migration 019).
- Sealed `transcripts(stage='raw')` immutability: hook + DB triggers
  enforce; raw rows are never updated.
- Migrations 001/002/003 sealed at `phase-1-complete`; subsequent schema
  changes are append-only.

### Security

- **Zero telemetry.** Crashes log locally; nothing phones home.
- DPAPI-wrapped Anthropic API key storage (Windows-only, opt-in cloud
  cleanup).
- Clipboard save-and-restore around every paste; secure-input field
  detection aborts injection with a toast.
- Mini Shai-Hulud IOC list enforced by hook; `npm install` blocked
  without `--ignore-scripts`.

### Known limitations

- **Windows-only.** macOS support is planned for Phase 9; not in this
  beta. Cross-platform abstractions are already in place behind
  `#[cfg(target_os)]` traits.
- **Mobile capture is iOS-only** via the Obsidian Sync + iOS Shortcut
  recipe (ADR 0046 / Phase 1E). Android path deferred to Phase 9.
- Knowledge Graph filing requires **Ollama running locally** with
  `qwen2.5:7b-instruct-q4_K_M` pulled. The pipeline degrades gracefully
  to tags-only on `qwen2.5:3b`.
- KG runtime model footprint: Whisper GGUF (~500 MB - 2 GB depending on
  variant) + Ollama qwen2.5:7b (~4.7 GB). Plan for ~6-8 GB on disk after
  first launch.
- KG retrieval is single-machine. Cross-device sync of the vault is
  delegated to Obsidian Sync (or any other folder sync the user prefers);
  Mockingbird does not synchronize SQLite across machines.
- The reverse-watcher reconciles Obsidian edits back into SQLite on
  ~3s p50; nightly timer-driven full reconcile sweep is deferred
  (`mb-srvh`, P3).
- Multi-entry filings collapse to the first entry's classification in
  the vault projection (`mb-ng1o`, P3).
- `cargo test --release` runner hits `STATUS_ENTRYPOINT_NOT_FOUND`
  (0xc0000139) on the canonical dev box (`mb-0n8c`); the **app binary
  itself launches fine** — only the test runner is affected. Pure-Rust
  modules go through a throwaway-crate test recipe; wired modules gate
  via `cargo test --release --no-run` + clippy + judge probes. The
  shipping binary is unaffected.
- Win11-on-real-hardware smoke matrix, installer / updater verification,
  and marketing-shaped privacy / docs polish are tracked as follow-up
  beads against this release (see `bd ready`).

### References

- ADR 0053 — KG Phase 1E Obsidian-as-source-of-truth (vault projection
  charter).
- ADR 0054 — Personal Knowledge Engine substrate (Karpathy/Clark
  framing; supersedes ADR 0053 framing while preserving its
  implementation).
- ADR 0049 — KG architectural pivot (the v1 binding).
- ADR 0046 — Mobile extension via synced Obsidian vault.
- ADR 0036 / 0037 — Activity Capture + Unified Recording Command Center.
- LESSONS PINNED P14 — Karpathy/Clark north star.
- Phase docs: `docs/phases/phase-{0..4,8,10,mc,1a..1e}.md`.

## [0.1.0] - never shipped publicly

Internal milestone marker. Phase 0 bootstrap landed under this version
string but it was never tagged or distributed. The first publicly-tagged
build is `v0.2.0-beta.1` above.
