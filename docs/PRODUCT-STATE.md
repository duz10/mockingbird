# Mockingbird — Product State

**Snapshot date:** 2026-05-23
**Branch:** `main` (commit `a4e0ec3` — post-MC-hotfix)
**Maturity:** Phase 0-4 + Phase 8 sealed; Meeting Capture (MC) sealed. Phases 5/6/7
(polish, windows, signing) and Phase 9 (macOS) remain ahead.

This is the durable "what does the app actually do today?" reference. Update when a
subsystem ships or materially changes — NOT every iteration. For session-by-session
state, see `STATUS.md`.

---

## 1. What Mockingbird is

A **local-first, privacy-respecting voice dictation app for Windows**, intended as
a drop-in replacement for Wispr Flow. Zero telemetry, zero cloud calls (unless the
user explicitly opts into the optional Unsplash photo background or chooses a cloud
LLM provider). Everything — speech-to-text, LLM cleanup, database, model weights —
runs on the user's machine.

Two top-level capture modes share one app:

1. **Dictation** — push-to-talk via a global hotkey. Records, transcribes with
   Whisper, optionally cleans with a local LLM, pastes into the focused app via
   the clipboard. ~1-3 second turn-around for short utterances.

2. **Meeting Capture** — chord-toggled long-form recording. Captures mic + system
   audio (loopback) in parallel, chunks both into 30s/2s-overlap WAVs, transcribes
   with rolling-prompt long-form Whisper, merges into a two-speaker markdown
   transcript via a deterministic formatter, optionally summarizes with an
   ephemeral LLM pass (not persisted).

Stack: **Tauri 2 + Rust + React 19 + TypeScript + Tailwind v4 + SQLite (rusqlite)**.
CUDA 12.8 via `whisper-rs` for STT, ORT 1.22 for Silero VAD, Ollama for LLM cleanup,
cpal for audio (mic + WASAPI loopback).

---

## 2. Architecture map

```
                 ┌──────────────────────────────────────┐
                 │   Global hotkey (WH_KEYBOARD_LL)     │
                 │   Right Alt → Dictation              │
                 │   Right Ctrl + . → Meeting Capture   │
                 └──────────┬───────────────────────────┘
                            │
            ┌───────────────┴───────────────┐
            │                               │
            ▼                               ▼
   ┌────────────────────┐         ┌─────────────────────┐
   │   DICTATION        │         │   MEETING CAPTURE   │
   │   (Phase 3)        │         │   (Phase MC)        │
   ├────────────────────┤         ├─────────────────────┤
   │ 1 mic stream       │         │ mic + loopback      │
   │ Silero VAD gate    │         │ 30s/2s chunker      │
   │ Whisper (one-shot) │         │ Long-form Whisper   │
   │ Cleanup (LLM, sync)│         │ Two-channel merge   │
   │ Clipboard inject   │         │ Persist → Meetings  │
   │ History row        │         │ Optional LLM pass   │
   └────────┬───────────┘         └──────────┬──────────┘
            │                                │
            ▼                                ▼
       ┌──────────────────────────────────────┐
       │   SQLite (migrations 001-011)        │
       │   transcripts (immutable raw)        │
       │   sessions, modes, prompts,          │
       │   dictionary, settings, meetings,    │
       │   meeting_chunks                     │
       └──────────────────────────────────────┘
            ▲                                ▲
            │                                │
       ┌────┴───────┐               ┌────────┴────────┐
       │ React UI   │               │ Recording       │
       │ (6 pages)  │               │ Overlay window  │
       │ + Meeting  │               │ + Meeting       │
       │   Overlay  │               │   Overlay       │
       └────────────┘               │   (pill)        │
                                    └─────────────────┘
```

---

## 3. Subsystems (Rust side)

Source root: `src-tauri/src/`. Each top-level module is a single concern; cross-cuts
go through trait boundaries so the cross-platform abstraction (Principle 5) is
preserved even in Windows-only v1.

### 3.1 `audio/` — Audio capture
- **Trait:** `AudioCapture` — `start() / stop() / frame_rx() / clone_handle()`.
- **Impl:** cpal-backed mic capture, 16 kHz mono i16. Whisper-shaped buffers.
- **Used by:** dictation + meeting capture (via separate instantiations).

### 3.2 `stt/` — Speech-to-text
- **Trait:** `SpeechToText` — `transcribe(samples, prompt, language) -> Transcript`.
- **Extended trait** (ADR 0030): `transcribe_segments` returns per-segment timestamps
  for long-form stitching.
- **Impl:** `WhisperStt` via `whisper-rs` 0.16, CUDA-built (ADR 0011). Model is
  `whisper-large-v3-turbo-q5_0.bin` by default.
- **VAD:** Silero v5 ONNX via ORT 1.22 — gates speech/silence frames before STT.

### 3.3 `cleanup/` — LLM cleanup pipeline
- **Trait:** `CleanupProvider::cleanup(CleanupRequest<'_>) -> Result<String>`.
  Synchronous (ADR 0021) so the dictation hotkey-release path stays predictable.
- **Provider impl:** `OllamaProvider` — talks to local Ollama daemon, arg-less
  `new()` constructor + optional `with_base_url()`.
- **Preprocessor** (`preprocessor.rs`, ADR 0022 Wave 1): deterministic, ~5ms.
  Strips Tier 1/2 fillers, collapses stutters, stitches self-corrections, renders
  verbal punctuation/quotes/layout cues, capitalizes sentence starts, adds terminal
  punctuation. Runs BEFORE the LLM. 34 unit tests.
- **Mode pipeline** (ADR 0022 Wave 2): three modes — `casual` / `normal` / `formal` —
  each with its own prompt, model, and temperature. Casual → qwen2.5:3b @ 0.2,
  normal+formal → qwen2.5:7b @ 0.1.
- **Empirically tuned** (ADR 0024 / migration 010): v2 prompts validated against
  52-fixture eval corpus. Preservation avg: casual 97.1%, normal 97.5%, formal 88.5%.
  Zero hallucinations. The 3B casual model's imperative-content failure mode
  (echoing example scaffolding) was specifically patched in `casual_v2.md`.
- **Prompt loader:** loads from `prompts` table (migrations 003+006+007+008+010
  define the rows). `include_str!`-embedded markdown lives at
  `src-tauri/src/cleanup/prompts/*.md`.

### 3.4 `hotkey/` — Global hotkey hook
- **Driver:** `HotkeyDriver` trait + Windows `WH_KEYBOARD_LL` impl. Runs on its own
  thread with thread-local state (ADR 0015). Hook proc always calls
  `CallNextHookEx` so MC's separate hook can coexist.
- **State machine:** Right Alt push-to-talk for dictation (`state.rs`). Three
  states: Idle / Pressed / Held. Hard-stop wins over key-up edge cases (LESSONS
  2026-05-17 § "State-machine precedence").
- **Conflict detection:** `probe.rs` — checks for likely OS-level chord collisions
  before binding. Surfaces a settings-side warning.
- **Sealed file list** (do NOT modify):
  `hotkey/state.rs`, `hotkey/windows.rs`, `hotkey/driver.rs`.

### 3.5 `dictation/` + `dictation.rs` — Dictation orchestrator
- Receives hotkey edges → starts audio capture + recording-window overlay →
  Silero-gates → runs Whisper once on the stopped buffer → runs Cleanup → emits
  `dictation:state` events → injects via clipboard → persists three-stage transcript
  (raw / cleaned / final) per ADR 0010 immutability.
- **Sealed:** any file under `dictation/` and `dictation.rs` itself.

### 3.6 `injection/` — Clipboard paste
- Save-restore the prior clipboard around every paste (Principle 7).
- SecureInputGuard (ADR 0017) checks `GUI_THREADINFO.GUIFLAGS_GUI_16BITTASK`
  shortcuts + focus class names before pasting — aborts into password fields
  with a toast. **Heap-corruption proof:** clipboard bitmap snapshot uses
  `SetClipboardData(CF_DIB)`, not the older bitmap handle path (LESSONS 2026-05-17).
- **Sealed:** all of `injection/`.

### 3.7 `meetings/` — Meeting Capture subsystem (Phase MC)

Self-contained sibling to dictation. 23 files, ~330 KB. Reuses
`audio::AudioCapture` (for mic), `stt::SpeechToText::transcribe_segments` (for
long-form stitching), and `cleanup::OllamaProvider`'s public constructor (for
the ephemeral summary pass). Does NOT extend `CleanupProvider`.

- **`activation.rs`** — chord state machine (Right Ctrl + `.` default). PauseToggle
  wins everywhere; MAIN_PRESSED suppresses key-repeat. 23 tests.
- **`hotkey_installer.rs`** — independent WH_KEYBOARD_LL on its own thread.
  Always-CallNextHookEx coexistence with the dictation hook (ADR 0027).
- **`vk_names.rs`** — string ⇄ VK code mapping for settings persistence. Covers
  modifiers, F1–F24, OEM punctuation safe-chord set, A-Z, 0-9. 22 tests.
- **`loopback_windows.rs`** — cpal WASAPI loopback for system audio (ADR 0031).
- **`capture.rs`** — `TwinStreamCapture` coordinator: owns mic + optional loopback,
  exposes a one-shot chunk receiver. Real cpal default-device probe lives here.
- **`chunker.rs`** — 30s rolling chunks with 2s overlap. CRC32 per chunk via
  `crc32fast`. Hound mono 16-bit WAV writer. `<uuid>_<channel>_<seq>.wav`.
  15 tests.
- **`long_form_stt.rs`** — chunked stitch driver (ADR 0029). Per-channel rolling
  initial_prompt (~224 tokens). Overlap-window dedup for chunks N≥1. Global
  timeline shift. 23 tests across pure + integration files.
- **`formatter.rs`** — deterministic two-stage formatter (ADR 0029). Greedy-longest
  phrase pass + filler strip + repeat collapse + paragraph-gap-aware join +
  UTF-8-safe capitalization. 30 tests including 2 proptests. 582 lines.
- **`filler_words.rs`** — Tier 1/2 filler-word set + canonicalization. "Basically"
  added in ADR 0032.
- **`merge.rs`** — two-channel speaker-labeled markdown merge. Uses
  `SpeakerLabels { mic, sys }` from settings (default: "You" / "Other(s)").
- **`levels.rs`** — `compute_dbfs` + lock-free `LevelsState`. Powers the live
  `meeting:tick` event for VU meters (ADR 0032).
- **`lifecycle.rs`** — meeting orchestrator. Start → capture → stop → long-form-STT
  → formatter → merge → persist → emit done. Tick-emitter thread (250ms) joined
  in finalize. Drop marks `interrupted`.
- **`llm_pass.rs`** — ephemeral summary. Fresh `OllamaProvider` per call via the
  existing public constructor. Output NOT persisted (kept in
  `MeetingCaptureRuntime::llm_pass_cache: HashMap<String, String>` evicted on
  shutdown). Three prompt presets in `meetings/prompts/*.md` (markdown files,
  NOT DB rows).
- **`overlay.rs`** — Tauri window control. `force_show_for_recording()` shows the
  pill without emitting `meeting:overlay-open` (avoids CHOOSE-mode flicker).
- **`runtime.rs`** — `MeetingRuntimeConfig` (from settings via
  `from_settings(&conn, chunk_base_dir)`), `InFlightMeeting`, `MeetingHotkeyState`.
  One-shot legacy chord migration lives here.
- **`persist.rs`**, **`repo.rs`**, **`export.rs`**, **`clipboard.rs`** —
  SQLite-backed meeting + chunk repo, markdown export, copy-to-clipboard
  (one-shot — meetings are user-initiated paste targets, not inline injection,
  so no save/restore).

**Invariant judges** (see `docs/judges/phase-mc/`):
- `mc-formatter-deterministic`
- `mc-long-form-stitched-losslessly`
- `mc-two-channel-merged`
- `mc-no-llm-in-critical-path`
- `mc-dictation-untouched`

### 3.8 `db/` — SQLite + migrations
- `rusqlite` (bundled SQLite), NOT `sqlx` (ADR 0004).
- 11 migrations applied in order at boot. 001-010 sealed at `phase-1-complete`.
  Migration 011 adds `meetings` + `meeting_chunks`.
- `raw` transcripts are immutable (Principle 1, hook-enforced).
- FTS5 virtual table for History search (`search_transcripts` IPC).

### 3.9 `settings/` — Typed settings store
- `SettingKey` enum + per-key value type. Allowlisted IPC writers prevent
  arbitrary key writes from UI. Meeting-specific keys use a dedicated typed
  IPC pair (`meeting_settings_get_all` / `meeting_settings_set`) carrying
  bool/number/string/null cleanly.

### 3.10 `commands/` — Tauri IPC surface
- One module per UI domain: `insights`, `sessions`, `dictionary`, `modes`,
  `settings`, `learning`, `system`, `meetings`.
- DTOs mirror `ui/src/lib/types.ts`. `into_err` helper dedupes the
  `map_err(|e| e.to_string())` boilerplate.

### 3.11 `secrets/` — DPAPI wrapper
- Windows DPAPI for at-rest secrets. Used by the (planned) Claude API key path.
  Unsplash API key still on `localStorage` pending `mb-eza` (release wiring).

### 3.12 `learning/` — Learning loop (Phase 8)
- Background job that mines history for high-confidence dictionary suggestions.
  Opt-in toggle in Settings → Advanced.

### 3.13 `window_context/` — Active-window introspection
- `K32GetModuleBaseNameW` under `PROCESS_QUERY_LIMITED_INFORMATION` for app
  attribution on transcript rows. (LESSONS 2026-05-17 documents the silent-fail
  case.)

### 3.14 `recording_window.rs` + `tray.rs` + `logging.rs`
- Recording overlay Tauri window (320×80, transparent, no-decoration, alwaysOnTop,
  non-activating per ADR 0016 §7). Drives the `dictation:state` event stream.
- Tray icon, single MockingbirdMark, hide-to-tray on X-close.
- `tracing`-based logger writing to local files (no telemetry, Principle 4).

### 3.15 `activity/` — Activity Capture (Phase 10, in-flight)
Chartered by ADR 0036 (subsystem) + ADR 0037 (Command Center). Three
waves shipped, three still ahead:

- **Waves 1A + 1B (sealed)** — Command Center surface, `Activity` page,
  migration 012 schema (`activity_sessions`, `activity_events`,
  `activity_blocks`, `activity_summaries`), runtime modules
  `lifecycle.rs` / `sampler.rs` / `runtime.rs` / `persist.rs` /
  `activity_level.rs` + IDs in `ids.rs`. Foreground polling + idle
  tracking write immutable rows to `activity_events`.
- **Wave 2 (sealed)** — UIA deep snapshots via `uia/` (Probe trait +
  Windows COM impl), multi-monitor attribution, v2 `snapshot_json`
  payload (focused field, visible-text fragments, control summary,
  password-field redaction).
- **Wave 3 (code-complete, awaiting Wave-4 buildable-upon confirmation)** —
  ADR 0040. Pure-Rust pipeline `segmenter.rs` (event normalization)
  → `blocker.rs` (5-rule boundary heuristic: app-switch, large title
  delta, idle ≥ 60s, monitor change, 30-min cap) → `abstractor.rs`
  (LLM via OllamaProvider; templated fast-path for `no_payload`
  Blocks) → `assembler.rs` (Markdown rendering, work-report variant).
  Persistence + CRUD in `blocks_persist.rs`; orchestration, export
  to file, clipboard in `export.rs`. Migration 013 adds
  `activity_blocks.label` + an FTS5 contentless shadow over
  `(label, generated_abstract)`. UI surface: Wave-3 Blocks panel on
  the Activity detail view (rename / rewrite-abstract / delete /
  regenerate / copy as Markdown), in sibling `ActivityBlocks.tsx`.
  Provenance per Principle 2: every Block row records `prompt_version_sha`
  + `source_event_ids` JSON.
- **Waves 4-6 (not started)** — audio Layer 2 (Wave 4); hardening +
  encryption-at-rest (Wave 5, gated on ADR 0038) + retention + crash
  recovery + PDF + Settings; invariant judges + final
  `phase-10-complete` tag (Wave 6). Wave 7 (Layer 3 screenshot + OCR)
  is optional post-seal via successor ADR 0039.

---

## 4. UI surface (React)

Source root: `ui/src/`. React 19, strict TS, Tailwind v4, design tokens in
`ui/src/design/tokens.css`. Routes are flat, no router framework — just a
sidebar + page switch via Zustand store. No `@tanstack/*` (ADR 0003,
hook-enforced).

### 4.1 Pages
| Page | What it does |
|---|---|
| `Insights.tsx`         | 7-day dashboard: words/sessions/recording/streak tiles + canvas sparkline + mode-mix + top-apps. |
| `History.tsx`          | Two-pane FTS5-backed transcript history. 200ms debounce, snippet highlighting, raw/cleaned/final stages, Copy/Mark-example/Delete actions. |
| `Dictionary.tsx`       | CRUD table for dictionary entries (user / learned / imported). |
| `Modes.tsx`            | Per-mode editor (casual/normal/formal). Provider+model+temp+max-tokens, 400ms autosave. |
| `Settings.tsx`         | 4-tab settings (General / Models / History+Data / Advanced) + Meetings tab in a sibling file. Background card for Unsplash. 715 lines (tracked `mb-17d` for split). |
| `Meetings.tsx`         | Meetings index list + recording controls (Start/Stop, source picker). |
| `MeetingDetail.tsx`    | Per-meeting detail: two-channel transcript, Run-Summary action, Save-As export, ephemeral-LLM warning notice. |
| `About.tsx`            | App info + open-source acknowledgments. |

### 4.2 Standalone windows (separate HTML entries)
- `recording.tsx` + `recording/RecordingWindow.tsx` — the dictation pill.
  States: idle / listening / transcribing / cleaning / pasting / done / aborted.
  MockingbirdMark in active state (ADR 0023 W5).
- `meeting_overlay.tsx` + `meeting_overlay/MeetingOverlay.tsx` — the meeting pill.
  Two modes: CHOOSE (source picker before record) and RECORDING (live levels +
  Stop + × dismiss).

### 4.3 Shared
- `lib/types.ts` — DTO mirrors of Rust commands.
- `lib/meetings.ts` — meeting-specific client helpers (`clampMaxDuration` etc.).
- `i18n/en.json` — single-source-of-truth for user-facing strings.
- `design/tokens.css` + `design/materials-v2.css` — token system, glass utility
  classes, `[data-photo-bg]` token-override scope for Unsplash readability
  (ADR 0023, ADR 0025).
- `components/` — primitives (Button, Input, Switch, Chip, Segmented, ListItem,
  Dialog), MockingbirdMark, UnsplashBackground (opt-in photo + adaptive scrim).

### 4.4 Test surface
- Vitest: 55 tests across 5 files (Meetings IPC contracts, settings tab,
  meeting overlay, lib/meetings). Run via `npm test`.
- Playwright visual baselines under `playwright-results/phase5-baselines/`
  (12 screenshots covering every page + recording overlay).
- ESLint: **broken pending `mb-yxh`** (ESLint v9 config migration). `tsc --noEmit`
  + Vitest cover type + behavior in the meantime.

---

## 5. Build / run / test env (Windows, CUDA 12.8)

**ALL cargo invocations** must go through:
```
powershell -File scripts\cargo-with-cuda.ps1 <args>
```
The wrapper imports MSVC env, pins CUDA_PATH, caps CMAKE parallelism (whisper-rs
CUDA OOMs at 16), and forwards args through `cmd.exe /c`. Plain `cargo` produces
binaries that may not launch.

**Running the app:** `powershell -File scripts\run-mockingbird.ps1` — never
`Start-Process target\release\mockingbird.exe` directly. The launcher sets
`ORT_DYLIB_PATH` + prepends CUDA bin so `onnxruntime.dll` and `cudart64_12.dll`
load at process start.

**Models** live at `%USERPROFILE%\mockingbird_models\`:
- `onnxruntime.dll` (~12 MB)
- `silero_vad.onnx`
- `whisper-large-v3-turbo-q5_0.bin` (~500 MB)

Restore scripts: `scripts\download-onnxruntime.ps1`, `scripts\download-models.ps1`.

**Known issue:** `cargo test --release` exits `STATUS_ENTRYPOINT_NOT_FOUND`
(0xc0000139) on this box despite the wrapper. The app binary launches fine —
only the test runner is affected. Sanctioned fallback:
- `check` + `clippy --release -- -D warnings` + `fmt --check` + `test --release --no-run`
- Pure-Rust modules go through the throwaway-crate recipe (LESSONS 2026-05-17).

---

## 6. Binding principles (must read `.code_puppy/AGENTS.md` for full text)

1. **Raw data is immutable.** Once `transcripts(stage='raw')` is written, no UPDATEs.
2. **Provenance is total.** Every session row pins prompt version + dict snapshot + example set.
3. **Layers are replaceable.** Platform/provider specifics live behind module-scoped traits.
4. **No telemetry.** Crashes log locally. Never phone home.
5. **Cross-platform from day one** — even in Windows-only v1.
6. **No shortcuts.** Hard-to-test means refactor. Hard-to-verify is the bug.
7. **Clipboard save/restore around every paste** (dictation, not meetings).
8. **Secure-input fields abort injection** — detect and toast.

---

## 7. What hasn't shipped yet

Listed in priority order. See `bd ready` for current unblocked state.

### Phases 5/6/7 from PLAN §10 — `mb-xwi` (P2)
- Phase 5: recording-overlay polish (live RMS waveform from orchestrator,
  default-theme decision, ARIA tablist parity, live hotkey chord capture in Modes).
- Phase 6: History details polish, Settings DPAPI Claude key modal,
  `purge_all_history` IPC backend, About window content pass.
- Phase 7: code signing (deferred per ADR 0005), MSIX/MSI packaging.

### ADR 0022 Wave 3 — `mb-cjc` (P2)
LLM-skip path for short casual utterances. Goal: ~300ms direct-paste of
preprocessor output when the utterance is short + clean enough that the LLM
adds zero value.

### Empirical mode tuning (continuous) — `mb-ez9` (P1, in_progress)
Add fixtures, run `mode_eval` grid, iterate prompts. Casual is at 97.1% / formal
at 88.5%; the bar is 95% / 80% so we're above, but the corpus is also small.

### P3 backlog
- `mb-dub` — tray menu deep-link to Settings → Meetings (deferred from MC hotfix)
- `mb-17d` — split 715-line Settings.tsx into per-tab components
- `mb-eza` — DPAPI migration for Unsplash API key
- `mb-ax9` — Unsplash glyph review
- `mb-yxh` — ESLint v9 config migration
- `mb-59h` — hide disabled AI command modes by default

### Phase 9 (not chartered yet)
- macOS support. Loopback strategy TBD (BlackHole? ScreenCaptureKit?).

---

## 8. Reading order for new sessions

1. `STATUS.md` (you should have read this first — 60 sec)
2. **This file** — 3 min skim
3. `docs/LESSONS.md` PINNED block at top — 1 min
4. `.code_puppy/AGENTS.md` — 2 min
5. **If working on a phase:** `docs/phases/phase{N}.md`
6. **If working on an active ADR-chartered epic:** the chartering ADR

Total cold-start cost: ~10 minutes to be productively oriented. Compare to the
old `STATUS.md`'s 1149-line diary that took ~20 minutes to skim and left you
with no model of the actual product, only of how it was built.
