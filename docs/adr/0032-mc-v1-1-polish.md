# ADR-0032: Phase MC v1.1 — post-seal polish (live audio levels, LLM-ephemerality UX, settings completeness, filler-set tuning)

- **Status:** Accepted
- **Date:** 2026-05-23
- **Accepted:** 2026-05-23 (single-iteration ship, all 5 MC judges preserved by construction)
- **Deciders:** Dustin (project lead), code-puppy/Bernard (implementor)

## Context

Phase MC (Meeting Capture) was sealed on 2026-05-22 at git tag
`phase-mc-complete`. A post-seal audit (same day, conducted by Bernard
under session `code-puppy-b14c19`) surfaced **four small gaps** between
the Phase MC plan (`docs/phases/phase-meeting-capture.md`) and the
shipped implementation. None of the gaps invalidate the seal — all five
Wave-6 judges (`mc-formatter-deterministic`,
`mc-long-form-stitched-losslessly`, `mc-two-channel-merged`,
`mc-no-llm-in-critical-path`, `mc-dictation-untouched`) remain green —
but each gap is a real user-visible quality deferral that should not
linger.

The four gaps:

1. **`meeting:tick` event + live VU meters missing.** PLAN §MC.6 spec'd
   a `meeting:tick { elapsed_ms, mic_db, sys_db }` event emitted from
   the capture loop so the overlay could render a live audio-level
   indicator during recording. The UI fakes `elapsed_ms` with a
   client-side `setInterval`, but `mic_db` / `sys_db` are not computed
   or emitted anywhere. Result: during a meeting, the user has zero
   live feedback that their mic is hot or that loopback is capturing —
   silent failure mode is "discover after stopping that nothing was
   recorded". Tracked as `bd: mb-nig` (P2 bug).

2. **LLM-pass ephemeral-output warning missing.** Risk 7 mitigation in
   the phase plan said the `MeetingDetail` LLM panel should surface a
   one-time notice (per-session, dismissable) the first time the user
   sees LLM output: "this output isn't saved; export it now if you want
   to keep it." The output **is** ephemeral by design
   (`MeetingCaptureRuntime::llm_pass_cache: HashMap<String, String>`,
   evicted on shutdown) per the no-LLM-in-critical-path invariant.
   Without the notice, the user runs a Summary, closes the app, reopens
   tomorrow, and finds nothing. Tracked as `bd: mb-rm7` (P3 bug).

3. **`MeetingMaxDurationSeconds` not reachable from Settings UI.** The
   setting is persisted, clamped server-side `[60, 21600]` (1 min to 6
   hr, default 4 hr), and the runtime enforces the cap — but
   `SettingsMeetingTab.tsx` doesn't expose it. Power users who want to
   shorten the cap as a self-imposed discipline (or lengthen it for an
   all-hands) have to poke raw IPC. Tracked as `bd: mb-mom` (P3 task).

4. **`"basically"` missing from default `FILLERS` set.** The phase plan
   names `"basically"` in its example filler list alongside
   `um/uh/like/you know/I mean/sort of/kind of`. All the others are in
   `meetings/filler_words.rs::FILLERS`; `"basically"` was missed.
   Tracked as `bd: mb-tn5` (P3 task).

These are the kind of gaps that are easy to find post-seal precisely
because the seal exists — the judges + the diff scan turn into a
forcing function. Without a structured way to absorb them, they rot in
backlog forever, drift further from the original implementor's context,
and eventually become tribal-knowledge folklore.

The AGENTS.md "Permanently sealed" rules are explicit about the path:

> If [a prompt] asks you to *add new work to a sealed phase*, that's a
> new ADR-chartered lateral epic — handle it like ADR 0022/0023
> (charter ADR → bd epic → wave briefs → seal via STATUS + ADR
> acceptance, NOT by re-tagging the phase).

This ADR is that charter.

## Decision

We adopt **Phase MC v1.1** as a small lateral epic that absorbs the
four polish gaps in a single wave. Specifically:

1. **Single-wave execution.** The four gaps are independent, total
   estimated effort is ~3–4 hours, and none requires architectural
   thinking the original MC waves didn't already do. A single wave
   keeps overhead proportional to scope (DRY/YAGNI applied to process
   itself — a six-wave breakdown for ~400 LoC would be silly).

2. **No new tag.** The seal tag `phase-mc-complete` is **not** moved
   and **no** `phase-mc-v1.1-complete` tag is created. Sealing happens
   via (a) ADR 0032 flipping from Proposed → Accepted, (b) the STATUS
   anchor block noting the epic complete with the closing commit hash,
   and (c) the bd epic + child issues all closed. This matches the
   ADR 0023 / ADR 0025 precedent for lateral polish epics. Git tags
   are reserved for PLAN §10 phase boundaries.

3. **Gap-by-gap implementation contract:**

   - **mb-nig (live audio levels + tick event):**
     - New module `meetings/levels.rs` with a `compute_dbfs(samples:
       &[i16]) -> f32` pure function (peak-amplitude-to-dBFS, clamped
       at `-100.0` floor, returns `0.0` for `samples.is_empty()`) plus
       a `LevelsState` thread-safe holder (`Arc<Mutex<(f32, f32)>>`
       for `(mic_db, sys_db)`).
     - `TwinStreamCapture` gains a `levels: Arc<LevelsState>` field
       and a `current_levels() -> (f32, f32)` accessor; the
       per-channel `owner_thread_loop` updates the holder each time
       it drains samples (additive — no signature change to existing
       fns; `start_with` gains no new args because the levels handle
       lives inside `TwinStreamCapture`).
     - `lifecycle.rs::start_meeting` spawns a third lightweight
       thread (`mockingbird-meeting-tick-<uuid>`) that, every 250ms,
       reads `(mic_db, sys_db)` + elapsed-since-`started_at_instant`
       and emits `meeting:tick { uuid, elapsedMs, micDb, sysDb }`
       via the `AppHandle::emit` clone. Thread exits when its
       per-spawn `Arc<AtomicBool>` `running` flag is cleared by
       `finalize_meeting`.
     - UI: `MeetingOverlay.tsx` subscribes to `meeting:tick`,
       renders two thin VU bars in the pill (mic + sys). Tailwind
       v4 + design-tokens-only, no new dependencies.
     - **No critical-path LLM contamination.** The tick thread is
       pure I/O — read shared state, emit. Zero Ollama / cleanup
       construction.

   - **mb-rm7 (ephemeral-LLM notice):**
     - Pure UI change in `MeetingDetail.tsx::LlmPassPanel`.
     - localStorage key `mockingbird.meetings.llmEphemeralAck` (bool).
     - First time `state.result` becomes truthy AND the key is unset,
       render an inline `<aside role="note">` with the warning text +
       a "Got it" button that sets the key.
     - i18n strings in `ui/src/i18n/en.json` under
       `meetings.llm.ephemeralNotice.{body,dismiss}`.

   - **mb-mom (`MeetingMaxDurationSeconds` Settings UI):**
     - New `<NumberField>` (or native `<input type="number">` wrapped
       in the existing Settings row primitive) in
       `SettingsMeetingTab.tsx` under the Recording section.
     - Clamp helper `clampMaxDuration(input: number): number` in
       `ui/src/lib/meetings.ts` that floors at 60, ceils at 21600.
     - i18n strings under `settings.meeting.maxDuration.*`.
     - The `meeting_settings_set` IPC allowlist already permits
       `MeetingMaxDurationSeconds` (verified during audit); no
       backend change required.

   - **mb-tn5 (`"basically"` filler):**
     - One-line addition to `phf_set! { … }` in
       `meetings/filler_words.rs::FILLERS`.
     - New test in the same `mod tests`:
       `basically_is_a_filler`.
     - No other filler tuning in this epic; if future tuning becomes
       a regular thing it earns its own ADR.

4. **Test density.** ~10 tests for the bundle:
   - `meetings/levels.rs`: 4 unit tests (silence → -100 dBFS,
     full-scale i16 → 0 dBFS, mixed amplitude monotone, empty input
     returns 0.0).
   - `meetings/capture.rs`: 1 additive test on `current_levels()`
     post-feed via the existing `StubCapture` seam.
   - `meetings/lifecycle.rs`: 1 #[ignore]'d integration smoke (the
     tick thread shuts down on `finalize_meeting`).
   - `ui` vitest: 2 tests (clamp lower/upper for
     `clampMaxDuration`).
   - `meetings/filler_words.rs`: 1 test (`basically_is_a_filler`).
   - LlmEphemeralNotice: 1 vitest (render + dismiss persists).

5. **Cargo + UI gate at seal:**
   - `cargo check --all-targets` clean
   - `cargo clippy --release --all-targets -- -D warnings` clean
   - `cargo fmt --check` clean
   - `cargo test --release --no-run` clean (run-time fallback per
     LESSONS 2026-05-17 `0xC0000139`)
   - `tsc --noEmit` clean
   - `vitest run` clean (+3 tests, project total ~48)

6. **Judge invariants preserved:**
   - `mc-dictation-untouched`: empty diff vs `phase-mc-start` across
     the binding list. None of the four fixes touch any sealed
     dictation/hotkey/injection/migration file. ✓ by construction.
   - `mc-no-llm-in-critical-path`: lifecycle.rs additions are tick
     emission only — no `OllamaProvider` or `LlmCleaner`
     construction. ✓
   - `mc-formatter-deterministic`: `FILLERS` change is additive +
     compile-time `phf_set!` (no `Lazy<HashSet>` regression). The
     `okay_is_not_a_filler` test still passes. ✓
   - `mc-long-form-stitched-losslessly`: no changes to `long_form_stt`. ✓
   - `mc-two-channel-merged`: no changes to `merge.rs` or
     `SpeakerLabels`. ✓

## Consequences

### Positive

- **The seal stays trustworthy.** Sealed phases that quietly grow a
  pile of "small things we didn't do" lose their seal's meaning. The
  ADR + epic vehicle turns those gaps into tracked, time-boxed work.
- **Live VU meters close a real silent-failure mode.** A user whose
  microphone is muted by Windows after a Teams call won't know
  unless they see audio levels; this fix closes that footgun.
- **Process precedent matures.** This is the second post-phase lateral
  polish epic (ADR 0023 was the first — Design Language v1 ran
  similarly post-Phase-3). The two together establish "ADR + epic +
  no new tag" as the durable pattern.

### Negative

- **Three threads per meeting** (mic-owner, sys-owner, tick-emitter).
  The tick thread is ~zero CPU (sleep 250ms, read mutex, emit Tauri
  event). Acceptable; documented in the ADR 0028 cross-reference.
- **`phf::Set` regeneration cost on filler add.** Negligible
  (compile-time), called out for completeness.
- **`meeting:tick` introduces a new IPC event channel.** Adds one
  more UI subscription path. The wins (live VU) dwarf the surface
  area cost; the same overlay window already subscribes to
  `meeting:state`, `meeting:overlay-open`, and `meeting:progress` —
  one more is structural noise, not architectural risk.

### Neutral

- **STATUS anchor block grows by ~one block.** Same shape as the ADR
  0023/0025 LATERAL EPICS DONE precedent.
- **The "what if a fifth gap shows up later" question.** Convention:
  one ADR per polish epic. Don't keep amending 0032; mint 0033 if a
  v1.2 batch shows up. Keeps ADR cross-references stable.

## Alternatives considered

- **Leave the gaps in bd and never charter.** Rejected: the
  AGENTS.md "Permanently sealed" rules explicitly require the
  ADR-charter path, and the LESSONS history (Phase 3, Phase 5
  post-ship) shows untriaged backlog rotting after ~2 weeks.
- **Move `phase-mc-complete` tag.** Rejected: violates the AGENTS
  rule that phase tags are PLAN §10 boundaries only. Tags are git's
  hardest-to-undo primitive; preserving the original seal point
  matters for the `mc-dictation-untouched` judge's diff reference.
- **Mint `phase-mc-v1.1-complete`.** Rejected: no PLAN section
  defines it, and the lateral-epic precedent (ADR 0023 → mb-36q;
  ADR 0025 → mb-biy) uses no version tag — closure is via STATUS
  anchor + bd epic close + ADR Accepted.
- **Split into per-gap micro-epics.** Rejected: four ADRs +
  four briefs for ~400 LoC is process bloat. The four gaps share an
  audit origin, a single judge-preservation argument, and a single
  seal moment. One ADR / one brief / one commit is the right grain.
- **Defer mb-nig to a separate streaming-UI epic.** Tempting because
  VU meters touch the same overlay as future waveform views, but the
  tick emitter is a 30-line addition and waiting risks the silent-
  failure mode biting Dustin in a real meeting first. Ship now;
  the future waveform epic can reuse `compute_dbfs` and the tick
  channel without rework.

## Cross-references

- **PLAN sections:** §MC.6 (Tauri commands + events — defined
  `meeting:tick` originally), §MC.7 (Risk 7 — LLM ephemerality).
- **Related ADRs:**
  - **ADR 0026** (Meeting sibling subsystem): preserved. This epic
    extends MC-owned files only.
  - **ADR 0027** (Chord activation): untouched.
  - **ADR 0028** (Twin-stream capture): extended additively — new
    `current_levels()` accessor, no signature breakage.
  - **ADR 0029** (Long-form chunked Whisper): untouched.
  - **ADR 0030** (Whisper segment exposure): untouched.
  - **ADR 0031** (Meeting loopback backend): untouched.
- **bd issues:**
  - **mb-{epic}** (epic, to be created): Phase MC v1.1 polish.
  - **mb-nig** (P2, open): tick event + VU meters.
  - **mb-rm7** (P3, open): LLM ephemerality notice.
  - **mb-mom** (P3, open): MaxDuration Settings UI.
  - **mb-tn5** (P3, open): "basically" filler.
- **LESSONS:**
  - **2026-05-17** `0xC0000139` STATUS_ENTRYPOINT_NOT_FOUND DLL-load
    wart — gate fallback applies as for Phase MC proper.
  - **2026-05-22** (to be appended at seal): "post-seal audit
    surfaced four gaps; lateral-epic vehicle worked smoothly; tick
    emitter is the right shape for future waveform features."
- **Artifacts:**
  - `docs/phases/phase-mc-v1-1-brief.md` — the wave brief.
  - `src-tauri/src/meetings/levels.rs` — new module.
  - Diff scope: ~400 LoC across 6 files.

---

_This ADR is **superseded** if a future polish epic changes the
"no new tag, ADR-only seal" convention itself. Adding more polish
items in a v1.2 epic does NOT supersede 0032 — that's a new ADR
(0033 or later) with its own bd epic._
