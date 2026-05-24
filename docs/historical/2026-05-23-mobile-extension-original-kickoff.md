<!--
  ════════════════════════════════════════════════════════════════════
  HISTORICAL DOCUMENT — DO NOT USE FOR IMPLEMENTATION
  ════════════════════════════════════════════════════════════════════

  This was Dustin's original feature kickoff for the mobile-extension
  effort, pasted into the repo root on 2026-05-23. It is preserved
  here as historical context for the source-of-truth narrative that
  motivated the work.

  It is **fully superseded** by:

    • `docs/adr/0046-mobile-extension-via-vault.md`
      — the canonical implementation plan and architectural contract.
    • `STATUS.md` § "Currently active" / ADR 0046
      — the live iteration sequence (Iter 1 + Iter 2 sealed; Iter 3
        in-flight at time of relocation).
    • `docs/spikes/iter3-sync-layer-findings.md`
      — the 8 sync-layer findings driving the Iter 3 inbound watcher
        + courier design.

  **Nothing — code, ADRs, beads, dispatches — should reference this
  file as authoritative.** Cite the ADR instead.

  Moved from repo root → `docs/historical/` on 2026-05-27 to clear
  the root namespace while preserving the original narrative for
  posterity.
  ════════════════════════════════════════════════════════════════════
-->

# mockingbird.exe — Mobile Extension Plan

**Concept:** Extend a desktop-only, local-only dictation/meetings app to mobile **without building a native mobile app**, by using a synced folder of plain Markdown files as the bridge between the desktop app and an Obsidian vault on the phone.

**Status:** Architecture plan / proof-of-concept spec. This document is written for a codebase agent that has access to the mockingbird.exe source. It deliberately does **not** assume the internal structure, language, schema, or storage engine of the existing app. The agent must cross-reference this plan against the actual codebase and produce its own implementation plan.

---

## 1. Goal and constraints

### 1.1 What we are building
A one-way-per-direction sync loop that lets a phone:
- **Read** a filtered, projected view of dictation and meetings history.
- **Submit** new dictation requests (primarily voice, optionally text) that the desktop app picks up and processes locally, exactly as if the recording had happened on the desktop.

### 1.2 What we are explicitly NOT building
- No native iOS/Android app.
- No public-facing server.
- No cloud processing of audio. All transcription/processing stays on the desktop machine.
- No use of the Apple Notes API (it does not exist as a usable third-party API; this was investigated and rejected).

### 1.3 Hard constraints
- **Proof-of-concept host OS: Windows only.** A later port to macOS is planned once features are stable. The design must therefore avoid hard dependencies on macOS-only mechanisms (AppleScript, macOS Shortcuts folder-automations). The desktop side uses a **native file-system watcher**, which is OS-agnostic.
- **Local-first / privacy.** Processing is local. The POC sync layer (Obsidian Sync) does route files through a third-party server; this is an accepted POC trade-off, with a documented migration path to a zero-cloud option (see §7).
- The synced folder must be treated as an **untrusted, failure-prone medium**. Sync conflicts, partial writes, and out-of-order delivery are expected. No component may treat the synced folder as a source of truth.

### 1.4 Start here — read before building anything

**This feature is a bolt-on.** The overwhelming majority of the work is *new, self-contained modules* that sit alongside the existing app (file-system watcher, export/projection job, settings panel, ledger). The existing dictation UX, meeting recording, storage engine, and UI are **not** being rewritten — they are harnessed and their visibility extended to mobile. If the new modules were deleted, the app would behave exactly as it does today.

**However, before writing any code, the agent must verify one thing — it determines the scope of the entire effort:**

> **Can the existing audio-processing pipeline be invoked headlessly — i.e. called with an audio file as input, without going through the recording UI?**

- **If yes:** this is a near-pure bolt-on. The watcher and the "+ Audio file" button simply call that existing entry point. Existing code is barely touched.
- **If no** (processing is currently entangled with the "Start dictation" UI flow, with no way to start processing except by recording through the interface): then the **first task** is to *extract a headless entry point* from the existing pipeline — separating "process this audio file" from "record audio via the UI." This is the **one place existing code gets restructured.** It is low-risk and good hygiene regardless, but it is real work and it must happen before the bolt-on modules can be built.

This single question is the pivot the whole plan turns on. It is also formally listed as **§9 risk #1**, but it is elevated here because it must be answered *first* — before even the sync-layer spike in the build sequence (§10).

Beyond that one check, the only other contact with existing code is **additive, not revising**: possibly adding one or two record fields (a `source` field and a stable record ID — §9 risks #2, #3), and adding a hook so that committing a new record triggers the export job (§4.3). Neither changes how existing features behave.

**First three things the agent should do, in order:**
1. Locate the audio-processing pipeline in the codebase and answer the headless-invocation question above. This sets the scope.
2. Confirm whether a stable record ID and a `source` field exist, or must be added (§9 risks #2, #3).
3. Then proceed to the build sequence in §10.

---

## 2. Architecture overview

```
  PHONE                              SYNC LAYER              DESKTOP (Windows, POC)
  ─────                              ──────────              ──────────────────────
  Capture: iOS Shortcut
   (Record Audio → Save File)  ──►   vault/inbox/   ──►  [ File-system watcher ]
                                                              │
                                                              ▼
                                                       mockingbird.exe
                                                       processes locally
                                                       (transcribe, create record
                                                        in canonical DB)
                                                              │
                                                              ▼
  Read: Obsidian mobile        ◄──   vault/history/ ◄──  [ Export / projection job ]
   (renders Markdown notes)                                   writes Markdown files
```

Two independent flows that **share a folder but do not depend on each other**:

- **Outbound (desktop → phone):** the app *projects* canonical records into Markdown files in `history/`.
- **Inbound (phone → desktop):** a phone Shortcut drops an audio file into `inbox/`; the app's watcher consumes it.

If either side is removed, the other still works. Obsidian is a **reader only**. The Shortcut is the **writer**. Neither is load-bearing for the other.

### 2.1 Component responsibilities

| Component | Owns | Does NOT do |
|---|---|---|
| mockingbird.exe (canonical DB) | Source of truth for all records | Never reads its truth back from the vault |
| Export/projection job (new) | Materializes canonical records → Markdown in `history/` | Does not delete user data; idempotent |
| File-system watcher (new) | Detects new audio in `inbox/`, hands to processing | Does not parse/transcribe itself |
| Obsidian mobile | Renders/searches `history/` | Not a queue, not a transport mechanism |
| iOS Shortcut | Records audio, writes file to `inbox/` | No knowledge of desktop internals |
| Sync layer (Obsidian Sync, POC) | Moves the vault folder between devices | No application logic |
| Desktop settings panel (new) | Owns user config: vault path, sync backend, retention window, opt-in (§8) | Does not perform sync itself |

---

## 3. The synced folder (vault) structure

The vault is a single Obsidian vault folder, synced as a unit. Proposed layout (agent may adjust naming to fit conventions, but the **separation of the outbound `history/` zone from the inbound `inbox/` zone must be preserved**):

```
mockingbird-vault/
  history/          # OUTBOUND: exported dictations + meetings. Projection of canonical DB.
                    #   Transcripts (Markdown) only — no source audio. Read-only from the phone.
  inbox/            # INBOUND: new request files land here (audio, optionally text).
                    #   The desktop watcher consumes this. Files are couriers, not blobs (§5.4).
  inbox/_failed/    # INBOUND: courier files that failed processing, with error sidecars (§9).
                    #   The only place inbound files persist, and only until the user resolves them.
  .mockingbird/     # State/metadata the app needs (e.g. export manifest, dedup ledger).
                    #   Hidden from Obsidian's normal view.
```

Note: there is **no `processed/` archive** and **no `_attachments/` audio store** in the synced vault. Successfully-processed courier files are discarded (§5.4); source audio retention is a desktop-side, toggle-governed decision and never enters the vault (§5.7).

**Rationale for zones:**
- `history/` and `inbox/` must be separate so the reader flow and the writer flow never collide.
- `inbox/` is self-cleaning: successful files are discarded (§5.4), failed ones go to `inbox/_failed/`. Sync re-delivery of an already-processed file is caught by the dedup ledger in `.mockingbird/`, not by retaining the file.
- `.mockingbird/` keeps app bookkeeping out of the user-facing note list.

---

## 4. Outbound flow — projecting history to the phone

### 4.1 Core principle: the export is a disposable projection
The vault's `history/` folder is a **materialized view** of the canonical database, not a second copy of record. The canonical DB inside mockingbird.exe remains the only source of truth.

**Why separate, not redundant:**
- The synced folder is failure-prone; if it *were* the database, every sync conflict would be data corruption. As a projection, every conflict is just "regenerate."
- The projection can have a **different, narrower shape** than the internal schema (the phone view needs a handful of fields, not the full internal record).
- The projection is cheap to fully rebuild from the DB if it ever drifts.

The agent should **not** introduce a second persistent store. The projection's only "storage" is the Markdown files themselves plus a lightweight manifest in `.mockingbird/`.

**`history/` is one-way and read-only by design — there is no merge-back.** The `history/` folder is intended purely for **downstream consumption**: a place to read, search, and reference dictation history that originated and is stored on the desktop. It is not a collaboration or editing surface. Obsidian cannot technically prevent a user from editing a `history/` note on their phone, so the agent must handle this consciously rather than ignore it:
- Any edit made to a `history/` note on the phone is **not synced back** to the canonical DB. mockingbird.exe never reads `history/` as input.
- On the next export of that record, the file is **overwritten** with the canonical version (the export is idempotent, §4.4). A phone-side edit to a projected note is therefore transient and will be lost.
- This is acceptable and intended: the canonical record is the single source of truth, and `history/` is a disposable view of it. The agent should **not** build merge, diff, or conflict-resolution logic for `history/`. If a user wants their dictation history edited, that happens in the desktop app, which then re-projects.
- The user-facing setup copy (§8.3) should make this clear so users understand `history/` notes are a read-only mirror, not editable documents.

### 4.2 The export query
Inside mockingbird.exe, add a configurable **export query** — a filter over canonical records that defines what gets projected. Examples of filter dimensions (agent decides which are feasible against the real schema):
- **Time window — a first-class, user-facing setting (see §4.2.1).**
- Record type (dictations, meetings, or both).
- Tag/flag-based (e.g. only records the user marked "sync").

The export query result is the complete set of records that *should* exist as files in `history/`. The export job reconciles the folder to match that set.

#### 4.2.1 Retention window setting
The desktop app must expose a user-facing setting: **"Sync history from the last ___ days."**
- **Default: 30 days.** Sensible options might include 7 / 30 / 90 / 365 / All.
- This setting *is* the time-window filter of the export query.
- **Changing this setting must trigger a full reconciliation, not just affect future exports:**
  - **Narrowing** the window (e.g. 90 → 30 days) moves now-out-of-window files out of `history/` per the §4.4 stale-file policy (archive, do not hard-delete).
  - **Widening** the window (e.g. 30 → 365 days) backfills: previously-excluded records are projected into `history/`.
- Rationale: this is the user's primary lever over *how much personal data leaves the desktop and enters the synced vault*. It is a privacy control as much as a sync-scope control, and it belongs in the desktop UI alongside the connection settings of §11.

### 4.3 Trigger
The export job runs:
- On every new dictation/meeting record being committed.
- On edit/re-processing of an existing record.
- On a manual "rebuild vault" command (full reconciliation).

### 4.4 Idempotency and stability requirements (important)
To avoid sync churn and phantom edits in Obsidian:
- Each canonical record maps to a **deterministic filename**, e.g. `history/2026-05-23-1430-standup.md`. The same record always produces the same path.
- Re-exporting an **unchanged** record must produce a **byte-identical file**. No timestamps-of-export, no nondeterministic ordering, no volatile fields in the output.
- Only changed records are rewritten. The `.mockingbird/` manifest should track a content hash per exported record so the job can skip unchanged ones.
- Deletes: if a record leaves the export query's result set, the agent should decide a policy — recommended default is to move the stale file to a `history/_archive/` subfolder rather than hard-delete, to avoid surprising the user.

### 4.5 Proposed file format
Each history file is Markdown with YAML front-matter. **Front-matter = structured fields; body = human-readable transcript.** Suggested schema (agent maps these to real internal fields; names are illustrative):

```markdown
---
id: mb-000182                      # stable canonical record ID
type: dictation                    # dictation | meeting
created: 2026-05-23T14:30:00        # original record time, NOT export time
duration_sec: 312
title: Standup notes
tags: [work, standup]
source: desktop                     # desktop | mobile-inbox
mockingbird_export_version: 1       # schema version of THIS file format
---

# Standup notes

(Full transcript text as the note body. Plain Markdown so Obsidian
renders, searches, and links it natively.)
```

Notes:
- `mockingbird_export_version` lets the format evolve without breaking older synced files.
- Keep the body plain. No app-specific markup that Obsidian would mangle.
- Internal-only fields (model version, confidence scores, file paths, processing timestamps) are **deliberately omitted** from the projection.

---

## 5. Inbound flow — submitting a dictation request from the phone

**Scope — inbound is dictation only, not meetings.** This inbound flow deliberately covers dictation and not meeting capture. The reason is architectural, not a deferral of effort: the desktop meeting-recording feature captures **two streams — microphone plus system audio** — whereas a phone can only practically capture a **single audio stream**, and that stream cannot separate speakers. A single-stream mobile recording is a natural fit for dictation (one speaker, transcription only) but does not produce a meaningful "meeting" by mockingbird's existing definition. Mobile meeting capture would therefore require its own dedicated design — deciding what a single-stream mobile meeting even *is* and whether it is worth supporting — and is out of scope for this plan. Meetings remain part of the **outbound** projection (§4): meetings recorded on the desktop still sync to the phone for reading.

### 5.1 Capture: an iOS Shortcut (not Obsidian)
Capture is handled by an **iOS Shortcut**, which runs entirely on the phone and has **no macOS dependency** — this is purely an iOS feature and is unaffected by the Windows-only POC host.

The Shortcut:
1. Uses the **Record Audio** action to capture a voice memo.
2. Saves the resulting audio file into `mockingbird-vault/inbox/` with a timestamped, collision-proof filename (e.g. `inbox/2026-05-23T143000-<shortuuid>.m4a`).
3. Optionally writes a tiny sidecar `.json` or `.md` next to it with request metadata (requested title, tags) — see §5.5.

Bind the Shortcut to the **Action Button** (iPhone 15 Pro and later) so capture is: press, talk, release.

**Why a Shortcut and not Obsidian's built-in audio recorder:** recording inside an Obsidian note couples capture to Obsidian's attachment-filing behavior and note-sync conflicts. The Shortcut writes a plain file to a plain folder — robust, and independent of Obsidian.

### 5.2 Transport
The sync layer carries the new `inbox/` file to the desktop. No app logic here.

### 5.3 Pickup: desktop file-system watcher
mockingbird.exe runs a **native file-system watcher** on `inbox/`. This is OS-agnostic (works on Windows now, macOS later). On detecting a new, fully-written file:

1. **Verify the file is complete.** Sync tools can expose partially-written files. The watcher must wait for write-stability (e.g. size unchanged for N seconds, or a sync-tool-specific completion signal) before processing. Do not process a file mid-sync.
2. **Deduplicate.** Check the file against the processed-file ledger in `.mockingbird/`. Sync layers can re-deliver a file; an already-processed file must be ignored.
3. **Write a processing-status placeholder** into `history/` (see §5.9) so the user has immediate feedback that the request was received, before the potentially-slow processing step begins.
4. **Hand the audio to the existing local processing pipeline** — the same code path mockingbird.exe already uses for a local dictation. The agent must locate this entry point in the codebase; this plan does not assume its shape. **Because the inbound audio enters the same pipeline, the existing global "keep audio blobs" setting governs the resulting record's audio automatically — see §5.7.**
5. On success, the pipeline creates a normal canonical record (with `source = mobile-inbox` so its origin is traceable).
6. **Discard the courier file** from `inbox/` (see §5.4).
7. The new canonical record triggers the **outbound export job** (§4.3), which **overwrites the placeholder** with the finished transcript note in `history/`; it then syncs back to the phone.

**Startup catch-up — the desktop app does not need to be running when files arrive.** A live file-system watcher only sees events while the app is running. Because the desktop app may be closed, asleep, or simply not running when the user records on their phone, inbox files will **queue in `inbox/`** and wait. On every startup, before (or alongside) starting the live watcher, mockingbird.exe must **scan `inbox/` for any files already present** and process them through the same pickup steps above. The dedup ledger (step 2) ensures a file is never double-processed if a startup scan and a watcher event race. Effect: the user can record freely on mobile regardless of desktop state, and the desktop "catches up" on the backlog the next time it launches.

### 5.9 Processing-status placeholder — no silent black hole
Because local processing can take minutes (§5.6), there would otherwise be a window where the user has recorded on their phone and sees **nothing** in `history/` — no way to tell whether the request was received, is still processing, or failed. A user staring at that gap may assume failure and re-record.

To prevent this, the moment the watcher accepts a file (§5.3 step 3, *before* processing begins), it writes a small **placeholder note** into `history/`:
- The placeholder is a normal Markdown file keyed to the eventual record ID, with a clear status — e.g. front-matter `status: processing` and a body line like "Dictation received, processing…" plus the receipt timestamp.
- This is **free**, not extra machinery: the export job is already idempotent and keyed by record ID (§4.4). The placeholder is simply the *first* write for that record; step 7's export is a normal idempotent **overwrite** of the same file with the finished transcript.
- The user opens Obsidian, sees "processing…", and watches it become the real transcript on the next sync — clear, honest feedback.
- **Failure case:** if processing fails, the placeholder must be updated to a visible failed state (e.g. `status: failed` with a short message) rather than left forever on "processing…". This complements the `inbox/_failed/` courier handling (§5.4) — one surfaces the failure on the phone, the other preserves the audio on the desktop for retry.
- Finished records carry `status: complete` (or omit the field); the agent should pick one convention and keep it consistent so Obsidian-side filtering/search behaves predictably.

### 5.4 The `inbox/` file is a courier, not a blob — discard it after processing
The audio file in `inbox/` exists only to transport a recording from the phone to the desktop. It is **not** the app's "audio blob" — it is a courier. Once processing succeeds and a canonical record exists, the courier file has done its job and must be removed, **regardless of the "keep audio blobs" setting.** That setting governs record audio (§5.7), not transport files.

Reasons the courier file is always cleaned up:
- Leaving consumed audio anywhere in the synced vault means personal recordings accumulate in synced space indefinitely — directly against the privacy posture.
- Hard-deletes across a sync layer can themselves cause conflicts, so the safe pattern is: after successful processing, **move the file out of the synced vault to a short-lived local-only staging area**, confirm the canonical record exists, then delete it locally. It never lingers in synced space.
- A re-delivered file landing back in `inbox/` is matched against the dedup ledger (§5.3 step 2) and ignored, so audit history of the courier is not needed.

On **failure**, the courier file is moved to `inbox/_failed/` with an error sidecar (§9) so the user can see and retry it; failed files are the one case where a courier is retained, and only until the user resolves it.

**Development and testing exception.** The discard rule above is a *production privacy* behavior, not an absolute. During development and testing, it is expected and acceptable to **retain mobile audio courier files** — keeping a set of real recordings from the mobile device is necessary for testing the inbound pipeline and for reprocessing while debugging. The agent should implement the discard as the *default production behavior*, but provide a development affordance (e.g. a debug/dev flag, or a `inbox/_keep/` staging area excluded from normal cleanup) so test audio can be preserved and re-run. The reasoning to preserve: the discard rule exists to avoid personal recordings piling up in synced space on *end-user installs* — it is not a reason to make debugging the inbound flow harder for the developer. Do not let the production rule erase the test corpus.

### 5.7 Audio retention is governed by the existing global "keep audio blobs" toggle
mockingbird.exe already has a global **"keep audio blobs"** setting that controls whether the app retains audio as part of a record. This plan introduces **no separate retention rule for mobile audio.** Mobile-originated audio must obey the same global toggle as desktop-originated audio:
- If the toggle is **on**, the canonical record created from an inbox file keeps its audio blob, exactly like a desktop dictation.
- If the toggle is **off**, the audio is used for processing and then discarded, exactly like a desktop dictation.
- This works automatically *if and only if* the inbound watcher routes audio through the same processing pipeline (§5.3 step 4). The agent must **not** build a separate audio-retention path for inbound files — doing so would bypass the global setting and create an inconsistency.
- Note the distinction: the global toggle governs the **record's blob**; the **courier file** in `inbox/` is always discarded (§5.4) and is outside the toggle's scope.

Consequence for the outbound projection: source audio is therefore **never written into the synced `history/` folder** by this plan. `history/` contains transcripts (Markdown) only. Whether a blob is retained at all is a desktop-side, toggle-governed decision; it does not enter the vault.

### 5.5 Optional: text-based requests
A plain `.md` or `.txt` file dropped into `inbox/` containing typed text can be treated as a **text dictation request**. This is robust (plain text in a file) and is a reasonable optional extension. Audio-embedded-in-an-Obsidian-note is **not** recommended as a request mechanism, for the same coupling reasons as §5.1.

### 5.6 Round-trip latency expectation
End-to-end latency (press button on phone → transcript visible on phone) is **not** dominated by the sync layer alone. The round-trip has **three sequential parts**:

1. **Sync up** — courier file travels phone → desktop (sync-layer latency, typically seconds to ~a minute).
2. **Local processing** — mockingbird.exe transcribes/processes the audio. **All LLM/transcription work runs locally on the desktop machine.** This term is highly variable and is often the *largest* of the three: it scales with recording length and is gated by the desktop machine's specs (CPU/GPU, model size). A long recording on modest hardware can be minutes.
3. **Sync down** — the resulting `history/` note travels desktop → phone (sync-layer latency again).

Implication: the experience is **"fire and forget, read it later,"** not real-time, and on slower machines "later" can be several minutes. Processing time, not sync time, is usually the variable to watch. The processing-status placeholder (§5.3) exists specifically so this delay is not a silent black hole for the user.

### 5.8 Bonus desktop feature — "+ Audio file" import button
Because this plan already builds a pipeline entry point that accepts an arbitrary audio file and turns it into a canonical record (§5.3 step 4), the same capability can be surfaced as a **dedicated desktop feature** at no meaningful extra cost: an **"+ Audio file"** button in the Dictations screen, placed next to the existing **"Start dictation"** button.

Behavior:
- The button opens a standard local file picker. The user selects an existing audio file from their computer.
- The selected file is handed to the **same processing pipeline entry point** the inbound folder-watcher uses. There is no new processing or record-handling logic — this is simply a second front door into the existing path.
- Processing produces an ordinary canonical record, which (per §4.3) triggers the outbound export like any other record.

Design notes for the agent:
- **One pipeline, two entry points.** The folder-watcher (files arriving from the synced `inbox/`) and this button (files chosen from the local disk) converge on the same processing function. Implement that function so it is agnostic to how the file arrived. Do not fork the logic.
- **Provenance.** Records created via this button should carry a distinct `source` value (e.g. `desktop-import` / `file-import`) to keep origin traceable, consistent with the `source` field in §4.5. Watcher-originated records remain `mobile-inbox`; desktop dictations remain `desktop`.
- **Audio retention.** An imported file's audio blob is governed by the existing global "keep audio blobs" toggle (§5.7), exactly like every other record. No special case.
- **Format handling.** Unlike the iOS Shortcut (which produces a known format), a user-picked file could be any format. The agent should apply the same format-compatibility / transcode handling identified in §9 risk #4, and surface a clear error for unsupported files.
- This feature is **independent of mobile sync.** It works whether or not the user has enabled the mobile-extension features — it is a plain desktop capability that happens to reuse the same plumbing.

---

## 6. Sync layer — POC decision and rationale

The sync layer is an **explicitly pluggable component**. Nothing else in the system depends on which tool is used; swapping it is a configuration change, not a redesign.

### 6.1 POC choice: Obsidian Sync
**Chosen for the proof-of-concept.** Rationale:
- Single vendor, single account, minimal setup — lowest operational burden while proving the architecture.
- Reliable, hands-off background sync on both Windows and iOS.
- Purpose-built to sync an Obsidian vault.

**Accepted trade-offs:**
- Files (including `inbox/` audio) pass through Obsidian's servers — a dent in the local-only privacy story, accepted for the POC stage.
- A recurring subscription cost.
- Obsidian Sync is tuned for Markdown notes and **may be fussy with binary attachments** (the `inbox/` audio files). **The agent must validate during a build spike** that audio files in the vault sync promptly and are not size-throttled.

### 6.2 Documented future option: Syncthing
Planned as an **optional, more-secure path** added later:
- Peer-to-peer, no cloud server — audio never leaves the user's two devices, fully restoring the local-only privacy promise.
- Free and open-source; vendor-neutral.
- **Known caveat:** Syncthing on iOS is a third-party app subject to iOS background-execution limits — sync may only fire when the app is foregrounded or the phone is charging/on-network, adding occasional lag.

Because the rest of the system is sync-agnostic, migrating POC → Syncthing (or running a hybrid: audio over Syncthing, Markdown over Obsidian Sync) is a later config decision, not a rework.

### 6.3 Rejected: iCloud Drive
Rejected for the POC. The Windows iCloud Drive client syncs on its own schedule, is slow to materialize files, and is awkward to watch programmatically. May become viable on the macOS port, but is not part of this plan.

---

## 7. Migration path (POC → hardened)

1. **POC:** Obsidian Sync, Windows desktop host. Prove the full loop works.
2. **Validate** the binary-attachment behavior of Obsidian Sync (§6.1). If audio sync is throttled, split transport: Markdown via Obsidian Sync, audio via a dedicated Syncthing folder.
3. **macOS port:** the desktop file-watcher is already OS-agnostic, so the port is mostly build/packaging. AppleScript/Notes integration is explicitly NOT reintroduced.
4. **Privacy hardening:** offer Syncthing as a user-selectable sync backend for those who want zero cloud.

---

## 8. Desktop configuration, connection management, and distribution

mockingbird.exe is **local-only software, but is being built as an open-source project intended for public distribution** — anyone can install it on their own machine. This has direct architectural consequences for how the mobile-extension features are built. The agent must treat the app as software for strangers, not for the developer's machine.

### 8.1 No machine-specific assumptions
Because every install is on an unknown machine:
- **No hardcoded paths.** Vault location, sync-tool install location, and any working directories must be user-supplied or discovered at runtime.
- **No assumed environment.** Do not assume Obsidian is installed, that a vault already exists, that any sync tool is present, or that the user has a particular OS layout. Detect, and guide the user when something is missing.
- **No bundled developer credentials or accounts.** The user supplies their own Obsidian Sync account / Syncthing setup. The app never ships with any.
- **Portable, per-install config.** All settings live in a config file in a standard per-user location (the agent should use the OS-appropriate convention, e.g. an app-data directory) — never inside the repo, never a fixed absolute path.

### 8.2 The desktop app owns the connection
The mobile-extension features handle **personal data** — dictation transcripts and meeting records. The desktop app is the single place where the user configures, consents to, and controls where that data goes. The connection to the sync route is **not** assembled out-of-band by the user manually wiring folders; it is a managed feature of mockingbird's own settings UI.

The desktop app must provide a **settings panel** that covers, at minimum:

| Setting | Purpose |
|---|---|
| **Mobile sync: on / off** | Master switch. When off, no projection is written and no folder is watched. Default: **off** — the user opts in. |
| **Vault folder location** | User picks the Obsidian vault path. The app creates the `history/`, `inbox/`, `.mockingbird/` zones inside it (§3). |
| **Sync backend** | Choice of sync layer (POC: Obsidian Sync; future: Syncthing). Pluggable per §6. The app does not perform the sync itself — it documents/links what the user must set up. |
| **Retention window** | The "last ___ days" control from §4.2.1. Default 30. |
| **Record types to sync** | Dictations / meetings / both (§4.2). |
| **Connection status / health** | Shows whether the vault path is valid, whether files are flowing, last successful export, last inbox pickup. |

### 8.3 Privacy-respecting defaults
Because this ships to the public and touches personal data:
- Mobile sync is **off by default**. The feature is opt-in; a fresh install behaves exactly like today's local-only app until the user deliberately enables it.
- When the user enables it, the UI must **state plainly what leaves the machine**: that enabling sync writes transcripts/recordings into the chosen vault, and that the selected sync backend (e.g. Obsidian Sync) transmits those files through a third-party server. This is informed consent, not a buried setting.
- The retention window (§4.2.1) is presented as the user's lever over *how much* personal data is exposed.
- Disabling mobile sync must stop the watcher and the export job, and should offer to clear the projected files from the vault.

### 8.4 First-run / setup guidance
Since the app cannot assume Obsidian or a sync tool is installed, the settings panel should detect what is missing and guide the user (link to install Obsidian, link to Obsidian Sync setup, or Syncthing instructions). The agent should treat "a non-developer installs this and can get mobile sync working from the UI alone" as the acceptance bar.

### 8.5 Security of the config itself
- Any credentials or tokens the sync backend requires should be stored using the OS credential store where available, not in plain text in the config file.
- The config file should never be committed to the open-source repo; ship a documented example/template instead.

---

## 9. Risks and open questions for the codebase agent

The agent must resolve these against the actual mockingbird.exe codebase:

1. **Processing entry point.** Where in the codebase is the local dictation/transcription pipeline invoked? The inbound watcher must call into the *same* path so a mobile-submitted recording becomes an ordinary record. This plan does not assume its signature.
2. **Canonical schema mapping.** Which real fields map to the projected front-matter in §4.5? Which fields are internal-only and must be excluded?
3. **Record identity.** Is there a stable, immutable record ID suitable for deterministic filenames (§4.4)? If not, one must be introduced.
4. **Audio format compatibility.** The iOS Shortcut's Record Audio action outputs a specific format/codec (commonly `.m4a`/AAC). Confirm the existing pipeline accepts it, or add a transcode step.
5. **File-write-completion detection.** What is the most reliable way to know an inbox file is fully synced and not mid-write? May depend on the sync tool's behavior; needs a spike.
6. **Concurrency.** What happens if the export job and a new dictation fire simultaneously? The export job must be safe to run concurrently or be serialized.
7. **Conflict files.** Sync tools create conflict copies (e.g. `file (conflicted copy).md`). The watcher and export job must recognize and quarantine these rather than process them.
8. **Obsidian Sync binary-attachment throttling** (§6.1) — validate early; it influences whether transport must be split.
9. **Ledger durability.** The processed-file ledger in `.mockingbird/` must survive app restarts and itself not be corrupted by sync (consider keeping the authoritative ledger in the canonical DB, with `.mockingbird/` as a derived hint only).
10. **Failure surfacing.** If an inbox file fails to process, how is the user notified? Recommended: move it to an `inbox/_failed/` folder with an error sidecar, so the failure is visible in Obsidian.
11. **Config and credential storage.** Where does the per-install config live on each supported OS, and what OS credential store is used for any sync-backend tokens (§8.5)? Must work for a non-developer installing from a public release.
12. **Vault format versioning and migration.** The exported file format carries `mockingbird_export_version` (§4.5), but there is no migration story for an *existing* vault when the app updates to a newer export format. The **desktop app must own this**: on a format change, it should be able to detect an older vault and re-project it (a full rebuild from the canonical DB is the safe path, since the projection is disposable). The agent must define this migration behavior so a future app update does not silently leave a user's vault in a stale, mixed-version state.
13. **Single-user / single-desktop assumption.** This plan assumes **one user, one desktop, one vault**. Undefined and out of scope: two desktops projecting into the same vault, the same vault opened by two phones, or any multi-writer arrangement. The agent should review this assumption against intended use and either confirm it as a documented limitation or flag the need for a separate design.

---

## 10. Build sequence (suggested)

A spike-first order so the riskiest unknowns are tested early:

1. **Spike — sync layer.** Set up an Obsidian vault + Obsidian Sync across the Windows machine and a phone. Confirm Markdown *and* audio files sync reliably and promptly. (Resolves risk #8.)
2. **Outbound MVP.** Implement the export/projection job: one canonical record → one deterministic, idempotent Markdown file in `history/`. View it in Obsidian mobile. (Resolves risks #2, #3.)
3. **Inbound MVP.** Build the iOS Shortcut (Record Audio → save to `inbox/`). Build the desktop file-watcher with write-completion detection and dedup. Wire it to the existing processing pipeline. (Resolves risks #1, #4, #5.)
4. **Close the loop.** Confirm a phone-recorded memo becomes a canonical record and its transcript syncs back into `history/` and appears in Obsidian.
5. **Harden.** Conflict-file handling, failure surfacing, concurrency, ledger durability. (Risks #6, #7, #9, #10.)
6. **Document** the user setup (vault location, Obsidian Sync config, installing the Shortcut, Action Button binding).

---

## 11. Summary

- The mobile extension is a **synced folder of plain Markdown files** bridging mockingbird.exe and an Obsidian vault on the phone — no native mobile app.
- **Outbound:** the desktop app projects canonical records into `history/` as idempotent Markdown files. The projection is disposable and separate from the canonical DB by design.
- **Inbound:** an iOS Shortcut writes voice recordings into `inbox/`; a native, OS-agnostic file-watcher in mockingbird.exe consumes them through the existing local processing pipeline.
- **Bonus desktop feature:** because the inbound pipeline accepts an arbitrary audio file, the same capability is surfaced as an **"+ Audio file"** import button in the Dictations screen — a second entry point into the same pipeline, working independently of mobile sync.
- **Obsidian is a reader; the Shortcut is a writer.** They share a folder but are independent.
- **Sync layer is pluggable.** POC uses Obsidian Sync for low setup cost; Syncthing is the documented future zero-cloud upgrade.
- **The desktop app owns the connection.** A settings panel controls vault location, sync backend, retention window (default 30 days), and record types — with mobile sync opt-in and off by default. Because the project is open-source and publicly distributed, the build must assume unknown machines: no hardcoded paths, no bundled credentials, portable per-install config, and first-run guidance.
- The Windows-only POC host imposes **no blocking constraints** — the design avoids macOS-only mechanisms, so the later macOS port is mostly packaging.
- The codebase agent must cross-reference §9's open questions against the real source before producing an implementation plan.
