# ADR-0036: Activity Capture is a sibling subsystem; chartered as Phase 10

- **Status:** Accepted
- **Date:** 2026-05-25 (Proposed) → 2026-05-24 (Accepted, by Dustin via Bernard planning round)
- **Deciders:** Dustin (project lead), Bernard / code-puppy (chartering)
- **Charter for:** Phase 10 — Activity Capture (numbered PLAN §10 phase, six waves; mirrors Phase MC's container shape: numbered + ADR-chartered + per-wave seal tags + final `phase-10-complete` tag)
- **Source plan:** `mockingbird-activity-capture-plan.md` at repo root (untracked feature plan; this ADR is the implementation charter against it)

## Context

Mockingbird ships two sealed user-facing subsystems today:

1. **Dictation** (sealed at `phase-4-complete` and the lateral ADRs 0022/0023/0024) — hold-to-talk, ≤300 s, three modes, clipboard-paste injection into the focused app.
2. **Meeting Capture** (sealed at `phase-mc-complete` and the polish ADRs 0032/0033/0034/0035) — chord activation, hours-long twin-stream capture, chunked Whisper, deterministic formatter, in-app transcript view with optional LLM pass.

A user request now asks for a third feature: **Activity Capture & Session Summary**. The full vision lives in `mockingbird-activity-capture-plan.md`; the short version is "a colleague taking notes over your shoulder while you work, then handing you a clean chronological summary at session end." Concrete shape:

- **Session-scoped** (Start / Pause / Stop) — explicitly NOT always-on background capture.
- **Three independent capture layers, any of which can fail without breaking the others:**
  - Layer 1 — Activity events from the OS accessibility layer (foreground app, window title, focused-field snapshot, visible UI text, app-switch + idle events). The primary signal. Replaces screen recording.
  - Layer 2 — Microphone audio + local transcription. The "narration track." Opt-in per session.
  - Layer 3 — Periodic screenshots + local OCR. Fallback for accessibility-blind apps. Opt-in per app. Deferred to a post-seal optional wave.
- **Summary pipeline:** at session stop, the merged event stream is segmented into Blocks, each Block gets an LLM-generated one-sentence description, then assembled into a chronological session summary. AI is an enhancement, never the critical path — the raw structured timeline is always deliverable even when Ollama is unavailable.
- **No injection.** Activity captures live in the app's own UI (a new `Activity` page + drill-down `ActivityDetail`). Nothing is ever pasted into another app's caret.
- **Local-first, ever.** No telemetry, no cloud sync, no employee-monitoring affordances.

The dictation pipeline, the meeting pipeline, and the activity pipeline share **four primitives** and nothing else: `audio::AudioCapture` (Layer 2), `meetings::long_form_stt` (Layer 2 — chunked Whisper; reused, not re-extended), `cleanup::OllamaProvider` (Stage 3 abstractor — instantiated directly via its existing `new()` and driven via `CleanupRequest<'_>`, exactly as `dictation_run_llm_pass` and `meetings/llm_pass.rs` already do), and the migration runner / SQLite repo layer. Every other dimension diverges:

| Dimension | Dictation | Meeting Capture | Activity Capture |
|---|---|---|---|
| Activation | Hold (Right Alt) | Chord (Right Ctrl + .) | Chord (Right Ctrl + , — proposed) + Pause |
| Duration | ≤300 s | ≤6 h | Open-ended within a session |
| Capture surface | Mic only | Mic + system loopback | UIA + GetLastInputInfo + (optional) mic + (optional) screenshots |
| Output destination | Focused-app caret (clipboard-paste) | In-app transcript view | In-app session-summary view |
| LLM role | Critical path (per-mode cleanup) | Optional post-hoc pass | Per-Block abstraction in a staged pipeline; degrades gracefully |
| Schema | `transcripts` + `sessions` + `modes` | `meeting_sessions` + `meeting_transcripts` + `meeting_chunks` | `activity_sessions` + `activity_events` + `activity_blocks` + `activity_transcript_segments` (new — Waves 1/3/4) |
| Threading | Single hotkey hook thread | Sibling hook thread + capture/STT thread | Sibling chord thread + UIA sampler thread + (optional) capture/STT thread |
| Raw data | `transcripts(stage='raw')` immutable (ADR 0010) | `meeting_chunks` PCM immutable | `activity_events` immutable; only `activity_blocks` editable |

**The forcing question** (exactly as ADR 0026 framed it for Meeting Capture):

> Do we extend the dictation or meeting modules to accommodate Activity Capture, or do we build it as a parallel subsystem that reuses only the genuinely-shared primitives?

Extending dictation is a non-starter (chord activation, no injection, multi-layer capture, schema sharing zero rows with `transcripts`). Extending Meeting Capture is less obviously wrong — both record long-form, both have an overlay UI, both surface in-app rather than via injection — but the activation gesture differs, the capture surface differs (UIA-first vs. audio-first), and the data model differs more than it shares. Stacking activity capture on top of `meetings/runtime.rs` would re-litigate ADR 0028's twin-stream contract (which has no notion of "the third stream is event-driven, not audio") and force conditional logic into every shared file.

The Phase MC precedent (ADR 0026) is therefore the right pattern: **sibling subsystem, shared primitives only, sealed boundary enforced by hook + judge.**

### Locked decisions (from the Bernard ↔ Dustin planning round)

These are inputs to the charter, not open questions, and they map 1:1 to the kickoff prompt's Q-table:

| # | Decision |
|---|---|
| Q1 | **Charter as Phase 10** — numbered PLAN §10 phase, mirrors Phase MC's container (numbered + ADR-chartered + per-wave seal tags + final `phase-10-complete` tag). NOT a lateral epic; this is a top-level subsystem and deserves the heavier vehicle. Phase 9 stays reserved for the macOS cross-platform sweep (PLAN §2.1). |
| Q2 | **Wave order:** Wave 1A Command Center → Wave 1B skeleton → Wave 2 UIA depth → Wave 3 summarization pipeline → Wave 4 audio (Layer 2) → Wave 5 hardening & polish → Wave 6 invariant judges + final seal. **Wave 7 (Layer 3 screenshot + local OCR) is OPTIONAL and explicitly POST-`phase-10-complete`** — sealed via a successor ADR (likely 0039) if/when shipped. (Wave 1A inserted after this ADR's first draft via ADR 0037; the sequencing here is updated to match.) |
| Q3 | **UIA depth:** Wave 1 ships titles-only (foreground app, window title, app-switch + idle events) as the structural skeleton. Wave 2 deepens to **full UIA snapshots**: focused-field text, visible text fragments, control structure, multi-monitor enumeration. |
| Q4 | **Encryption-at-rest decision deferred to a Wave-5 sub-ADR (ADR 0038).** Three candidates pre-named, weighed in 0038: (a) SQLCipher (encrypt whole DB; one decision, one perf bill, simplest UX), (b) DPAPI-per-row on the `activity_events` JSON payload (Windows-native, no new crate, but per-row CryptProtectData call cost), (c) app-layer AES-GCM with key sealed via DPAPI master (most flexible, most code). 0038 picks one; this ADR refuses to pre-litigate it. (Originally reserved as 0037; renumbered after ADR 0037 was taken by the Unified Recording Command Center charter in Wave 0.5.) |
| Q5 | **Default exclusion list (capture-time, NOT display-time):** 1Password, Bitwarden, KeePass, browser windows whose `WindowTitle` matches `(?i)\b(bank|login|password|signin)\b`, the lock screen, and UAC dialogs. **Additionally**: the UIA sampler MUST check `UIA_IsPasswordPropertyId` on the currently focused control on every sample — if true, the entire snapshot for that tick is dropped at capture time (not redacted at display time). This is strictly stronger than Dictation's `SecureInputGuard` (ADR 0017) because UIA's password-property bit works across every Win32/UWP/WinUI/Electron app that exposes accessibility, not just classic Edit controls. Folded into Wave 5. |
| Q6 | **Multi-project tagging deferred to v2 for the surfaced UI**, but migration 012 includes nullable `project_id TEXT` + `project_label TEXT` columns on `activity_sessions` from day 1. Schema-future-proofing only — no IPC, no UI, no settings keys for project tagging in v1. |
| Q7 | **User edits on Blocks are purely cosmetic in v1.** Renaming, merging, splitting, deleting, rewriting a Block updates `activity_blocks` and that's it. NO feedback signal into a learning loop, NO automatic prompt-iteration, NO correction-rate tracking. The learning loop (Phase 8) is dictation-scoped and stays that way. |
| Q8 | **Session-scoped only (Start / Pause / Stop). Always-on mode is permanently out of scope for this phase.** Called out in Non-Goals below. A future "always-on" feature, if ever pursued, is a successor ADR + a separate epic, not a Phase 10 backfill. |
| Q9 | **Windows-only v1**, behind a `#[cfg(target_os = "windows")]` trait per PLAN §2.1 / Principle 5. The cross-platform abstraction (`AccessibilitySnapshot` trait — see Wave 2) is required from day one with macOS / Linux files as `todo!()` stubs. macOS impl deferred to the Phase 9 macOS sweep. |

## Decision

**Activity Capture is built as a sibling subsystem at `src-tauri/src/activity/`, reusing only `audio::AudioCapture`, `meetings::long_form_stt::*`, `cleanup::OllamaProvider`, and the existing migration runner / SQLite repo plumbing. Everything else under `activity/` is greenfield. It is chartered as Phase 10 — a numbered PLAN §10 phase — and sealed via the `phase-10-complete` git tag once Wave 6's judges pass.**

Concretely:

1. **Dictation modules are sealed for Phase 10.** No edits to `hotkey/state.rs`, `hotkey/windows.rs`, `hotkey/driver.rs`, `dictation/*`, `injection/*`, `recording_window.rs`, `cleanup/provider.rs`, `cleanup/llm_cleaner.rs`, or migrations 001–010. The existing `block-cross-module-coupling-meeting-dictation` pre-commit hook (authored in Phase MC Wave 1) is extended in Wave 1 to a generalized `block-cross-module-coupling` that also rejects imports of `dictation::*` from `activity::*` and vice versa.

2. **Meeting Capture modules are sealed for Phase 10.** No edits to `meetings/activation.rs`, `meetings/runtime.rs`, `meetings/capture.rs`, `meetings/loopback_windows.rs`, `meetings/chunker.rs`, `meetings/formatter.rs`, `meetings/persist.rs`, `meetings/llm_pass.rs`, `meetings/overlay.rs`, `meetings/prompts/*`, or migration 011. The exceptions — both **read-only reuse** — are:
   - `meetings::long_form_stt` (the chunked Whisper driver) — Wave 4 calls it as a library; does not modify it.
   - `meetings::export` (the markdown serializer) — Wave 3 may share the shape via composition (re-export, helper trait), not by editing the file. If a shared abstraction is needed, it lives at a new `src-tauri/src/export/` module, not inside `meetings/`.

3. **No new `modes` rows, no new `cleanup_kind` values.** Activity Capture's per-Block abstractor uses markdown-file prompts at `src-tauri/src/activity/prompts/*.md` baked via `include_str!`, exactly as Phase MC / dictation `dictation_run_llm_pass` do. The `modes` table stays a dictation-only contract.

4. **No `CleanupProvider` trait extension.** The Stage-3 abstractor constructs `OllamaProvider::new()` and drives each per-Block request through the existing `CleanupRequest<'_>` struct. If a future Phase 10.N or beyond demands a real multi-prompt batch API or a streaming callback, it gets its own trait (e.g. `BlockAbstractor`), not an extension of the dictation trait. The dictation 383+-test surface stays byte-identical.

5. **No `SpeechToText::transcribe` or `transcribe_segments` signature change.** Wave 4 calls `meetings::long_form_stt::transcribe_chunks` (or its current public entry point — Wave 4 confirms the symbol name) as a library. If Wave 4 discovers it needs a fundamentally different chunking cadence (the activity-capture mic is event-paused per UIA-driven exclusion-list triggers, which `meetings::long_form_stt` does not currently model), the right answer is a thin `activity/audio_orchestrator.rs` that re-drives the chunker, NOT a modification of the shared module.

6. **Sibling state machine, sibling threads, sibling windows, sibling tables.**
   - **State:** `activity/lifecycle.rs` (pure Rust, fully unit-testable) owns the Idle → Active → Paused → Active → Stopped session FSM.
   - **Threads:** a dedicated chord listener thread (proposed `Right Ctrl + ,` — sits next to MC's `Right Ctrl + .` for muscle-memory; conflict probe at startup per ADR 0019); a dedicated UIA sampler thread (event-driven via `IUIAutomation::AddFocusChangedEventHandler` where available, with a coarse-grained poll as backstop); optionally a Layer-2 audio capture thread in Wave 4.
   - **Windows:** a new persistent `recording_indicator` Tauri overlay window that lives whenever ANY layer is live, mirroring `meeting_overlay`'s `focus: false` / `decorations: false` / `alwaysOnTop: true` config but with a different size and content. Same precedent as ADR 0026's "two windows is the right answer until window #3" — and Phase 10 is window #3, so Wave 1 may also consider extracting a shared `WindowConventions` helper, but ONLY if the helper is genuinely cleaner than the boilerplate; YAGNI-bias toward leaving the boilerplate until window #4 if extraction is awkward.
   - **Persistence:** four new tables in migration 012 (`activity_sessions`, `activity_events`, `activity_blocks`, `activity_transcript_segments`). The existing `transcripts`, `sessions`, `meeting_sessions`, `meeting_transcripts`, `meeting_chunks` tables are untouched.

7. **Cargo gate fallback honored.** Per LESSONS PINNED P2 (`cargo test --release` broken on this box), Phase 10's testing strategy is the existing accepted gate:
   - Pure-Rust modules with no whisper-rs / ort / cuda deps (`activity/lifecycle.rs`, `activity/segmenter.rs`, `activity/blocker.rs`, `activity/assembler.rs`, `activity/filler_words.rs` if reused, `activity/exclusion.rs`) → throwaway-crate recipe (LESSONS 2026-05-17): copy module + minimal deps into `$env:TEMP\<modname>_tests\`, run vanilla `cargo test`, merge back when green.
   - Wired modules (`activity/uia.rs`, `activity/sampler.rs`, `activity/audio.rs`, `commands/activity.rs`) → cargo check + clippy + `test --release --no-run` (link-only proof) + the per-wave human-in-loop smoke matrix documented in `docs/phases/phase10.md`. **No new cargo gate is proposed by this ADR.** A parallel investigation bead (see Cross-references) is open against the root-cause `STATUS_ENTRYPOINT_NOT_FOUND` — its resolution would let Phase 10 (and every future phase) run live test exec, but it is NOT blocking.

8. **Wave 6 ships five invariant judges + a live-OS smoke matrix.** Per LESSONS PINNED P7 (Wave-6 judges don't catch live-OS regressions), Phase 10 cannot seal on judges alone. The five judges (`ac-raw-events-immutable`, `ac-no-keystroke-content`, `ac-exclusion-honored-at-capture`, `ac-no-llm-in-critical-path`, `ac-summary-degrades-gracefully`) prove the contract; the 5-minute human smoke matrix in `phase10.md` proves the integration. Both gates must pass before the `phase-10-complete` tag.

## Consequences

### Positive

- **Dictation and Meeting Capture stay sealed.** The `block-cross-module-coupling` hook (generalized from the Phase MC original) statically rejects any import that crosses the activity/dictation or activity/meetings line. The 383+ dictation tests and all Phase MC tests pass byte-identically through every Phase 10 wave. The `ac-raw-events-immutable` judge enforces Principle 1 against the new `activity_events` table the same way ADR 0010 does for `transcripts`.

- **AI is an enhancement, not a dependency.** The staged pipeline (merge → segment → block → abstract → assemble) means that if Ollama is down or slow, the user still gets the raw structured timeline rendered straight from `activity_events` joined with `activity_blocks` (un-abstracted Blocks show the primary app + title + time range, no AI prose). The `ac-no-llm-in-critical-path` and `ac-summary-degrades-gracefully` judges enforce this contract.

- **Provenance is total** (Principle 2). Every Block row references the prompt version used to generate its abstract; the abstractor's prompt files live under git and are loaded via `include_str!` with their SHA recorded on each `activity_blocks` row.

- **Layers are replaceable** (Principle 3). Layer 1 / Layer 2 / Layer 3 are independent capture surfaces behind their own traits (`AccessibilitySnapshot`, `AudioOrchestrator`, future `ScreenSampler`). A future swap from UIA to AT-SPI on Linux, or from local Whisper to a faster STT, touches one trait impl, not the whole subsystem.

- **The third subsystem validates the sibling pattern.** ADR 0026 established sibling-by-default with one second consumer (MC). ADR 0036 makes it three. The pattern is now load-bearing: any future top-level subsystem (e.g. a hypothetical clipboard-history feature, a hypothetical agent-runner feature) inherits the boundary discipline mechanically.

- **Per-row encryption-at-rest decision is properly scoped.** Deferring to ADR 0038 in Wave 5 means the choice is made after Wave 1B's schema is on disk and Wave 2's snapshot-payload sizes are empirically known — the SQLCipher-vs-DPAPI-vs-AES-GCM tradeoff depends on payload size and write frequency, neither of which is precisely known today.

### Negative

- **A fourth message-pump thread.** Dictation's hook thread + Meeting Capture's hook thread + (new) Activity's chord thread + (new) UIA sampler thread = four Windows threads sitting in `GetMessageW` or polling. Cost: sub-microsecond overhead per keystroke per pump, plus the UIA sampler's poll-tick (proposed 1 Hz coarse + event-driven focus changes). Benefit: zero modification to the two sealed driver threads. Same calculus as ADR 0027.

- **Schema growth.** Four new tables, all of which need migration 012, FTS5 search indexing on the Block-abstract text (added in Wave 3), and audit triggers (added in Wave 1 per the migration 002 pattern — `activity_events` is immutable so audit is INSERT-only). The `dictionary` and `modes` tables grow by zero rows.

- **UIA is a large, unfamiliar API surface.** Wave 2 must choose between `windows-rs` raw COM (verbose, exact) and the third-party `uiautomation` crate (ergonomic, audit-required per AGENTS.md "no dependency without checking it works with the cross-platform abstraction"). Wave 2 also has to handle Electron / Chromium apps whose accessibility trees are notoriously incomplete — the `(app, snapshot-quality)` matrix needs documenting as part of the Wave 2 smoke test.

- **Storage management UX surface grows.** Activity sessions, especially with audio + future screenshots, are larger than dictations or meetings. The Settings tab's storage-management UI needs a third row in Wave 5. Retention auto-delete config grows by one row.

- **Three "what is currently being recorded?" indicators** (dictation pip, meeting overlay, new activity overlay). Wave 1 must ensure they don't visually collide — the kickoff proposes the activity overlay sits in a different screen position by default (top-right corner vs. dictation's pip and the meeting overlay's location). Phase 10 Wave 5 may revisit this if user feedback flags it.

### Neutral

- **No new top-level dependencies in Wave 1.** UIA (Wave 2) brings either no-new-dep (raw COM via existing `windows`-rs features) or one new crate (`uiautomation`); Wave 4 brings no new deps (reuses Phase MC's audio + `long_form_stt`); Wave 7 (if ever) brings local OCR (Tesseract bindings or a Rust-native OCR — out of scope here). Each Cargo.toml change is decided in its own wave's brief.

- **Cross-platform stubs from day one** are required (Principle 5). `activity/uia.rs` is `#[cfg(target_os = "windows")]`; `activity/uia_macos.rs` and `activity/uia_linux.rs` ship as `todo!()` stubs in Wave 2. Cost: ~five empty files. Benefit: Phase 9's macOS sweep doesn't have to refactor the abstraction.

- **Documentation density grows.** This ADR + ADR 0037 (Command Center, Wave 1A charter) + ADR 0038 (encryption-at-rest, Wave 5) + (optional) ADR 0039 (Wave 7 screenshot/OCR) + `docs/phases/phase10.md` + (probable) per-wave briefs in `docs/phases/phase10-wave{N}-brief.md` for the deeper waves (W1A Command Center, W2 UIA, W3 summarization pipeline, W4 audio, W5 hardening) follow the Phase MC documentation precedent. This is intentional — Phase MC's per-wave briefs were load-bearing during execution and remain useful as historical reference.

## Sub-ADRs deferred to subsequent waves

- **ADR 0037 — Unified Recording Command Center.** Authored 2026-05-25 in Wave 0.5; charters Wave 1A. Inserted after this ADR's first draft to address the three-overlay UX problem; ADR 0037's §Boundary list is the explicit authorization for the surgical edits Wave 1A makes to sealed Dictation + Meeting Capture surfaces.
- **ADR 0038 — Activity Capture encryption-at-rest strategy.** Authored in Wave 5. Decides between SQLCipher, DPAPI-per-row, or app-layer AES-GCM. References Q4 above. Status: Proposed when Wave 5 starts. (Originally reserved as 0037; renumbered after 0037 was taken by the Command Center charter.)
- **ADR 0039 (optional, post-seal) — Layer 3 screenshot fallback + local OCR.** Authored only if Wave 7 is ever scheduled. References Q2 above. NOT part of `phase-10-complete`. (Originally reserved as 0038; renumbered for the same reason.)

Additional sub-ADRs may be needed mid-execution (e.g. a UIA dependency-choice ADR in Wave 2 if the `uiautomation` crate is chosen, a chord-conflict-fallback ADR if `Right Ctrl + ,` collides with a user-configured shortcut). Author them as discovered; this ADR does not enumerate them upfront.

## Non-Goals (v1)

Explicit, binding. Future asks against these items require a successor ADR, not a Phase 10 backfill.

- **Always-on / 24-7 background capture.** v1 is session-scoped (Start / Pause / Stop) and that's it. The consent surface, the storage cost, the retention policy, and the privacy model for an always-on mode are all materially different products; they do not get retrofitted via a settings toggle.
- **Mobile or browser-extension capture.** Mockingbird is a Windows desktop app; macOS is a planned Phase 9 sweep; nothing else.
- **Cloud sync, multi-device timelines, team dashboards.** No telemetry of any kind (Principle 4). The "this data physically cannot leave your machine" promise is load-bearing for the consent dialog and is enforced by the absence of any outbound HTTP from `activity/` to anywhere except `localhost:11434` (Ollama).
- **Employee monitoring / surveillance use cases.** This is a personal tool the user runs on themselves. The README, the in-app privacy statement (Wave 5), and the first-run consent flow MUST state this plainly. If a future enterprise ask appears, it is a separate product fork, not a Phase 10 extension.
- **Real-time live summarization during a session.** Summarization is a post-stop pipeline. Live summarization changes the resource budget, the model size/latency tradeoff, and the UI model (live-updating timeline view); none are designed for in v1.
- **Pixel-perfect screen replay or video scrubbing.** Even Wave 7 (if scheduled) caps Layer 3 at periodic stills + OCR; continuous video is permanently off the table — it's the exact cost model the local-first design exists to avoid (source plan §3).
- **Inline correction → learning loop.** User edits on Blocks are purely cosmetic (Q7). The Phase 8 learning loop stays dictation-scoped.
- **Project-tagging UI in v1.** Schema columns are present (Q6) but no IPC, no settings, no UI. v2 surfaces this; v1 does not.

## Alternatives considered

- **Extend `meetings/runtime.rs` to handle activity capture as a "no-audio meeting."** Rejected. Activity capture's primary signal is UIA events, not audio; its session duration is open-ended; its data model has no notion of `meeting_chunks` or per-channel transcripts; and the optional Layer-2 audio in Wave 4 is event-paused on UIA-driven exclusion-list triggers, which the meeting capture model does not contemplate. Stacking it on `meetings/runtime.rs` would force every shared file to branch on `kind: Meeting | Activity` and re-baseline the Phase MC test surface. Same calculus as ADR 0026's "extend dictation" rejection.

- **Reuse the `meeting_overlay` Tauri window for the activity recording indicator.** Rejected. The meeting overlay is a control surface (Start / Stop / source pick / duration); the activity indicator is a passive "you are being recorded" pip with maybe a Pause button. Different size, different lifecycle, different interaction model. Two overlays is the right answer; the third one (if a future subsystem appears) might motivate a shared `WindowConventions` helper — but YAGNI here.

- **Make audio (Layer 2) a Wave 1 deliverable.** Rejected (and the kickoff Q2 confirms). The source plan calls Layer 1 alone "a usable feature" and Layer 2 a strict enhancement. Shipping Wave 1 as a UIA-skeleton-only release gives Dustin a working raw-timeline UX in one wave, and the Wave 4 audio integration is then cheaper because the persistence layer + lifecycle FSM already exist and only need a new table + a new toggle.

- **Skip ADR 0038; pick encryption-at-rest now.** Rejected (Q4). The three candidates have materially different perf profiles and the actual `activity_events` payload size is unknown until Wave 2 lands. Premature decision is more expensive than the cost of one ADR.

- **Add this work as a lateral ADR-chartered epic (no `phase-10-complete` tag, just ADR 0036 Accepted + STATUS update).** Rejected (Q1). Activity Capture is multi-wave, multi-session, ≥10 files, ≥1 week of work, introduces four new tables, two new threads, and a brand-new top-level user-visible page. The work-sizing matrix in AGENTS.md is explicit that this is a PLAN §10 phase, not a lateral epic. The `phase-10-complete` tag is reserved for exactly this case.

- **Use a new `CleanupProvider` trait method `cleanup_with_block_context(block_ctx, prompt) -> ...` for the Stage-3 abstractor.** Rejected. Same reasoning as ADR 0026's rejection of the equivalent meeting-LLM-pass trait extension: the existing `OllamaProvider::new()` + `CleanupRequest<'_>` already does what's needed, and adding a trait method makes the contract two-shaped-things-in-one. If/when a third consumer of free-form-prompt LLM also exists AND a real shared abstraction emerges, that's a future ADR.

## Cross-references

- **Source plan:** `mockingbird-activity-capture-plan.md` (repo root, untracked).
- **Phase doc:** `docs/phases/phase10.md` (chartered in this iteration; wave-by-wave brief).
- **PLAN spine:** `PLAN-mockingbird-v2.md` § 10 — adds Phase 10 entry mirroring Phase MC's depth.
- **Companion / future ADRs:**
  - ADR 0037 — Unified Recording Command Center (Wave 1A charter; authored 2026-05-25).
  - ADR 0038 — Activity Capture encryption-at-rest strategy (deferred to Wave 5).
  - ADR 0039 (optional, post-seal) — Layer 3 screenshot fallback + local OCR.
- **Sibling-subsystem precedent:** ADR 0026 (Meeting Capture sibling-subsystem charter). This ADR mirrors 0026's structure deliberately.
- **Sealed primitives reused:**
  - `audio::AudioCapture` (Wave 4 only) — ADR 0013 (cpal ringbuf design).
  - `meetings::long_form_stt` (Wave 4 only) — ADRs 0028, 0029, 0030.
  - `cleanup::OllamaProvider` (Wave 3 abstractor + Wave 5 onward) — ADR 0021 (sync cleanup provider) + the precedent of `dictation_run_llm_pass` (STATUS 2026-05-24) and `meetings/llm_pass.rs` (Phase MC Wave 4).
  - Migration runner + SQLite repo layer — ADR 0004.
- **Sealed surfaces refused for extension:**
  - All of `dictation/`, `hotkey/`, `injection/`, `recording_window.rs`, `cleanup/provider.rs` — sealed at `phase-4-complete` + ADRs 0017, 0018, 0019, 0021.
  - All of `meetings/` except `long_form_stt` (read-only library reuse) — sealed at `phase-mc-complete` + ADRs 0026–0035.
  - `transcripts` table immutability — ADR 0010. `activity_events` inherits the same immutability rule; the `block-immutable-raw-events` SQL trigger in migration 012 mirrors the existing raw-transcripts trigger.
- **Binding principles touched:**
  - Principle 1 (raw immutability) — `activity_events` is raw; trigger enforces.
  - Principle 2 (provenance total) — every `activity_blocks` row carries prompt version + SHA.
  - Principle 3 (layers replaceable) — `AccessibilitySnapshot` / `AudioOrchestrator` traits.
  - Principle 4 (no telemetry) — explicit Non-Goal above.
  - Principle 5 (cross-platform from day one) — `#[cfg(target_os)]` from Wave 1.
  - Principle 6 (no shortcuts) — pure modules go through throwaway-crate (LESSONS P2); wired modules go through smoke matrix (LESSONS P7).
  - Principle 7 (clipboard save/restore) — N/A; activity capture never touches the clipboard.
  - Principle 8 (secure-input fields abort) — strengthened via UIA `UIA_IsPasswordPropertyId` at sample time (Q5); see Wave 5.
- **Cargo gate (binding):** LESSONS PINNED P2 fallback gate. Pure modules → throwaway-crate (LESSONS 2026-05-17). Wired modules → check + clippy + `test --release --no-run` + per-wave human-in-loop smoke matrix. NO new gate proposed by this ADR. The parallel investigation bead (see Beads below) tracks root-cause analysis on the broken `cargo test --release` runner.
- **Judges (final wave):** `docs/judges/phase-10/{ac-raw-events-immutable, ac-no-keystroke-content, ac-exclusion-honored-at-capture, ac-no-llm-in-critical-path, ac-summary-degrades-gracefully}.md`. Authored in Wave 6.
- **Beads:**
  - Parent epic: `Phase 10: Activity Capture (sibling subsystem)` — type `feature`, priority 1. Bead id assigned at create-time.
  - Six wave children (one per Wave 1–6) — each `task`, priority 1, linked so wave N+1 depends on wave N.
  - Independent parallel investigation bead: `cargo test --release runner exits STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139) on this box — DLL load chain investigation` — type `chore`, priority 2. NOT a Phase 10 child; NOT blocking. One-session timebox; close as `wontfix-with-workaround` if unresolved.

---

_The `adr-format` judge validates this structure exists in every numbered ADR. Keep section headings stable._
