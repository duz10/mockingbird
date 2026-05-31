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
- **Preprocessor** (`preprocessor.rs`, ADR 0022 Wave 1; `looks_listy()` un-stubbed
  in ADR 0047 Wave 2.2): deterministic, ~5ms. Strips Tier 1/2 fillers, collapses
  stutters, stitches self-corrections, renders verbal punctuation/quotes/layout
  cues, capitalizes sentence starts, adds terminal punctuation. Runs BEFORE the
  LLM. `looks_listy()` now returns `true` when the preprocessor recorded ≥2
  ordinal cues OR ≥3 enumeration markers — feeds the LLM-skip gate (see below).
- **Cleanup level dial** (`SettingKey::DictationCleanupLevel`, ADR 0047 Wave 2.1):
  `None` (raw STT, no preprocessor, no LLM) / `Light` (preprocessor only,
  ~5 ms) / `Medium` (preprocessor + LLM with the additive-only
  `normal_v6_additive` prompt — never deletes content) / `High` (preprocessor +
  mode-specific LLM, same as pre-ADR-0047 behaviour). **Default: `High`.**
  Power-users tune via direct settings edit; the Settings UI surface is
  deferred to `mb-h0nn`.
- **LLM skip on short utterances** (ADR 0047 Wave 2.2): when the post-preprocessor
  word count is ≤ `SettingKey::LlmSkipWordThreshold` (default 12) AND
  `!looks_listy()`, `run_cleanup` short-circuits and returns the preprocessor-only
  text in ~5 ms. Roughly 70 % of casual one-liners take this path; the LLM
  retains its job for multi-paragraph dictations and implicit lists.
- **Shrink-fallback guard** (ADR 0047 Wave 1.2): after every LLM cleanup call,
  `cleaned_words / pre_words` is checked against
  `SettingKey::LlmShrinkFallbackThreshold` (default 0.65). If the ratio falls
  below threshold AND the preprocessor recorded no legitimate self-corrections,
  the cleaner falls back to the preprocessor-only text, logs a `warn!`, and
  appends `-shrink-fallback` to `last_model_used` so provenance tells the truth.
- **Whisper dictionary substitution upstream** (ADR 0047 Wave 1.3):
  `stt::prompt_builder::build_prompt` wires the user's dictionary (top-N by
  `use_count`, capped at 200 tokens) into Whisper's `initial_prompt` at both
  dictation call sites. Dictionary substitution moves UPSTREAM of the LLM,
  reducing its workload.
- **Mode pipeline** (ADR 0022 + ADR 0047 Waves 1.4 + 2.3 + 2.4): three modes —
  `casual` / `normal` / `formal` — each with its own prompt, model, and
  temperature. Per migration 019, all three modes run at temperature **0.2**
  (was 0.1 for normal/formal). Per migration 021, casual runs on
  `qwen2.5:7b-instruct-q4_K_M` (was 3B); the LLM-skip path absorbs the
  latency tax on one-liners so the 7B upgrade doesn't hurt casual UX.
  Q5_K_M model variants ship as opt-in via `SettingKey::PreferQ5Models`
  (default off; VRAM-probe-gated at ≥6 GB total; migration 022); existing
  installs stay on Q4_K_M unless the user opts in.
- **Empirically tuned** (ADR 0024 / migration 010 baseline; migration 019
  temperature bump verified clean via `mode_eval` re-run, owned by
  `mb-nc9u`): v2 prompts (`casual_v2`, `normal_v5`, `formal_v2`) remain the
  level-`High` bodies; `normal_v6_additive` is the level-`Medium` body.
- **Prompt loader:** loads from `prompts` table (migrations
  003+006+007+008+010 + ADR 0047 prompt rows). `include_str!`-embedded
  markdown lives at `src-tauri/src/cleanup/prompts/*.md`
  (`normal_v6_additive.md` new in Wave 2.1).
- **On-demand `LlmPassCard` Transforms** (Dictations detail; lateral
  Stable Alpha + ADR 0047 Wave 2.6): user can re-run the LLM on a saved
  dictation via a built-in (`summary` / `action_items` / `cleaner_punctuation`
  / **`compress`**, the pull-only Transform new in Wave 2.6) or a custom
  prompt body. Drives `meetings::llm_pass::run_llm_pass`. The
  `cleaner_punctuation` pass uses `SYSTEM_HEADER_PUNCTUATION` ("Preserve
  every word; modify only whitespace and punctuation") — the load-bearing
  Wave 1.1 fix. `summary` / `action_items` / `Custom(_)` retain the
  concision header. Empirically validated at 18/20 preserve-rate on the
  default meetings model — see
  `docs/cleanup/eval-adr0047-cleaner-punctuation.md`.
- **Quality signal** (ADR 0047 Wave 2.5): `sessions.edit_free_within_5min`
  (Option<bool>) flips `false` if the user opens the `LlmPassCard` for that
  session OR copies the raw transcript within 5 minutes of inject; otherwise
  flips `true` at the 5-minute mark. Surfaced as an Insights "Your usage"
  tile. This is the metric that will tell us whether to flip
  `DictationCleanupLevel`'s default to `Light` in a future ADR amendment.

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
- **Headless ingest (ADR 0046 Iter 1, shipped 2026-05-27):**
  `dictation::ingest::headless_ingest(deps, samples, provenance)` is a pure-Rust
  entry point that runs VAD + STT + Cleanup on a pre-decoded sample buffer (no
  mic, no PTT). The `+ Audio file` button on the Dictations page calls the
  `dictation_import_file` IPC, which decodes via `symphonia` off-thread and
  queues a `HeadlessIngestRequest` onto a sibling `crossbeam-channel` next to
  the orchestrator's existing `StateAction` stream (ADR 0046 §3.2). Same
  VAD/STT/Cleaner instances are reused — no fresh allocations per import.
  Sessions land with `source='desktop-import'` and `start_mode='in_app'`
  (migration 018 + ADR 0045 columns). `SessionsEventBus` trait drives UI
  refetch identically to the PTT path. No progress indicator yet — tracked as
  `mb-q1xt` for Iter 4 polish.
- **Sealed:** any file under `dictation/` and `dictation.rs` itself, except for
  edits explicitly authorized by ADR 0045 (programmatic start/stop) and
  ADR 0046 §3 / §3.1 / §3.2 (headless ingest).

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

### 3.15 `activity/` — Activity Capture (Phase 10, **sealed at `phase-10-complete`**)
Chartered by ADR 0036 (subsystem) + ADR 0037 (Command Center).
Shipped 2026-05-26. Privacy posture is opt-in everything: audio off by
default, retention TTLs zero by default (no auto-delete), exclusion
rules + secure-input guard fire at capture time (not post-hoc).

**Subsystem entry point.** `ActivityCaptureRuntime::spawn(conn, audio_chunk_base_dir)`
from `lib.rs::setup`, immediately after `crash_recovery::recover_all`. The
runtime owns one foreground sampler thread + the lifecycle FSM
(`Idle ↔ Active ↔ Paused`); all DB writes go through `persist.rs` /
`blocks_persist.rs` / `segments_persist.rs`, which gate every row with
the matcher from `exclusion.rs` (loaded via `ExclusionMatcher::load`
from `activity_exclusion_rules`).

**Capture (Layer 1).** `sampler.rs` foreground-polls (1Hz default) +
`activity_level.rs` tracks idle; UIA deep snapshots come from `uia/`
(Probe trait + Windows COM impl; multi-monitor attribution + v2
`snapshot_json` schema with focused-field, visible-text fragments,
control summary, password-field redaction). `record_event` consults
the exclusion matcher BEFORE INSERT — capture-time enforcement, not
post-hoc scrubbing.

**Summarization (Layer 1.5 — ADR 0040).** Pure-Rust pipeline
`segmenter.rs` (event normalization) → `blocker.rs` (5-rule boundary
heuristic: app-switch, large title delta, idle ≥ 60s, monitor change,
30-min cap) → `abstractor.rs` (LLM via OllamaProvider with a
templated fast-path for `no_payload` Blocks) → `assembler.rs` (Markdown
rendering, work-report variant). Persistence + CRUD in `blocks_persist.rs`;
export orchestration in `export.rs`. Migration 013 adds
`activity_blocks.label` + an FTS5 contentless shadow over
`(label, generated_abstract)`. Provenance per Principle 2: every Block
records `prompt_version_sha` (`template_no_payload_v1` /
`abstract_v1-XXXXXXXX` / `abstract_v2_audio-XXXXXXXX`) +
`source_event_ids` JSON.

**Optional audio (Layer 2 — ADR 0041).** Opt-in via `activity_audio_enabled`
typed setting (default OFF). Pure-Rust `audio.rs` defines the
`AudioPipeline` trait + `LongFormAudioPipeline` impl that *wraps*
(does not duplicate) Meeting Capture's WASAPI twin-stream +
`meetings::long_form_stt::LongFormStt` chunked Whisper machinery
(Principle 5). Per-channel transcript segments land in
`activity_transcript_segments` (migration 014) via `segments_persist.rs`,
time-shifted from capture-relative to global epoch-ms at insert.
`block_audio_stitcher.rs` assigns each segment to exactly one Block via
the midpoint rule. The abstractor swaps to `abstract_block.audio_aware.md`
+ the `abstract_v2_audio-` fingerprint family when a Block has audio;
`user_edited` Blocks are still respected.

**Hardening (Wave 5 — ADRs 0042 / 0043 / 0044; migration 015).**
- `exclusion.rs` — built-in rules seed (8 entries: 1Password, KeePass, LastPass, Bitwarden, consent.exe, LogonUI.exe, builtin-secure-input password-field guard, builtin-browser-bank regex). User-editable via Settings; reloads hot through `runtime.reload_exclusion_rules()`.
- `retention.rs` — three independent TTLs (`events_days`, `segments_days`, `blocks_days`); sweep_once is one transaction; cascade-option-(a): when an `activity_events` row deletes, every Block that references it via `source_event_ids` JSON sets `raw_events_purged_at` (the abstract text survives intact).
- `crash_recovery.rs` — boot sweep promotes `in_progress` sessions to `crashed_recovered`, deletes orphan chunk_dirs under `audio_chunk_base_dir`; idempotent.
- `pdf_export.rs` — `printpdf` 0.7 two-mode render (`Full` shows time-stamped headers + abstracts; `WorkReport` strips times + apps, abstracts only). `pdf-extract` is a dev-only round-trip dep for the judge fixtures.

**Command surface.** 16 `activity_*` IPC commands in `commands/mod.rs`
(start / start_with_audio / pause / resume / stop / shutdown / list /
get_detail / delete / list_blocks / rename_block / rewrite_abstract /
delete_block / regenerate_block / export_blocks_markdown / export_pdf).

**Front door.** Unified Recording Command Center (ADR 0037) is now the
entry point for both Dictation and Meeting Capture. Legacy
`Right Ctrl + .` Meeting chord respects `legacy_meeting_chord_enabled`
(one-shot migration in `meetings/runtime.rs::migrate_legacy_meeting_chord_flag_once`
flips it ON for existing users with prior meeting rows; new installs
leave it OFF and reach Meeting Capture via the Command Center mode
picker).

**Invariant judges (Wave 6).** `docs/judges/phase-10/` contains six
LLM-grader specs + `scripts\dry-run-phase10-judges.ps1` mechanical-layer
rig: `exclusion-is-total`, `retention-preserves-abstracts`,
`crash-recovery-idempotent`, `pdf-renders-correct-block-count`,
`sealed-phases-untouched` (with verdict file at
`sealed-phases-untouched-verdict.md`), `provenance-is-total`. All 6
green on Wiggum loop iteration 1.

**Deferred:** ADR 0038 (encryption-at-rest) RESERVED for v0.2.
ADR 0039 (Layer 3 screenshot + OCR) optional post-seal via successor
ADR.

**Live-fire smoke test:** Dustin's post-seal step (LESSONS P7 pattern;
judges prove invariants but not a clean OS bring-up).

### 3.16 `vault/` — Mobile extension via Obsidian Sync (ADR 0046, Iters 1-3 IMPLEMENTATION COMPLETE)
Chartered by ADR 0046. Iter 1 (desktop file-ingest pipeline) sealed
2026-05-27. **Iter 2 (outbound Obsidian projection) SEALED 2026-05-27** —
live-fire smoke green on desktop + iPhone Obsidian Mobile. **Iter 3
(mobile inbox → desktop courier) IMPLEMENTATION COMPLETE 2026-05-27** —
watcher + courier + runtime live; live-fire end-to-end smoke green on a
pre-existing voice memo (see §3.17). Iter 3 judge (`mb-ksau`) + Shortcut
smoke (`mb-1yxp`) still owed. Iter 4 (full Mobile Sync settings tab +
connection health + nested-vault setup wizard `mb-3xww`) still to ship.

**Subsystem entry point.** `VaultRuntime::new(&db)` constructed in
`lib.rs::setup` immediately after `app.manage(AppState::new(...))`, BEFORE
the dictation + meeting runtimes spawn. Both runtimes accept the
`Arc<VaultRuntime>` handle in their `spawn()` signatures. The runtime is
`.manage()`'d as Tauri state so the `vault_*` IPC commands can grab it.

**Modules.**
- `layout.rs` — idempotent zone creation under `<vault>/`:
  `dictation/`, `meeting/`, `history/`, `history/_archive/`. Pure-Rust;
  unit-tested via the throwaway-crate recipe.
- `manifest.rs` — `manifest.json` schema + atomic `.tmp+rename` save +
  BTreeMap-ordered serialization so on-disk bytes are deterministic
  across runs. Per-record entries record relative path + content SHA-256.
- `project.rs` — pure record→markdown projection. Hand-rolled YAML
  front-matter (8-key fixed order, optional-omit, sorted tags, conservative
  scalar quoting); SHA-256 content address; `YYYY-MM-DD-HHMM__<uuid8>.md`
  filename. Golden-snapshot test pins the exact bytes.
- `export_job.rs` — `VaultRuntime`. Single-in-flight job lock
  (`Arc<Mutex<()>>`) + coalescing-trigger flag (`Arc<AtomicBool>`) so
  concurrent `trigger()` calls collapse into one re-run rather than
  piling up workers. `run_once_blocking(&db)` for the manual
  `vault_export_now` IPC; `trigger(db)` for the fire-and-forget post-commit
  hooks. Reconciliation pass: ensure zones → load manifest → query in-scope
  records (dictation + meeting, filtered by `VaultSyncRecordTypes` +
  `VaultRetentionDays`) → project each → atomic write only if content SHA
  changed → archive stale records to `history/_archive/` → save manifest.

**Trigger sites.** All purely additive (zero edits removed):
- `dictation.rs::persist_complete` — PTT path, after the `session_saved`
  event.
- `dictation.rs::handle_headless` — file-import path (ADR §3.2) and
  future mobile-inbox ingest (ADR §6).
- `meetings/lifecycle.rs::stop_meeting` Complete branch — after the
  meeting row + session-saved event commit.
- Settings IPC (`vault_settings_set`) — flipping `MobileSyncEnabled` or
  changing `VaultPath` triggers an immediate backfill so users don't have
  to click "Export now".

**IPC surface** (`commands/vault.rs`):
- `vault_settings_get` / `vault_settings_set` — typed get/set for the
  four user-visible keys (`MobileSyncEnabled`, `VaultPath`,
  `VaultSyncRecordTypes`, `VaultRetentionDays`). Set IPC also refreshes
  runtime config + fires trigger.
- `vault_export_now` — manual reconciliation; returns `VaultExportSummary`
  with `{ total, changes, archived, skipped }` driving the toast.
- `vault_pick_directory` — native folder picker via `tauri-plugin-dialog`,
  exposed as a Rust IPC so the renderer doesn't need a new JS dep.

**Settings keys** (Iter 2 wired 4 of the 8 per ADR §10): `MobileSyncEnabled`
(default false, opt-in), `VaultPath` (default null), `VaultSyncRecordTypes`
(default "both"), `VaultRetentionDays` (default 30). Iter 4 stubs landed but
unwired: `VaultSyncBackend`, `SyncTierByteCap`, `VaultDebugKeepCouriers`,
`KeepAudioBlobs`.

**UI surface** (Iter 2 preview): `ui/src/pages/SettingsMobilePreview.tsx`
mounted in Settings → Advanced as a `<Card>` titled "Mobile Sync (preview)".
Master toggle, vault path input + Browse button, status line, Export-now
button. Iter 4 will lift this into a dedicated Mobile Sync tab with the
remaining 6 controls.

**Live-fire smoke verification (2026-05-27).** Dustin's hands-on: backfill
of 90 records against `C:\Users\dboyd\mockingbird-vault\` succeeded;
auto-trigger on new PTT dictation fires within seconds; iPhone receives
via Obsidian Sync within ~30s. Zero implementation bugs. One operational
gotcha (Obsidian nested-vault trap) surfaced and resolved out-of-band;
Iter 4 setup wizard will detect + guide (`mb-3xww`). See LESSONS
2026-05-27.

---

### 3.17 `inbox/` — Mobile inbox courier (ADR 0046 Iter 3, implementation complete)
Sibling subsystem to `vault/`. Where `vault/` projects desktop records
OUT to the Obsidian vault for the iPhone to read, `inbox/` watches the
vault for files the iPhone (via iOS Shortcut + Voice Memos) drops IN
and walks them through the headless dictation ingest channel published
by ADR 0046 §3.2 (Iter 1).

**Gating.** The SAME `MobileSyncEnabled` + `VaultPath` settings that gate
the outbound projection also gate the inbox. One toggle controls both
directions; flipping it routes through `vault_settings_set` which refreshes
BOTH runtimes in the same IPC tick.

**Modules.**
- `watcher.rs` — `notify`-crate filesystem watcher on `<vault>/inbox/`.
  Filters: path-segment exclusions (`.obsidian` / `.git` / `.mockingbird` /
  `_archive` / `_failed` / `_keep`), extension exclusions for
  partial-write markers (`.tmp` / `.partial` / `.icloud` / `.crdownload` /
  `.swp` / `.lock`), filename prefix exclusions (`~$` Office lockfiles),
  and an extension allowlist of `m4a` / `wav` / `mp3`. 100ms debounce
  per Wave 0 Finding 3 (every FS event fires 3-4x within ~5ms), 2-second
  stability check per Wave 0 Finding 4 (defensive size-unchanged window
  against future streaming-write semantics; binary files arrive
  atomically today per Finding 1 so check completes on first try). Emits
  `StableInboxFile { path, size, observed_at }` via crossbeam channel.
- `courier.rs` — `Courier` worker, single-in-flight via `Mutex<()>`.
  Per file: extension/size validation (`> 0`, `< 50 MB`), decode via
  `audio::decode::decode_to_pcm16_mono_16k`, send
  `HeadlessIngestRequest{provenance: SessionSource::MobileInbox, ...}`
  on the Iter 1 channel, await reply. Success → atomic rename to
  `inbox/_archive/<YYYY-MM-DD>/<original-filename>` (date subdir prevents
  cross-day collisions). Failure → atomic rename to `inbox/_failed/`
  with a `-N` suffix on collision. `FileOps` trait + `ProductionFileOps`
  split keeps every routing branch fully testable without real disk I/O.
- `runtime.rs` — `InboxRuntime`, mirrors `VaultRuntime`'s shape. Owns
  `InboxConfig { enabled, vault_path }` snapshot + state machine
  (`Stopped` ↔ `Running { vault_path, watcher, courier }`). 5 transitions
  in `refresh_config`: stopped/idle → no-op; stopped/active → start;
  running/idle → stop; running/same-path → no-op; running/different-path
  → stop + start. On `start`, walks `<vault>/inbox/` non-recursively for
  pre-existing audio files and pre-fills the channel BEFORE spawning the
  watcher — closes the "recorded while laptop closed" startup-catchup
  case from ADR §6.

**Wiring in `lib.rs`.** Constructed inside the `Ok(DictationRuntime)` arm
right after `app.manage(headless_ingest_tx.clone())`, parallel to the
vault export-job runtime construction earlier in `setup`. Lives behind
`#[cfg(target_os = "windows")]` for the same reason the dictation runtime
does — the headless ingest channel is only published on Windows.

**Live-fire end-to-end smoke (2026-05-27).** First launch after wiring:
initial scan found a pre-existing `New Recording 38.m4a` (29.8s voice
memo) sitting in `<vault>/inbox/`. Courier decoded it via symphonia,
Iter 1's `headless_ingest` ran it through whisper-rs CUDA (1079ms STT) +
Ollama cleanup (9866ms), wrote `sessions` row `session_id=116` with
`source = 'mobile-inbox'`, and the courier atomically archived the
source file to `<vault>/inbox/_archive/2026-05-24/`. Zero implementation
bugs surfaced.

**What's NOT in scope here** (Iter 4 hardening matrix `mb-qxrm`):
conflict-file quarantine, dedup ledger, placeholder-note writes,
machine-fingerprint mismatch handling, oversized silent-skip with
sidecar marker, app-offline catch-up retry policy. The implementation
slate (Waves 3.1-3.3) is the happy-path baseline; Iter 4 hardens it.

### 3.18 `experimental/kg-validation/` — Knowledge Graph validation sandbox (Phase 0 + 0.5)

**Status:** R&D sandbox. **Nothing in this directory ships to users.** The
live Mockingbird app has no knowledge-graph surface as of this writing.

**Charter ADRs:**
[ADR 0048](adr/0048-kg-phase-0-validation-methodology.md) (Phase 0
methodology, Accepted) and
[ADR 0049](adr/0049-knowledge-graph-phase-0-5-and-v1-architectural-pivot.md)
(Phase 0.5 architectural pivot + v1 charter, Accepted). Reports at
[`docs/knowledge-graph/REPORT.md`](knowledge-graph/REPORT.md) (Phase 0)
and [`docs/knowledge-graph/PHASE-0-5-REPORT.md`](knowledge-graph/PHASE-0-5-REPORT.md)
(Phase 0.5 SEAL).

**Structure:** standalone Cargo workspace (its own `[workspace]`; **NOT** a
member of the root Mockingbird workspace) so vanilla `cargo test` runs
live and sidesteps LESSONS PINNED P2 (test-runner launch bug on this box).
No whisper-rs / ort / CUDA deps. Drives Ollama over HTTP for LLM passes
(`qwen2.5:3b-instruct-q4_K_M`, `qwen2.5:7b-instruct-q4_K_M`,
`nomic-embed-text`, `llama3.1:8b-instruct` for PCRP).

**Pipeline (Phase 0.5 sealed shape):** segment → classify → extract →
`extract_entities` → normalize. Driven by `SCHEMA.md` (portable Markdown
contract) with per-model-class calibration profiles (`small-conservative`,
`mid-confident`). Deterministic scorer with versioned synonym map
(`judge-calibration/synonym-map.json`, v1.1).

**v1 architecture commitments (binding, NOT YET in production):** see
[PHASE-0-5-REPORT.md §6](knowledge-graph/PHASE-0-5-REPORT.md). Two-field
structured entry schema (`tags:` + `entities:`); qwen2.5:7b pinned for
entity-aware operation (3b = tags-only degraded mode); embeddings
infrastructure preserved for entity disambiguation (NOT classification);
closed-vocab Move 3 deferred to v1.1 awaiting two-field corpus re-labeling;
opt-in graph layer (existing dictation users see zero regression); ~1 min
intake latency budget.

**Production graduation path:** Phase 1A SEALED 2026-05-31 — the
schema-driven pipeline now lives at `src-tauri/src/kg/` (see §3.19
below). The sandbox stays alive as the v1.1+ regression rig per ADR
0049 binding parameter D5; Phase 1B (SQLite entity/tag/edge tables) is
the next graduation window per ADR 0049 §"Sandbox isolation".

---

### 3.19 `src-tauri/src/kg/` — Knowledge Graph library (Phase 1A graduation + Phase 1B persistence/worker/dictation hook + Phase 1C retrieval UX/activation/concept modal + Phase 1D source-gated filing + first-class KG screen)

**Status:** library + storage + async filing worker + dictation-tail
hook + **full retrieval UX surface (5 of 6 axes) + activation toggle
+ failed-filings fix-it loop + concept modal** all in place.
**Default-off** via `SettingKey::KgGraphEnabled = false` (migration
024 seed); user can now flip the toggle from Settings → Knowledge
Graph and the worker picks up the change within ~5s (per-tick poll
promoted at 1C.1). Graph-off invariant enforced on both sides: the
Rust-side `kg_graph_off_invariant` probe (Phase 1B) asserts no
`kg_*` table writes; the UI-side `ui/tests/kg-graph-off-invariant.spec.ts`
Playwright spec (Phase 1C.5) asserts no `kg_*` IPC invocations
beyond the read-only `kg_settings_get_all`. **Category axis explicitly
deferred to Phase 1D** — see §"Phase 1C" subsection below for the gap.

**Charter ADRs:** [ADR 0049](adr/0049-knowledge-graph-phase-0-5-and-v1-architectural-pivot.md)
(parent epic; Phase 0.5 + v1 architectural pivot) +
[ADR 0050](adr/0050-kg-phase-1b-persistence-and-dictation-hook.md)
(Phase 1B sub-charter for persistence + worker + dictation hook;
Accepted 2026-06-03) +
[ADR 0051](adr/0051-kg-phase-1c-retrieval-ux-and-activation.md)
(Phase 1C sub-charter for retrieval UX + activation toggle +
concept modal; Accepted 2026-05-31). Wave briefs at
[`docs/knowledge-graph/phase-1a-brief.md`](knowledge-graph/phase-1a-brief.md)
and
[`docs/knowledge-graph/phase-1b-brief.md`](knowledge-graph/phase-1b-brief.md).

**Public surface (D6 minimum):** `kg::run_pipeline`, `kg::PipelineResult`,
`kg::Entry` / `Category` / `EntryType` / `EntityType` / `Status` /
`AnswerKey`, plus `kg::run_parity_probe` (consumed only by the
`kg_parity` binary). Everything else is `pub(crate)` — `OllamaDispatcher`,
`Schema`, the pass modules, `MockOllama`, etc. — to keep the Phase 1B/1C
wire-up design space open.

**Bundled assets:** `src-tauri/src/kg/assets/SCHEMA.md` + the seven
prompt files under `prompts/`, all `include_str!`-baked. The bundled
`SCHEMA.md` is the **v1 slice** of the research sandbox file —
`schema_revision: phase-1a-v1-open-vocab` — with the Wave 0.5.3
closed-vocab list stripped per ADR 0049 amendment A2. The closed-vocab
Rust wiring (`synonyms.rs` + `tag_validator.rs::validate_tags`) stays
graduated as dead-but-tested code for the v1.1 starting point;
activation is one `#### Vocabulary list` re-introduction away (via
`MOCKINGBIRD_KG_SCHEMA_DIR` env override; tested by
`schema_loader::tests::closed_vocab_path_still_active_via_env_override`).

**Pipeline shape (Phase 1B Chunk 3 closed the 4-pass gap):** segment →
classify → extract → normalize → **extract_entities**. Five LLM passes
driven by `SCHEMA.md` + per-model-class calibration profiles. `kg::ollama`
speaks Ollama HTTP via `ureq::Agent` (no `reqwest`, per binding
parameter D1). All passes return `Result<T, PassError>` with typed
error variants (`thiserror`, no `anyhow`). The 5th pass was authored
in Phase 0.5 (Wave 0.5.4) but had been silently absent from
`run_pipeline` in production until Phase 1B Chunk 3 — the discovery
adds an additive `PipelineResult.segment_entities: Vec<Vec<ExtractedEntity>>`
field without breaking the parity probe's structural JSON
comparison (additive-field-invisibility pattern; LESSONS body
2026-06-02).

**Storage (Phase 1B Chunk 2 / migration 024):** SQLite schema lives
at `db/migrations/024_kg_phase_1b.sql`.

- `kg_entities` — canonical entity rows. `(name, entity_type)` UNIQUE.
  `entity_type` is the 5-bucket taxonomy `person|organization|object|place|project`
  (Rust truth at `EntityType::as_str()`; the ADR 0050 DDL docstring
  was fixed at Chunk 5 seal after Chunk 2 flagged the drift).
- `kg_canonical_tags` — v1.1 inert scaffold (closed-vocab path stays
  shipped per ADR 0049 amendment A2). No INSERTs from the v1 worker.
- `kg_entity_mentions` + `kg_tag_mentions` — per-segment provenance
  rows. `(entry_id, segment_idx, entity_id|tag_slug, surface_form)`
  UNIQUE. **Write-once** via `BEFORE UPDATE` triggers (DELETE allowed
  to flow through Phase 1D's re-file path + the FK CASCADE from
  sessions; UPDATE raises `... is write-once (Principle 2; ADR 0050)`).
- `kg_filing_queue` — FIFO of `(entry_id, captured_iso, state,
  attempts, last_error)`. `state` is `'pending'|'in_progress'|'done'|'failed'`.
  UNIQUE(entry_id) collapses duplicate enqueues idempotently.
- Two `concept_page_*` VIEWs join the mention rows back to the
  underlying sessions + the two-field schema for Phase 1C retrieval.

**Store layer (`kg::store`):** typed wrappers around the migration
024 surface — `enqueue_for_filing(conn, entry_id, captured_iso)`,
`apply_filed_outcome(conn, entry_id, &segments, captured_iso)`
(materializes a `Vec<SegmentOutput>` into the four mention/entity
tables + flips the queue row to `done`), plus the FIFO drain helpers
`pop_next_pending`, `mark_done`, `mark_failed`, `requeue_for_retry`,
`reap_done_older_than`. `SegmentOutput { segment_idx, entities,
tag_slugs }` is the single source of truth carrier produced by
`kg::worker::build_segment_outputs(&PipelineResult)`. All mutations
are idempotent under UNIQUE constraints — same `apply_filed_outcome`
call twice produces identical row counts (Chunk 5 `--persist` mode
asserts this on all 32 fixtures).

**Async filing worker (`kg::worker::KgFilingRuntime`, Phase 1B Chunk 3):**
Spawned at app boot from `lib.rs::run()` under
`cfg(target_os = "windows")` iff `SettingKey::KgGraphEnabled = true`
at boot time (Chunk 3 Decision C: boot-vs-poll choice was "once at
boot" — standing bead `mb-7w5f` surfaces the promotion to per-tick
if runtime-toggle UX in 1C demands it). The runtime owns a
`tokio::task` that:

1. Sweeps `kg_filing_queue` for `state='in_progress'` rows that
   crash-recovered from a prior boot (re-flips them to `'pending'`).
2. Loops `pop_next_pending` → `kg::run_pipeline` (the 5-pass
   pipeline) → `apply_filed_outcome`. Failures bump `attempts` and
   set `state='failed'` after `MAX_FILING_ATTEMPTS`.
3. Reaps `state='done'` rows older than 30 days on a daily cadence.

No retrieval surface yet — the worker writes mentions; reading
is Phase 1C's job.

**Dictation-tail hook (`kg::worker::try_enqueue_for_kg_filing`,
Phase 1B Chunk 4):** ONE free-fn helper called from
`dictation.rs::persist_complete`, the moment after the edit-free
fast-path event fires. Outcome gate: enqueues iff
`matches!(outcome, Ok | OkClipboardNotRestored | InAppNoInject)`
(Chunk 4 Decision B; ADR 0045 in-app dictations DO enqueue, since
the `InAppNoInject` outcome means "text exists, just wasn't pasted").
Reads `SettingKey::KgGraphEnabled` directly from `Settings::new(&conn).get()`,
short-circuits with `.unwrap_or(false)` fallback. **Ignore-error**
semantics: any failure (settings probe, enqueue write) logs
`tracing::warn!` and discards — the kg hook never propagates an
error back to the dictation persist path (ADR 0050 invariant
`kg-graph-failure-non-regressing`).

**Default-off binding holds.** Phase 1B Chunk 5's
`kg_graph_off_invariant` probe (binary at
`src-tauri/src/bin/kg_graph_off_invariant.rs`) sweeps all 8
`InjectionOutcome` variants with `KgGraphEnabled = false` and
asserts every `kg_*` table row count is `0`; a positive-control
flip at the end confirms the helper IS structurally able to write
when the toggle is on (catches vacuous-pass regressions).

**Parity gate (two modes):** `target\release\kg_parity.exe`
(`src-tauri/src/bin/kg_parity.rs`) re-runs the full pipeline against
the Wave 0.5.4 seed-42 fixture (`docs/knowledge-graph/parity/`) via a
fixture-scripted `OllamaDispatcher` impl, asserting 32/32
bit-identical reproduction. Binary-only (no `#[test]`) per LESSONS
PINNED P2. Invocations:

- Default: `... cargo-with-cuda.ps1 run --release --bin kg_parity`
  — Phase 1A graduation gate; still 32/32 green.
- `--persist` (Phase 1B Chunk 5 / ADR 0050 §D8 gate 1):
  `... cargo-with-cuda.ps1 run --release --bin kg_parity -- --persist`
  — ALSO round-trips every fixture through `kg::store::apply_filed_outcome`
  against a tempfile-backed SQLite with all 24 migrations applied +
  `PRAGMA foreign_keys = ON`. Asserts row counts derived from the
  PipelineResult itself, idempotency under re-application, and
  fires the migration 024 immutability triggers once at end-of-run.
  32/32 green.
- `kg_graph_off_invariant` (Phase 1B Chunk 5 / ADR 0050 §D8 gate 2):
  `... cargo-with-cuda.ps1 run --release --bin kg_graph_off_invariant`
  — graph-off principal invariant probe (see worker section above).
  8/8 + positive control green.

**Phase 1C (sealed 2026-05-31 via ADR 0051 Accepted) — retrieval UX + activation + concept modal:**

- **Settings KG tab + activation toggle** (`ui/src/pages/SettingsKgTab.tsx`,
  Wave 1C.1). Optimistic-flip toggle backed by `kg_settings_get_all` /
  `kg_settings_set` (single-key allowlist accepting only
  `kg_graph_enabled` — same shape as the Phase MC meeting-settings
  allowlist). Conditional `role="status"` notice block (scoped via
  `aria-label={t("kg.settings.notice.title")}` at 1C.5 for a11y
  uniqueness next to the sibling SettingsKgFailedFilings status
  region). Tab body mounts SettingsKgFailedFilings iff toggle ON
  (clean re-fetch on every off→on flip).
- **Boot-vs-poll worker promotion** (Wave 1C.1). `KgGraphEnabled`
  gate moved from once-at-boot (Phase 1B Chunk 3 shortcut) into the
  worker's drain loop as a per-tick poll (~1 SQL per 5s idle loop
  when off). Runtime toggle takes effect within ~5s, no
  restart-required nag. Fail-closed on mutex poison.
- **Failed-filings UX + queue status** (`ui/src/pages/SettingsKgFailedFilings.tsx`,
  Wave 1C.2). Three new IPCs: `kg_list_failed_filings(limit)` (default
  cap 50, newest-first), `kg_requeue_failed(queue_id)` (idempotent
  on already-pending rows — J3 invariant), `kg_queue_status()` →
  `{ pending, processing, failed, lastDoneIso }`. Per-row Retry
  button disables mid-flight (UI-side idempotency belt-and-suspenders).
- **Dictations retrieval UX — 5 of 6 axes**
  (`ui/src/pages/Dictations{,FilterBar,List}.tsx` +
  `DictationKgChips.tsx`, Wave 1C.3 + Wave 1C.4). Four new IPCs:
  `kg_search_entries(SearchFilter)` (within-axis OR, across-axis AND,
  query-axis UNION with FTS hits), `kg_list_entities` /
  `kg_list_tags` (prefix autocomplete; 200ms debounce), and
  `kg_entries_summary(Vec<i64>)` (batched per-entry chip data). UI:
  entity multi-select combobox + tag multi-select combobox + per-row
  top-5 entity chips + top-N tag chips + filing-state Pill
  (pending/processing/failed). All gated on `kgGraphEnabled === true`;
  zero `kg_*` IPCs fire when toggle off (1C.5 invariant).
- **Concept modal** (`ui/src/pages/ConceptModal.tsx`, Wave 1C.4).
  Two new IPCs: `kg_entity_detail(entityId)` → `EntityDetail` and
  `kg_tag_detail(tagSlug)` → `TagDetail`, both via the
  `kg_concept_entities_view` / `kg_concept_tags_view` joined with a
  CTE-built `EntryRef` view. Chip click on Dictations rows opens the
  modal; entry-row click in the modal closes-then-selects to surface
  the row in the detail pane.
- **Graph-off-UI invariant judge** (Wave 1C.5). Opt-in
  `window.__KG_IPC_SPY__: (cmd: string) => void` hook installed in
  `ui/src/lib/tauri.ts::invoke` (one `if` per IPC call; zero cost
  when no test has opted in). Playwright spec
  `ui/tests/kg-graph-off-invariant.spec.ts` walks Settings → KG tab,
  Dictations page, dictation-row click with the toggle OFF, asserting
  the recorded `kg_*` set == `{ "kg_settings_get_all" }` only.
  Positive-control flip ON proves the spy is not vacuously passing
  (`kg_list_failed_filings` + `kg_queue_status` fire from
  `SettingsKgFailedFilings` mount).

**Phase 1D (sealed 2026-06-04 via ADR 0052 Accepted) — source-gated filing + first-class KG screen:**

Two drift corrections + a first-class home for the subsystem. Both
the "who triggers filing" semantic and the "where does the user
live" question were rescoped at the 2026-06-04 source-of-truth
alignment review against the original product spec §15 + Clark
article. The standing "Phase 1D = backfill of pre-Phase-1
dictations" framing is now moot; backfill (if it ships) is a
post-v1 per-row promote-to-graph affordance on the Dictations page,
not a bulk operation.

**Charter ADR:** [ADR 0052](adr/0052-knowledge-graph-phase-1d-charter.md)
(Accepted 2026-06-04). Phase doc:
[`docs/phases/phase-1d.md`](phases/phase-1d.md). Six waves, no
`phase-*-complete` tag (lateral epic per LESSONS PINNED P5).

- **Migration 025 + 3-gate cascade** (Wave 1D.1, `mb-pxzk`). Adds
  `sessions.capture_kind TEXT NOT NULL DEFAULT 'dictation'`
  (`'dictation'` | `'kg-note'` | `'kg-note-text'`) and
  `sessions.category TEXT NULL` (consumes the standing `mb-oji5`
  category-axis defer from Phase 1C), plus the composite index
  `idx_sessions_capture_kind(capture_kind, started_at DESC)` and
  a one-transaction `kg_*` mention/queue/entity purge +
  `kg_graph_enabled` reset. The column was originally proposed as
  `source` and renamed to `capture_kind` to avoid collision with
  the pre-existing `sessions.source` from migration 018
  (audio-origin axis). `dictation::try_enqueue_for_kg_filing` now
  enforces a **3-gate cascade** (outcome → source → toggle): a
  standard `Dictation` capture **NEVER** enqueues, regardless of
  toggle state. The drift this corrects: pre-1D, every successful
  dictation auto-filed the moment the toggle was on — the user had
  no per-capture opt-in.

- **KG screen scaffold + 5-band dashboard** (Wave 1D.2, `mb-j00j`).
  New top-level route `/knowledge-graph` + sidebar entry (gated on
  `KgGraphEnabled`, reactive to toggle via the zustand store).
  Read-only dashboard with five bands: Counts, Queue state, Recent
  activity, Flagged for review, Upcoming due dates. Single new
  read-only IPC `kg_dashboard_snapshot()` (one round-trip; returns
  an empty snapshot without DB reads when toggle is off, honoring
  the graph-off contract). The dashboard composition lives at
  `kg::dashboard` (pure-Rust, sibling of `kg::latency_bench`) so the
  IPC layer is a thin wrapper.

- **Capture surface — audio + text notes** (Wave 1D.3, `mb-0gt6`).
  New `CaptureBand` on the KG dashboard with two lanes.
  - **Audio note lane:** new IPC `dictation_start_kg_note` flips a
    sibling `next_start_is_kg_note: Arc<AtomicBool>` flag
    (independent axis from `next_start_is_programmatic` so plain
    in-app dictations stay `capture_kind='dictation'`). Orchestrator
    swap-and-pins `CaptureKind::KgNote` at `start_capture`. Row
    **dual-writes** into Dictations history AND fires KG enqueue
    via the 1D.1 source-gate. Stop reuses the existing
    `dictation_stop` IPC.
  - **Text note lane:** new IPC `kg_ingest_text_note(text)` routes
    through `kg::ingest_text::ingest_text_note` (new ~420-line
    module, sibling of `dictation::ingest`). Bypasses Whisper
    entirely — the typed string IS raw/cleaned/final transcript;
    row lands with `capture_kind='kg-note-text'`.
    `commands/sessions.rs::list_sessions` filters
    `WHERE capture_kind != 'kg-note-text'` so text notes are
    KG-only (never surface on the Dictations history page).
    The module docstring documents the Phase 1E reverse-watcher
    seam ("this row originated outside the vault" marker).

- **Retrieval surface relocation** (Wave 1D.4, `mb-6hm2`/`mb-f4gn`).
  The Phase 1C filter chips + concept modal moved off the
  Dictations page (which lost -211 LoC and reverted to its pre-1C
  shape: history list + FTS5 search + detail pane) and onto the
  new KG dashboard as `Retrieval.tsx` (~280 LoC). The single
  ConceptModal instance lives on the dashboard; both Retrieval
  rows AND RecentActivityBand rows surface chip clicks via
  `onConceptOpen`. FlaggedBand gained click-to-retry
  (`kg_requeue_failed`, idempotent per 1C.5 J3). Category badges
  were dropped from Dictations rows entirely (would be NULL on
  100% of `capture_kind='dictation'` rows by definition);
  Category now lives on the KG screen where its source-gating
  semantic is the default expectation. `SettingsKgFailedFilings`
  deleted (FlaggedBand on the dashboard subsumes it).

- **Settings panel expansion + Obsidian launch** (Wave 1D.5,
  `mb-navi`). Settings → KG tab now carries four read-only
  reference cards (Vault — read-only mirror of `vaultPath` from
  the ADR 0046 Mobile Sync settings via the new `onOpenMobileSync`
  cross-nav callback; Vocabularies — static enum-derived display
  of `kg::schema::{Category, EntryType}` via new
  `kg_vocabularies_get()` IPC pinned by the
  `vocabularies_matches_schema_enums` unit test;
  Processing-mode — "Ingest mode: silent" indicator per spec
  §15.5; Dual-write — reminder that KG audio notes also land in
  Dictations history but text notes are KG-only). New
  **Launch-into-Obsidian** button shipped on both the Settings
  tab AND the KG dashboard's `ActionsBand`. Click invokes
  `kg_launch_obsidian()` (new IPC), which reads
  `SettingKey::VaultPath` and shells out to
  `obsidian://open?vault=<encoded-leaf-name>` via new pure-Rust
  `kg::launcher` module (Windows arm + macOS/Linux stub error
  arms per Principle 5; minimal 10-line RFC-3986 percent-encoder
  with unit-pinned space → `%20` (NOT `+`) behavior, added
  rather than pulling the `url` crate for one call site).
  Graph-off-UI invariant `OFF_MODE_ALLOWLIST` extended from
  `{kg_settings_get_all}` to
  `{kg_settings_get_all, kg_vocabularies_get}` with the explicit
  comment that the contract is "no graph DATA touched when off",
  not the literal "no kg_* IPC at all".

- **Wave 1D.6 judges + seal** (`mb-q2p1`). Three acceptance gates
  per ADR 0052 §"Acceptance gates":
  - **J1 `kg-source-gate-invariant`** (NEW) — deterministic Rust
    probe at `kg::source_gate_invariant` + binary
    `kg_source_gate_invariant`. 6/6 corpus cells
    (3 `capture_kind` values × 2 toggle states) match expected
    `kg_filing_queue` row counts (0, 0, 0, 1, 0, 1). Drives both
    `dictation::try_enqueue_for_kg_filing` AND
    `kg::ingest_text::ingest_text_note` entry points; sibling of
    `kg_graph_off_invariant`.
  - **J2 `kg-dictation-untouched`** — runtime twin of Phase MC's
    diff-judge of the same name; formalizes the assertion that a
    standard `Dictation` capture produces zero `kg_*` writes
    regardless of toggle state.
  - **J3 `kg-graph-off-ui-tightened`** — documents the
    consolidated Playwright invariant from Waves 1D.2 (KG screen
    walk), 1D.4 (Dictations KG-free assertion), 1D.5
    (vocabularies allowlist) as fully satisfied; no new code
    shipped at 1D.6 for this judge.

**User-visible behavior post-1D:** KG entries live in the
Mockingbird DB (no vault projection yet); the user opens the KG
from the sidebar, sees a read-only dashboard, captures via the
dedicated KG audio + text note lanes (separate from PTT), filters
+ drills into concepts inline, and can launch Obsidian to the
configured vault. **Phase 1E (Obsidian as source of truth) is NOT
YET SHIPPED** — markdown projection of KG entries to
`<vault>/knowledge-graph/`, the reverse-watcher (vault → SQLite
ingest), the KG-Inbox courier, the history archive folder, the
Obsidian Tasks format emission, and the pre-built Kanban/dashboard
boards are all Phase 1E (future ADR 0053) scope. The v1 beta tag
awaits Phase 1F.

**Standing carry-forwards** (not gating Phase 1E kickoff):
`mb-bbl2` (sonner retrofit), `mb-y6pq` (`--status-bad` token
sweep), `mb-26aw` (`smoke.spec.ts` ×4 pre-1C Playwright failures),
`mb-2wbk` (KG row → Dictations deep-link, P3, filed in 1D.4),
`mb-0ui1` (vocabularies editor, P3, filed in 1D.5).

ADR 0051's §"UI sealed-surface authorization" window closed at the
1C.5 seal; ADR 0052's analog window closed at the 1D.6 seal —
Phase 1E opens its own ADR-0049 §"Sandbox isolation" window per
the established pattern.

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
