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

**Last consolidated:** 2026-05-26 (Phase 10 Activity Capture sealed — git tag `phase-10-complete`).

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
- ADR 0045 — Dictation programmatic start/stop (Accepted 2026-05-27). Amends ADR 0037 §4: the `NoProgrammaticStart` rule is removed for Dictation; the kind now supports two start modes — Right Alt PTT (UNCHANGED) and programmatic via `dictation_start` / `dictation_stop` IPC. Both modes drive the same `HotkeyStateMachine` via a sentinel VK (`0x07`) so the FSM, orchestrator, and `dictation:state` event stream are mode-agnostic. CC Dictation tile now lands on `ShowingSessionCard{Dictation}` (closes the silent-dismiss gap `mb-ytex`). New `<DictationRecordButton>` above the search input on the Dictations page. Shipped as bead `mb-ddfx` (commit `b313742`); no new tag, Phase 10 seal unchanged. **Follow-up beads `mb-tfyp` + `mb-sowc` (2026-05-27):** added `sessions.start_mode` column (migration 017, `'ptt'` / `'in_app'`) so the in-app start path no longer incorrectly produces `ABORTED_FOCUS_CHANGED` session rows. UI list-pill now renders `IN_APP` (neutral) for programmatic sessions; detail panel shows "Push-to-talk" vs "In-app" next to the mode. Recording-pill overlay gains a primary Stop button only when `startMode === 'in_app'` (PTT pill unchanged — zero regression). Plumbed via `dictation:state` event payload (new optional `startMode` field). New `InjectionOutcome::InAppNoInject` variant (db str `"in_app"`) replaces the abort path for in-app sessions — same observable result (no paste), cleaner semantics.
- **Design System v1** — bead-only lateral epic (`mb-n455`, sealed 2026-05-26). Glass-tier semantic tokens (`--surface-glass-strong/soft/faint`), `--glass-blur-cap` (12px), canonical sticky-sidebar scroll convention (single-page scroller + `scrollbar-gutter: stable`), outline-button glass-faint default fill, full `100vh` → `100dvh` sweep, native form-control polish (themed range pill + custom select chevron + dark-pill retention inputs), Activity-page dead-token legacy bridge. 8/8 P1 + 9/12 P2 baseline-audit findings resolved (3 false-positives). 14 modified CSS files; no Rust changes. Baseline + final audits at `docs/audits/2026-05-26-design-v1-{baseline,final}/REPORT.md`. Conventions at `docs/design/conventions.md`. No ADR — work was token + CSS refinement, not architectural.

If a kickoff prompt asks you to re-execute any of the above, **STOP** and surface
the conflict before any tool call. See `.code_puppy/AGENTS.md` § "Permanently sealed".

## 🟢 Currently active

### In-flight: ADR 0046 — Mobile extension via synced Obsidian vault

- **Status:** Accepted 2026-05-27 (`docs/adr/0046-mobile-extension-via-vault.md`). **Iteration 1 COMPLETE** (desktop file-ingest pipeline). **Iteration 2 IMPLEMENTATION COMPLETE** (outbound Obsidian projection: all phases A/B/C.1/C.2/C.3/D shipped). **Pending Dustin hands-on smoke** before Iter 2 marked sealed and Iter 3 (mobile inbox) starts.
- **Iteration 1: COMPLETE 2026-05-27.** All four implementation beads + judge + smoke green.
  - `mb-jqhw` closed (migration 018 `sessions.source` at `0ecfda2`).
  - `mb-hxm4` closed (symphonia decode helper at `0c250e5`).
  - `mb-evn3` closed (headless ingest extraction + SessionsEventBus at `fcf8008`). `dictation::ingest::headless_ingest(deps, samples, provenance)` is the new pure-Rust entry point; `dictation::events::SessionsEventBus` trait extracted with a `RecordingWindow` blanket impl.
  - `mb-7vyz` closed (Phase D + ADR §3.2 amendment at `2a4ea12`). ADR §3.2 authorizes the orchestrator's two-channel topology: a sibling `crossbeam-channel` carries `HeadlessIngestRequest`s alongside the existing `StateAction` stream. `+ Audio file` button live on the Dictations page; `dictation_import_file` IPC decodes off-thread, queues the request, replies via per-request bounded(1) channel. No fresh VAD/STT/Cleaner allocations per import.
  - `mb-thmd` closed 2026-05-27 (sealed-phases-untouched judge 🟢 PASS @ 95% conf). Verdict: `docs/judges/mobile-extension/sealed-phases-untouched-iter1-verdict.md`. Diff `d99a4cd..a004efa` stayed inside ADR §3 / §3.1 / §3.2 boundary; migrations 001-017 modification-free; no `UPDATE` against `stage='raw'`.
  - `mb-jbf7` closed 2026-05-27 (live-fire smoke) — Dustin's hands-on verification against fixture `C:\Users\dboyd\Downloads\New Recording 38.m4a`. All 7 pass criteria green: decode, Whisper transcript (session 112, dur_ms=29775), cleanup applied, DB row correct (`source='desktop-import'`, `start_mode='in_app'`, `status='complete'`), UI auto-refetch via SessionsEventBus, PTT regression-free. Programmatic schema half also independently verified via `verify_iter1_schema.py` at `0d7375f`. Smoke example (`src-tauri/examples/smoke_iter1_ingest.rs`) remains in-tree but Strategy A stays blocked by `mb-0n8c` (LESSONS PINNED P2 refinement).
- **Iteration 2: IMPLEMENTATION COMPLETE 2026-05-27.** Seven commits across two sessions:
  - `bc077fc` — Phase A / `mb-yheh` CLOSED. `src-tauri/src/vault/{mod,layout,manifest}.rs`: idempotent zone creation, atomic manifest save with BTreeMap ordering for deterministic on-disk bytes, `AppError::Vault` typed variant. 15 throwaway-crate tests green.
  - `3e8b536` — Phase B / `mb-p7cp` CLOSED. `src-tauri/src/vault/project.rs`: pure record→markdown projection. Hand-rolled YAML front-matter (8-key fixed order, optional-omit, sorted tags, conservative scalar quoting); SHA-256 content address; `YYYY-MM-DD-HHMM__<uuid8>.md` filename. 18 throwaway-crate tests + golden-snapshot pin. `sha2 = 0.10` added as direct workspace dep.
  - `47b35ef` — Phase C.1 (`mb-lvzw` partial). 8 new `SettingKey` variants per ADR §10: 4 behavioral (`MobileSyncEnabled`, `VaultPath`, `VaultSyncRecordTypes`, `VaultRetentionDays`) + 4 Iter 4 stubs (`VaultSyncBackend`, `SyncTierByteCap`, `VaultDebugKeepCouriers`, `KeepAudioBlobs`). All opt-in / OFF by default per ADR §10.
  - `958c029` — Phase C.2 (`mb-lvzw` partial). `src-tauri/src/vault/export_job.rs`: `VaultRuntime { config: Arc<RwLock<VaultConfig>>, job_lock: Arc<Mutex<()>>, coalesced: Arc<AtomicBool> }` + `run_once_blocking(&db)` reconciliation pass + `trigger(db)` fire-and-forget with coalescing semantics. Query helpers cover dictation (transcripts `final`/`cleaned`/`raw` fallback) + meetings (`formatted_merged`/`mic`/`sys` fallback). Atomic `.tmp + rename` write to avoid Obsidian-Sync partial-state. Stale records move to `history/_archive/`. 9 integration tests in the throwaway crate pin backfill / no-op / single-content-change / hard-delete / retention-narrowing / concurrent-trigger-coalesce / record-types-filter behavior.
  - `62d12a1` — Phase C.3 + `mb-lvzw` CLOSED. Post-commit hooks at `dictation.rs::persist_complete` (PTT path), `dictation.rs::handle_headless` (file-import + future mobile-inbox), and `meetings/lifecycle.rs::stop_meeting` Complete branch. `DictationOrchestrator::new` + `DictationRuntime::spawn` + `MeetingCaptureRuntime::spawn` all gained an `Arc<VaultRuntime>` constructor param (same additive shape as Iter 1's `SessionsEventBus`). `VaultRuntime` constructed in `lib.rs` Tauri setup, `.manage()`'d as state. New IPCs in `src-tauri/src/commands/vault.rs`: `vault_settings_get`, `vault_settings_set` (also calls `refresh_config` + `trigger` so flipping the toggle on backfills immediately), `vault_export_now`, `vault_pick_directory` (server-side folder picker via `tauri-plugin-dialog` to avoid taking a new JS dep). Diff against Iter 1 seal `9ec2d91` for `dictation.rs` is +23/-0 (purely additive); `dictation/ingest.rs` untouched.
  - `dd48904` — Phase D / `mb-vg3p` PARTIAL. New "Mobile Sync (preview)" section in Settings → Advanced via `ui/src/pages/SettingsMobilePreview.tsx`: master toggle + vault path input + Browse... button (native folder picker) + status line (Loading/Disabled/No-path/Ready) + Export-now button with toast on completion. Three pure helpers (`deriveStatus`, `statusLabel`, `formatExportToast`) extracted + exported so `SettingsMobilePreview.test.ts` pins every branch without `@testing-library/react` — same convention as `MeetingRecordBar.test.ts`. 11 new vitest specs (74/74 UI tests green). `mb-vg3p` description updated; Iter 4 still owns VaultSyncBackend selector / SyncTierByteCap / VaultDebugKeepCouriers / detailed record-types selector / health card / tier copy / dedicated tab lift.
- **Iter 2 gates green:** cargo check + clippy --release -D warnings + fmt --check + test --release --no-run + build --release all pass. App launches clean (whisper-large-v3-turbo + Silero VAD + Ollama warmup; no vault errors in tracing log). `mb-lvzw` CLOSED.
- **Pending live-fire smoke (Dustin's hands-on):** Settings → Advanced → Mobile Sync (preview). Click-by-click from kickoff §15: (a) confirm "Disabled" status, (b) flip master toggle ON → status → "Vault path not set", (c) click "Browse..." → pick `C:\Users\dboyd\mockingbird-vault` → status → "Ready" → backfill should fire automatically (look at vault folder for `dictation/` + `meeting/` + manifest files within seconds), (d) click "Export now" → toast renders something like "Vault up to date (N records).", (e) confirm files appear in Obsidian desktop, (f) wait for Obsidian Sync round-trip → confirm on iPhone, (g) do a fresh PTT dictation → confirm new .md appears within ~1-2s and syncs. Any failure surfaces a clear toast (status line drives the Export-now disable state).
- `mb-s8s2` (sync-layer spike) re-anchored to Iter 2 setup.
- **POC config (locked at Accept, sync green):** vault at `C:\Users\dboyd\mockingbird-vault\`, remote vault name `mockingbird-vault`, Obsidian Sync Standard tier with End-to-End encryption (key in user's password manager, Mockingbird never touches it). iOS Shortcut locked to Quick capture (Variant 1, one-tap, Low/32 kbps AAC mono). See ADR §"Realized POC configuration".
- **Seal mechanic:** ADR Accepted (final) + STATUS update at end of Iter 4. **No new `phase-*-complete` tag** (lateral epic per LESSONS PINNED P5).
- **Charter bead:** `mb-7c2c` (closed).
- **Follow-up beads logged from Iter 1:**
  - `mb-0uqb` (P3) — revisit descoped sidecar-based silent-skip detection if POC users hit silent-skip in practice (descoped from §8).
  - `mb-q1xt` (P2, Iter 4 polish) — progress indicator during `+ Audio file` import pipeline. UX gap surfaced during Iter 1 smoke: ~10-60s blind wait between file-pick and the session row appearing. Linked as follow-up to `mb-7vyz`. Recommended approach (option 3): reuse the existing RecordingWindow overlay state machine with import-aware labels.

Live-fire Win11 smoke test for Phase 10 is still Dustin's post-seal step
(LESSONS PINNED P7 pattern; same shape as the post-MC `mb-x1x` flow) — judges
don't catch live-OS regressions.

**Discovered + filed during Iter 2 Phase B:**
- `mb-4j81` (P3 chore) — `clippy --release --all-targets` surfaces pre-existing `manual_str_repeat` (`activity/uia/windows_com.rs:378` + `:498`) and `identity_op` (`overlay_conventions.rs:200`) in test code. Standard kickoff gate (`clippy --release` without `--all-targets`) misses them. Not a regression from Iter 2; fix when next in that code.

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
