# Phase MC — Meeting Capture (lateral feature epic)

**Phase entry tag:** `phase-4-complete` (383+ tests, three-mode pipeline live, Ollama + Claude providers swappable; lateral epics ADR 0022/0023/0024 sealed). **Actual HEAD at entry:** `0ae3475` (post-`phase-4-complete`, includes lateral epics ADR 0025 "optional remote ambient background" and `mb-14l` tray/icon polish — neither touches the dictation pipeline, modes table, or migrations; both are dormant w.r.t. Phase MC scope).
**Phase exit tag:** `phase-mc-complete` (target — adds `MeetingCapture` / `LongFormStt` / `Formatter` AppError variants, migration 011 `meeting_sessions` + `meeting_transcripts`, new `meetings/` module tree, **does not touch** the dictation state machine, modes table, or cleanup-provider trait)
**Planner:** planning-agent (this doc)
**Implementor:** code-puppy. **No sub-agents** required — every module is greenfield Rust + greenfield React under the existing crate/workspace; the bd-task fan-out for sub-agents was Phase 3's pattern (multi-file Win32 surgery) and doesn't apply here.
**Estimated iterations:** 5–6

> Lateral feature epic — *sibling* to the dictation pipeline, not a child. The dictation state machine, the `modes` table, the `CleanupProvider` trait, the existing `recording_window`, the hotkey hook driver, and migrations 001–010 are **sealed for this phase** (binding rule). Meeting Capture builds in parallel and reuses three primitives only: `audio::AudioCapture`, `stt::SpeechToText` (extended; see ADR 0030), and `cleanup::OllamaProvider`. Everything else is greenfield.

## Status

Phase MC is the standalone "record a long-form session, get a clean transcript, optionally run an LLM pass" feature. It targets parity with WisprFlow's meeting mode but local-only. Activation is a **Right Ctrl + M chord** (configurable modifier + main-key; default `VK_RCONTROL` + `VK_M`, fallback `VK_RCONTROL` + `VK_F13` per ADR 0019 conflict-probe — keeps the user's hand in the same modifier zone as the dictation Right Alt hold). On stop, the audio is fed through a **chunked Whisper inference path** (the long-form fix the standing P1 `mb-2bi` has been queued for — Phase MC ships it), then a **deterministic formatter** (filler removal + paragraph breaks from segment timestamps + capitalization), then saved as the canonical transcript. No LLM runs in the critical path. The user can optionally trigger a secondary LLM pass against the canonical transcript and export the result as a file — not persisted.

## Overview

Phase MC adds a meeting-recording feature that lives alongside dictation. Trigger: **Right Ctrl + M** chord in any focused app → an overlay surfaces three audio-source choices (Microphone / System / Both). Press Start → the audio capture pipeline runs until the user presses Stop (or the configured ceiling fires; default 4 hours, hard ceiling 6 hours). On stop:

1. **Chunked Whisper** transcribes the sealed buffer in 30 s windows with 2 s overlap, stitching at segment boundaries. Returns timestamped segments per channel.
2. **Deterministic formatter** runs the segments through a pure Rust pass: filler-word removal (`um`/`uh`/`like`/`you know`/`I mean`/`basically`/`sort of`/`kind of`, configurable list), paragraph breaks on inter-segment gap ≥ 2.0 s OR channel change (when two-channel), capitalization at paragraph + sentence starts, trust-Whisper's-own-`.,!?` punctuation otherwise. **Zero LLM in the critical path.**
3. **Persist** — one `meeting_sessions` row + one or two `meeting_transcripts` rows (one per channel; stage = `formatted`). Audio blob retained per existing `audio_retention_days` setting at a meeting-specific path.
4. **Transcript view** in the main window: copy-to-clipboard, export-as-Markdown, and an **optional LLM pass** dropdown (summary / action items / cleaner punctuation / custom prompt). LLM output is shown in a side panel, exportable to file, **not persisted to the DB** (it's reproducible from the canonical transcript).

**Two-channel handling** (Microphone + System source): captures two simultaneous cpal streams into two ringbufs → two VAD passes → two Whisper passes → two formatted transcripts, with the mic transcript labeled `You` and the system transcript labeled `Other(s)`. No diarization-within-channel in v1 (deferred — see "Out of scope").

**Sibling, not child.** The dictation hotkey state machine (`hotkey/state.rs`) is not extended; meeting activation runs through a **separate listener** (`meetings/activation.rs`) on a separate channel. The `modes` table is not touched; meeting feature uses its own config rows in `settings`. The `CleanupProvider` trait is not modified; the optional LLM pass instantiates a fresh `OllamaProvider` per request with the user-chosen prompt. The existing dictation `recording_window` is not reused; a new `meeting_overlay` window handles activation UI.

## Pre-flight — ADRs authored in Wave 1

### ADR 0026 — Meeting Capture is a sibling subsystem, not an extension of dictation

The dictation feature is hold-to-talk, ≤300 s, three modes, one canonical "cleaned" pass that goes into the user's caret via clipboard injection. The meeting feature is push-to-start/push-to-stop, hours-long, no modes, no injection at all (transcripts live in the app, not in another window). The two pipelines share only audio capture, the Whisper STT primitive, and the Ollama HTTP client. Forcing them to share the hotkey state machine, the `modes` table, the cleanup-provider trait, or the recording-window overlay would (a) introduce conditional logic in every shared module ("is this dictation or meeting?") that makes refactors twice as expensive forever and (b) explode the dictation surface area's test matrix. ADR 0026 cements the boundary: shared primitives are `audio::AudioCapture`, `stt::SpeechToText`, and `cleanup::OllamaProvider`; everything else under `meetings/` is greenfield. Hooks `block-cross-module-coupling-meeting-dictation` (to be authored Wave 1) reject diffs that import `meetings::*` from `dictation::*` or vice versa, and that touch `hotkey/state.rs`, the `modes` table, or `cleanup/provider.rs` from inside `meetings/`.

### ADR 0027 — Chord activation (Right Ctrl + M) via dedicated `WH_KEYBOARD_LL` listener on a dedicated meetings thread

The dictation hook (`hotkey/windows.rs`) is the *single* `WH_KEYBOARD_LL` install in the app today, and per ADR 0015 it must not do work in the callback. Adding chord-discrimination on top of its tap-vs-hold discrimination would entangle two unrelated gesture state machines and re-litigate every dictation hotkey change. Instead, Phase MC installs a **second** `WH_KEYBOARD_LL` on a **dedicated message-pump thread owned by the meetings runtime** (the dictation driver's thread is owned by `hotkey/driver.rs` and is sealed for this phase per the binding list; Windows permits multiple system-wide LL keyboard hooks — each is installed by its own thread, each runs its own `GetMessageW` loop, and Windows itself dispatches `WM_KEYBOARD_LL` events to every installer, chained via `CallNextHookEx` per-thread). The new listener observes ONLY the configured modifier + main-key pair (default `VK_RCONTROL` + `VK_M`; fallback `VK_RCONTROL` + `VK_F13`). It tracks modifier-held state, fires `MeetingActivation::Toggle` on the **first** main-keydown while the modifier is held, and suppresses Windows key-repeat until main-keyup. The dictation hook continues to see its own configured key (default Right Alt) unchanged — disjoint VK sets, zero cross-talk. **Why Right Ctrl + M and not Right Alt + M:** Right Alt is the dictation hold-to-talk key; chording on top of it would fire a phantom dictation session every meeting trigger. Right Ctrl is one key over on every standard keyboard, preserving the user's right-modifier muscle memory without the conflict. **Cost of the extra thread:** one Windows message-pump thread sitting in `GetMessageW`, sub-microsecond overhead per keystroke. **Benefit:** zero modifications to the sealed dictation driver — the binding list in this plan, and the `block-cross-module-coupling-meeting-dictation` hook authored in Wave 1, can both stay strict. Conflict probe: if Pause/Break is owned by another app's hotkey, fall back to F23 then F24 then "user picks" — same ladder as ADR 0019.

### ADR 0028 — Two-channel capture via twin cpal streams + clock-aligned merge

CPAL streams are `!Send` on Windows but multiple streams can coexist on different devices. For "Both" mode, Phase MC opens two streams in the meeting thread: one against `host.default_input_device()` (mic, exactly as dictation does today) and one against the **default output device's loopback endpoint** (system audio). Each stream feeds a dedicated ringbuf (sized for the configured meeting ceiling — default 4 h, max 6 h; math: 6 × 3600 × 16000 × 2 bytes ≈ 690 MB per channel — these are allocated lazily, only when the user picks Both/System, and the buffer can spill to disk via the chunk-writer in ADR 0028). Each stream is timestamped at capture start (`Instant::now()` snapshot, stored on the meeting session row); after stop, the two PCM streams are processed independently (VAD → Whisper → formatter), then the two transcript sets are merged into a single chronological view by `(channel, segment_start_ms)`. WASAPI loopback availability is probed at "Both" / "System" selection time; if the OS reports no render endpoint or loopback is denied (rare; can happen with exclusive-mode audio), the UI demotes Both → Microphone-only with a toast.

### ADR 0029 — Long-form chunked Whisper inference (closes `mb-2bi`)

The dictation ring buffer is sized for 300 s and Whisper runs once over the whole sealed buffer. Neither scales to a 4-hour meeting. Phase MC ships the long-form path: during recording, the meeting-thread consumer drains the ringbuf every 30 s into **fixed 30-second PCM chunks** with **2-second leading overlap** (the second chunk starts at 28 s, the third at 58 s, etc.), each written to a temp WAV under `<appdata>\Mockingbird\meeting_audio\<uuid>\<chunk_index>.wav`. The ringbuf only ever holds ≤32 s of PCM live; the 690 MB ceiling in ADR 0028 is a worst-case-paranoid bound, not the actual RAM. After stop, each chunk is fed to `WhisperStt::transcribe_segments` (new method — see ADR 0030) sequentially; the chunker carries the **last 1 s of segment text** from chunk N as Whisper's `initial_prompt` for chunk N+1 (preserves context across the boundary). Overlapping segments are stitched: any segment whose start falls inside the overlap window of the previous chunk is dropped (the previous chunk already emitted it). The full audio is also concatenated to a single canonical `meeting.wav` for retention. Failure mode: if Whisper fails on chunk K, the formatter still runs on chunks 0..K-1, the session is marked `partial` (status enum value), and the user sees a "transcription incomplete after N minutes — retry?" affordance in the transcript view.

### ADR 0030 — Whisper segment exposure as a new STT-trait method

The current `SpeechToText` trait returns `Transcript { text, gpu_used, latency_ms, model_id }` — no segments. The deterministic formatter needs `(text, t0_ms, t1_ms)` triples per Whisper segment. Phase MC adds a sibling method `transcribe_segments(req) -> AppResult<TranscriptWithSegments>` that returns `Vec<TimedSegment { text, t0_ms, t1_ms }>` alongside the existing top-line text. The original `transcribe` stays untouched (dictation doesn't need segments and shouldn't pay the marginal cost of building the segment vec — it's small but it's also a sealed API surface for dictation's 383 tests). Implementation: whisper-rs already exposes `state.full_n_segments()`, `state.full_get_segment_text(i)`, `state.full_get_segment_t0(i)`, `state.full_get_segment_t1(i)`; the new method walks those and packages them. The dictation `transcribe` could be re-implemented as `transcribe_segments(...).map(|t| t.flatten())` after Phase MC ships, but the rewrite is **out of scope for MC** (would touch the dictation orchestrator and re-baseline its tests).

## Phase MC Cargo deps (incremental to post-Phase-4 manifest)

Add to `src-tauri/Cargo.toml`:

```toml
# Phase MC — long-form chunked PCM I/O.
# `hound` is already in deps for fixture I/O; we just exercise it more.
# `crc32fast` for chunk-file integrity (optional; cheap insurance against
# partial writes during a power loss mid-recording).
crc32fast = "1.4"

# Phase MC — markdown export of canonical + LLM-pass outputs.
# Pulled in for `meetings/export.rs`. Keep it tiny — no full
# CommonMark parser; we're emitting, not parsing.
# (No new crate required if we hand-write the markdown serializer;
#  recommended path is hand-write to keep the dep surface lean.)
```

**No new windows-rs features** — WASAPI loopback is reachable through `cpal` 0.15's `Host::default_output_device()` + cpal's recent loopback support on the WASAPI backend. If cpal turns out to not support loopback on the bundled version, fall back to the small `wasapi` crate (0.15+) gated `[target.'cfg(windows)']`; Wave 2 confirms which path lands and documents in the wave brief.

**No tokio**. Phase MC stays sync (ADR 0021 binding). Long-form Whisper is sequential per chunk; the UI side uses Tauri events to surface progress.

**No tauri-plugin-clipboard-manager**. Markdown export goes through `arboard` only if Wave 4 finds the existing dictation `injection::paste` save-restore protocol too coupled to dictation's clipboard semantics (it is); the simpler path is a fresh `meetings/clipboard.rs` that does a one-shot `SetClipboardData(CF_UNICODETEXT)` with no save/restore (the user is intentionally putting the transcript on the clipboard — no need to preserve their prior contents from the meeting's perspective). Wave 4 decides.

## AppError carry-forward

Wave 1 adds three new variants to `src-tauri/src/error.rs`:

```rust
/// Meeting-capture subsystem failures (activation conflicts,
/// chord listener init, twin-stream init, loopback probe).
#[error("meeting capture error: {0}")]
MeetingCapture(String),

/// Long-form STT failures (chunk-writer I/O, mid-recording chunk
/// transcription failure, stitching divergence, segment-API errors).
#[error("long-form stt error: {0}")]
LongFormStt(String),

/// Deterministic formatter failures (UTF-8 boundary errors on
/// filler-word excision, segment-time inconsistencies). All errors
/// here are bugs — the formatter is pure and deterministic; reaching
/// any of these means the input from upstream is malformed.
#[error("formatter error: {0}")]
Formatter(String),
```

Same `String`-wrapping pattern as `Hotkey` / `Injection` / `Cleanup`.

## File layout (sealed — DO NOT relitigate)

```
src-tauri/src/
├── meetings/
│   ├── mod.rs                  # Pub re-exports + MeetingCaptureRuntime owner
│   ├── activation.rs           # Chord (RCtrl+M) listener (second WH_KEYBOARD_LL)
│   ├── runtime.rs              # Threading + lifecycle (capture → STT → formatter → persist)
│   ├── capture.rs              # Twin-stream coordinator (mic + loopback)
│   ├── loopback_windows.rs     # WASAPI loopback (or wasapi-crate wrapper)
│   ├── chunker.rs              # 30 s windows, 2 s overlap, temp-WAV writer
│   ├── long_form_stt.rs        # Chunked Whisper driver (calls transcribe_segments)
│   ├── formatter.rs            # Deterministic formatter (PURE — fully unit-testable)
│   ├── filler_words.rs         # Static phf set + pure helpers (data-only sibling)
│   ├── persist.rs              # meeting_sessions + meeting_transcripts inserts
│   ├── export.rs               # Markdown serializer + clipboard helper
│   ├── llm_pass.rs             # Optional secondary LLM call (reuses cleanup::OllamaProvider)
│   ├── overlay.rs              # Tauri webview owner for the meeting overlay window
│   └── prompts/                # Built-in LLM-pass prompts (markdown bodies)
│       ├── summary.md
│       ├── action_items.md
│       └── cleaner_punctuation.md
└── stt/
    ├── mod.rs                  # ADD transcribe_segments to SpeechToText trait
    └── whisper.rs              # ADD WhisperStt::transcribe_segments impl

src-tauri/src/db/migrations/
└── 011_meeting_capture.sql     # NEW migration — meeting_sessions, meeting_transcripts

ui/src/
├── pages/
│   ├── Meetings.tsx            # NEW page — list + detail view
│   └── MeetingDetail.tsx       # Transcript viewer + export + LLM-pass panel
├── meeting_overlay/
│   ├── MeetingOverlay.tsx      # Activation overlay (source picker + Start/Stop)
│   └── styles.module.css
├── meeting_overlay.tsx         # NEW entry point (mirrors recording.tsx)
└── lib/
    └── meetings.ts             # Typed IPC client for the new commands

src-tauri/tauri.conf.json       # ADD "meeting_overlay" webview window
```

Every `mod.rs` defines its trait + cfg-gated `pub use platform::*` where platform-specific. The 600-line cap applies; `runtime.rs`, `capture.rs`, and `long_form_stt.rs` will press against it — pre-split helpers into sibling files before hitting 500.

## Section MC.1 — Activation state diagram

Binding spec for `meetings/activation.rs`. Implementation MUST be pure Rust (no Windows API calls) so it's fully unit-testable. Inputs come from the new `WH_KEYBOARD_LL` listener via an `mpsc::Sender<KeyEvent>`; outputs drive the meeting runtime.

```
IDLE
  ├─ on modifier_down                       → MOD_HELD
  ├─ on main_key_down                       → IDLE  (chord broken: main without modifier)
  └─ (any other event)                      → IDLE

MOD_HELD
  ├─ on main_key_down                       → emit MeetingToggle { source = LAST_CHOSEN }
  │                                            → MAIN_PRESSED  (suppress key-repeat)
  ├─ on modifier_up                         → IDLE
  └─ (any other event)                      → MOD_HELD

MAIN_PRESSED   (chord fully down; suppressing Windows key-repeat)
  ├─ on main_key_up                         → MOD_HELD  (ready for next chord if mod still held)
  ├─ on modifier_up                         → IDLE      (chord broken; also fine)
  ├─ on main_key_down (key-repeat)          → MAIN_PRESSED  (suppressed; already fired this hold)
  └─ (any other event)                      → MAIN_PRESSED

Edge cases (verbatim test inputs in activation.rs):
- Chord fires DURING an in-progress meeting → emits MeetingToggle, which the
  runtime interprets as Stop. (Activation is a toggle.)
- Chord fires DURING a dictation hold → harmless. Dictation uses Right Alt;
  meeting hook only sees its configured modifier (default Right Ctrl) + main-key.
  The two hooks observe disjoint VK sets. If a user configures the meeting
  modifier to the same VK as the dictation hotkey, the conflict probe at startup
  rejects it.
- Pause-meeting tray toggle (Phase MC tray-menu addition) → activation events
  no-op until unpaused.
- User holds the chord for 5 seconds → fires once on the first main-keydown, then
  suppresses Windows key-repeat until main-keyup. NOT a spam.
- Modifier released while main is held → IDLE; subsequent main-keyup also IDLE
  (clean state, no half-fires).
- Main pressed before modifier → no fire (chord broken).
```

The state machine takes `ActivationEvent` inputs (`ModifierDown { ts }`, `ModifierUp { ts }`, `MainKeyDown { ts }`, `MainKeyUp { ts }`, `Tick { ts }`, `PauseToggle { paused }`) and emits `ActivationAction` outputs (`MeetingToggle { source }`, `Noop`). The `Tick` event is unused by the chord state machine itself (no timing windows like double-tap had) but is kept on the input enum for symmetry with future timing-based gestures. Test density target: ≥20 tests covering all chord edges (modifier-only, main-only, modifier-then-main fires-once, hold-chord-fires-once, release-and-re-press fires-again, modifier-released-first, main-released-first, paused-state, dictation-state-aware noop, etc.).

## Section MC.2 — Capture / chunking / STT pipeline

Binding spec for `meetings/capture.rs`, `meetings/chunker.rs`, `meetings/long_form_stt.rs`.

```
MEETING_THREAD (spawned by MeetingCaptureRuntime on MeetingToggle{Start})
  │
  ├─ build dep stack (all !Send-safe inside this thread):
  │     mic_capture = CpalCapture::new()                  (existing trait)
  │     [if source ∈ {System, Both}]
  │     sys_capture = LoopbackCapture::new()              (new trait impl)
  │     chunker = MeetingChunker::new(uuid, chunk_dir)
  │
  ├─ START loop (50 ms tick):
  │     drain mic_capture → mic_pcm_buf
  │     drain sys_capture → sys_pcm_buf  (if present)
  │     chunker.feed_mic(mic_pcm_buf)   →  emits chunk WAV every 30 s
  │     chunker.feed_sys(sys_pcm_buf)   →  emits chunk WAV every 30 s
  │     emit("meeting:tick", { elapsed_ms, mic_db, sys_db })  for UI
  │
  ├─ on MeetingToggle{Stop}:
  │     mic_capture.stop()
  │     [sys_capture.stop()]
  │     chunker.finalize()              →  flushes the partial trailing chunks
  │     emit("meeting:state", "transcribing")
  │
  ├─ LONG_FORM_STT pass (per channel, sequentially):
  │     for each chunk wav (in order):
  │         segments = stt.transcribe_segments(chunk_pcm, prev_tail_prompt)
  │         drop any segment whose t0 < overlap_boundary (already emitted by prev chunk)
  │         prev_tail_prompt = last ~64 chars of last-segment-text
  │         emit("meeting:progress", { channel, chunks_done, chunks_total })
  │     yield Vec<TimedSegment> per channel
  │
  ├─ FORMATTER pass (per channel — pure):
  │     formatted_text = formatter::format(&segments, &filler_set, &format_opts)
  │     emit("meeting:state", "formatting")
  │
  ├─ MERGE (if Both):
  │     merge mic + sys segments into a chronological view tagged
  │     "You: ..." / "Other(s): ..." per the speaker_labels setting
  │
  ├─ PERSIST:
  │     INSERT meeting_sessions(...)
  │     INSERT meeting_transcripts(stage='formatted', channel='mic') [+ sys]
  │     concat all chunk WAVs → meeting.wav under retention path
  │     emit("meeting:state", "done")
  │
  └─ Failure paths:
       any chunk-STT failure  → mark session status='partial', persist what we have
       OOM on system audio    → demote to Microphone-only mid-run, mark 'demoted'
       user closes app        → finalize on Drop, mark 'interrupted'
```

`MeetingChunker` is a pure-state struct (per-channel) whose only effect is writing chunk WAVs. It can be unit-tested by feeding synthetic PCM and asserting which WAV paths get produced at which sample positions.

## Section MC.3 — Deterministic formatter rules

Binding spec for `meetings/formatter.rs` + `meetings/filler_words.rs`. **Pure Rust. Zero RNG. Same input → same output, byte-for-byte. Property-testable.**

```
Inputs:
  segments: &[TimedSegment]    // (text, t0_ms, t1_ms), sorted by t0
  filler_set: &phf::Set<&'static str>
  opts: FormatOpts {
      paragraph_gap_ms: u32         // default 2000
      strip_fillers: bool           // default true
      strip_repeats: bool           // default true  (collapse "the the" → "the")
      capitalize_paragraph_starts: bool   // default true
      capitalize_sentence_starts: bool    // default true (after . ! ?)
      strip_leading_trailing_ws: bool     // default true
  }

Output:
  String  (the formatted transcript)

Algorithm:
  1. For each segment, tokenize on whitespace (preserve internal punctuation).
  2. Lowercase a copy of each token for filler-set lookup ONLY.
     If lowercase ∈ filler_set, DROP the original token.
     Multi-word filler phrases ("you know", "I mean") are matched greedy-longest
     by sliding the filler_set over the token stream with a 3-token max lookahead.
  3. If strip_repeats: collapse exact-match consecutive tokens after lowercase
     normalization (preserves the first occurrence's original case).
  4. Walk segments in order, joining tokens with single spaces.
     - Between segments, if (segment[i+1].t0_ms - segment[i].t1_ms) >= paragraph_gap_ms,
       insert "\n\n" (paragraph break).
     - Otherwise insert a single space.
  5. Capitalization pass (single forward walk):
     - First non-whitespace character → uppercase.
     - First non-whitespace character after "\n\n" → uppercase.
     - First non-whitespace character after [.!?] + whitespace → uppercase.
  6. Trim leading/trailing whitespace if opts.strip_leading_trailing_ws.

Invariants enforced by tests (≥25):
  - Empty input → empty output, no panic.
  - Single segment with no fillers → output == input (modulo capitalize-first).
  - "um uh um um the the cat" with strip_fillers+strip_repeats → "The cat".
  - Two segments with gap 1500 ms → joined with " ".
  - Two segments with gap 2500 ms → joined with "\n\n".
  - Filler phrase "you know" at start of sentence → dropped.
  - Filler word at end of segment (no trailing space) → no double-space artifact.
  - Multi-byte UTF-8 (CJK, emoji) → no panic, no character splitting.
  - Whisper-emitted "." punctuation preserved.
```

Test format: `rstest` parametrized + a `proptest` invariant that re-running the formatter on its own output is idempotent (formatter is a fixpoint).

## Section MC.4 — Schema (migration 011)

```sql
-- 011_meeting_capture.sql
-- Phase MC. Schema_version 10 → 11.

PRAGMA foreign_keys = ON;

BEGIN TRANSACTION;

CREATE TABLE meeting_sessions (
  id                       INTEGER PRIMARY KEY,
  uuid                     TEXT NOT NULL UNIQUE,
  title                    TEXT,                       -- user-given, nullable
  started_at               TEXT NOT NULL,              -- ISO-8601 UTC
  ended_at                 TEXT NOT NULL,              -- ISO-8601 UTC
  status                   TEXT NOT NULL,              -- 'complete'|'partial'|'demoted'|'interrupted'|'failed'
  error_message            TEXT,
  source                   TEXT NOT NULL,              -- 'mic'|'system'|'both'
  total_duration_ms        INTEGER NOT NULL,
  mic_duration_ms          INTEGER,
  sys_duration_ms          INTEGER,
  hotkey_pressed           TEXT NOT NULL,
  audio_blob_path          TEXT,                       -- canonical meeting.wav path
  whisper_model_id         TEXT NOT NULL,              -- e.g. 'whisper-large-v3-turbo-q5_0'
  formatter_version        TEXT NOT NULL,              -- 'mc-v1' — bump to regenerate transcripts later
  chunk_count_mic          INTEGER,
  chunk_count_sys          INTEGER,
  stt_latency_ms           INTEGER,
  formatter_latency_ms     INTEGER
);

CREATE INDEX idx_meeting_sessions_started ON meeting_sessions(started_at DESC);
CREATE INDEX idx_meeting_sessions_status  ON meeting_sessions(status, started_at DESC);

CREATE TABLE meeting_transcripts (
  id                       INTEGER PRIMARY KEY,
  meeting_session_id       INTEGER NOT NULL REFERENCES meeting_sessions(id) ON DELETE CASCADE,
  channel                  TEXT NOT NULL,              -- 'mic'|'system'|'merged'
  stage                    TEXT NOT NULL,              -- 'raw_segments'|'formatted'  (raw_segments is JSON)
  text                     TEXT NOT NULL,              -- formatted prose OR segments JSON
  created_at               TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(meeting_session_id, channel, stage)
);

CREATE INDEX idx_meeting_transcripts_session ON meeting_transcripts(meeting_session_id);

-- FTS5 search across formatted meeting transcripts.
-- Mirrors the existing transcripts_fts pattern; same porter+unicode61 tokenizer.
CREATE VIRTUAL TABLE meeting_transcripts_fts USING fts5(
  text,
  content='meeting_transcripts',
  content_rowid='id',
  tokenize='porter unicode61'
);

CREATE TRIGGER meeting_transcripts_fts_insert
  AFTER INSERT ON meeting_transcripts BEGIN
  INSERT INTO meeting_transcripts_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE TRIGGER meeting_transcripts_fts_delete
  AFTER DELETE ON meeting_transcripts BEGIN
  INSERT INTO meeting_transcripts_fts(meeting_transcripts_fts, rowid, text)
    VALUES('delete', old.id, old.text);
END;

UPDATE schema_meta SET value = '11' WHERE key = 'schema_version';

COMMIT;
```

**No edits to existing migrations.** This is purely additive (binding rule, ADR 0010 — append-only after `phase-1-complete`). No new `settings` rows in 011; instead, Phase MC introduces new typed `SettingKey` variants at the Rust layer (see Section MC.5) that lazy-default through the existing `default_value` path without a migration write.

## Section MC.5 — New typed settings

Add to `settings/model.rs::SettingKey`:

```rust
MeetingHotkeyModifier,          // string VK name; default "VK_RCONTROL"; allowed set {RCtrl, LCtrl, RAlt, LAlt, RShift, LShift, RWin, LWin}
MeetingHotkeyKey,               // string VK name; default "VK_M"; fallback "VK_F13" per ADR 0019 ladder
MeetingDefaultSource,           // string; "mic"|"system"|"both"; default "mic"
MeetingMaxDurationSeconds,      // u32; default 14400 (4 h), clamp [60, 21600]
MeetingFillerStripEnabled,      // bool; default true
MeetingParagraphGapMs,          // u32; default 2000, clamp [500, 10000]
MeetingAudioRetentionDays,      // u32; default = inherits AudioRetentionDays (30)
MeetingLlmPassEnabled,          // bool; default true (UI affordance available)
MeetingLastSelectedSource,      // string; UI state — last source picked by user
MeetingSpeakerLabelMic,         // string; default "You"
MeetingSpeakerLabelSys,         // string; default "Other(s)"
```

Round-trips via `as_str` + `try_parse` + `default_value` per the existing pattern (test-enforced — see `model.rs::tests::every_key_round_trips_via_as_str_and_try_parse`).

## Section MC.6 — IPC commands (Tauri)

New `#[tauri::command]` registrations in `commands/meetings.rs` (and `commands/mod.rs::register`):

| Command                         | Args                                  | Returns                  | Purpose |
|---------------------------------|---------------------------------------|--------------------------|---------|
| `meeting_probe_sources`         | —                                     | `MeetingSourceProbe`     | Lists available sources (mic always; system if loopback works; both if both); called by the overlay on open. |
| `meeting_start`                 | `{ source: 'mic'\|'system'\|'both' }` | `{ uuid: string }`       | Begins capture. Idempotent if already running (returns existing uuid + a warning event). |
| `meeting_stop`                  | `{ uuid: string }`                    | `()`                     | Stops capture. Transcription runs async; UI listens to `meeting:state` + `meeting:progress` events. |
| `list_meetings`                 | `{ limit?: number; offset?: number }` | `Vec<MeetingSummary>`    | History list. |
| `get_meeting_detail`            | `{ uuid: string }`                    | `MeetingDetail`          | Full session + all transcript channels. |
| `delete_meeting`                | `{ uuid: string }`                    | `()`                     | Cascade-deletes transcripts; unlinks audio blob. |
| `search_meeting_transcripts`    | `{ query: string }`                   | `Vec<MeetingMatch>`      | FTS over `meeting_transcripts_fts`. |
| `meeting_export_markdown`       | `{ uuid: string; include_llm_pass?: { id: string } }` | `{ path: string }` | Writes a markdown file to a user-chosen path via `tauri::api::dialog`. |
| `meeting_copy_to_clipboard`     | `{ uuid: string; include_llm_pass?: { id: string } }` | `()`             | One-shot `SetClipboardData(CF_UNICODETEXT)` — see ADR carry-forward. |
| `meeting_run_llm_pass`          | `{ uuid: string; prompt_id: string \| { custom: string }; model_id?: string }` | `{ id: string; text: string; latency_ms: number }` | Runs the optional LLM pass. Output is **NOT persisted** to DB. The `id` is a client-side handle for the export commands above. Server-side keeps it in a `HashMap<id, text>` keyed by a fresh UUID, evicted on app shutdown. |

Event names emitted to the meeting overlay + main window:

- `meeting:tick`  — `{ elapsed_ms, mic_db, sys_db }`
- `meeting:state` — `"capturing"|"transcribing"|"formatting"|"done"|"error"`
- `meeting:progress` — `{ channel, chunks_done, chunks_total }`
- `meetings:session-saved` — fires after the session row is committed (so the History/Meetings page can live-refresh, mirroring `history:session-saved` for dictation).

## Task waves

Priority key: **P0** blocks the wave; **P1** must ship in the wave; **P2** ships in the wave if cheap, otherwise documents the deferral; **P3** stretch.

**Brief-pattern reminder (cross-wave):** code-puppy authors `docs/phases/phase-mc-waveN-brief.md` at the end of each wave for N+1. Same convention as Phase 3.

### Wave 1 — Decisions, ADRs, deps, AppError, scaffolds (Iteration 1)

| bd-task title (prefix `Phase MC:`) | priority | files |
|-----------------------------------|----------|-------|
| ADR 0026 — Meeting Capture is a sibling subsystem | P0 | `docs/adr/0026-meeting-sibling-subsystem.md` |
| ADR 0027 — Chord activation (Right Ctrl + M) via dedicated WH_KEYBOARD_LL listener on a dedicated meetings thread | P0 | `docs/adr/0027-chord-activation.md` |
| ADR 0028 — Two-channel capture via twin cpal streams | P0 | `docs/adr/0028-twin-stream-capture.md` |
| ADR 0029 — Long-form chunked Whisper (closes `mb-2bi`) | P0 | `docs/adr/0029-long-form-chunked-whisper.md` |
| ADR 0030 — Whisper segment exposure (`transcribe_segments`) | P0 | `docs/adr/0030-whisper-segment-exposure.md` |
| Cargo deps (`crc32fast`) + AppError MeetingCapture/LongFormStt/Formatter variants + module scaffolds (traits in `mod.rs`, `todo!()` macOS/Linux stubs per binding rule §15) | P0 | `src-tauri/Cargo.toml`, `src-tauri/src/error.rs`, all files under `meetings/` |
| New `SettingKey` variants + every-key-round-trips test extension | P0 | `src-tauri/src/settings/model.rs` |
| Migration 011 authored + applied; tests in `src-tauri/tests/db_migrations.rs` extended to verify it | P0 | `src-tauri/src/db/migrations/011_meeting_capture.sql`, `src-tauri/tests/db_migrations.rs` |
| Hook `block-cross-module-coupling-meeting-dictation` authored + dry-run-passes on Wave 1 diff | P1 | `scripts/hooks/block-cross-module-coupling-meeting-dictation.py`, `.code_puppy/settings.json` (registration) |

### Wave 2 — Pure state machine + formatter + filler-set + chunker (Iteration 2)

| bd-task title (prefix `Phase MC:`) | priority | files |
|-----------------------------------|----------|-------|
| `meetings/activation.rs` — pure chord (modifier + main-key) state machine per Section MC.1 with ≥20 unit tests | P0 | `src-tauri/src/meetings/activation.rs` |
| `meetings/formatter.rs` + `meetings/filler_words.rs` — pure formatter per Section MC.3 with ≥25 unit tests + proptest fixpoint invariant | P0 | `src-tauri/src/meetings/{formatter,filler_words}.rs` |
| `meetings/chunker.rs` — pure-state chunker (input: PCM samples + sample-clock; output: chunk-WAV write events) with ≥12 unit tests (boundary at 30 s, overlap at 28 s, finalize at <30 s residual, multi-channel separation) | P0 | `src-tauri/src/meetings/chunker.rs` |
| `stt::SpeechToText::transcribe_segments` — trait method + `WhisperStt` impl + 4 unit tests against existing fixture WAVs (assert segment count > 0, t0 < t1 monotonic, joined text == existing `transcribe` text up to whitespace) | P0 | `src-tauri/src/stt/{mod,whisper}.rs` |

### Wave 3 — Twin-stream capture + loopback + long-form STT driver (Iteration 3)

| bd-task title (prefix `Phase MC:`) | priority | files |
|-----------------------------------|----------|-------|
| `meetings/loopback_windows.rs` — `LoopbackCapture` impl of `AudioCapture` trait against the default render endpoint (cpal loopback OR `wasapi` crate; decision documented in wave brief) | P0 | `src-tauri/src/meetings/loopback_windows.rs` |
| `meetings/capture.rs` — `TwinStreamCapture` coordinator; manages mic + (optional) loopback; per-channel ringbufs; clock-aligned drain; integration test with a synthetic PCM source proving deterministic capture-end timestamps | P0 | `src-tauri/src/meetings/capture.rs` |
| `meetings/long_form_stt.rs` — chunked driver: walks chunk WAVs, calls `transcribe_segments` per chunk with rolling `initial_prompt`, drops overlap, emits progress events; integration test against a 90 s synthetic fixture (3 chunks) verifying stitch is loss-less and overlap dedup is correct | P0 | `src-tauri/src/meetings/long_form_stt.rs` |
| `meetings/activation.rs` — wire to a second `WH_KEYBOARD_LL` install on the existing message-pump thread; respects the dictation hook's chain order via `CallNextHookEx` (synthetic-event integration test) | P0 | `src-tauri/src/meetings/activation.rs` (impl side), `src-tauri/tests/meeting_activation_integration.rs` |
| Conflict probe: if `MeetingHotkeyModifier` equals the dictation `hotkey.binding` VK, reject at startup with tray toast + log; fallback ladder `VK_RCONTROL`+`VK_M` → `VK_RCONTROL`+`VK_F13` → `VK_RCONTROL`+`VK_F14` → user-pick (UI-pick deferred to Wave 5) | P1 | `src-tauri/src/meetings/activation.rs` + extend `src-tauri/src/hotkey/probe.rs` interface |

### Wave 4 — Persist + UI + LLM pass + export (Iteration 4 — the heavy wave; HUMAN-IN-LOOP)

| bd-task title (prefix `Phase MC:`) | priority | files |
|-----------------------------------|----------|-------|
| `meetings/persist.rs` — atomic insert (session row + 1–3 transcript rows + audio blob path); failure non-fatal for individual transcript rows (mirrors Wave 4.9 Bug A fix) | P0 | `src-tauri/src/meetings/persist.rs` |
| `meetings/runtime.rs` — full lifecycle wiring (start → capture → stop → long-form-stt → formatter → merge → persist → emit done); Drop tears down cleanly mid-recording (mark `interrupted`) | P0 | `src-tauri/src/meetings/runtime.rs` |
| `meetings/llm_pass.rs` — instantiates a fresh `OllamaProvider` per call (reuses existing `cleanup/ollama.rs`); builds prompt = system header + selected prompt body (markdown file) + transcript body; in-memory `HashMap<Uuid, String>` keyed cache for exports; eviction on app shutdown | P0 | `src-tauri/src/meetings/llm_pass.rs`, `src-tauri/src/meetings/prompts/*.md` |
| `meetings/export.rs` + `meetings/clipboard.rs` — markdown serializer (frontmatter: title, started_at, duration, source; body: speaker-tagged paragraphs) + one-shot clipboard helper (no save/restore — see Cargo-deps note) | P0 | `src-tauri/src/meetings/{export,clipboard}.rs` |
| Tauri commands per Section MC.6, registered in `commands/mod.rs::register` | P0 | `src-tauri/src/commands/meetings.rs`, `src-tauri/src/commands/mod.rs` |
| `meeting_overlay` Tauri window declared in `tauri.conf.json`; React entry point + activation UI (source picker + Start + cancel) | P0 | `src-tauri/tauri.conf.json`, `ui/src/meeting_overlay.tsx`, `ui/src/meeting_overlay/MeetingOverlay.tsx` |
| `ui/src/pages/Meetings.tsx` (history list) + `MeetingDetail.tsx` (transcript view + Copy / Export / LLM-pass panel) + `ui/src/lib/meetings.ts` typed IPC + Sidebar nav link | P0 | listed paths + `ui/src/components/Sidebar.tsx`, `ui/src/App.tsx` route registration |
| Hands-on QA matrix run — **requires human at keyboard.** Test scenarios: 60 s mic-only meeting; 60 s system-only meeting (YouTube tab); 60 s Both with you-and-a-podcast-talking-over-each-other; 10 min mic-only stress; 4-hour idle-loop endurance (overnight; checks no memory leak via Task Manager working-set delta < 200 MB). Record results in `docs/phases/phase-mc-qa-matrix.md`. | P0 | `docs/phases/phase-mc-qa-matrix.md` |

### Wave 5 — Tray toggle + LLM-pass save-as-file + polish + brief (Iteration 5)

| bd-task title (prefix `Phase MC:`) | priority | files |
|-----------------------------------|----------|-------|
| Tray-menu: "Pause meeting hotkey" toggle wired through `ActivationEvent::PauseToggle` (mirrors dictation's pause toggle) | P1 | `src-tauri/src/tray.rs` |
| Settings UI surface for the new `SettingKey` variants (hotkey rebind via the existing conflict-probe; default source dropdown; filler-strip toggle; paragraph gap slider; speaker label inputs) | P1 | `ui/src/pages/Settings.tsx` |
| `meeting_export_markdown` "Save As…" dialog — Markdown frontmatter + body; optional `--include-llm-pass <id>` appends the LLM output as a trailing section | P1 | `src-tauri/src/commands/meetings.rs`, `ui/src/pages/MeetingDetail.tsx` |
| Live `meeting:progress` chunk counter wired into the MeetingDetail view ("transcribing 4/12") | P2 | `ui/src/pages/MeetingDetail.tsx` |
| Accessibility pass: reduced-motion respected on the overlay; keyboard focus order in MeetingDetail; ARIA labels on the Copy/Export buttons | P2 | `ui/src/meeting_overlay/MeetingOverlay.tsx`, `ui/src/pages/MeetingDetail.tsx` |

### Wave 6 — Judges, retrospective, seal (Iteration 6)

| bd-task title (prefix `Phase MC:`) | priority | files |
|-----------------------------------|----------|-------|
| 5 judge cards + JSON entries: `mc-formatter-deterministic`, `mc-long-form-stitched-losslessly`, `mc-two-channel-merged`, `mc-no-llm-in-critical-path`, `mc-dictation-untouched` | P0 | `docs/judges/phase-mc/*.md`, `.code_puppy/judges-template.json` |
| Retrospective in `docs/LESSONS.md` (`[phase-mc-retrospective]` tag) + STATUS.md update + bd close all Phase-MC issues | P0 | `docs/LESSONS.md`, `STATUS.md` |
| Cargo gate green (release): `cargo check + clippy --release -D warnings + test --release + fmt --check` → seal commit → `git tag phase-mc-complete` | P0 | git |
| Close standing P1 `mb-2bi` (audio streaming + chunked Whisper) — Phase MC delivers it | P0 | bd |

## Cross-wave invariants

1. **Dictation is sealed for this phase.** No edits to `hotkey/state.rs`, `hotkey/windows.rs`, `hotkey/driver.rs`, `dictation/`, `injection/`, `recording_window.rs`, `cleanup/provider.rs`, `cleanup/llm_cleaner.rs`, or migrations 001–010. The pre-commit hook `block-cross-module-coupling-meeting-dictation` enforces this; violations fail CI.
2. **`modes` table is not touched.** No new mode rows. The meeting LLM-pass picker reads from `meetings/prompts/*.md` (file-backed) and an optional user-custom prompt passed inline to `meeting_run_llm_pass` — *not* a `modes` row.
3. **`CleanupProvider` trait shape is sealed.** Phase MC instantiates `OllamaProvider` through its existing `pub fn new() -> Self` constructor (or `with_base_url(...)` for tests) and passes the LLM-pass parameters (`model_id`, `temperature`, `max_tokens`, the assembled prompt) per-call via the existing `CleanupRequest<'_>` struct — no new trait method, no new variant. If the LLM-pass surface area grows to need a multi-prompt batch API later (Phase MC+1), it's a fresh trait, not an extension of `CleanupProvider`.
4. **`SpeechToText::transcribe` signature is sealed.** New segment-returning method is additive (`transcribe_segments`). Dictation calls the original; meeting calls the new one. 383 existing tests stay green without modification.
5. **No LLM in the critical path.** The path from `meeting_stop` to `meeting:state = done` MUST NOT make an HTTP call to Ollama or any LLM. Judge `mc-no-llm-in-critical-path` asserts via code-review and via runtime instrumentation (`OllamaProvider` is wrapped in a tracer-counting handle during the integration test that exercises the critical path; counter must be 0).
6. **Formatter is pure and deterministic.** Same input → same output, byte-for-byte. Proptest invariant: `format(format(x)) == format(x)` (fixpoint). No RNG, no system clock, no global state.
7. **File size hard limit: 600 lines.** `runtime.rs`, `capture.rs`, `long_form_stt.rs` will press; pre-split helpers into siblings before hitting 500.
8. **Test density target: ~10 tests per ~500 LoC of pure code.** Pure modules (`activation`, `formatter`, `chunker`) target 25–30 tests each. Wired modules (`runtime`, `capture`, `long_form_stt`) target 6–10 tests each (their job is glue; the pure modules they call carry the coverage). Phase MC's total test-count delta target: **+90 to +120 tests** across ~3500 new lines of Rust; total project test count exits at **~470–500** (current is 383).
9. **Cross-platform traits from day one.** Every new module pairs a trait with `#[cfg(target_os = "windows")]` impl + macOS/Linux `todo!()` stubs. Wave 1 lays all stubs; later waves only flesh out the Windows side.
10. **`tracing` only — no `println!` outside CLI harnesses.** Carried forward from Phase 2.
11. **Brief pattern.** Code-puppy authors `docs/phases/phase-mc-waveN-brief.md` at end-of-wave-N for each transition (1→2, 2→3, 3→4, 4→5, 5→6).
12. **The cargo gate is four green lights:** `cargo check + clippy --release -D warnings + test --release + fmt --check`. Clippy MUST be `--release` to reuse the whisper-rs-sys artifacts (LESSONS 2026-05-15).
13. **Migrations stay sealed.** Migration 011 is the *only* schema change in Phase MC. If a defect is found post-seal, repair via 012+, never edit 011 in place.
14. **Audio blob retention follows the existing `audio_retention_days` setting** (with a per-meeting override via `MeetingAudioRetentionDays`). Phase MC does not introduce a new audio-cleanup daemon; it adds meeting blobs to the same retention sweep that dictation blobs ride on (re-uses the existing retention path; LESSONS to confirm whether one exists today or whether this is a deferral — Wave 1 brief settles this).

## Exit criteria

1. **Functional:** Right Ctrl + M chord with no app focused → overlay appears, source pickable, Start/Stop works, transcript appears in the new Meetings page on stop. Verified manually by Dustin in:
   - 30 s mic-only meeting → transcript correct, fillers stripped, paragraph breaks at ≥2 s pauses.
   - 30 s system-only meeting against a YouTube tab → transcript correct.
   - 30 s Both with you talking and a podcast in the background → mic transcript labeled "You", system labeled "Other(s)", both visible in chronological order.
   - 10 min stress test → no chunk-stitch artifacts at the 30/60/120/… second boundaries; full audio playable from the canonical WAV.
2. **No-LLM-in-critical-path:** Judge runtime instrumentation confirms zero Ollama HTTP calls between `meeting_stop` and `meeting:state = done`.
3. **Dictation regression:** All 383 existing tests still green; manual smoke of a dictation hold confirms no behavioural change.
4. **Cargo gate:** `cargo check + clippy --release -D warnings + test --release + fmt --check` all four green. ~470–500 tests total.
5. **Judges green:** all 5 new judges (`mc-formatter-deterministic`, `mc-long-form-stitched-losslessly`, `mc-two-channel-merged`, `mc-no-llm-in-critical-path`, `mc-dictation-untouched`) PASS; all carry-forward judges still PASS.
6. **ADRs 0026–0030 present** with `Status: Accepted`.
7. **bd `mb-2bi` closed** with link back to ADR 0029.
8. **STATUS.md** updated: meeting capture epic marked sealed; `phase-mc-complete` tag recorded.
9. **`git tag --list "phase-*"`** includes `phase-mc-complete`.

## Risks & mitigations

| # | Risk | Severity | Mitigation |
|---|------|----------|------------|
| 1 | **Loopback unavailable on the target machine** (exclusive-mode audio, headless audio stack, unusual driver) | **High** | Probe at "Both" / "System" selection time; if loopback fails, demote to mic-only and surface a toast. UI hides the System/Both options when the probe fails. Don't crash the meeting. |
| 2 | **Whisper segment stitching artifacts** at chunk boundaries (duplicated phrase, dropped word) | **High** | 2 s overlap + rolling `initial_prompt` from the prior chunk's tail; integration test against a 90 s synthetic fixture verifies stitch is loss-less. Manual QA on a 10 min real meeting before seal. If real-world drops appear post-seal, hotfix overlap to 3 s + LESSONS entry. |
| 3 | **Memory blow-up on a long meeting** (ringbuf swelling because the chunker can't drain fast enough) | **Medium** | Chunker writes WAVs every 30 s; ringbuf only ever holds ≤32 s of PCM live. Watchdog: if ringbuf fill exceeds 75 % for >2 ticks, log + flush + drop the trailing 5 s (with a toast — "audio dropped, transcript may have a gap"). |
| 4 | **Chord conflicts with an app-level shortcut** (e.g. an IDE that uses Right Ctrl + M for line-wrap toggle) | **Medium** | Conflict probe at startup checks the configured modifier + main-key against the dictation hotkey only (we can't enumerate every app's shortcut surface). Setting is exposed in Settings → Meeting → Activation with the allowed-modifier dropdown + any-VK main-key picker. Fallback ladder `RCtrl+M` → `RCtrl+F13` → `RCtrl+F14` → user-pick. Users seeing a conflict in their IDE rebind to a different main-key. |
| 5 | **Whisper model load latency on first transcribe** (10–30 s on first cold start) | **Low** | Model is already loaded for dictation; the meeting feature reuses the same `WhisperStt` instance via a lazy-init handle on the meeting thread. First meeting after app boot pays the load; subsequent meetings re-use it. |
| 6 | **User fires chord during a dictation hold** | **Low** | Disjoint VK sets — dictation uses Right Alt, meeting uses Right Ctrl + M. Each hook observes only its own keys; zero cross-talk. Conflict probe at startup rejects same-VK modifier configuration. If a user somehow forces overlap (manual settings edit), the meeting hook's `Noop` branch handles the case (it ignores activation if the dictation state is `Recording`, queried via a non-blocking peek into the dictation state). |
| 7 | **LLM-pass output non-determinism** — user runs a summary twice, gets different output, complains | **Low** | UI shows "this output isn't saved; export it now if you want to keep it" the first time the user opens the LLM-pass panel in a session. Setting `temperature = 0` on Ollama doesn't fully fix it (sampling stochasticity remains), but it tightens the variance. Sequential runs that disagree fall on the user. |
| 8 | **Clipboard data loss on `meeting_copy_to_clipboard`** | **Low** | One-shot `SetClipboardData(CF_UNICODETEXT)` — the user *intends* to put the transcript on the clipboard, so no save/restore. Markdown export to file is the durable path. Document in MeetingDetail's UI ("Copy replaces your clipboard"). |
| 9 | **Meeting overlay window steals focus** | **High** (mirrors dictation's `ADR 0016 §7`) | `focus: false`, `decorations: false`, `alwaysOnTop: true`, `skipTaskbar: true` declared in `tauri.conf.json`. Never `.set_focus()` on the overlay. The "Start" button is the *only* interactive element; the Stop UI is a small floating chip in the same non-activating window. |
| 10 | **Two-channel timestamp drift** (mic clock vs loopback clock skew over a 4-hour meeting) | **Medium** | Both streams stamp on capture start (`Instant::now()`); per-chunk timestamps are sample-counted (samples_so_far / 16000), not wall-clock. The two channels remain locally consistent. Cross-channel drift over 4 h on typical hardware is <100 ms; documented but not eliminated. If a user reports >500 ms drift, fall back to per-segment wall-clock anchoring (LESSONS extension). |

## Iteration estimate

**5–6 iterations**, honest reasoning:

| Iteration | Wave | Why this fits in one iteration |
|-----------|------|------------------------------------------------------------|
| 1 | Wave 1 | ADRs + scaffolds + AppError + migration 011 + hook + settings keys. No platform code yet. Mirrors Phase 3 Wave 1 in scope. |
| 2 | Wave 2 | Pure Rust: activation state machine + formatter + filler-set + chunker + STT segment method. Six pure modules; each is independently testable. Heavy in test count, light in integration risk. |
| 3 | Wave 3 | First wave to touch WASAPI loopback. Real chance of "cpal doesn't support loopback the way I thought" costing a half-iteration to swap to the `wasapi` crate. Buffer is baked in. The long-form STT driver also lands here — its integration test against a 90 s fixture is non-trivial. |
| 4 | Wave 4 | **Heavy.** Persistence + runtime wiring + Tauri commands + two new webview windows + the entire React-side Meetings page + LLM-pass + export + clipboard + hands-on QA matrix. The QA matrix alone is a half-day of Dustin's keyboard time. |
| 5 | Wave 5 | Tray toggle + Settings UI + Save-As dialog + progress chunk counter + accessibility. Buffer for one 5-attempt-rule escalation if Wave 4 ran long. |
| 6 | Wave 6 | Judges + retrospective + seal. Closes `mb-2bi`. |

**Phase 3 hit 5 iterations including the 4.9 mini-wave.** Phase MC has more total surface area than Phase 3 (UI side is bigger) but less Win32 risk (no clipboard surgery, no secure-input edge cases, no per-app paste matrix). 6 iterations is the realistic estimate; **5 only if Wave 3's loopback path comes back clean on first try** (it might not — WASAPI loopback has been known to vary by Windows build).

## Judge roster at phase exit

| Judge | Origin | Run? | Notes |
|---|---|---|---|
| All carry-forward judges (Phase 0–4) | — | YES | Including `stt-correct`, `db-provenance`, `clipboard-restored`, `secure-input-respected` — dictation must be unchanged. |
| **`mc-formatter-deterministic`** *(new)* | Phase MC | YES | `format(x)` is a fixpoint and produces identical output across 1000 randomized runs (proptest harness). |
| **`mc-long-form-stitched-losslessly`** *(new)* | Phase MC | YES | 90 s synthetic fixture → 3 chunks → stitched transcript equals the single-pass transcript on the full fixture within an edit-distance threshold of 0.5 % (allowing for Whisper's chunk-context jitter). |
| **`mc-two-channel-merged`** *(new)* | Phase MC | YES | Two synthetic PCM streams, known overlap pattern → merged transcript shows correct interleaving and correct speaker labels. |
| **`mc-no-llm-in-critical-path`** *(new)* | Phase MC | YES | Runtime instrumentation counts zero Ollama HTTP calls between `meeting_stop` and `meeting:state = done`. |
| **`mc-dictation-untouched`** *(new)* | Phase MC | YES | All 383 pre-MC tests pass byte-identically; static check: `git diff phase-4-complete..HEAD -- src-tauri/src/hotkey src-tauri/src/dictation src-tauri/src/injection src-tauri/src/cleanup/provider.rs src-tauri/src/recording_window.rs` is empty. |

Five NEW judge prompts authored in Wave 6; cards live under `docs/judges/phase-mc/`.

## Out of scope (DEFER)

- **Speaker diarization within a single channel** (telling apart two voices on the mic). Deferred to a possible "Phase MC.2" once two-channel feedback comes in. Pyannote-onnx or NeMo would be the implementation path; both add ~80–150 MB of model weight and a meaningful CPU cost. Don't build it without a real user request.
- **Live transcription during the meeting** (streaming Whisper view as the meeting goes). Phase MC ships post-stop transcription only. A streaming variant is feasible (chunks are already 30 s; the formatter is pure) but adds UI complexity and a non-trivial test matrix; defer.
- **Per-attendee identity** (i.e. mapping "Other(s)" → a named participant). Out of v1; would require either calendar integration or a manual labeling UI.
- **Translation** (transcribe-in-X, output-in-Y). Whisper supports it via its translate task flag; the formatter doesn't change. Defer to a future "languages" epic.
- **macOS / Linux capture impls.** Stubs only; full impls land in the platform-expansion epic.
- **Cloud sync of transcripts.** Local-only forever (principle #4).
- **A `corrections` learning-loop tie-in.** The dictation `corrections` table is wired to the dictation `transcripts` rows; meeting transcripts don't feed the learning loop in v1.
- **Custom user prompts saved across sessions.** v1 supports a free-text "custom prompt" in the LLM-pass panel, but the prompt is ephemeral. A "save custom prompts" UI is a future polish.

---

## CodePuppy handoff prompt

> Copy the block below directly into CodePuppy when you're ready to start Wave 1.

```
You are implementing Phase MC — Meeting Capture for the Mockingbird app
(local-first voice dictation, Tauri 2 + Rust + React). The full plan
is at docs/phases/phase-meeting-capture.md — READ IT END-TO-END FIRST,
then re-read STATUS.md, .code_puppy/AGENTS.md, and any LESSONS entries
that reference dictation, audio, or migrations.

Scope: build the meeting-recording feature as a SIBLING subsystem to
dictation. Bindings:

  * Do NOT modify src-tauri/src/hotkey/state.rs, hotkey/windows.rs,
    hotkey/driver.rs, dictation/, injection/, recording_window.rs,
    cleanup/provider.rs, cleanup/llm_cleaner.rs, or migrations 001–010.
  * Do NOT add a row to the modes table. Meeting LLM prompts live in
    src-tauri/src/meetings/prompts/*.md (markdown files), not in the DB.
  * Do NOT extend the CleanupProvider trait. The optional LLM pass
    constructs an OllamaProvider via its existing arg-less new() and
    drives it through the existing CleanupRequest<'_> per call.
  * Do NOT add an LLM call to the critical recording-to-canonical-
    transcript path. The deterministic formatter is the canonical pass.

Reuse only: audio::AudioCapture trait (existing), stt::SpeechToText
trait (extended with a new transcribe_segments method per ADR 0030),
and cleanup::OllamaProvider's existing public constructor.

Execute in the wave order documented in phase-meeting-capture.md:
  Wave 1 → Wave 2 → Wave 3 → Wave 4 → Wave 5 → Wave 6.

End every wave by authoring docs/phases/phase-mc-waveN-brief.md for
wave N+1 (Phase 3 / Phase 4 pattern). Each brief includes: type
definitions, function signatures, test specs with inputs/expected
outputs, deviations from this plan with justification, and the
cargo-gate checklist.

Test density target: ~10 tests per 500 LoC of pure code; pure modules
(activation, formatter, chunker) target 25–30 tests each. Total test-
count delta target: +90 to +120 tests. Exit project test count:
~470–500.

Cargo gate (must be green at every wave seal):
  cargo check
  cargo clippy --release -- -D warnings
  cargo test --release
  cargo fmt --check

Five new judges land in Wave 6: mc-formatter-deterministic,
mc-long-form-stitched-losslessly, mc-two-channel-merged,
mc-no-llm-in-critical-path, mc-dictation-untouched.

Open standing P1 mb-2bi (audio streaming + chunked Whisper) — Phase
MC ADR 0029 is its closer. Close it in Wave 6 alongside the seal tag
phase-mc-complete.

If anything in this plan conflicts with the codebase as you read it,
STOP and ask before deviating. The plan was written against the post-
phase-4-complete tag; if HEAD has moved meaningfully, surface the
delta in the Wave 1 brief and propose a reconciliation before
authoring any ADR.
```
