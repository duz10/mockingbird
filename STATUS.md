<!-- ════════════════════════════════════════════════════════════════════
     SESSION ANCHOR — read this block BEFORE any tool call.
     This file is intentionally small. For:
       • What the app currently does, by subsystem  →  docs/PRODUCT-STATE.md
       • Why we made each big call                   →  docs/adr/
       • Non-obvious findings from past sessions     →  docs/LESSONS.md
                                                       (PINNED block at top)
       • Old session-by-session diary                →  docs/archive/STATUS-2026-05-23.md
     ════════════════════════════════════════════════════════════════════ -->

# Mockingbird — STATUS

**Last consolidated:** 2026-05-24 (MC v1.2 Stable Alpha seal — ADR 0035 + git tag `stable-alpha-v0.1`).

## ✅ Sealed (do not re-execute)

| Tag | What it sealed |
|---|---|
| `bootstrap-complete`     | PLAN §0.5 — AGENTS.md, hooks, JSON agents, judges template, skills. |
| `phase-0-complete`       | Rust + Tauri 2 scaffold, hook engine, CI sanity. |
| `phase-1-complete`       | SQLite schema (migrations 001-003), repo layer, settings store. |
| `phase-2-complete`       | Audio capture (cpal) + Whisper-rs CUDA + Silero VAD via ORT. |
| `phase-3-complete`       | Hotkey hook + dictation pipeline + SecureInputGuard + clipboard injection. |
| `phase-4-complete`       | Cleanup provider trait + Ollama provider + prompt loader. |
| `phase-8-complete`       | UI sprint — all six pages + recording overlay (App/Insights/History/Dictionary/Modes/Settings + recording window). |
| `phase-mc-start` (anchor)| Stable diff reference for `mc-dictation-untouched` judge. |
| `phase-mc-complete`      | Meeting Capture subsystem: chord activation, twin-stream capture, long-form chunked Whisper, deterministic formatter, two-channel merge, ephemeral LLM pass, overlay UI, Meetings/MeetingDetail pages, 5 invariant judges. |
| `stable-alpha-v0.1`      | First user-visible stable build. MC subsystem at UX parity with PLAN (start/pause/stop/cancel/rename/auto-title/search/delete/export). Insights two-tab redesign. On-demand LLM pass on dictations. Reference checkpoint for distinguishing pre-/post-enhancement work in future sessions. |

**Lateral epics accepted via ADR** (no new phase tag — see `docs/adr/`):

- ADR 0022 — Three-mode cleanup pipeline (casual/normal/formal)
- ADR 0023 — Design Language v1 (warm-earth Liquid Glass + Fraunces)
- ADR 0024 — Empirical mode-prompt tuning + migration 010
- ADR 0025 — Optional Unsplash ambient background (opt-in BYO-key)
- ADR 0032 — MC v1.1 polish (VU meters, LLM-ephemeral notice, MaxDuration UI)
- ADR 0033 — MC chord-collision hotfix (VK_M → VK_OEM_PERIOD + settings actually-read-at-boot + overlay UI wires)
- ADR 0034 — MC overlay event-delivery hotfix (show-before-emit + `emit_to` re-broadcast + defensive latch clear + emit-state observability; fixes mb-z5y)
- ADR 0035 — MC v1.2 Stable Alpha (Tauri `capabilities/default.json` migration — the *real* root cause of mb-z5y class bugs; `meeting_cancel`; `meeting_rename`; `meeting_overlay_hide`; auto-derived meeting title; WASAPI loopback `build_stream` config-discovery fix; forensic JS-listener-ping beacon scheduled for removal in v1.3 — see `mb-xnn7`)

If a kickoff prompt asks you to re-execute any of the above, **STOP** and surface
the conflict before any tool call. See `.code_puppy/AGENTS.md` § "Permanently sealed".

## 🟢 Currently active

**Phase 10 — Activity Capture (sibling subsystem). Wave 0.5 — Command Center charter integration shipped; awaiting Dustin review of ADR 0036 + ADR 0037 before Wave 1A code.**

Chartered 2026-05-25 (Bernard, Wave 0). Wave 0.5 (this iteration) — Command Center charter integration — adds ADR 0037 + Wave 1A in response to the three-overlay UX flag Bernard raised in the Wave 0 summary; ADR 0037 is the explicit authorization for the surgical edits Wave 1A will make to sealed Dictation + Meeting Capture surfaces. Numbered PLAN §10 phase mirroring Phase MC's container: numbered + ADR-chartered + per-wave seal tags + final `phase-10-complete` tag. Phase 9 stays reserved for the macOS cross-platform sweep.

**Decision matrix (Wave 0.5):** Chord = `Right Ctrl + Space` (user-configurable, ADR 0019 probe); mutual-exclusion-while-recording = open Command Center showing SessionCard + Stop button, returns to mode picker after Stop; tray entry = yes ("Open Command Center"); legacy `Right Ctrl + .` meeting chord = user setting `legacy_meeting_chord_enabled` (default OFF; one-shot migration sets ON for existing users with the prior chord configured, mirrors ADR 0033); first-run = auto-open with Welcome header band, tracked via `command_center_seen_v1`.

- **Charter ADRs:**
  - [ADR 0036](docs/adr/0036-activity-capture-sibling-subsystem.md) — Activity Capture sibling-subsystem. **Status: Proposed** (Dustin flips to Accepted before Wave 1A code).
  - [ADR 0037](docs/adr/0037-unified-recording-command-center.md) — Unified Recording Command Center (Wave 1A charter + explicit boundary authorization for surgical edits to sealed Dictation + Meeting Capture surfaces). **Status: Proposed** (Dustin flips to Accepted before Wave 1A code).
- **Phase doc:** [`docs/phases/phase10.md`](docs/phases/phase10.md) — seven-wave brief (1A Command Center → 1B skeleton → 2 UIA depth → 3 summarization → 4 audio → 5 hardening → 6 judges + seal). Wave 7 (Layer 3 screenshot + OCR) is OPTIONAL post-seal via successor ADR 0039.
- **Source plan:** [`mockingbird-activity-capture-plan.md`](mockingbird-activity-capture-plan.md) — vision doc; the implementation charter against it lives in ADRs 0036 + 0037.
- **PLAN §10 amendment:** Phase 10 entry in `PLAN-mockingbird-v2.md` updated to include Wave 1A as the first wave, with Wave 1 re-lettered to 1B.
- **Sub-ADRs deferred:** ADR 0038 (encryption-at-rest — Wave 5; SQLCipher / DPAPI-per-row / app-layer AES-GCM candidates pre-named; renumbered from 0037 after the Command Center charter took 0037). ADR 0039 (optional, post-seal — Layer 3 screenshot + OCR; renumbered from 0038 for the same reason).
- **Bead epic:** `mb-a2w9` (status `in_progress`).
- **Wave beads** (each P1, dependency-chained so each subsequent wave depends on its predecessor; the epic depends on all of them):
  - `mb-jtbk` **Wave 1A: Unified Recording Command Center** ← unblocked, awaits ADR 0037 Acceptance (Wave 0.5 iteration)
  - `mb-hnl3` Wave 1B: Activity-Log Skeleton (titles-only) ← blocked-by `mb-jtbk`
  - `mb-hr1u` Wave 2: UIA deep snapshots + multi-monitor
  - `mb-pwup` Wave 3: Summarization pipeline
  - `mb-g1w2` Wave 4: Audio layer (Layer 2)
  - `mb-a6tz` Wave 5: Hardening and polish (charters ADR 0038, encryption-at-rest)
  - `mb-8r5p` Wave 6: Invariant judges + final seal
- **Parallel investigation bead (INDEPENDENT — not blocking Phase 10):** `mb-0n8c` (P2 chore) — root-cause `cargo test --release` `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)` on this box. Open since 2026-05-17 (LESSONS PINNED P2). 1-session timebox; falls back to wontfix-with-workaround if unresolved. Resolution would let every future phase run live test exec.

**Standing P3 follow-up:** `mb-xnn7` — remove the `meeting_debug_listener_ping`
IPC + its TS callers in `Meetings.tsx` / `MeetingOverlay.tsx` before the next
MC enhancement epic ships. The beacon was added in v1.2 as forensic evidence
for JS-listener firing during the mb-z5y class of bug; it's not load-bearing
and the noise should not survive the next iteration.

---

### Previous in-flight summary (now sealed)

**Dictation polish — shipped 2026-05-24** (commit `dda676a`). Four-in-one
lateral cleanup session:

1. **Paste payload sanitization** — `dictation/paste_payload.rs` strips a
   single trailing space from the LLM-cleaned text before clipboard handoff
   (deterministic; doesn't rely on prompt-engineering the model to omit
   trailing whitespace). 11 unit tests. Wired into `dictation.rs::complete()`.
2. **History → Dictations rename** — Git-detected rename of
   `History.{tsx,module.css}` to `Dictations.{tsx,module.css}`,
   `/history` redirect kept for in-flight bookmarks, full i18n key sweep
   (`history.*` → `dictations.*`), Sidebar nav updated.
3. **On-demand LLM pass on a saved dictation** — new
   `dictation_run_llm_pass` IPC; takes built-in prompt id
   (`summary` / `action_items` / `cleaner_punctuation`) OR custom text;
   constructs an `OllamaProvider` via its existing arg-less `new()` and
   drives via `CleanupRequest<'_>` (does NOT extend the `CleanupProvider`
   trait — same constraint as MC). Prompts live as markdown in
   `src-tauri/src/dictation/prompts/*.md`, baked via `include_str!`.
   Defensive fence-stripping postprocess for small models that wrap output
   in ```` ```markdown ... ``` ````. Collapsible card under each session in
   `Dictations.tsx` with prompt picker + custom textarea + Prism-highlighted
   markdown render.
4. **Insights two-tab redesign** — "Your usage" (lifetime tiles, 365-day
   GitHub-style heatmap, 7-day spark, mode mix, top apps, today snapshot) vs.
   "Your voice" (WPM, peak-hours histogram, top dictionary terms,
   top-corrected words, latency, learning loop). 7 new additive backend
   aggregations in `commands/insights.rs` (no existing field touched);
   heatmap intensity uses `oklch(from var(--mode-normal) l c h / N)` so
   theme swaps inherit; WPM excludes <5s sessions and caps outliers at
   300 wpm. Lifetime totals tolerate pre-migration-011 DBs (treats missing
   `meeting_sessions` table as zeroes).

Gate: cargo check / clippy / fmt clean on touched files; `tsc --noEmit`
clean; vitest 55/55 pass; release binary rebuilt; live-exec verification on
Dustin.

**Pre-existing dirty state NOT touched this session** — there's an in-flight
epic in the tree from a prior session:
`mockingbird-activity-capture-plan.md` (untracked), `meetings/title.rs`
(untracked, ~310 lines), `src-tauri/capabilities/` (untracked dir), plus
modifications to `audio/capture.rs` (+32), `commands/meetings.rs` (+83),
`meetings/{lifecycle,mod,overlay,repo,runtime}.rs`,
`MeetingDetail.tsx` (+198), `Meetings.tsx` (+47), `meetings.ts`, `Icon.tsx`,
`MeetingOverlay.tsx`, `Meetings.module.css`. Looks like a meeting-activity-
capture feature in mid-flight. **Action item:** triage with Dustin next
session — read the plan file, decide if it ships as-is or needs more work,
then decide on commit vs revert. Not a bug — just unfinished work parked in
the tree.

**Standing P1:** `mb-ez9` — empirical mode-prompt iteration across casual/normal/formal
(in_progress; long-running quality improvement loop, picks up whenever Dustin has
fixture additions to feed the mode_eval rig).

**Standing P2s:**
- `mb-xwi` — Phases 5/6/7 main-phase work from PLAN §10 (Recording UX polish,
  History/Settings/About windows, code signing). The long pole.
- `mb-cjc` — ADR 0022 Wave 3 (LLM-skip for short casual utterances; ~300ms direct-paste path).

**P3 backlog:** see `bd ready` — 6 issues (tray deep-link, Settings.tsx split,
DPAPI for Unsplash key, Unsplash glyph review, ESLint v9 migration, hide-disabled-AI-modes toggle).

## ▶ How to resume

1. **Read this file** (you are here). 30 seconds.
2. **Read `docs/PRODUCT-STATE.md`** — the durable "what does the app actually do today?"
   reference. Replaces 1000+ lines of old session diary. 2-3 min skim.
3. **Read `docs/LESSONS.md` PINNED block** (top of file). The load-bearing
   gotchas (cargo wrapper, test-binary launch bug, stale-prompt incident). 1 min.
4. **Read `.code_puppy/AGENTS.md`** — rules, principles, never-do list. 2 min.
5. **For active phase work:** read `docs/phases/phase{N}.md` and any wave briefs.
   For one-off epics: read the chartering ADR (`docs/adr/`).
6. `bd ready` — what's unblocked.
7. `git log --oneline -20` and `git status` — what shipped recently / dirty tree.
8. **Then start work.** If the kickoff prompt conflicts with sealed-phase state,
   STOP and ask before tool calls.

## 📐 What goes where

| Doc | Update cadence | Purpose |
|---|---|---|
| `STATUS.md` (this file) | End of every iteration | Anchor: what's sealed, what's in-flight, how to resume. Stays slim. |
| `docs/PRODUCT-STATE.md` | When a subsystem ships or materially changes | "Current state of the product" reference. Stable. |
| `docs/LESSONS.md`      | When non-obvious finding emerges | Append-only journal. TOC + PINNED at top, body chronological. |
| `docs/adr/####-*.md`    | Per architectural decision | Immutable once Accepted (supersede via new ADR). |
| `docs/archive/`         | Read-only             | Old STATUS diaries, deprecated docs. |
| `bd` issue DB           | Per task              | Live work queue + dependency graph. |
| `git tag phase-N-complete` | Per phase seal     | Immutable boundary marker. |

## 🛠 End-of-iteration checklist

(Hooks enforce most of this; see `.code_puppy/AGENTS.md` § "At the end of every iteration".)

1. **Update STATUS.md** — flip Currently-active block if epic state changed; otherwise leave alone.
2. **Update PRODUCT-STATE.md** only if a subsystem shipped or materially changed.
3. **Close/create beads** (`bd close <id>` / `bd create ...`).
4. **Cargo gate** (all via `scripts\cargo-with-cuda.ps1`):
   - `fmt --check`
   - `clippy --release -- -D warnings`
   - `test --release --no-run` (live exec blocked on this box — see LESSONS PINNED)
   - `build --release` (when shipping)
5. **UI gate:** `npx tsc --noEmit`, `npm test`, `npm run build`.
6. **LESSONS append** if a non-obvious thing happened.
7. **Commit with a descriptive message** referencing the bead id + ADR if any.
8. **No new phase tag** unless completing a numbered PLAN §10 phase (lateral epics seal via ADR, not tag).
