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

### 3.19 `src-tauri/src/kg/` — Knowledge Graph library (Phase 1A graduation)

**Status:** callable library, **no consumers wired yet.** The
dictation orchestrator, command center, and UI do not yet call any
`kg::` function — that's Phase 1C. Phase 1A delivered the library
subset only.

**Charter ADR:** [ADR 0049](adr/0049-knowledge-graph-phase-0-5-and-v1-architectural-pivot.md)
(Phase 0.5 + v1 architectural pivot — same ADR; Phase 1A graduation
sealed under it as a scoped exception window). Wave brief at
[`docs/knowledge-graph/phase-1a-brief.md`](knowledge-graph/phase-1a-brief.md).

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

**Pipeline shape (unchanged from sandbox Wave 0.5.4):** segment →
classify → extract → normalize → extract_entities. Five LLM passes
driven by `SCHEMA.md` + per-model-class calibration profiles. `kg::ollama`
speaks Ollama HTTP via `ureq::Agent` (no `reqwest`, per binding
parameter D1). All passes return `Result<T, PassError>` with typed
error variants (`thiserror`, no `anyhow`).

**Parity gate:** `target\release\kg_parity.exe`
(`src-tauri/src/bin/kg_parity.rs`) re-runs the full pipeline against
the Wave 0.5.4 seed-42 fixture (`docs/knowledge-graph/parity/`) via a
fixture-scripted `OllamaDispatcher` impl, asserting 32/32 bit-identical
reproduction. Binary-only (no `#[test]`) per LESSONS PINNED P2.
Invocation: `powershell -File scripts\cargo-with-cuda.ps1 run --release --bin kg_parity`.

**Phase 1B+ to come:** SQLite entity/tag/edge tables (1B), retrieval
UX with six axes (1C), migration backfill over existing transcripts
(1D), v1 beta tag (1E). Each opens its own ADR-0049 §"Sandbox
isolation" window — the Phase 1A close-out does NOT lift `ui/**` /
`migrations/**` isolation.

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
