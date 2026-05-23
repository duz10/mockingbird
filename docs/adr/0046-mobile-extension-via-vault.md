# ADR-0046: Mobile extension via synced Obsidian vault (no native mobile app)

- **Status:** Accepted — 2026-05-27. POC configuration locked in (see "Realized POC configuration" section). Wave 0 spike re-anchored to Iter 2 setup per the Iteration plan; not charter-blocking.
- **Date:** 2026-05-27 (Proposed)
- **Deciders:** Dustin (project lead), Bernard / code-puppy (chartering)
- **Charter for:** ADR-lateral epic — Mockingbird Mobile Extension. Six waves; sealed via ADR Accepted + STATUS update + five invariant judges green. **NO new `phase-*-complete` tag** (lateral epic, not a numbered PLAN §10 phase).
- **Source plan:** `mockingbird-mobile-extension-plan.md` at repo root (untracked feature plan; this ADR refines it where the codebase reality differs).
- **Sibling-precedent ADR:** [ADR 0036](0036-activity-capture-sibling-subsystem.md) — sibling-subsystem charter pattern (sealed primitives + greenfield module + boundary-enforced isolation).
- **Boundary-authorization precedent:** [ADR 0037](0037-unified-recording-command-center.md) — explicit authorization to make surgical edits to sealed Dictation + Meeting Capture surfaces, scoped to a listed boundary. This ADR uses the same pattern, scoped tighter (one function body in one file).

## Context

Mockingbird ships as a Windows desktop dictation + meeting-capture app. Users have asked for a way to use it from their phone — capture a quick voice memo on the move, see their existing transcript history without sitting at the desktop — but a native iOS/Android app is a multi-month parallel product. The cheap-and-true alternative: extend the desktop app to read/write a **synced Obsidian vault**, and let the phone interact with that vault via Obsidian Mobile (reads) + an iOS Shortcut (writes). The desktop remains the only place audio is processed; the phone is purely a UI surface and a capture device.

### The §1.4 PIVOT QUESTION ANSWERED

The source plan §1.4 elevates one question above everything else:

> Can the existing audio-processing pipeline be invoked headlessly — i.e. called with an audio file as input, without going through the recording UI?

**Answer: not today.** The dictation pipeline lives in `src-tauri/src/dictation/`, with the work happening inside `DictationOrchestrator::complete()`. That function:

- Consumes `StateAction` events from the hotkey FSM (`hotkey/state.rs`).
- Drains a live `cpal` ring buffer that only exists when an audio stream is active.
- Is welded to the `RecordingWindow` overlay (start/stop UI affordance), the `WindowContext` (focused-window snapshot, taken at hotkey-press time to decide where to paste), `SecureInputGuard` (abort-if-password-field-focused), and `Injector` (clipboard save → set → paste → restore).

Every one of those couplings makes sense for a live PTT dictation. **None** of them belong in the path that turns a `.m4a` file dropped by a phone Shortcut into a `sessions` row. A headless `ingest()` entry point must therefore be extracted from `complete()`. This is **the single place sealed code is restructured** by this epic — and it's the same boundary-authorization pattern ADR 0037 used for its Command Center edits (small, scoped, listed in the ADR, enforced by judge).

### Vault as transport, not source of truth

The synced folder is treated as an **untrusted, failure-prone medium**. Sync conflicts, partial writes, out-of-order delivery, silent-skip on oversized files, and Windows `ReadDirectoryChangesW` event drops under bursts are all expected. The canonical SQLite DB inside the desktop app stays the only source of truth. The vault's `history/` folder is a **disposable projection** (regenerate-on-divergence is always safe); the `inbox/` folder is a **courier zone** (files are transport, not records — discarded once successfully ingested).

### Web research (paraphrased)

- **Obsidian Sync local-write behavior is undocumented.** The exact moment Obsidian-Sync-on-Windows writes a synced file to disk relative to the rename/replace/atomic-create dance, and how that interacts with a desktop watcher, is empirically only determinable on real hardware. This is the blocking unknown that Wave 0 exists to answer.
- **The `notify` crate on Windows** has well-known duplicate-event and buffer-overflow behaviors under bursts. `notify-debouncer-full` is the standard mitigation. Even with it, **periodic reconciliation scan** is required as a correctness safety net — the watcher is a latency optimization, not a guarantee.
- **Combined-detector pattern** (size+mtime stable AND exclusive-open probe succeeds AND minimum absolute age) is the documented best practice for syncing-folder watchers; any single signal alone is unreliable.
- **Obsidian Sync Standard caps individual files at 5 MB.** Oversized files are **silently skipped** on the sync side. The cap itself is not the trap; the *silence* is — users don't learn their long voice memo never left the phone until they sit down at the desktop hours later and notice nothing arrived.

## Decision

**Charter a six-wave ADR-lateral epic adding a `vault/` subsystem to `src-tauri/src/`. Outbound projection writes Markdown into `<vault>/history/`. Inbound watcher consumes `.m4a` couriers from `<vault>/inbox/` through a newly-extracted headless dictation `ingest()` entry point. A bonus "+ Audio file" desktop button reuses the same headless entry point. Sync layer is pluggable; the POC is Obsidian Sync (Standard, 5 MB cap), with Syncthing as a documented future option. Mobile sync is opt-in (default OFF). Seal via ADR Accepted + STATUS update + five invariant judges green. NO new `phase-*-complete` git tag.**

## Iteration plan

The six-wave structure in [Detailed Design](#detailed-design) below remains the **engineering decomposition** — it is how the work is *built*. **Delivery order**, however, is restructured into four iterations, each gated by a human-in-the-loop checkpoint, so the highest-feedback / lowest-risk piece ships first and so each subsequent iteration builds on a known-good base.

This is a deliberate inversion of the natural "Wave 0 first" reading. The Wave 0 sync-layer spike is still required before any code that depends on its findings lands — but its findings only inform the *inbound watcher* (Wave 3). Outbound projection and the headless ingest pipeline don't need them. So Wave 0 slides forward in real-clock time without becoming a charter-wide blocker, and the first thing the user actually sees is a working desktop import path that proves the pivot question end-to-end.

### Iteration 1 — Desktop file-ingest pipeline (no Obsidian, no sync)

**Goal:** drag-drop (or file-pick) an audio file — e.g. an iPhone Voice Memo `.m4a` transferred via AirDrop, iCloud Drive, or USB cable — into the Dictations page and see a transcribed row appear with `source = 'desktop-import'`.

**Why first.** This validates the headless-ingest extraction (§3), the audio-decode helper (§4), and the schema migration (§2) in the *tightest possible debug loop*, with **zero sync-layer noise**. If anything in those three pieces is wrong, we find out before Obsidian, file watchers, conflict resolution, or content-SHA reconciliation enters the picture. Every later iteration depends on this trio working correctly; getting it green in isolation collapses the diagnostic surface enormously when Iterations 2 and 3 introduce their own moving parts.

**Ships from:** Waves 1.1 + 1.2 + 1.3, plus Wave 4.1 (the "+ Audio file" button, pulled forward from Wave 4).

**Sealed-surface boundary.** The `dictation.rs` edit (§3) lands in this iteration. The `sealed-phases-untouched` invariant judge (§17) therefore runs at the **Iteration 1 boundary**, not at final seal — we want the boundary check the same iteration the boundary is touched, not three iterations downstream. The remaining four judges (canonical-db, courier-discarded, projection-deterministic, mobile-sync-opt-in) cannot run yet — their preconditions (vault layout, courier flow, opt-in toggle, projection output) don't exist until Iterations 2-4. They ride with Iteration 4.

### Iteration 2 — Outbound projection

**Goal:** with mobile sync opted in and a vault path configured, all dictations and meetings project as Markdown into `<vault>/history/` deterministically. Open the vault on iPhone via Obsidian Mobile; confirm read access; confirm front-matter renders and body Markdown is legible.

**Ships from:** all of Wave 2 (vault layout + manifest + projection + reconciliation export job), plus a minimal subset of Wave 4 — just the `MobileSyncEnabled` master toggle and the `VaultPath` picker, with no full Mobile tab yet. The remaining six settings keys (§10) and the full Settings tab UI (§12) defer to Iteration 4 where they ship as one cohesive unit.

**Real-clock note on Wave 0.** The Wave 0 sync-layer spike (`mb-s8s2`) is run **during this iteration's setup**, since Dustin is already configuring Obsidian Sync for the first time and the observation cost is near-zero while he's there. The spike's findings are *recorded* in this iteration but only *consumed* by Iteration 3's watcher.

### Iteration 3 — Inbound from iOS Shortcut

**Goal:** record on iPhone via the Shortcut → file arrives in `<vault>/inbox/` → desktop watcher detects it → headless ingest transcribes it → the row appears in the Dictations page with `source = 'mobile-inbox'` AND the projected Markdown appears in `<vault>/history/` on the next export pass.

**Ships from:** all of Wave 3 (watcher + courier pickup + iOS Shortcut spec + setup docs).

**Wave 0 dependency.** The spike's findings (gathered in Iteration 2 setup) directly parameterize the combined detector in §6 — specifically the quiet-window duration, the minimum absolute age, and the conflict-filename regex. This iteration is the first time those parameters get consumed in code, which is why the spike is allowed to slip from charter-blocking to Iteration-3-blocking without weakening the design.

### Iteration 4 — Polish, hardening, and seal

**Goal:** the full Settings Mobile Sync tab with all eight settings keys, the concurrency model validated under load, format versioning + machine fingerprint wired and tested, the remaining four invariant judges authored and green, the full live-fire smoke matrix run by Dustin, ADR moved to Accepted, STATUS updated.

**Ships from:** Waves 4.2 + 4.3 (remainder of UI) + all of Wave 5 + all of Wave 6.

**Seal mechanics.** ADR Accepted + STATUS update + five invariant judges green + smoke matrix green. **No new `phase-*-complete` git tag** (lateral epic per LESSONS PINNED P5; same pattern as ADRs 0022/0032/0033/0037).

## Realized POC configuration (2026-05-27)

The POC instance is configured as follows, locked in by Dustin at ADR Accept. These values are the defaults that ship with Iteration 1's settings additions and the values the Iteration 3 smoke matrix exercises against.

**Vault setup (Win11 desktop):**

- Local vault path: `C:\Users\dboyd\mockingbird-vault\`
- Remote vault name (Obsidian Sync): `mockingbird-vault`
- Obsidian Sync tier: **Standard plan** ($4/mo). Per-file cap **5 MB** — codified as the `SyncTierByteCap` default in §10 and consumed by Wave 4.3's silent-skip surface.
- Region: North America.
- Encryption: **End-to-end** (Obsidian Sync E2E mode). The encryption key is user-managed and lives in the user's password manager. **Mockingbird code, settings, and the SQLite DB never touch the key.** This is by architectural design: Obsidian Sync E2E operates at Obsidian's sync boundary (local disk ↔ Obsidian relay servers); files on local disk on both ends are plaintext to the OS. Mockingbird therefore reads/writes ordinary Markdown / `.m4a` files in `<vault>/` and is mechanically incapable of seeing the ciphertext form. If Mockingbird ever needed at-rest encryption of its own state, that would be a separate decision (ADR 0038, currently RESERVED) — orthogonal to the Obsidian Sync transport.

**iPhone setup (deferred to Iter 3 user-setup; documented here so the design is locked):**

- Obsidian Mobile vault storage: **"On My iPhone"** (the Obsidian app's sandbox), NOT iCloud Drive. This is the load-bearing choice that locks the iOS Shortcut to the Quick-capture variant in §8 — the silent zero-tap Action-Button path requires the vault to live in iCloud Drive, which we explicitly do not do, since that would put a second sync engine on the vault and break the single-sync-engine assumption Wave 0 / §14 rest on.

**Sync exclusions** (configured during Iter 2 setup, once the vault folders exist on disk):

- `.mockingbird/` — desktop-only bookkeeping (manifest, machine fingerprint per §15/§16). Must never round-trip through sync.
- `inbox/_failed/` — quarantined courier failures (debug-only; user shouldn't see these on their phone).
- `inbox/_keep/` — dev-mode preserved couriers (only populated when `VaultDebugKeepCouriers = true`).
- `history/_archive/` — retention-aged records (kept on desktop for forensics; not surfaced on the phone).

Documentation for this exclusion list ships as part of Iteration 2's setup docs.

**Sealed architectural property.** Mockingbird has **zero coupling** to Obsidian's authentication, encryption-key management, or sync mechanics. The `VaultPath` setting is the only Obsidian-related state Mockingbird stores; everything else flows through the filesystem. If the user ever swaps the sync layer (e.g. to Syncthing for self-hosted, or to a local-network-only setup), **no Mockingbird code changes** — only the `VaultSyncBackend` setting label flips and the user's setup docs change. This property is what makes the `VaultSyncBackend` enum in §10 a real escape hatch rather than vendor-lock copy.

## Detailed Design

### 1. Vault layout

```
<vault>/
├── history/                       # OUTBOUND. Projection of canonical DB. Phone reads via Obsidian.
│   ├── 2026-05-27-1408__a4f7c2d3.md
│   ├── ...
│   └── _archive/                  # OUTBOUND. Retention-narrowed records moved here, never hard-deleted.
├── inbox/                         # INBOUND. iOS Shortcut drops .m4a (+ optional .json sidecar) here.
│   ├── 2026-05-27T1411-{uuid}.m4a
│   ├── 2026-05-27T1411-{uuid}.json
│   └── _failed/                   # INBOUND. Couriers that failed processing, with .error.json sidecars.
└── .mockingbird/                  # Hidden from Obsidian's note list. App bookkeeping only.
    └── manifest.json              # schema_version, machine fingerprint, last-export iso, content-sha map.
```

**Zone contracts (one-way-flow guarantees):**

- **`history/`** is read-only-by-design from the phone. Phone-side edits to `history/*.md` are **overwritten on the next export pass**; they are **never merged back** into the canonical DB. The DB is truth; Markdown is projection. The `canonical-db-is-source-of-truth` judge (Wave 5.3) enforces this by static analysis — no code path in Mockingbird ever reads `<vault>/history/*.md` as input.
- **`inbox/`** is the only direction in. Couriers (audio + optional sidecar) land here, get ingested, then get **discarded** (success) or **moved to `_failed/`** (failure). The synced `inbox/` never accumulates a backlog except as observed failures.
- **`history/_archive/`** is the only place projected records persist after their retention window narrows. Hard-delete is forbidden by the same projection-discipline that forbids reading Markdown back as input — a user who manually empties `_archive/` is fine, but Mockingbird never does it.
- **`.mockingbird/`** is leading-dot so Obsidian's default note list hides it.

### 2. Schema migration 018 — `sessions.source`

```sql
ALTER TABLE sessions ADD COLUMN source TEXT NOT NULL DEFAULT 'desktop';
```

Values:
- `'desktop'` — the existing PTT or in-app dictation path (backfill default for all pre-migration-018 rows).
- `'mobile-inbox'` — written by the inbox watcher when a phone-Shortcut courier is ingested.
- `'desktop-import'` — written by the "+ Audio file" button when a user picks a file from disk.

Additive only. No backfill logic beyond the DEFAULT clause. `sessions.uuid` already exists (migration 001) and is the stable record ID — it's the deterministic filename anchor for the Markdown projection (§5).

`sessions.start_mode` (migration 017) already distinguishes `'ptt'` vs `'in_app'`; the new `source` column is orthogonal (the `'in_app'` start_mode value is reused for both `'mobile-inbox'` and `'desktop-import'` rows, since neither is a PTT release).

### 3. Headless ingest extraction (the pivot)

New module: `src-tauri/src/dictation/ingest.rs`. Public API:

```rust
pub struct IngestProvenance {
    pub source: IngestSource,       // MobileInbox { courier_path } | DesktopImport { picked_path }
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub mode_id: Option<i64>,       // None ⇒ use user's default mode setting
    pub captured_duration_sec: Option<f64>,  // from Shortcut sidecar if present
}

pub fn headless_ingest(
    deps: &IngestDeps,              // borrowed handles: stt, cleanup, repo, vad, settings
    samples: Vec<i16>,              // 16 kHz mono i16 — caller decodes to this shape
    provenance: IngestProvenance,
) -> AppResult<i64>;                // returns the new sessions.id (rowid)
```

Pipeline:
1. **VAD trim** (Silero) — strip leading/trailing silence, same threshold as PTT path.
2. **STT** (whisper-rs CUDA) — single one-shot transcribe; mobile recordings are short enough to skip long-form chunking. If a future Wave finds them long enough, swap to `meetings::long_form_stt` library-mode.
3. **Cleanup** (mode-aware Ollama pass, same provider trait as PTT).
4. **Persist** `raw` / `cleaned` / `final` transcript rows + `sessions` row. `sessions.source` from `provenance.source`. `sessions.start_mode = 'in_app'`. No `audio_blob_path` unless `KeepAudioBlobs` is true (§11).

**Explicitly NOT in `headless_ingest`:**
- No `RecordingWindow` interaction (no overlay show/hide).
- No `WindowContext` (no focused-window snapshot — there is no caret to paste into).
- No `SecureInputGuard` (no injection target → no password-field concern).
- No `Injector` (mobile recordings never paste; they land in the Dictations history view).
- No clipboard save/restore (Principle 7 N/A — no paste path).

**Existing `DictationOrchestrator::complete()` after extraction:** keeps the injection-side logic (overlay, window context, secure-input check, injector). The audio-to-DB middle is delegated to `headless_ingest`. Specifically:

```rust
// after extraction, complete() looks roughly like:
fn complete(...) -> ... {
    self.recording_window.hide();
    let samples = self.drain_ring_buffer();
    let provenance = IngestProvenance { source: IngestSource::Desktop, ... };
    let session_id = dictation::ingest::headless_ingest(&self.deps, samples, provenance)?;
    self.injector.paste(read_final_transcript(session_id))?;  // unchanged path
    ...
}
```

**Boundary authorization (cite ADR 0037 §"Boundary authorization" precedent):** the diff surface inside sealed Phase 3 code is **only**:

- `src-tauri/src/dictation.rs` (or `src-tauri/src/dictation/mod.rs`, whichever the current layout uses) — `complete()` function body refactor. Function signature, surrounding struct fields, and all sibling methods unchanged.
- New file: `src-tauri/src/dictation/ingest.rs`.

**Explicitly NOT touched by this epic:**
- Hotkey FSM (`hotkey/state.rs`, `hotkey/windows.rs`, `hotkey/driver.rs`).
- Injection (`injection/*`).
- Secure-input guard (`hotkey/secure_input.rs` or wherever the current home is).
- Recording window (`recording_window.rs`).
- All of `meetings/*` except `meetings/persist.rs`, which gains a **read-only** source-tracking-aware export-job read path (it does not write to its own tables; it just exposes meeting rows to the projection job).
- All of `activity/*`.
- All UI design tokens, the Recording Command Center, the hotkey driver, the chord listener.

The `sealed-phases-untouched` judge (Wave 5.3) verifies this by diffing against a pre-epic baseline tag.

#### 3.1 SessionsEventBus companion refactor

`DictationOrchestrator::complete()` currently ends — after the paste step — with `self.recording_window.emit_session_saved(id)`, a Tauri event that signals the React Dictations page to refetch its list. Headless ingest needs to fire the same event (so an imported file or a mobile-inbox courier appears in the UI without a manual reload), but the `recording_window` handle is a PTT-specific dependency that **must not** be a parameter of `headless_ingest` — doing so would re-couple the headless path to the recording-overlay subsystem and defeat the §3 boundary.

Resolution: introduce a small `SessionsEventBus` trait with one method:

```rust
pub trait SessionsEventBus: Send + Sync {
    fn emit_session_saved(&self, session_id: i64);
}
```

A default implementation backed by `RecordingWindow::emit_session_saved` covers the PTT path — `complete()` continues to fire the event via the trait, with no observable behavior change. `headless_ingest` accepts a `&dyn SessionsEventBus` (carried inside `IngestDeps`) and fires the same event on success. Both code paths therefore converge on a single emit point, and the UI's refetch trigger is identical regardless of whether the row was produced by a PTT release, a desktop file import, or a mobile-inbox courier. Total cost: roughly twenty lines plus one trait. Falls inside the boundary-authorized diff surface for sealed `dictation.rs` (the function-body refactor of `complete()` already authorized above).

**File placement.** The trait lives in a new file `src-tauri/src/dictation/events.rs` — explicitly **NOT** inside `recording_window.rs`. Keeping it under the dictation submodule preserves cohesion (it belongs to the dictation domain, not the windowing domain) and lets `headless_ingest` depend on a strictly smaller surface than `recording_window.rs` (which carries the full overlay show/hide API the headless path must not reach for). The `RecordingWindow` impl of `SessionsEventBus` is a thin wrapper that delegates to the existing inherent method; no logic moves.

### 4. Audio decode helper

New module: `src-tauri/src/audio/decode.rs`. Uses **`symphonia`** (MIT, pure-Rust, no external libs). Features: `aac` + `isomp4` (for iOS `.m4a`), `wav`, `mp3`, `ogg`.

```rust
pub fn decode_to_pcm16_mono_16k(path: &Path) -> AppResult<Vec<i16>>;
```

Handles:
- iOS Shortcut output (`.m4a`, AAC-LC mono, ~32 kbps, 22.05/44.1 kHz source sample rate).
- Open-ended file-picker imports for the "+ Audio file" desktop button (Wave 4) — any format symphonia opens.

Always resamples to 16 kHz mono i16 (Whisper's expected shape). Resampling via `symphonia`'s built-in resampler or `dasp_signal` (TBD in Wave 1 brief — Wave 1 picks whichever has the smaller dep footprint).

### 5. Outbound projection

New top-level module: `src-tauri/src/vault/`.

```
src-tauri/src/vault/
├── mod.rs              # Public API: VaultRuntime + start_export_job + tick.
├── layout.rs           # Zone enum + canonical paths + sanity checks.
├── manifest.rs         # .mockingbird/manifest.json read/write + machine fingerprint.
├── project.rs          # The actual record → Markdown serializer (deterministic).
├── export_job.rs       # The reconciliation engine (DB rows → vault state diff).
└── version.rs          # MOCKINGBIRD_EXPORT_VERSION constant + migration trigger.
```

**Deterministic filename:**

```
history/YYYY-MM-DD-HHMM__<uuid8>.md
```

where `<uuid8>` is the first 8 hex chars of `sessions.uuid`. Collision-resistance: a single user is not going to have two sessions starting in the same minute with colliding UUID prefixes; if they ever do, the projection falls back to `<uuid12>` — but this case is not engineered for in v1 because the math says it won't happen.

**Front-matter v1 schema** (sorted keys, no export timestamps in body):

```yaml
---
id: a4f7c2d3-9c61-4e8a-9c1f-2b8e0d61f9a2
type: dictation              # dictation | meeting
created: 2026-05-27T14:08:42Z
duration_sec: 23.4
title: Quick note about the projection job
tags: [mockingbird, dictation, normal]
source: desktop              # desktop | mobile-inbox | desktop-import
mockingbird_export_version: 1
---

[body = the `final` transcript stage. Plain markdown. No HTML.]
```

**Byte-identical output for unchanged records.** Required for the `projection-is-deterministic` judge. Sorted YAML keys, no `exported_at` timestamp, no machine-name fingerprint in the per-file front-matter (that lives in `.mockingbird/manifest.json` instead).

**Reconciliation against manifest by content-SHA256.** On each export pass:
1. Compute the projected Markdown for every eligible record (in-memory).
2. Hash each.
3. Compare against the manifest's `content_sha[record_uuid]` map.
4. Write only the diff (unchanged records skipped entirely).
5. Update the manifest atomically (write to `.tmp`, then rename).

**Stale-record policy.** When a record falls out of the retention window:
- Archive to `history/_archive/` (same filename, same content).
- **Never hard-delete** from the vault.
- The manifest tracks archived records separately so reconciliation knows not to re-promote them.

**Retention-window narrowing** (user reduces `VaultRetentionDays` from 90 → 30): triggers a **full reconciliation pass**, not just future-projection. Records now outside the window archive immediately.

**Both `sessions` AND `meeting_sessions` are projected.** The export job UNION-reads both tables. The `type` field in the front-matter distinguishes them. `VaultSyncRecordTypes` (§10) lets the user opt one or both out.

### 6. Inbound watcher (the riskiest piece)

New modules under `src-tauri/src/vault/`:

```
inbox_watcher.rs        # notify-debouncer-full subscriber + reconciliation timer.
inbox_pickup.rs         # The actual courier → ingest pipeline (decode → headless_ingest → dispose).
```

**Library: `notify-debouncer-full`**, NOT raw `notify`. Per the web research, Windows `ReadDirectoryChangesW` events under bursts collapse poorly without debouncing — `notify` alone produces duplicates, lost events, and surprise event coalescing.

**Combined detector** (a courier is ready to pick up iff ALL hold):
1. **Size + mtime stable for ≥2s** (the quiet-window probe).
2. **Exclusive-open succeeds** (`OpenOptions::new().read(true).share_mode(0).open()` — proves no other process is still writing).
3. **Minimum 1s absolute age** since first observation (defends against the watcher firing the same tick the sync engine atomically renames the file in).

All three required. Any single signal alone is documented-unreliable.

**Conflict-file quarantine.** Obsidian Sync's conflict-resolution emits filenames matching `(?i)\(Conflict \d{4}-\d{2}-\d{2}.*\)`. The watcher regex-matches that pattern and **moves the conflict file directly to `inbox/_failed/`** with an `.error.json` sidecar explaining "this looked like a sync conflict; we didn't process it; resolve manually." Never ingest a conflict file.

**Periodic reconciliation scan every 30-60 seconds.** This is the correctness guarantee — the live watcher is a latency optimization. `ReadDirectoryChangesW` drops events under bursts; the scan catches anything the watcher missed. Implementation: a `tokio::time::interval` task that lists `inbox/`, applies the same combined-detector to anything not already in-flight, and pushes ready couriers into the same pickup channel.

**Dedup ledger.** New SQLite table:

```sql
CREATE TABLE vault_inbox_ledger (
    sha256        TEXT PRIMARY KEY,    -- content hash, NOT filename
    filename      TEXT NOT NULL,       -- last-seen filename for diagnostics
    processed_at  TEXT NOT NULL,       -- ISO 8601 UTC
    session_uuid  TEXT NOT NULL        -- the sessions.uuid this courier became
);
```

**Keyed on content SHA-256, NOT filename.** Obsidian's conflict-resolution sometimes re-touches a file (mtime updated, contents unchanged) → if we keyed on filename, this would look like a re-delivery. SHA-256-keyed means re-touch is a no-op.

**Startup catch-up.** On app boot, scan `inbox/` synchronously before starting the live watcher. Anything past the combined-detector → ingest immediately. Closes the "app was offline while the phone was capturing" gap.

### 7. Courier flow + placeholder notes

Closes the source plan §5.9 silent-black-hole UX gap.

**On courier detection (before decode/ingest starts):**
1. Generate a placeholder UUID for the eventual session.
2. Write `history/YYYY-MM-DD-HHMM__<uuid8>.md` with `status: processing` in the front-matter and a body of `*Processing on desktop...*`. This appears on the phone within one sync round-trip.

**On success:**
1. `headless_ingest` returns the real `sessions.id` + `sessions.uuid`.
2. Update the placeholder note in place with the real content + remove the `status` key (or set `status: complete`).
3. Move the courier `.m4a` **out of the synced space** to a local-only temp dir, confirm DB row, then delete locally. Two-step (move-then-delete) means a crash between steps leaves nothing in the synced `inbox/` — the worst case is a transient extra file in `%TEMP%`.
4. Insert ledger row keyed on content SHA-256.

**On failure** (decode error, STT crash, Ollama unreachable beyond retries):
1. Update placeholder to `status: failed` with a one-line summary in the body.
2. Move courier to `inbox/_failed/` with an `.error.json` sidecar (timestamp + error class + stack trace + retry hint).
3. Insert ledger row anyway (sha256 + `session_uuid = ''`), so a retry attempt of the same bytes doesn't loop.

**Dev affordance:** new setting `VaultDebugKeepCouriers` (default `false`). When `true`, successful couriers move to `<vault>/inbox/_keep/` instead of being discarded. Preserves the test corpus during development without polluting the synced space for end users.

### 8. iOS Shortcut spec

The Shortcut is the writer. **v1 ships exactly one variant — "Quick capture" — a chain of two built-in Shortcut actions and nothing else.** No iOS Scripting JS, no custom URL handlers, no third-party-app dependencies.

```
1. Record Audio
   - Recording Quality: Low (32 kbps AAC mono).
   - Stop condition: Ask Each Time (press-talk-release semantics).
2. Save File
   - Input: the recorded audio from step 1.
   - Ask Where to Save: ON.
   - Filename: default ("Audio Recording YYYY-MM-DD at HH.MM.SS.m4a") preserved.
```

**Why these specific knob settings:**

- **Low (32 kbps AAC mono).** This single config choice extends the practical recording length on the 5 MB Sync Standard cap from ~2.5 min (High Quality, ~256 kbps) to ~20 min (Low, ~32 kbps). 20 min covers ~95% of realistic dictation lengths — more than enough for the POC, with Sync Plus (~200 MB cap) as the upgrade path for power users.
- **Stop condition "Ask Each Time".** Gives the user press-talk-release semantics: tap to start, see the recording UI, tap Stop when done. This is the natural mobile-capture rhythm and matches the desktop PTT mental model.
- **"Ask Where to Save" ON.** User taps once per capture to confirm destination (the Obsidian vault's `inbox/` folder under "On My iPhone" storage). One extra tap per capture is acceptable for POC; in exchange it **unlocks vault-location flexibility** — the silent zero-tap "Ask Where to Save OFF" variant requires the vault to live inside iCloud Drive, which we explicitly do NOT do (see "Realized POC configuration" → iPhone setup).

**Binding.** Bind the Shortcut to either the Action Button or a Home Screen icon — both work. Action Button binding is "best-effort": user taps once after the press to confirm the destination prompt. There is no silent-launch path in v1.

**Out of scope for v1.** The silent zero-tap variant (Action Button → Record → Save with no user interaction) is **explicitly out-of-scope for v1** and is not documented; it would require relocating the vault to iCloud Drive, which conflicts with Obsidian Sync being the sole sync engine on the vault.

**Iter 3 smoke verification item.** Verify the Shortcut still runs correctly after toggling iOS recording-duration / quality settings in the Shortcut editor and back. This guards against the iOS 14-era bug where a Shortcut's Record Audio action could lose its quality binding after a quality-toggle round-trip; the bug should be fixed in current iOS but a one-time smoke check is cheap insurance.

Shortcut documentation (Wave 3 deliverable) lives at `docs/mobile/ios-shortcut.md` with screenshots of each action's parameter pane and an import URL for the prebuilt Shortcut.

### 9. Silent-skip gap detection

Obsidian Sync Standard silently skips files >5 MB. The trap is the silence, not the cap.

**Amendment 2026-05-27 (ADR Accept).** The original sidecar-based per-file detection mechanism described below is **descoped for v1 POC**. The §8 iOS Shortcut was simplified to two built-in actions only (no `.json` sidecar write), which removes the per-file early-warning signal this section was built on. The v1 mitigation is the combined pre-emptive + reconciliation pair below; the sidecar mechanism is preserved as a documented future enhancement should POC users hit silent-skip in practice. Tracked as a P3 follow-up bead.

**v1 mitigation (what actually ships):**

1. **Pre-emptive UI copy on the iPhone side.** The Mobile tab in Settings (§12) and the `docs/mobile/ios-shortcut.md` setup guide both surface the `SyncTierByteCap` value and the practical-minutes-per-recording math (~20 min at Low/32 kbps under the 5 MB Standard cap). User is told *up front* what the ceiling is and how to avoid it.
2. **Reconciliation-based eventual detection.** The periodic reconciliation scan (§5 / §14) inspects `<vault>/inbox/` on its scheduled cadence and surfaces any audio files older than a configurable threshold that the watcher never picked up — which would include any files that briefly appeared in sync state on the iPhone but never made it to desktop. This is **eventual, not real-time**, and is silent about iPhone-side files that *never even started syncing* due to the cap. That's the gap the descoped sidecar mechanism would have closed.

**Descoped detection mechanism (preserved for future revisit):**

The original design had the Shortcut write **both** the `.m4a` audio AND a tiny `.json` sidecar (always <1 KB, always under any conceivable sync cap → always syncs). Desktop watcher would see the sidecar, wait 5 minutes for the audio partner, and on timeout raise a user notification carrying `duration_sec` from the sidecar so the warning is specific. The notification would be one-shot per sidecar UUID; the sidecar would then move to `inbox/_failed/` with a synthesized error. This converted Obsidian Sync's silent skip from a black hole into a visible, actionable warning — at the cost of adding a third action to the iOS Shortcut chain. The POC trades that early-warning signal for a strictly-two-built-in-actions Shortcut spec; if real-world POC use shows the gap matters, the sidecar can be re-added as a Shortcut v2 (and §8's strict two-action lock-in re-evaluated).

### 10. New settings keys

Eight new entries in the `SettingKey` enum at `src-tauri/src/settings/model.rs`:

| Key | Type | Default | Purpose |
|---|---|---|---|
| `MobileSyncEnabled` | `bool` | `false` | Master switch (opt-in, privacy by default). |
| `VaultPath` | `Option<String>` | `None` | User-picked absolute path to the synced vault folder. |
| `VaultSyncBackend` | enum `obsidian_sync \| syncthing \| manual` | `obsidian_sync` | Documents the backend; affects copy + tier dropdown only. |
| `VaultRetentionDays` | `i64` | `30` | Retention window for `history/`. Triggers archive on narrowing. |
| `VaultSyncRecordTypes` | enum `dictation \| meeting \| both` | `both` | Which canonical tables to project. |
| `VaultDebugKeepCouriers` | `bool` | `false` | Dev/test affordance; routes processed couriers to `_keep/`. |
| `SyncTierByteCap` | `i64` | `5_242_880` (5 MB) | Threshold for silent-skip detection (§9). |
| `KeepAudioBlobs` | `bool` | `false` | Global toggle for persisting WAV blobs (§11). |

### 11. KeepAudioBlobs decision — resolves source plan §5.7 ambiguity

The source plan §5.7 refers to "the existing global keep audio blobs toggle." That toggle **does not exist** as described:

- What exists is `AudioRetentionDays` (a TTL setting, not on/off).
- `sessions.audio_blob_path` exists in the schema but is always `None` today — desktop dictation never persists WAVs.
- Meetings have their own retention key (`MeetingAudioRetentionDays`) unrelated to dictation.

**ADR 0046 introduces `KeepAudioBlobs` as a true boolean.** Default OFF (privacy-by-default, consistent with ADR 0036's Activity audio default-off and ADR 0041's per-Block-audio default-off).

**When ON:** both desktop dictation (PTT and in-app) AND mobile-inbox dictation AND desktop-import dictation persist their decoded 16 kHz mono WAV under a path written to `sessions.audio_blob_path`. The existing `AudioRetentionDays` TTL applies as usual.

**Meetings unchanged.** They have their own retention key already; this toggle is dictation-only.

**Courier files in `inbox/` are ALWAYS discarded regardless of this toggle.** They are transport, not records. The `KeepAudioBlobs` toggle controls whether the decoded PCM is persisted by `headless_ingest` post-decode, not whether the source courier survives.

### 12. Settings UI — new "Mobile" tab

New file: `ui/src/pages/SettingsMobileTab.tsx` (sibling of `SettingsMeetingTab.tsx`).

**Settings tab order:** General / Models / History+Data / Meetings / **Mobile** / Advanced.

**Tab contents:**

- **Master switch** — "Enable mobile sync" (writes `MobileSyncEnabled`). Disabled state grays out the rest of the tab.
- **Vault folder picker** — Tauri folder picker, writes `VaultPath`. Validity card below (vault exists? `history/` and `inbox/` writable? `.mockingbird/manifest.json` parseable?).
- **Sync backend selector** — radio: Obsidian Sync (POC default) / Syncthing (documented future) / Manual (you sync the folder yourself). Just changes copy; no behavior fork in v1 — the watcher and projection work identically against any synced folder.
- **Retention slider** — `VaultRetentionDays`, 7-365 days.
- **Record types checkboxes** — `VaultSyncRecordTypes`: [x] Dictations, [x] Meetings.
- **Sync tier dropdown** — Standard (5 MB) / Plus (200 MB) / Custom (numeric input). Sets `SyncTierByteCap`.
- **Connection health card** (read-only): last successful export iso, last successful inbox pickup iso, vault-zone validity, count of failed couriers in `inbox/_failed/`.

**Plain-language opt-in copy:**

> Enabling Mobile Sync writes your transcripts and recordings into your chosen Obsidian vault. If you use Obsidian Sync as the backend, files pass through Obsidian's servers as part of the sync process. Mockingbird's processing remains 100% local; only the synced files leave your machine, and only through your chosen sync backend.

**Tier copy** (under the dropdown):

> Your Obsidian Sync Standard plan limits individual files to 5 MB, which is about 20 minutes of voice memo at the low-quality preset. Longer recordings may not sync. Upgrade to Sync Plus (200 MB) for long dictations, or switch to Syncthing for no cap.

### 13. "+ Audio file" desktop button

New `dictation_import_file` Tauri IPC. UI: a "+ Audio file" button on the Dictations page (existing page sealed by Phase 8; this is a small additive UI change).

Flow:
1. User clicks button → Tauri file picker (filters: `.m4a`, `.mp3`, `.wav`, `.ogg`, `.flac`).
2. IPC handler calls `audio::decode::decode_to_pcm16_mono_16k(picked_path)`.
3. Calls `dictation::ingest::headless_ingest(deps, samples, IngestProvenance { source: DesktopImport, ... })`.
4. Returns the new session UUID to the UI.
5. UI navigates to / highlights the new row in the Dictations list.

**Independent of mobile sync.** Works whether or not `MobileSyncEnabled` is true. Useful even on a single-machine workflow (e.g. drop an old recording into the app to transcribe it).

### 14. Concurrency model

A single `VaultRuntime` mutex serializes export-job runs. Live dictation `complete()` can fire **in parallel** with an export pass because:

- `complete()` writes to the canonical DB and commits.
- The export pass reads the canonical DB AFTER `complete()`'s commit point (the export job's read transaction snapshots whatever is committed at read time).
- Per-record write to the vault is idempotent (content-SHA-256 reconciliation in §5).
- Worst case is one extra projection of a newly-committed record on the same pass — harmless.

Inbox watcher and export job share the vault filesystem but never touch the same files: watcher reads `inbox/*` and writes to `inbox/_failed/` + `history/<placeholder>.md`; export job reads canonical DB and writes to `history/*.md`. They can interleave; placeholder writes from the watcher are honored as "this UUID exists" by the next export pass, which then updates the placeholder to the full content via the same atomic write path.

### 15. Format versioning and migration

```rust
pub const MOCKINGBIRD_EXPORT_VERSION: u32 = 1;
```

On boot, the `VaultRuntime` reads `.mockingbird/manifest.json`. If `manifest.schema_version < MOCKINGBIRD_EXPORT_VERSION`, trigger a **full vault rebuild** (re-project every eligible record from scratch). Safe because the projection is disposable.

Future bumps (v2 schema, new front-matter fields, etc.) flip the constant; the migration is automatic.

### 16. Single-user single-desktop assumption

The manifest carries a `machine_fingerprint` field: SHA-256 of `(hostname + canonical_install_dir)`. On boot, if the manifest's fingerprint differs from this machine's, the export job **refuses to run** and surfaces:

- A clear log entry: `vault: manifest fingerprint mismatch (manifest={...}, this_machine={...}). Refusing to export. Either another Mockingbird desktop is also writing this vault, or this vault was copied from another machine.`
- A UI notice on the Mobile tab.

This surfaces multi-desktop misuse rather than silently corrupting the vault (two desktops both believing they own `history/` would race-fight every reconciliation pass).

**Documented limitation in user docs:** "Mockingbird Mobile Sync v1 supports one desktop per vault. Multi-desktop is a future feature."

### 17. Five invariant judges (Wave 5.3)

Per the Phase MC + Phase 10 Wave 6 pattern. All judges live under `docs/judges/mobile-extension/`.

| Judge | Mechanism | What it proves |
|---|---|---|
| `canonical-db-is-source-of-truth` | `ripgrep` + LLM-grader: scan every file under `src-tauri/src/` for any read of `<vault>/history/*.md` as input. Must find zero. | The vault projection is disposable; the DB is truth (§1, §5). |
| `courier-discarded-on-success` | Fixture test: pickup happy-path leaves zero residual `.m4a` in the synced `inbox/` (and zero new files under `inbox/_keep/` when `VaultDebugKeepCouriers=false`). | §7 transport-not-records contract. |
| `projection-is-deterministic` | Fixture test: project the same `sessions` row twice; assert SHA-256 equality of both Markdown outputs. | §5 byte-identical-for-unchanged contract; the deterministic-filename + sorted-keys + no-export-timestamp invariants. |
| `mobile-sync-opt-in` | Filesystem-call audit on a fresh DB with `MobileSyncEnabled=false`: zero writes to `<vault>/*` across a full app session including a dictation. | §10 default-off / Principle 4 (no surprise data movement). |
| `sealed-phases-untouched` | `git diff` between pre-epic baseline tag and head, restricted to `dictation.rs`, `meetings/`, `activity/`, `hotkey/`, `injection/`, `recording_window.rs`, `cleanup/provider.rs`. Only the `complete()` body refactor + new `ingest.rs` submodule is allowed. LLM-grader writes verdict file at `docs/judges/mobile-extension/sealed-phases-untouched-verdict.md`. | The boundary authorization scope was honored. |

## Risks and open questions

Carrying forward source plan §9 risks 1-13, marked RESOLVED or OPEN:

| # | Risk (source plan §9) | Status here |
|---|---|---|
| 1 | Headless invocation possible? | **RESOLVED** — §3 above designs the extraction. |
| 2 | Stable record ID exists? | **RESOLVED** — `sessions.uuid` (migration 001) is the ID. |
| 3 | `source` field exists? | **RESOLVED** — added by migration 018 in §2. |
| 4 | Decoder for iOS audio formats? | **RESOLVED** — `symphonia` with AAC/isomp4 features (§4). |
| 5 | Sync layer local-write behavior? | **REWORDED** — **BLOCKING spike (Wave 0).** Obsidian Sync's local-write atomicity, ordering, and conflict-resolution behavior are undocumented and only empirically determinable on real iOS-phone-to-Win11-desktop hardware. No code lands before Wave 0 produces a sync-layer behavior report. |
| 6 | Concurrency between live dictation and export? | **OPEN (designed)** — §14 above is the design; Wave 5 validates under load. |
| 7 | Conflict-file handling? | **OPEN (designed)** — §6 quarantine pattern; Wave 5 validates on a forced-conflict fixture. |
| 8 | Sync tier file-size cap? | **RESOLVED** — 5 MB confirmed; mitigated by §8 + §9. |
| 9 | Ledger durability across crashes? | **OPEN (designed)** — SQLite location + WAL covers it in principle; Wave 5 validates with a kill-mid-pickup fixture. |
| 10 | Failure surfacing in UI? | **OPEN (designed)** — §7 placeholder updates + §9 silent-skip notification + §12 Mobile tab health card; smoke-matrix validates. |
| 11 | Secrets-at-rest for sync-backend credentials? | **RESOLVED** — existing DPAPI + typed-settings infrastructure covers it; no new secret-storage surface. |
| 12 | Format version migration? | **RESOLVED** — §15 constant + full-rebuild trigger. |
| 13 | Multi-desktop misuse? | **RESOLVED** — §16 machine-fingerprint guard + UI surface. |

**Open questions that don't map to source-plan numbers but matter:**

- **Q-A.** Does `symphonia`'s AAC decoder handle the exact tagged AAC-LC stream the iOS Shortcut emits, or do we need to feed it through `isomp4` demux first? **To validate:** Wave 1 builds a fixture test against a real Shortcut-generated `.m4a`.
- **Q-B.** What's the realistic latency floor for "phone hits Stop → desktop shows transcript"? Sum of (Shortcut save → Obsidian Sync upload → Obsidian Sync push to desktop → debounce quiet window → decode → STT → cleanup → DB commit → next export pass). **Pre-spike estimate:** 30s-2min on a good connection; pathological case unbounded. Wave 0 confirms.

## Consequences

### Positive

- **Mobile capture without a native app.** ~3 weeks of work vs. ~3 months for a real iOS app, with the privacy model preserved (only audio leaves the desktop via the user's chosen sync backend, and only as a courier file the desktop will delete on success).
- **Vault is reversible.** Disable the toggle, delete the vault — Mockingbird's canonical DB is untouched. No lock-in.
- **Sibling pattern preserved.** `vault/` is a new top-level module; the dictation pipeline gains exactly one new entry point (`headless_ingest`); the Phase 3 + Phase MC seal holds for everything else. Same shape as ADR 0036 + ADR 0037.
- **"+ Audio file" import is a freebie.** Once `headless_ingest` exists, the desktop import button costs one IPC + one file picker. Useful even on single-machine workflows.

### Negative

- **A third party is now in the trust loop** (Obsidian Sync, by default). This is honest in the Settings copy and reversible by switching to Syncthing or manual sync, but it's a real change to the "100% local" promise. The principles (no telemetry, no cloud processing) hold; the data-transport channel is the new asterisk.
- **A new top-level Rust module + a new top-level UI page.** Code surface grows; LESSONS-discipline grows.
- **Watcher correctness is genuinely hard** (§6). The combined detector + periodic reconciliation + dedup ledger + conflict-quarantine + ABI-stable courier-format is a lot of moving parts. The Wave 0 spike and the Wave 5 smoke matrix are explicit forcing functions to validate them.
- **Two new runtime deps.** `symphonia` (AAC + ISO/MP4 + WAV + MP3 + OGG) and `notify-debouncer-full`. Both are pure-Rust, MIT/Apache, widely used. Audit each in Wave 1 against the cross-platform abstraction (Principle 5) and the "no dep without audit" rule in AGENTS.md.

### Neutral

- **Migration 018 is additive only.** No backfill logic beyond DEFAULT.
- **No new cargo gate.** Existing P2-fallback gate (LESSONS PINNED) applies as usual: pure-Rust modules (`vault/project.rs`, `vault/manifest.rs`, parts of `inbox_pickup.rs`) → throwaway-crate; wired modules (anything calling `whisper-rs` or `ort`) → check + clippy + `test --release --no-run` + smoke matrix.
- **No new phase tag.** Lateral epic; seals via ADR Accepted + STATUS update + judges green (LESSONS P5).

## Sealed phases / files NOT touched by this epic

Per §3's explicit boundary list, plus the `sealed-phases-untouched` judge enforcement:

- Hotkey FSM (`hotkey/state.rs`, `hotkey/windows.rs`, `hotkey/driver.rs`).
- Injection (`injection/*`).
- Secure-input guard.
- Meeting Capture (`meetings/*`) — **except** `meetings/persist.rs`, which gains a read-only source-tracking-aware export-job read path (no writes to meeting tables, no changes to meeting recording behavior).
- Activity Capture (`activity/*`).
- Recording Command Center (`command_center/*`, `overlay_conventions.rs`).
- All UI design tokens (`ui/src/design/tokens.css` and friends).
- Cleanup pipeline (`cleanup/*`) — used as library; not modified.
- All migrations 001-017.

## Wave structure (six waves)

| Wave | Scope | Beads |
|---|---|---|
| **Wave 0** | Charter (this ADR) + sync-layer behavior spike on real iOS → Win11 hardware (Dustin runs). Spike report attaches to the ADR before Accept. **Blocking** — no code lands before this passes. | mb-? (charter, Bernard), mb-? (spike, Dustin) |
| **Wave 1** | Foundation: extract `headless_ingest`, build `audio::decode`, ship migration 018. | 3 beads |
| **Wave 2** | Outbound: `vault/` layout + manifest + projection + reconciliation export job. | 3 beads |
| **Wave 3** | Inbound: watcher + pickup pipeline + iOS Shortcut spec + user setup docs. | 3 beads |
| **Wave 4** | UI + bonus: "+ Audio file" import button + Settings Mobile tab + 8 new settings keys. | 2 beads |
| **Wave 5** | Hardening + judges: concurrency validation + format versioning + machine fingerprint + 5 invariant judges + dry-run rig + live-fire smoke matrix (Dustin). | 3 beads |

**Wave 0 must pass before any Wave 1 work begins.** The spike's findings directly determine the combined-detector parameters in §6 (specifically the quiet-window duration, the minimum absolute age, and the conflict-filename regex). Spec-then-implement order is non-negotiable.

**Live-fire smoke matrix is BLOCKING for seal** (LESSONS PINNED P7). Judges prove contracts hold; only the smoke matrix on real iOS-phone-to-Win11-desktop hardware proves the integration works end-to-end.

## Cross-references

- **Source plan:** `mockingbird-mobile-extension-plan.md` (repo root).
- **Sibling-subsystem precedent:** ADR 0036 (Activity Capture — sibling under `activity/`, sealed primitives only).
- **Boundary-authorization precedent:** ADR 0037 (Command Center — explicit list of sealed-surface edits, enforced by judge).
- **Most recent Phase-3 amendment:** ADR 0045 (programmatic dictation start/stop — same `complete()`-adjacent code; this ADR continues the pattern of small, scoped, ADR-charters for sealed-Phase-3 edits).
- **Sealed primitives reused:**
  - `dictation/` pipeline trio (Silero VAD + whisper-rs CUDA STT + cleanup-provider trait) — all called from `headless_ingest`, none modified.
  - SQLite repo layer + migration runner — ADR 0004.
  - DPAPI + typed-settings — for backend credentials (no new key needed for the POC backend, but the path is open).
- **Binding principles touched:**
  - Principle 1 (raw immutability) — `transcripts(stage='raw')` is written by `headless_ingest`, never updated. Inherits the existing trigger.
  - Principle 2 (provenance total) — `sessions.source` + `sessions.start_mode` + cleanup prompt version cover the new sources.
  - Principle 3 (layers replaceable) — sync backend is pluggable (Obsidian Sync POC; Syncthing as documented future); audio decode is behind a single helper.
  - Principle 4 (no telemetry) — no outbound HTTP. Sync-backend traffic is the user's chosen backend's responsibility; Mockingbird itself never phones home. Opt-in default-off (§10).
  - Principle 5 (cross-platform) — `vault/` is pure-Rust with no Windows-specific calls except where filesystem-watcher backends naturally differ; `notify-debouncer-full` is cross-platform.
  - Principle 6 (no shortcuts) — fixtures + judges + smoke matrix all required for seal.
  - Principle 7 (clipboard save/restore) — N/A; vault pipeline never touches clipboard.
  - Principle 8 (secure-input fields abort) — N/A; vault pipeline never injects.
- **LESSONS touched:**
  - P2 (test-runner broken) — fallback gate applies as usual.
  - P5 (lateral epics seal via ADR, not tag) — no new phase tag.
  - P7 (judges don't catch live-OS regressions) — Wave 5 smoke matrix is BLOCKING.

---

_The `adr-format` judge validates this structure exists in every numbered ADR. Keep section headings stable._
