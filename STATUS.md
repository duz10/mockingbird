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

**Last consolidated:** 2026-05-25 (ADR 0047 Cleanup pipeline refinement SEALED — 3 waves across 13 commits; lateral epic, no `phase-*-complete` tag). Prior anchor: 2026-05-24 (ADR 0046 Mobile Extension via Vault sealed).

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
| `phase-10-complete`      | Activity Capture sibling subsystem (ADR 0036). Foreground polling + idle tracking + UIA v2 snapshots → LLM block summarization → optional per-block audio transcription → retention sweep + crash recovery + PDF export + capture-time exclusion. Unified Recording Command Center (ADR 0037) as the front door for both Dictation and Meeting Capture. 22 modules under `src-tauri/src/activity/`, migrations 012-015, 6 invariant judges (`docs/judges/phase-10/`) gated the seal. **Live-fire Win11 smoke test pending Dustin — judges don't catch live-OS regressions (LESSONS P7 pattern).** ADR 0038 (encryption-at-rest) RESERVED for v0.2. |

**Lateral epics accepted via ADR** (no new phase tag — see `docs/adr/`):

- ADR 0022 — Three-mode cleanup pipeline (casual/normal/formal)
- ADR 0023 — Design Language v1 (warm-earth Liquid Glass + Fraunces)
- ADR 0024 — Empirical mode-prompt tuning + migration 010
- ADR 0025 — Optional Unsplash ambient background (opt-in BYO-key)
- ADR 0032 — MC v1.1 polish (VU meters, LLM-ephemeral notice, MaxDuration UI)
- ADR 0033 — MC chord-collision hotfix (VK_M → VK_OEM_PERIOD + settings actually-read-at-boot + overlay UI wires)
- ADR 0034 — MC overlay event-delivery hotfix (show-before-emit + `emit_to` re-broadcast + defensive latch clear + emit-state observability; fixes mb-z5y)
- ADR 0035 — MC v1.2 Stable Alpha (Tauri `capabilities/default.json` migration — the *real* root cause of mb-z5y class bugs; `meeting_cancel`; `meeting_rename`; `meeting_overlay_hide`; auto-derived meeting title; WASAPI loopback `build_stream` config-discovery fix; forensic JS-listener-ping beacon scheduled for removal in v1.3 — see `mb-xnn7`)
- ADR 0036 — Activity Capture sibling-subsystem charter (Phase 10 numbered phase; Accepted 2026-05-24)
- ADR 0037 — Unified Recording Command Center (Wave 1A charter + explicit boundary authorization for surgical edits to sealed Dictation + Meeting Capture surfaces; Accepted 2026-05-24)
- ADR 0040 — Activity Capture Wave 3 abstractor pipeline (Accepted, sealed in `phase-10-complete`)
- ADR 0041 — Activity Capture Wave 4 audio layer (Accepted, sealed in `phase-10-complete`)
- ADR 0042 — Activity Capture retention cascade (Accepted, sealed in `phase-10-complete`)
- ADR 0043 — Activity Capture exclusion list + built-in rules (Accepted, sealed in `phase-10-complete`)
- ADR 0044 — Activity Capture PDF export via `printpdf` (Accepted, sealed in `phase-10-complete`)
- ADR 0046 — Mobile extension via synced Obsidian vault (Accepted — sealed 2026-05-24). Four iterations shipped (desktop file ingest → outbound vault projection → inbound mobile courier → polish). User-facing surface: `+ Audio file` desktop import button, deterministic Markdown projection of dictation + meeting history to `<vault>/history/`, inbox courier auto-processing iOS-Shortcut-delivered voice memos from `<vault>/inbox/`, full Mobile Sync settings tab (8 keys + connection-health card), nested-vault detection wizard, import progress overlay, iOS Shortcut recipe (`docs/mobile/ios-shortcut.md`, 3 actions per Wave 0 Finding 5). Channel boundary preserved across 3 reuse sites: `dictation/ingest.rs` (Iter 1 ADR §3.2 amendment) consumed by IPC handler, inbox courier, and import progress overlay event-tap with zero further sealed-surface modifications. Two `sealed-phases-untouched` judges PASS (Iter 1 @ 95%, Iter 3 @ 99%); Iter 2 + Iter 4 didn't need them (greenfield + UI-side). 19 beads closed. Seal commit: HEAD of `main` at consolidation time (this STATUS update was committed in the seal commit itself; see `git log --grep='ADR 0046 SEALED'`). Wave 5 hardening matrix (`mb-qxrm`) remains open as live-corpus catch-up; not gating epic seal. **No new `phase-*-complete` tag** (lateral epic per LESSONS PINNED P5).
- ADR 0047 — Cleanup pipeline refinement (Accepted 2026-05-25). Per-pass system headers in `meetings/llm_pass.rs` (`cleaner_punctuation` no longer carries the global "Be concise" instruction — the load-bearing fix); length-ratio shrink fallback (`SettingKey::LlmShrinkFallbackThreshold`, default 0.65); Whisper `initial_prompt` wired from the user's dictionary at both dictation call sites; temperature standardized to 0.2 across casual / normal / formal / meetings (migration 019); new `DictationCleanupLevel` dial (`None` / `Light` / `Medium` / `High`; default `High` preserves prior behaviour; `Medium` uses the new `normal_v6_additive` prompt); LLM-skip-on-short-utterance (`SettingKey::LlmSkipWordThreshold`, default 12 words; gated on `!looks_listy()`; consumed `mb-cjc` / ADR 0022 Wave 3); casual mode repointed to `qwen2.5:7b-instruct-q4_K_M` (migration 021; one-liners absorbed by the skip path); opt-in Q5_K_M via `SettingKey::PreferQ5Models` with VRAM-gated runtime substitution (migration 022; defaults off); Compress Transform on `LlmPassCard` as on-demand pull-only affordance (`dictation/prompts/compress.md`); `sessions.edit_free_within_5min` instrumentation as the empirical quality signal (surfaced in Insights "Your usage"). UI surface for the dial + Q5 toggle deferred to `mb-h0nn`. Empirically validated by `docs/cleanup/eval-adr0047-cleaner-punctuation.md` (18/20 fixtures preserve all expected phrases on `qwen2.5:3b-instruct-q4_K_M`; zero over-consolidation regressions). Sealed via 13 commits `c7af486..` + this seal commit; **no `phase-*-complete` tag** per LESSONS PINNED P5.
- ADR 0045 — Dictation programmatic start/stop (Accepted 2026-05-27). Amends ADR 0037 §4: the `NoProgrammaticStart` rule is removed for Dictation; the kind now supports two start modes — Right Alt PTT (UNCHANGED) and programmatic via `dictation_start` / `dictation_stop` IPC. Both modes drive the same `HotkeyStateMachine` via a sentinel VK (`0x07`) so the FSM, orchestrator, and `dictation:state` event stream are mode-agnostic. CC Dictation tile now lands on `ShowingSessionCard{Dictation}` (closes the silent-dismiss gap `mb-ytex`). New `<DictationRecordButton>` above the search input on the Dictations page. Shipped as bead `mb-ddfx` (commit `b313742`); no new tag, Phase 10 seal unchanged. **Follow-up beads `mb-tfyp` + `mb-sowc` (2026-05-27):** added `sessions.start_mode` column (migration 017, `'ptt'` / `'in_app'`) so the in-app start path no longer incorrectly produces `ABORTED_FOCUS_CHANGED` session rows. UI list-pill now renders `IN_APP` (neutral) for programmatic sessions; detail panel shows "Push-to-talk" vs "In-app" next to the mode. Recording-pill overlay gains a primary Stop button only when `startMode === 'in_app'` (PTT pill unchanged — zero regression). Plumbed via `dictation:state` event payload (new optional `startMode` field). New `InjectionOutcome::InAppNoInject` variant (db str `"in_app"`) replaces the abort path for in-app sessions — same observable result (no paste), cleaner semantics.
- **Design System v1** — bead-only lateral epic (`mb-n455`, sealed 2026-05-26). Glass-tier semantic tokens (`--surface-glass-strong/soft/faint`), `--glass-blur-cap` (12px), canonical sticky-sidebar scroll convention (single-page scroller + `scrollbar-gutter: stable`), outline-button glass-faint default fill, full `100vh` → `100dvh` sweep, native form-control polish (themed range pill + custom select chevron + dark-pill retention inputs), Activity-page dead-token legacy bridge. 8/8 P1 + 9/12 P2 baseline-audit findings resolved (3 false-positives). 14 modified CSS files; no Rust changes. Baseline + final audits at `docs/audits/2026-05-26-design-v1-{baseline,final}/REPORT.md`. Conventions at `docs/design/conventions.md`. No ADR — work was token + CSS refinement, not architectural.

If a kickoff prompt asks you to re-execute any of the above, **STOP** and surface
the conflict before any tool call. See `.code_puppy/AGENTS.md` § "Permanently sealed".

## 🟢 Currently active

**Knowledge Graph Phase 0 epic — ADR 0048 chartered (Wave 0 complete).**
*Lateral epic per LESSONS PINNED P5; no `phase-*-complete` tag will be cut.*
Wave 0 (charter + scaffold) landed 2026-05-28: spec imported to
`docs/knowledge-graph/spec.md` (immutable), ADR 0048 drafted (Proposed),
10-bead epic with dependency graph (`mb-4wxw` → `mb-0baz`), sandbox crate
at `experimental/kg-validation/` (standalone — its own `[workspace]`, **not**
a member of the root Mockingbird workspace, zero CUDA / whisper-rs / ort
deps so vanilla `cargo test` runs live and sidesteps LESSONS P2), schema
types + serde round-trip tests (4/4 passing on vanilla `cargo test`).
Closed in this iteration: `mb-4wxw`, `mb-w1lw`, `mb-i9l1`.
**Wave 1 (corpus authoring, `mb-t7w5`) is ready for human-in-loop kickoff
next iteration** — 30 dictation + answer-key pairs across 6 personas ×
5 difficulty buckets per spec §6.2 needs Dustin's voice on persona /
difficulty calibration. Q1 / Q2 / Q3 v1 architectural decisions (vault
subtree, positional routing, files-as-source-of-truth) are recorded
verbatim in ADR 0048 for inheritance by the future v1 charter ADR
(provisionally 0049, drafted post-gate).

---

Live-fire Win11 smoke test for Phase 10 is still Dustin's post-seal step
(LESSONS PINNED P7 pattern; same shape as the post-MC `mb-x1x` flow) — judges
don't catch live-OS regressions.

**Standing P3 follow-ups carried over from ADR 0046 (Mobile Extension):**
- `mb-0uqb` — revisit the descoped sidecar-based silent-skip detection if POC users hit silent-skip in practice (see ADR 0046 §9 descoped mechanism).
- `mb-qxrm` — Wave 5 hardening matrix (conflict-file injection, machine-fingerprint mismatch, retention nightly, oversized silent-skip, app-offline catch-up). Picks up when live-corpus surfaces a failure mode worth synthesizing.
- `mb-4j81` — `clippy --release --all-targets` surfaces pre-existing `manual_str_repeat` (`activity/uia/windows_com.rs:378` + `:498`) and `identity_op` (`overlay_conventions.rs:200`) in test code (standard kickoff gate without `--all-targets` misses them). Not an ADR 0046 regression; fix when next in that code.

**Standing P3 follow-ups carried over from Phase 10:**
- `mb-vfyd` — `activity/blocker.rs` is 669 lines, over the 600-line guideline; split candidate.
- `mb-1fqu` — dictation: direct started-from-command-center param path (Wave 1A deferral #2).
- `mb-fzeo` — phase10-deferral2: dictation runtime direct signal path (replace `cc_update_session` UI roundtrip).
- `mb-mxal` — Activity Capture: consider relocating `mockingbird.db` from APPDATA Roaming to LOCALAPPDATA.
- `mb-xnn7` — remove the `meeting_debug_listener_ping` IPC + its TS callers in `Meetings.tsx` / `MeetingOverlay.tsx` before the next MC enhancement epic ships.

**Standing P3 follow-ups carried over from Design System v1 (`mb-n455`):**
- `mb-5856` — SettingsMeetingTab: refactor its inline `<label.toggle><input><span>text</span></label>` markup to the canonical `<Switch>` component from `design/components/`. Visual pill flip is currently absent on those 5 toggles (text flow is fixed; only the green/coral state indicator is missing).
- `mb-km6j` — Consolidate the 3 segmented-control patterns (Settings sub-tab strip / theme picker / sidebar nav) into a single primitive.

**Parallel investigation bead (carryover, INDEPENDENT):** `mb-0n8c` (P2 chore) —
root-cause `cargo test --release` `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)`
on this box. Open since 2026-05-17 (LESSONS PINNED P2). Resolution would
let every future phase run live test exec instead of the `--no-run`
fallback.

**Optional follow-on (post-seal):** ADR 0039 — Layer 3 (screenshot + OCR)
for Activity Capture. Reserved, not chartered; pick up via a new ADR if
the vision-grade signal becomes worth the cost. ADR 0038 (encryption-at-rest)
remains RESERVED until at-rest secrets justify the SQLCipher / DPAPI / AES-GCM
bake-off.

---

### Previous in-flight summary (now sealed) — Phase 10

**Phase 10 — Activity Capture (sibling subsystem). Sealed 2026-05-26 at
`phase-10-complete`.**

Chartered by ADR 0036 (subsystem) + ADR 0037 (Command Center). Numbered
PLAN §10 phase mirroring Phase MC's container: numbered + ADR-chartered
+ per-wave seal commits + final `phase-10-complete` tag. Phase 9 stays
reserved for the macOS cross-platform sweep.

**Final wave ledger:**

| Wave | Bead | Seal commit | Summary |
|---|---|---|---|
| 1A | `mb-jtbk` | `33e2cca` | Unified Recording Command Center (ADR 0037). |
| 1B | `mb-hnl3` | `7333a98` | Activity-Log Skeleton (titles-only); migration 012. |
| 2  | `mb-hr1u` | `9155f40` | UIA deep snapshots + multi-monitor; v2 `snapshot_json`. |
| 3  | `mb-pwup` | `bb77a09` | LLM Block summarization (ADR 0040); migration 013. |
| 4  | `mb-g1w2` | `e3f90db` | Audio Layer 2 — per-Block transcription (ADR 0041); migration 014. |
| 5  | `mb-a6tz` | `1740bdb` | Hardening: exclusion list + retention sweep + crash recovery + PDF export (ADRs 0042/0043/0044); migration 015. |
| 6.A | `mb-8r5p` | `95e57cd` | 6 invariant judges authored + dry-run rig. |
| 6.B | `mb-8r5p` | `f7582d8` + this commit | 12 fixture tests + 2 rig fixes + sealed-phases LLM verdict + **SEAL**. |

**Wiggum loop on Wave 6:** 6/6 judges green on iteration 1 (cap 3).
Mechanical layer via `scripts\dry-run-phase10-judges.ps1`; LLM-grader
verdict for `sealed-phases-untouched` in
`docs/judges/phase-10/sealed-phases-untouched-verdict.md`.

**ADR 0038 (encryption-at-rest):** RESERVED per Dustin's Wave 5 option B.
Not chartered for v0.1; revisit when secrets-at-rest justify the
SQLCipher / DPAPI-per-row / app-layer AES-GCM bake-off.

**Live-fire Win11 smoke test:** Dustin's post-seal step (LESSONS P7
pattern). Judges proved invariants; they do not prove a clean OS
bring-up of a recording session.

---

### Previous in-flight summary (Phase MC + Stable Alpha)

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
- `mb-nc9u` — `mode_eval` grid re-run for migration 019 (normal/formal temperature 0.1 → 0.2). Owned by Dustin; picks up when fixtures are ready.
- `mb-e2t8` — ADR 0047 Wave 2.4 follow-up: expose `cleanup::vram_probe::probe_vram_mib()` as a Tauri command. UI consumer (Settings → Dictation tab Q5 toggle readout) is already in place; ships the "VRAM probe unavailable" placeholder until this Rust command lands. Single-command dispatch — code-puppy / Rust-side scope.

**Recently closed standing P2s:**
- `mb-h0nn` — ADR 0047 Wave 2C UI: SHIPPED 2026-05-25 in commit `efe08ed`. Promoted the slim "Dictation data" tab into a full "Dictation" tab mirroring SettingsMeetingTab's shape (Cleanup behaviour / Activation / Per-mode tuning / Data retention). DictationCleanupLevel dial + PreferQ5Models toggle live via the typed-settings registry (same `legacy_get_setting` / `legacy_set_setting` pattern SettingsMeetingTab uses). VRAM probe display deferred to mb-e2t8.

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
