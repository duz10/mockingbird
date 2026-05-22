# Phase 10 — Activity Capture (numbered PLAN §10 phase)

**Phase entry tag:** `stable-alpha-v0.1` (post-MC + post-dictation-polish reference checkpoint, 2026-05-24).
**Phase exit tag:** `phase-10-complete` (target — adds `Activity` AppError variants, migration 012 (`activity_sessions`, `activity_events`, `activity_blocks`, `activity_transcript_segments`), new `src-tauri/src/activity/` module tree, new chord listener + UIA sampler threads, new `recording_indicator` Tauri overlay window, new `Activity.tsx` + `ActivityDetail.tsx` UI pages, five Wave-6 invariant judges. **Does NOT touch** dictation, meeting-capture-except-`long_form_stt`-as-a-library, the `modes` table, the `cleanup_provider` trait, migrations 001–011, or any sealed file enumerated in ADR 0036 §Decision items 1 & 2).
**Charter ADR:** [ADR 0036](../adr/0036-activity-capture-sibling-subsystem.md) — Proposed; Dustin flips to Accepted after charter review (this Wave 0).
**Source plan:** `mockingbird-activity-capture-plan.md` (repo root, untracked).
**Planner:** Bernard / code-puppy (Wave 0). **Implementor:** code-puppy unless a project JSON agent (e.g. `migration-author` for Wave 1's migration 012) takes a narrow wave.
**Estimated iterations:** 6–8 (one per Wave, possibly two for Waves 2 + 3, possibly one shared for Waves 5 + 6 if the smoke matrix is small).

> **Numbered PLAN §10 phase.** Mirrors Phase MC's container shape: numbered + ADR-chartered + per-wave seal tags + final `phase-10-complete` tag. Phase 9 stays reserved for the macOS cross-platform sweep (PLAN §2.1). Activity Capture is a sibling subsystem to dictation + meeting capture; everything outside the four shared primitives is greenfield.

## Status

Wave 0 (this doc + ADR 0036 + PLAN amendment + bead epic) authored 2026-05-25. ADR 0036 is **Proposed**; Wave 1 cannot start until Dustin flips it to **Accepted**. Stop-and-hand-back-for-review is the explicit end of Wave 0.

## Overview

Activity Capture records what the user did during a session and produces a chronological, human-readable summary at session end. The full vision lives in `mockingbird-activity-capture-plan.md`; this doc is the wave-by-wave implementation brief against it.

**Three independent capture layers feeding one merge-and-summarize pipeline:**

1. **Layer 1 — Activity events (primary signal).** Foreground-app + window-title sampling, focused-field snapshot, visible UI text fragments, control structure, app-switch + idle transitions. From the OS accessibility layer (Windows UIA in v1; macOS AX / Linux AT-SPI behind `#[cfg(target_os)]` stubs from day one). Wave 1 ships titles-only as the structural skeleton; Wave 2 deepens to full snapshots.

2. **Layer 2 — Audio (Wave 4).** Microphone capture + local chunked transcription, opt-in per session. Reuses `meetings::long_form_stt` as a library; does not extend it.

3. **Layer 3 — Screenshot fallback (Wave 7, OPTIONAL, post-seal).** Periodic stills + local OCR for accessibility-blind apps. NOT part of `phase-10-complete`.

**Pipeline at session-stop:**

```
[Layer 1 events] + [Layer 2 segments] + [Layer 3 OCR (W7)]
    │
    ▼
merge & normalize (pure Rust: dedupe redundant "still in <app>" snapshots, resolve overlaps, time-order)
    │
    ▼
segment into Blocks (pure Rust: contiguous events grouped by app + document, broken on app-switch + idle)
    │
    ▼
abstract per Block (Stage 3 — Ollama via OllamaProvider::new() + CleanupRequest<'_>, per-Block prompt;
                   degrades gracefully — if Ollama down, Block.abstract is empty, UI shows raw-only)
    │
    ▼
assemble session summary (pure Rust: chronological list of Blocks, optional totals, markdown export)
```

**No injection.** Output lives in a new `Activity.tsx` page (list of sessions) + `ActivityDetail.tsx` (one session: timeline + Block CRUD + drill-down to raw events + export).

**Activation:** chord `Right Ctrl + ,` (proposed — sits next to MC's `Right Ctrl + .` for muscle-memory). Conflict probe at startup per ADR 0019 ladder. Wave 1 wires it.

**Persistence (migration 012, Wave 1):**

```sql
-- Skeleton (Wave 1) — full DDL drafted in Wave 1's brief.
CREATE TABLE activity_sessions (
  id TEXT PRIMARY KEY,                -- ULID
  started_at INTEGER NOT NULL,        -- unix epoch ms (Tauri-side Date.now())
  ended_at INTEGER,                   -- NULL until Stop
  status TEXT NOT NULL,               -- 'in_progress' | 'completed' | 'crashed_recovered' | 'partial'
  audio_enabled INTEGER NOT NULL,     -- 0/1; opt-in per session
  screenshot_enabled INTEGER NOT NULL,-- 0/1; Wave 7 only — Waves 1-6 always 0
  label TEXT,                         -- user-rename; defaults to start-time auto-title
  project_id TEXT,                    -- Q6: schema-future-proofed, NO UI in v1
  project_label TEXT,                 -- Q6: schema-future-proofed, NO UI in v1
  summary_markdown TEXT,              -- final assembled summary; NULL until session ends
  prompt_set_sha TEXT,                -- SHA of the prompts/*.md set used for Stage-3 abstraction
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE activity_events (        -- RAW. IMMUTABLE. Principle 1. Trigger blocks UPDATE/DELETE.
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES activity_sessions(id) ON DELETE CASCADE,
  ts INTEGER NOT NULL,
  kind TEXT NOT NULL,                 -- 'app_switch' | 'context_snapshot' | 'idle_start' | 'idle_end' | 'paused' | 'resumed' | 'layer_error'
  app_name TEXT,
  window_title TEXT,
  snapshot_json TEXT,                 -- Wave 1: NULL or {title-only}. Wave 2: full UIA snapshot.
  created_at INTEGER NOT NULL
);

CREATE TABLE activity_blocks (        -- DERIVED. EDITABLE (Wave 3). Cosmetic edits only (Q7).
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES activity_sessions(id) ON DELETE CASCADE,
  started_at INTEGER NOT NULL,
  ended_at INTEGER NOT NULL,
  primary_app TEXT,
  generated_abstract TEXT,            -- LLM Stage-3 output; NULL if Ollama unavailable
  user_edited INTEGER NOT NULL DEFAULT 0,
  source_event_ids TEXT NOT NULL,     -- JSON array; provenance
  prompt_version_sha TEXT,            -- per-Block, references the abstractor prompt SHA
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE activity_transcript_segments ( -- Wave 4. Optional Layer-2.
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES activity_sessions(id) ON DELETE CASCADE,
  started_at INTEGER NOT NULL,
  ended_at INTEGER NOT NULL,
  text TEXT NOT NULL,
  source TEXT NOT NULL,               -- 'mic' (v1) | future 'system' if loopback ever added
  created_at INTEGER NOT NULL
);

CREATE TRIGGER activity_events_no_update BEFORE UPDATE ON activity_events
  BEGIN SELECT RAISE(ABORT, 'activity_events is immutable (Principle 1)'); END;

CREATE TRIGGER activity_events_no_delete BEFORE DELETE ON activity_events
  WHEN (SELECT 1 FROM activity_sessions WHERE id = OLD.session_id AND status != 'in_progress')
  BEGIN SELECT RAISE(ABORT, 'cannot delete events from a completed session; delete the session instead'); END;
-- (CASCADE-on-session-delete is the only delete path — the trigger guards stray DELETEs.)

CREATE INDEX activity_events_session_ts ON activity_events(session_id, ts);
CREATE INDEX activity_blocks_session_started ON activity_blocks(session_id, started_at);
CREATE INDEX activity_segments_session_started ON activity_transcript_segments(session_id, started_at);
-- FTS5 on activity_blocks.generated_abstract: added in Wave 3 (not needed for Wave 1's title-only timeline).
```

## Pre-flight — work sealed in Wave 0 (this iteration)

- **ADR 0036** authored, Status: Proposed.
- **`docs/phases/phase10.md`** (this file) authored.
- **PLAN-mockingbird-v2.md §10** — Phase 10 entry added.
- **Beads** — parent epic + 6 wave children + 1 independent investigation bead created.
- **STATUS.md** — Currently active block flipped from "No in-flight epic" to "Phase 10 Wave 0 — chartering."

## Wave structure

### Wave 1 — Activity-log skeleton (Layer 1 titles-only)

**Goal:** A user can press `Right Ctrl + ,`, see a recording indicator, switch between a few apps, press `Right Ctrl + ,` again to stop, and see a chronological raw-events timeline in a new `Activity.tsx` page. No AI. No audio. No snapshots. Title-level signal only — proves the structural skeleton end-to-end.

**Deliverables (files to create / touch):**

- `src-tauri/src/activity/mod.rs` — module owner, public types (`ActivitySessionId`, `ActivityEventKind`, `ActivitySession`), `ActivityCaptureRuntime` struct stub.
- `src-tauri/src/activity/lifecycle.rs` — pure-Rust session FSM (Idle → Active → Paused → Active → Stopped). Inputs: `ChordEvent`, `PauseToggle`, `Stop`, `Tick { ts }`, `LayerFailure { layer }`. Outputs: `LifecycleAction`. ≥20 unit tests via throwaway-crate.
- `src-tauri/src/activity/sampler.rs` — Windows-only foreground-window poller (1 Hz coarse + `EVENT_SYSTEM_FOREGROUND` event hook), emits `app_switch` + `context_snapshot` events with `app_name + window_title` only (no UIA depth yet).
- `src-tauri/src/activity/persist.rs` — `activity_sessions` + `activity_events` inserts; raw-events table is INSERT-only by trigger.
- `src-tauri/src/activity/exclusion.rs` — stub. Placeholder type + empty default list. Wave 5 fills it.
- `src-tauri/src/activity/uia_macos.rs` / `uia_linux.rs` — `todo!()` stubs behind `#[cfg(target_os)]` per Principle 5.
- `src-tauri/src/commands/activity.rs` — Tauri IPC: `activity_start`, `activity_pause`, `activity_resume`, `activity_stop`, `activity_list_sessions`, `activity_get_session_detail` (events only — no Blocks yet).
- `src-tauri/src/db/migrations/012_activity_capture.sql` — full schema above (DDL + immutability trigger + indexes; FTS5 deferred to Wave 3).
- `src-tauri/src/error.rs` — add three new `AppError` variants (`Activity(String)`, `ActivitySampler(String)`, `ActivityPersist(String)`).
- `src-tauri/tauri.conf.json` — register `recording_indicator` webview window (frameless, no-decorations, alwaysOnTop, focus:false; small pip ~280×60 top-right by default).
- `ui/src/recording_indicator.tsx` — new entry point (mirrors `meeting_overlay.tsx` shape).
- `ui/src/recording_indicator/RecordingIndicator.tsx` — "Recording activity • <duration> • [Pause] [Stop]" pip.
- `ui/src/recording_indicator/styles.module.css`.
- `ui/src/pages/Activity.tsx` — list-of-sessions page (date, duration, app-switch count, raw-only badge until Wave 3).
- `ui/src/pages/ActivityDetail.tsx` — chronological raw-events list (timestamp · app · title), grouped by 5-minute buckets visually.
- `ui/src/lib/activity.ts` — typed IPC client.
- `ui/src/lib/i18n/keys/activity.json` — copy strings.
- Sidebar nav entry + route.
- Hotkey wiring: `activity/chord.rs` — second installer of `WH_KEYBOARD_LL` per ADR 0027's precedent (this is now the THIRD hook thread; document the chain order in the wave brief).
- Pre-commit hook update: generalize `block-cross-module-coupling-meeting-dictation` → `block-cross-module-coupling` covering all three subsystems (dictation/meetings/activity). Live in `.code_puppy/settings.json` or its referenced script.

**Test plan:**

- **Pure-Rust modules** (`lifecycle.rs`, `exclusion.rs` stub) → throwaway-crate recipe (LESSONS P2).
- **Wired modules** (`sampler.rs`, `chord.rs`, `persist.rs`, `commands/activity.rs`) → cargo check + clippy --release -- -D warnings + fmt --check + test --release --no-run.
- **UI side** → `npx tsc --noEmit` + `npm test` (vitest) + `npm run build`.
- **Live smoke matrix (≥5 min):**
  1. Launch app; verify chord conflict probe runs at startup (no panic; logs say "activity chord = RCtrl+,").
  2. Press chord → recording_indicator window appears top-right; `activity_sessions` row with status='in_progress' inserted.
  3. Alt-tab through Notepad → Chrome → VS Code → terminal; each transition writes an `app_switch` event row.
  4. Press chord again → indicator window hides; row.status = 'completed'; ended_at populated.
  5. Navigate to Activity page; the new session is in the list. Drill in; raw events render in order.
  6. Restart app mid-session: verify `crashed_recovered` status promotion on next boot.
  7. Press dictation hotkey during an activity session → dictation still works, no cross-talk.

**Seal criteria:**
- All deliverables in tree.
- Cargo gate passes (check + clippy + fmt + `--no-run`).
- UI gate passes (tsc + vitest + build).
- Smoke matrix all-green (Dustin signs off).
- `block-cross-module-coupling` hook rejects a deliberate test-commit that imports `dictation::*` from `activity::*` (and vice versa with `meetings::*`).
- bd-wave-1 closed; STATUS.md updated.

**Seal tag:** `phase-10-wave-1-complete`.

---

### Wave 2 — UIA deep snapshots + multi-monitor

**Goal:** Promote the title-only sampler to full UIA snapshots. Every `context_snapshot` event carries focused-field text, visible-text fragments, and control structure. Multi-monitor enumeration. Coarse activity-level signal (no keystroke content, ever — Principle 8 + Q5).

**Decisions in-wave:**

- **`windows-rs` raw COM vs the `uiautomation` crate.** Audit required. Default lean: `uiautomation` crate IF audit passes (active maintenance, narrow dependency tree, no `@tanstack/*`-class IOC concerns, plays nicely with Phase 9's macOS abstraction). Fallback: raw COM via existing `windows`-rs features. Document the choice in a Wave-2 brief (`docs/phases/phase10-wave2-brief.md`).
- **Snapshot payload size.** Hard cap per event (proposed 32 KB JSON; truncate visible-text fragments to most-relevant-N if larger). The cap is empirical — measure during wave dev, document in the brief.

**Deliverables:**

- `src-tauri/src/activity/uia.rs` — `AccessibilitySnapshot` trait + Windows impl. Exposes `snapshot(hwnd) -> Result<UiaSnapshot>` returning `{ focused_field, visible_text_fragments, control_structure_summary, monitor_index, password_field_active }`.
- Update `sampler.rs` — every `context_snapshot` now carries the UIA payload as JSON in `activity_events.snapshot_json`.
- `src-tauri/src/activity/activity_level.rs` — `GetLastInputInfo`-backed coarse signal (active / idle), feeds `idle_start` / `idle_end` events. **NEVER** captures keystroke content (Principle 8 invariant).
- Multi-monitor enumeration in `uia.rs` — `IUIAutomation::ElementFromPoint` or `EnumDisplayMonitors` to attribute snapshots to a display id.
- Update `ActivityDetail.tsx` — drill-down panel shows the snapshot JSON formatted; users can collapse / expand per event.

**Test plan:**
- Pure: `activity_level.rs` (idle math), `uia.rs` payload-shape & truncation helpers → throwaway-crate.
- Wired: cargo check + clippy + fmt + `--no-run`.
- Smoke (≥10 min): record a session covering Notepad (good UIA), Chrome (decent UIA), VS Code (Electron — variable), a Win11 native Settings page (excellent UIA), a Steam game window (likely poor UIA — verify graceful "details unavailable" event), and ≥2 monitors. Verify snapshot payloads in each event; verify no event from a focused password field (use a browser sign-in form — `password_field_active` should be true on the snapshot whose target is the password input, and the snapshot text payload should be empty/redacted per Q5 even at this wave). Idle-event surfacing: walk away for 90 s; verify `idle_start` event written.

**Seal criteria:**
- All deliverables in tree.
- Cargo + UI gates pass.
- Smoke matrix all-green; the `(app, snapshot-quality)` matrix documented in the Wave 2 brief.
- bd-wave-2 closed; STATUS updated.

**Seal tag:** `phase-10-wave-2-complete`.

---

### Wave 3 — Summarization pipeline (Stage 1–4)

**Goal:** A finished session goes through merge → segment → block → abstract → assemble and produces a Markdown session summary. Block CRUD on derived data. Graceful degradation when Ollama is unavailable.

**Deliverables:**

- `src-tauri/src/activity/segmenter.rs` — Stage 1 (merge + normalize). Pure Rust. Dedupes redundant "still in <app>" snapshots, resolves overlapping events, time-orders the result.
- `src-tauri/src/activity/blocker.rs` — Stage 2 (group into Blocks). Pure Rust. Block-break rules: app switch; document/title change beyond N edit-distance; idle ≥ 60 s; channel boundary (Wave 4 onward — gated). Configurable thresholds with sensible defaults.
- `src-tauri/src/activity/abstractor.rs` — Stage 3 (per-Block LLM abstraction). Constructs `OllamaProvider::new()` and drives via `CleanupRequest<'_>`. NO `CleanupProvider` trait extension (ADR 0036 §Decision item 4). Records prompt SHA on `activity_blocks.prompt_version_sha`.
- `src-tauri/src/activity/assembler.rs` — Stage 4. Pure Rust. Blocks → ordered Markdown session summary with optional per-app totals.
- `src-tauri/src/activity/prompts/abstract_block.md` — built-in prompt; baked via `include_str!`.
- `src-tauri/src/activity/prompts/abstract_block.audio_aware.md` — Wave 4 audio-aware variant; placeholder file in Wave 3, content fleshed in Wave 4.
- `src-tauri/src/activity/export.rs` — Markdown export (file write + clipboard helper). Re-use `meetings::export` shape via composition; if shared abstraction emerges, extract to `src-tauri/src/export/` (NEW shared module), not into `meetings/`.
- `src-tauri/src/commands/activity.rs` — extend with `activity_run_summary(session_id)`, `activity_block_rename(block_id, new_label)`, `activity_block_merge(left_id, right_id)`, `activity_block_split(block_id, at_ts)`, `activity_block_delete(block_id)`, `activity_block_rewrite_abstract(block_id, custom_text)`, `activity_export_session_markdown(session_id, path)`.
- Migration 013 (additive): FTS5 virtual table on `activity_blocks.generated_abstract` for search. (NOTE: 013 is a v1 add, NOT a migration 012 edit; 012 stays sealed after Wave 1 commits.)
- `ui/src/pages/ActivityDetail.tsx` — extend with Block view (default), Block edit affordances, drill-down to raw events kept as collapsible, "Re-run summary" button, export-Markdown button, graceful-degradation banner ("Summary unavailable — Ollama offline") that still renders the raw-event timeline.

**Test plan:**
- Pure: `segmenter.rs`, `blocker.rs`, `assembler.rs`, `prompts/*.md` loader → throwaway-crate. Block-CRUD command implementations are sealed against `activity_events` (try to UPDATE a raw event → 'activity_events is immutable' SQL error surfaces correctly; assert it).
- Wired: `abstractor.rs` (Ollama HTTP), `export.rs` → cargo check + clippy + fmt + `--no-run`.
- Smoke (≥10 min): run a Wave-2 session through summary; check Block boundaries match what a human would draw; rename / merge / split / delete; verify `activity_blocks.user_edited = 1` after; verify `activity_events` rows are untouched (drill-down still shows the original raw); kill Ollama mid-summary and rerun — verify raw timeline still renders and the banner shows.

**Seal criteria:**
- All deliverables in tree.
- Cargo + UI gates pass.
- Smoke matrix all-green.
- A deliberate test row UPDATE on `activity_events` is rejected by the trigger (proves Principle 1 is wired).
- bd-wave-3 closed; STATUS updated.

**Seal tag:** `phase-10-wave-3-complete`.

---

### Wave 4 — Audio layer (Layer 2)

**Goal:** Per-session opt-in microphone capture + chunked Whisper transcription, interleaved with Layer 1 events on the same clock. Audio-aware abstractor prompt.

**Decisions in-wave:**

- **Reuse `meetings::long_form_stt` as a library** (read-only; ADR 0036 §Decision item 5). If a fundamentally different chunking cadence is needed (event-paused mic on UIA-driven exclusion-list triggers), build a thin `activity/audio_orchestrator.rs` that re-drives the chunker.
- **Reuse `audio::AudioCapture`** with a fresh instantiation (separate ringbuf, no shared state with Dictation / MC instances). One Tauri app, multiple CPAL streams is fine on Windows — Phase MC twin-stream proved this. The activity-capture audio instance lives in the meetings-thread-equivalent for activity (a new long-lived audio thread).
- **Table choice:** new `activity_transcript_segments` table (per the schema above) vs. reusing the `meeting_chunks` shape. Default: new table — Phase MC's chunks carry per-channel metadata (Mic / System) that activity doesn't need, and `activity_transcript_segments` keeps the foreign-key joins clean.

**Deliverables:**

- `src-tauri/src/activity/audio.rs` — Layer 2 orchestrator: instantiates `AudioCapture`, pipes into a chunker, calls `meetings::long_form_stt::transcribe_chunks` post-stop, writes `activity_transcript_segments` rows.
- Per-session toggle in `Activity.tsx` "Start session" affordance: { record audio: on / off }.
- Persistent mic-live overlay indicator — extend the `recording_indicator` window with a mic-state badge ("🎙️ on" / "off"). Same window, additive layout.
- Update `abstractor.rs` — when a Block has overlapping transcript segments, pass them in the context bundle; pick `abstract_block.audio_aware.md` prompt variant.
- Update `ActivityDetail.tsx` — drill-down shows interleaved events + transcript segments on a single timeline.
- Update `export.rs` — Markdown export includes transcript snippets per Block when audio was on.
- Update `exclusion.rs` (stub from W1) — UIA-driven exclusion-list triggers now ALSO pause Layer 2 (mic mute) for the duration the excluded app is focused. (Full exclusion-list semantics ship in Wave 5; this wave wires the mic-pause hook.)

**Test plan:**
- Pure: any new pure helpers (segment-to-block alignment) → throwaway-crate.
- Wired: cargo check + clippy + fmt + `--no-run`.
- Smoke (≥15 min): record a session with audio on, talk through a few app-switches; verify the transcript appears in `ActivityDetail.tsx` drill-down; verify the abstractor produced audio-aware Block descriptions; verify the persistent mic indicator on the overlay updates within 500 ms when the mic is toggled or paused; verify Ollama-unavailable still renders raw timeline + raw transcript segments.

**Seal criteria:**
- All deliverables in tree.
- Cargo + UI gates pass.
- Smoke matrix all-green; mic-indicator latency verified.
- No cross-talk with Phase MC: start an activity session with audio on, then ALSO trigger a meeting via `Right Ctrl + .` — verify both sessions record independently OR one explicitly refuses with a clear toast ("meeting already recording; stop one first"). Wave 4 decides which is the right UX; document in a Wave 4 brief.
- bd-wave-4 closed; STATUS updated.

**Seal tag:** `phase-10-wave-4-complete`.

---

### Wave 5 — Hardening & polish

**Goal:** The feature stops being a demo and starts being shippable. Encryption-at-rest decision made. Exclusion list at capture time. Retention policy. Crash recovery. PDF export. Settings tab.

**Deliverables:**

- **`docs/adr/0037-activity-capture-encryption-at-rest.md`** — weigh SQLCipher / DPAPI-per-row / app-layer AES-GCM (Q4); Status: Proposed → Accepted by Dustin in this wave. Implement the chosen path. Migration 014 if a schema-altering route is picked (e.g. AES-GCM blob column on `activity_events.snapshot_json`).
- `src-tauri/src/activity/exclusion.rs` — full implementation. Capture-time enforcement (events for excluded windows are dropped before the INSERT). Defaults: 1Password, Bitwarden, KeePass, browser windows with `(?i)\b(bank|login|password|signin)\b` in the title, the Win lock screen, UAC dialogs. **Plus**: UIA `UIA_IsPasswordPropertyId` check on every snapshot — if true, the whole snapshot tick is dropped (Q5; stronger than `SecureInputGuard` because it works on any focused edit across any UIA-exposing app).
- `src-tauri/src/activity/retention.rs` — configurable auto-delete (default: keep forever; opt-in N-day TTL). Wired into a Tauri scheduled task or a startup-sweep helper.
- `src-tauri/src/activity/crash_recovery.rs` — at app boot, detect `activity_sessions` rows with status='in_progress' and no `ended_at` → promote to `crashed_recovered`, run Wave 3 summarization on what survives. Cover the case in a unit test.
- `src-tauri/src/activity/export_pdf.rs` — PDF export of the session summary (likely via `printpdf` or the `pulldown-cmark` → HTML → wkhtmltopdf path; Wave 5 picks; no new heavy dep without ADR if the choice is non-trivial).
- "Work-report mode" toggle in the export flow — strips drill-down detail; outputs only Block-abstracts + totals.
- `ui/src/pages/Settings.tsx` — new "Activity Capture" subtab mirroring the Meetings tab. Toggles: enabled, default-audio-on, exclusion-list editor, retention TTL, storage-used display, "Delete all activity data" big-red button.
- `ui/src/components/PrivacyStatement.tsx` (or reuse if present) — in-app privacy statement explicitly stating "this data physically cannot leave your machine" + per-Layer description.

**Test plan:**
- Pure: `retention.rs` math, `crash_recovery.rs` state-promotion, `exclusion.rs` title-regex + UIA-password-bit handling → throwaway-crate.
- Wired: cargo check + clippy + fmt + `--no-run`.
- Smoke (≥20 min): record a session that focuses 1Password mid-flow → verify event for that window is NOT in `activity_events`; focus a sign-in form's password field → verify the snapshot for that tick is dropped (or has empty payload + `password_field_active=true`); set retention TTL to 1 day and verify a fixture session older than that gets purged on next sweep; kill the app mid-session and reboot → verify `crashed_recovered` promotion + Wave-3 summary runs on the partial; export a session as PDF in both regular and work-report modes; flip encryption-at-rest setting and verify ADR 0037's chosen approach is honored on disk (binary-inspect the DB file).

**Seal criteria:**
- All deliverables in tree.
- ADR 0037 Accepted.
- Cargo + UI gates pass.
- Smoke matrix all-green; exclusion-list verified empirically (NOT just unit-tested).
- bd-wave-5 closed; STATUS updated.

**Seal tag:** `phase-10-wave-5-complete`.

---

### Wave 6 — Invariant judges + final seal

**Goal:** Five Wave-MC-style invariant judges land + a live-OS smoke matrix runs green + the phase seals.

**Deliverables:**

- `docs/judges/phase-10/ac-raw-events-immutable.md` — judge that programmatically attempts to UPDATE / DELETE rows in `activity_events` on a fixture DB and verifies the trigger aborts with the expected message. Falls back to grep for the trigger DDL in migration 012 if live exec is blocked.
- `docs/judges/phase-10/ac-no-keystroke-content.md` — judge that `rg`-searches `src-tauri/src/activity/` for any keystroke-capture API surface (`SetWindowsHookEx(WH_KEYBOARD)` outside the chord listener, `GetKeyboardState`, `MapVirtualKey` used for character extraction, etc.) and fails if found outside the chord listener whitelist. Cross-checks `activity_level.rs` to confirm it only reads `GetLastInputInfo` (a tick count, NOT key content).
- `docs/judges/phase-10/ac-exclusion-honored-at-capture.md` — judge that runs a fixture session against a mock exclusion-list-target window and asserts no `activity_events` row was written for that window. Pure-Rust test through throwaway-crate.
- `docs/judges/phase-10/ac-no-llm-in-critical-path.md` — judge that `rg`-searches `activity/lifecycle.rs`, `activity/sampler.rs`, `activity/persist.rs`, `activity/audio.rs`, `activity/segmenter.rs`, `activity/blocker.rs`, `activity/assembler.rs` for any reference to `OllamaProvider`, `cleanup::`, or HTTP-to-`localhost:11434`. The ONLY file allowed to touch Ollama is `activity/abstractor.rs` (the Stage-3 abstractor) + tests. Failing in any other file = fail.
- `docs/judges/phase-10/ac-summary-degrades-gracefully.md` — judge that runs a fixture session through summarization with Ollama mocked to "503 unavailable"; asserts the session still has a renderable summary (Markdown with un-abstracted Block headings — primary app + title + time range — and no panic, no abort).
- **Live-OS smoke matrix** (per LESSONS PINNED P7 — judges don't catch OS-integration regressions). Documented in this Wave-6 section, signed off by Dustin on a fresh boot:
  1. Cold boot. Launch app. Chord conflict probe runs; `Right Ctrl + ,` is bound.
  2. Press chord. Recording indicator appears top-right. `activity_sessions` row inserted.
  3. App-switch through ≥4 apps over ≥3 minutes. App-switch events written.
  4. Focus a password field. Snapshot for that tick has `password_field_active=true` and empty/redacted payload.
  5. Press the dictation hotkey during the activity session. Dictation works. No cross-talk.
  6. Press `Right Ctrl + .` (MC chord) during the activity session. Wave 4's UX decision plays out (parallel OR explicit refusal — confirm against the Wave 4 brief).
  7. Press chord again. Recording indicator hides. Summary auto-runs. Activity page shows the session with its Block summary.
  8. Edit a Block (rename + merge + split). `activity_blocks.user_edited=1`. Raw events drill-down still shows the originals.
  9. Export as Markdown. Export as PDF. Work-report mode. All produce valid files.
  10. Kill the app mid-session. Reboot. `crashed_recovered` summary appears.

**Seal criteria:**
- All five judge files in `docs/judges/phase-10/` and individually green (each runnable per its own description; concrete shell-driven judges preferred per PLAN §10 Judge prompt design).
- Live-OS smoke matrix all-green on a real Win11 box, signed off by Dustin in this iteration's STATUS-update line.
- Cargo + UI final gate: check + clippy + fmt + `--no-run` + tsc + vitest + build all clean.
- `STATUS.md` updated: move "Phase 10 Activity Capture" from Currently active → Sealed table.
- `docs/PRODUCT-STATE.md` updated: new "Activity Capture" subsystem section describing what the app does today.
- `docs/LESSONS.md` — append any non-obvious finding discovered during Phase 10 execution; promote to PINNED only if truly load-bearing.

**Seal tag:** `phase-10-complete`. Top-level seal. STATUS Sealed table gains a row.

---

### Wave 7 (OPTIONAL, post-`phase-10-complete`)

Layer 3 screenshot fallback + local OCR. Chartered via successor ADR (likely 0038). NOT part of `phase-10-complete`. NOT planned in this phase doc.

If/when scheduled:
- Author ADR 0038 (Status: Proposed → Accepted).
- Add `activity/screenshot.rs` + `activity/ocr.rs`.
- Add new bead `Phase 10 Wave 7 (post-seal): Layer 3 screenshot + OCR`.
- Seal via STATUS update + ADR Accepted, NOT a new `phase-10-w7-complete` tag (lateral-epic shape per AGENTS.md, not numbered-phase shape — Wave 7 lives outside the numbered phase).

---

## Cross-wave invariants (binding — every wave must honor)

1. **`activity_events` is immutable.** Principle 1 + ADR 0036 §Decision item 6 + migration 012 trigger.
2. **NO LLM in the critical path.** The capture and persistence loops must work with zero network access. Only the Stage-3 abstractor (`activity/abstractor.rs`) calls Ollama, and even it degrades gracefully.
3. **NO keystroke content captured.** `activity_level.rs` reads `GetLastInputInfo` (a tick count) only. NO `WH_KEYBOARD_LL` outside the chord listener whitelist (which only watches the chord VKs, not arbitrary keystrokes).
4. **Exclusion list honored at capture, not display.** Excluded windows' events never reach the `INSERT` statement.
5. **Sealed modules stay sealed.** ADR 0036 §Decision items 1 & 2. The `block-cross-module-coupling` hook enforces.
6. **Cross-platform stubs from day one.** `#[cfg(target_os)]` everywhere a Windows API is touched.

## Cargo gate (binding per LESSONS P2)

Phase 10 uses the existing accepted fallback gate. No new gate proposed.

- Pure-Rust modules → throwaway-crate recipe (LESSONS 2026-05-17). Modules eligible: `lifecycle.rs`, `segmenter.rs`, `blocker.rs`, `assembler.rs`, `activity_level.rs` math helpers, `retention.rs`, `crash_recovery.rs`, `exclusion.rs`, plus any pure helpers under `uia.rs` / `abstractor.rs` / `audio.rs` extracted explicitly for testability.
- Wired modules (touch `windows`-rs / `cpal` / `whisper-rs` / `ort` / Ollama HTTP / Tauri) → cargo check + clippy --release -- -D warnings + fmt --check + test --release --no-run + per-wave human-in-loop smoke matrix.

A parallel investigation bead is open against the `cargo test --release` `STATUS_ENTRYPOINT_NOT_FOUND` root cause; its resolution is not required for Phase 10 seal but would obviously make this gate cheaper to run for every future phase. See the bead's description for the investigation timebox + fallback.

## UI gate (binding)

- `npx tsc --noEmit` clean.
- `npm test` (vitest) clean.
- `npm run build` clean.
- `npm run lint` currently broken (`mb-yxh`); ignore until that's resolved.

## Out of scope for the whole of Phase 10 (do not absorb)

- Always-on capture (ADR 0036 Non-Goals).
- macOS / Linux UIA impls (Phase 9 sweep).
- Layer 3 screenshot + OCR (Wave 7, post-seal, optional).
- Cloud sync / telemetry / multi-device timelines (Principle 4).
- Inline correction → learning loop (Q7).
- Project-tagging UI (Q6 — schema present, UI deferred to v2).
- Real-time live summarization (ADR 0036 Non-Goals).

## How to resume Phase 10 mid-execution

1. Read STATUS.md "Currently active" — it'll point at the in-flight wave.
2. Read this file (you're here).
3. Read ADR 0036 + (when present) ADR 0037.
4. Read the in-flight wave's brief (`docs/phases/phase10-wave{N}-brief.md`) if one was authored (Waves 2, 3, 4, 5 are deep enough to merit one; Waves 1 + 6 may be brief-less).
5. `bd ready -t task` filtered for the in-flight wave id.
6. `git tag --list "phase-10-*"` to confirm which waves have sealed.
7. THEN start work.
