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

**Last consolidated:** 2026-06-08 (**KG Phase 1E Wave 1E.8 SEALED —
iOS Shortcut docs for KG-Inbox shipped (`mb-wnsm`). New file
`docs/mobile/ios-shortcut-kg.md` (372 LoC, LF, ASCII-only, no BOM) mirrors
ADR 0046 §8 / `docs/mobile/ios-shortcut.md` for the Knowledge Graph
destination. Same three-action shape (Record Audio -> Save File -> Open
App: Obsidian); save destination is `<vault>/Knowledge Graph/Inbox/` and
recommended Shortcut name is `Mockingbird Quick Capture (Knowledge
Graph)`. KG-specific framing: Layer 1 raw destination per ADR 0054 §H
(chat-LLM Ingest crystallizes to Layer 2 in Phase 2); positional routing
per ADR 0048 Q2; audio preserved in `History/<YYYY-MM>/` post-ingest
(diverges from standard courier which discards); `_failed/` quarantine
path documented; two-Shortcut coexistence story up front. Cross-link
integrity bidirectional: precedent doc got a 5-line additive pointer at
the top to the KG variant. Pure docs wave - no Rust, no UI, no cargo/tsc
gates needed. Closes `mb-wnsm`. **Resume:** Wave 1E.8 sealed; next-up is
1E.9 (`mb-kazi`, dual ADR 0053+0054 seal + judges J1-J4 reverse-watcher-
loop-prevention / file-wins-on-conflict / subtree-bootstrap-idempotent /
serializer-golden-roundtrip). Prior consolidation 2026-06-08 (**KG Phase
1E Wave 1E.6 SEALED —
KG-Inbox courier shipped (`mb-i46v`). Sibling of the ADR 0046 inbox courier
rooted at `<vault>/Knowledge Graph/Inbox/` for iOS-Shortcut + desktop
drag-and-drop audio drops. Three new vault modules:
`vault/kg_inbox_courier.rs` (546 LoC), `vault/kg_inbox_courier_fs.rs`
(236 LoC; FileOps trait + helpers split to fit 600-LoC cap),
`vault/kg_inbox_runtime.rs` (448 LoC; watcher+courier lifecycle gated by
`KgGraphEnabled` + `VaultPath`). Tests: 442 LoC in
`vault/kg_inbox_courier_tests.rs` (success-no-move, idempotent-skip,
failure-quarantines-to-KG-Inbox/_failed, validation matrix, collision
suffix) + 3 inline runtime tests. **Success path performs ZERO
filesystem moves** — the worker phase-4 `archive_session_history` owns
disposition (rename to `History/<YYYY-MM>/<uuid>.<ext>`); fully reuses
existing `IngestProvenance::mobile_inbox_kg_note` so source-gate enqueues
automatically into `kg_filing_queue` and 5a/5b/5c (projection / archive /
stubs / INDEX+LOG+Tags) fire by construction. Idempotency probe checks
`sessions.audio_blob_path` BEFORE calling headless ingest; crash-recovery
re-emit + initial-scan re-pickup both short-circuit cleanly. Lifecycle
refresh wired into both `kg_settings_set` and `vault_settings_set` so
flipping `KgGraphEnabled` OR changing `VaultPath` starts/stops/restarts
the watcher pair in the same IPC tick. All gates GREEN: `cargo check`,
`cargo clippy --release -D warnings`, `cargo fmt --check`,
`cargo test --release --no-run` (9m00s — 17 binaries linked clean),
`cargo build --release` MANDATORY per P13 (worker + watcher + IPC
surface touched; fresh `target/release/mockingbird.exe` mtime 6/1/2026
9:10:49 AM). KG invariants `kg_parity 32/32` + `kg_source_gate 6/6` +
`kg_graph_off 8/8` re-run GREEN post-courier-wire (source-gate path
unchanged; `mobile_inbox_kg_note` provenance already gated correctly).
LESSONS append for the IngestProgressBus type-erasure gotcha (Arc<dyn T>
vs Arc<Concrete>) + the 600-LoC split discipline for new vault modules.
Closes `mb-i46v`. **Resume:** Wave 1E.6 sealed; next-up unblocked is
1E.8 (`mb-wnsm`, iOS Shortcut docs — docs-only) and 1E.9 (`mb-kazi`,
dual-ADR seal + judges). Prior consolidation 2026-06-08 (**KG Phase 1E
Wave 1E.7 PART 2 SEALED —
worker + kg_layout refactor under 600-LoC cap, closes `mb-5lla` AND `mb-bgpt`
(Wave 1E.7 fully sealed).** Pure rearrangement, zero behaviour change. `kg/
worker.rs` 2050 → 505 LoC root + new `kg/worker/` submodule tree
(`filing.rs` 357, `projection.rs` 496, `stubs.rs` 254, `index_log.rs` 185,
`transcripts.rs` 122, `time_iso.rs` 119, `archive.rs` 114). `vault/
kg_layout.rs` 698 → 420 impl + new sibling `kg_layout_tests.rs` 344 via the
existing `#[cfg(test)] #[path = "..."] mod tests;` pattern. Public APIs
(`KgFilingRuntime::spawn`, `build_segment_outputs`, bootstrap helpers) all
unchanged — call sites in `lib.rs` / IPC / parity probe / latency bench
needed zero edits. Phase 4b/5a/5b/5c orchestration sequence in
`filing::process_one` byte-identical to the pre-split flow. All gates GREEN:
`cargo check` (release), `cargo clippy --release -- -D warnings`,
`cargo fmt --check`, `cargo test --release --no-run` (4m32s — 17 test
binaries linked clean), `cargo build --release` MANDATORY per P13 (worker
pipeline surface touched; fresh `target/release/mockingbird.exe` mtime
2026-06-01 06:57), `npx tsc --noEmit`. KG-invariant runtime checks
(`kg_parity` 32/32, `kg_source_gate` 6/6, `kg_graph_off` 8/8) preserved by
construction — pure refactor; the new structure compiles + links + lints
clean and behaviour is preserved by code-motion. 1 housekeeping bead filed
(`mb-ngtv` — KG serializer Tasks-plugin checkbox default OFF per ADR 0054
§L; defer fix to `mb-qw7n` or independent later). Closes `mb-5lla` +
`mb-bgpt`. **Resume:** Wave 1E.7 fully sealed; next-up unblocked work is
1E.6 (`mb-i46v`, KG-Inbox courier) + 1E.8 (`mb-wnsm`, iOS Shortcut docs) +
1E.9 (`mb-kazi`, dual-ADR seal + judges). Prior consolidation 2026-06-07
(**KG Phase 1E hotfix #3 `mb-y390` shipped** — dictation #143 KG-filing
failure (deterministic 3-retry on "Get a New Computer" project)
root-caused to the `segment` pass: qwen2.5:7b emits a
mixed-quote array (`["I've got...", 'title is "Get a New Computer"', ...]`)
the moment a string contains an unescaped double-quote; `serde_json::from_str`
fails strict at column 46 and the pipeline aborts with `PassError::Parse {
pass: "segment" }`. Fix: pass-layer `relax_pythonish_quotes` + `parse_pass_json`
helpers in `src-tauri/src/kg/passes/mod.rs` — strict serde first, relaxed
Python-style re-quote on fallback; cold path on parity corpus. Wired into
`segment` + `extract` + `extract_entities` (no change to `classify` — its
responses don't carry user-quoted noun phrases). 6 inline tests including a
live-fire reproducer of the literal dictation #143 raw bytes. UI: new
`<details>` expander on Flagged-for-review rows surfaces the `last_error`
inline (snippet up to 140 chars in summary, full text in mono-pre body) so
future filing failures don't require host-side DB diagnosis; 2 new i18n keys
(`kg.failed.errorSummary` / `kg.failed.errorReveal`) + `flaggedErrorDetails`/
`flaggedErrorSummary`/`flaggedErrorFull` CSS classes; `lastError` field was
already on the FailedFiling IPC payload from Phase 1C Wave 1C.2 so zero IPC
churn. Retry-flow audit (Phase 4 of brief): `kg_retry_failed_filing` IPC
resets `attempt_count`→0 and re-enqueues — semantics correct; the bug is
deterministic, not retry-related. All gates green incl. `cargo build
--release` per P13 (worker pipeline code path touched); fresh exe at
`target/release/mockingbird.exe` mtime 2026-06-01 00:20; `kg_parity` 32/32 +
`kg_source_gate_invariant` 6/6 + `kg_graph_off_invariant` 8/8 GREEN.
LESSONS body entry appended (4 findings: corpus blind-spot for adversarial
input characters; relaxer-vs-prompt-tuning tradeoff; throwaway-crate recipe
re-pays; `beforeBuildCommand` doesn't fire under plain `cargo build`). Closes
`mb-y390`. **User: relaunch `scripts\run-mockingbird.ps1` to pick up the new
exe + UI; on the Flagged row for #143 click Retry once — if it still fails,
the error text now appears inline under the row.** Prior consolidation
2026-06-06 (**KG Phase 1E Wave 1E.7 hotfix `mb-wzcj` shipped** — restored
writer-side / parser-side symmetry during the type-vocab
realignment window: `kg/worker.rs::kg_entry_type_to_vault` now returns a
`(VaultEntryType, Option<&'static str>)` sidecar so the call site in
`maybe_commit_to_vault` emits a `tracing::warn!` whenever the classifier's
legacy `task`/`idea` values get bridged to canonical `note`/`observation`
(mirrors the reverse-watcher parser's existing tolerance); phases 5a/5b/5c
non-fatal-continue sites upgraded `tracing::warn!` -> `tracing::error!` so
silent post-seal artifact failures surface in logs/UI; 4 new unit tests cover
the bridge sidecar (task->note+legacy, idea->observation+legacy, canonical
pass-throughs silent, exhaustive variant coverage); LESSONS PINNED **P15**
(writer-side permissiveness during vocabulary realignments) added above P14;
`cargo build --release` MANDATORY per P13 since worker pipeline code path
touched; `mb-qw7n` (classifier prompt realignment) description updated to
note the new symmetry means it can land independently without coordinating
writer-strictness flip — strictness collapse to a follow-up wave AFTER
`mb-qw7n` ships. Prior consolidation 2026-06-06 (**KG Phase 1E Alignment
Wave shipped** —
zero-code docs-only realignment to the Karpathy/Clark Personal Knowledge
Engine pattern; **ADR 0054 Proposed** (Personal Knowledge Engine substrate)
adopts a richer framing OVER ADR 0053's vault foundation, with two-agent role
separation (Mockingbird = capture + first-pass synthesis layer; user's
chat-LLM = wiki author performing Ingest/Query/Lint); ADR 0053 amended with
supersession pointer (§D3 vocab partial, §D8 seeds scope partial, §D9 Tasks
default-on de-emphasized); `phase-1e.md` Amendment 2026-06-06 #2 rescopes
Wave 1E.7 (drop `Kanban-Tasks.md`; ADD `SCHEMA.md` / `INDEX.md` / `LOG.md` /
`Tags/` subtree / type-vocab realignment); LESSONS PINNED **P14** (Karpathy/Clark
north star) added; PRODUCT-STATE.md adds §3.20 KG subsystem + bumps snapshot
framing; AGENTS.md project-context paragraph updated; type vocabulary realigned
to nine knowledge shapes (`source`/`note`/`concept`/`entity`/`project`/
`question`/`decision`/`reference`/`observation` — drops `task`/`event`); 4
beads reframed (`mb-82h6`/`mb-ifun`/`mb-xqcf` description rewrites; `mb-il83`
closed by vocab realignment); 4 new follow-up beads filed (`mb-rik9` this
wave; UI copy; pipeline prompt realignment; `spec.md` realignment;
Phase 2 charter draft); single `[alignment]` commit; closes `mb-rik9`).
Prior consolidation 2026-06-06 (**KG Phase 1E Wave 1E.5 shipped** — reverse-watcher reconciles Obsidian Entry edits back into SQLite within ~3s; SHA-256 loop-prevention against own writes; entity/project/history/inbox routed to IGNORED; new `vault::watcher` + `vault::watcher_reconcile` + `vault::markdown_parser` modules (all under 600-LoC cap); wiki-link alias polish — entities now emit `"[[Entities/<slug>|<slug>]]"`; 3 LF-normalized golden updates; 3 pipeline-quality beads filed for post-1E sprint (`mb-82h6`, `mb-ifun`, `mb-xqcf`); all gates green incl. cargo build --release; closes `mb-qwfy`). Prior consolidation 2026-06-06 (**KG Phase 1E charter amendment shipped** — four ADR 0053 amendments (subtree → 5 folders; entities emit as `"[[Entities/<slug>]]"` wiki-links; auto-generated entity pages §D11; auto-generated project pages §D12); new `vault::entity_pages` module; serializer retrofit; worker integration; 19 new live tests + 1 new golden + 2 property tests; closes `mb-08za`; fresh release exe built per PINNED P13). Prior consolidation 2026-06-06 (**KG Phase 1E hotfix #2 shipped** — vault entry body now sources from `transcripts(stage='final')` cleaned cascade, not `entries[0].body` segmenter output; multi-bullet KG notes no longer drop segments 1..N; closes `mb-wzui`; fresh release exe built). Prior consolidation 2026-06-06 earlier (**KG Phase 1E hotfix #1 shipped** — fresh release binary + `kg_reconcile_vault` + `kg_reconcile_history` IPCs + dashboard "Reconcile vault" button; closes `mb-43xw`; LESSONS PINNED P13 added). Prior consolidation 2026-06-05 (**KG Phase 1E Wave 1E.4 shipped** — `vault::history` module ships per-session JSON sidecar + audio file archive to `History/<YYYY-MM>/<session-uuid>.{json,ext}`; KG worker phase-4 integration; reconcile scan helper. Three golden JSON fixtures locked. All gates green; kg_parity 32/32, source_gate 6/6, graph_off 8/8 GREEN). Prior anchors: 2026-06-04 (Wave 1E.3 two-phase commit shipped); 2026-06-04 earlier (**KG Phase 1D SEALED via ADR 0052 Accepted** — Wave 1D.6 three judges + epic seal); 2026-05-31 (**KG Phase 1C SEALED via ADR 0051 Accepted**); 2026-06-03 (ADR 0050 KG Phase 1B SEALED).

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
- **ADR 0052 — KG Phase 1D source-gated filing + first-class KG screen** (sealed 2026-06-04 as lateral epic under ADR 0049 §"Sandbox isolation"). Six waves shipped: 1D.0 (`b5c17bf`, charter + clean-slate verify + phase doc + bead epic), 1D.1 (`f3b9a4a`, migration 025 + `capture_kind` source-gate + 3-gate cascade), 1D.2 (`37feed7`, KG screen scaffold + 5-band dashboard + new `kg_dashboard_snapshot` IPC), 1D.3 (`846ecd5`, capture surface — audio + text notes via new `kg::ingest_text` module + `dictation_start_kg_note` + `kg_ingest_text_note` IPCs), 1D.4 (`acb8f9a`+`0142ddb`, chip/modal relocation Dictations → KG dashboard — -211 LoC from Dictations, +Retrieval band on KG screen, click-to-modal wired, FlaggedBand click-to-retry), 1D.5 (`0cd54ff`+`f9cb27c`, Settings KG tab expansion — Vault/Vocabularies/ProcessingMode/Dual-write cards + Obsidian launcher via new `kg::launcher` module + `kg_launch_obsidian` IPC). Wave 1D.6 (this seal) lands three judges + ADR seal + STATUS update. **Two drift corrections shipped:** (1) ADR 0050's dictation-tail auto-file replaced by 3-gate cascade (outcome → source → toggle); standard PTT + in-app dictations NEVER enqueue regardless of toggle. (2) KG promoted from "filter chips on Dictations" to first-class sidebar destination per spec §15.3. Acceptance gates: **J1 `kg-source-gate-invariant` (NEW) GREEN** — deterministic Rust probe (`src-tauri/src/kg/source_gate_invariant.rs` + binary `kg_source_gate_invariant`); 6/6 corpus cells (3 `capture_kind`s × 2 toggle states); drives both `try_enqueue_for_kg_filing` AND `ingest_text_note` entry points. **J2 `kg-dictation-untouched` GREEN** — runtime twin of Phase MC's diff-judge of the same name; cells 1+2 of J1 + cross-check that only two `kg::store::enqueue_for_filing` call sites exist. **J3 `kg-graph-off-ui-tightened` GREEN** — no new code; consolidated Playwright invariant from Waves 1D.2/1D.4/1D.5 documented as fully satisfied; `npx playwright test kg-graph-off-invariant` 1/1 passed. Regression: `kg_graph_off_invariant` 8/8 + controls; `kg_parity` 32/32 default + `--persist`. Charter: `docs/adr/0052-knowledge-graph-phase-1d-charter.md` (Accepted 2026-06-04, §"Phase 1D SEALED" carries close-out). Judge docs: `docs/judges/phase-1d/`. Phase doc: `docs/phases/phase-1d.md`. Beads closed: `mb-x7f9` (epic) + `mb-qhll` (1D.0) + `mb-pxzk` (1D.1) + `mb-j00j` (1D.2) + `mb-0gt6` (1D.3) + `mb-6hm2`+`mb-f4gn` (1D.4) + `mb-navi` (1D.5) + `mb-q2p1` (1D.6) + `mb-oji5` (category-axis, consumed at 1D.1). **Standing beads carried forward** (not gating seal): `mb-bbl2` (sonner retrofit), `mb-y6pq` (`--status-bad` token sweep), `mb-26aw` (`smoke.spec.ts` ×4 pre-1C Playwright failures), `mb-2wbk` (KG row → Dictations deep-link, P3, filed in 1D.4), `mb-0ui1` (vocab editor, P3, filed in 1D.5). **No new `phase-*-complete` tag** — lateral epic per LESSONS P5.
- **ADR 0051 — KG Phase 1C retrieval UX + activation toggle + concept modal** (sealed 2026-05-31 as lateral epic under ADR 0049 §"Sandbox isolation"). Six waves shipped: 1C.0 (charter + empirical p95=59s latency baseline + `PassTimings` instrumentation), 1C.1 (Settings KG tab + activation toggle + worker boot-vs-poll promotion), 1C.2 (failed-filings UX + queue-status line + idempotent `kg_requeue_failed`), 1C.3 (Dictations retrieval UX — 5 of 6 axes: entity + tag + free-text + per-row chip strip + filing-state pills), 1C.4 (concept modal for entity + tag drill-down + click-to-open chips), 1C.5 (graph-off-UI invariant Playwright judge via opt-in `__KG_IPC_SPY__` hook on `lib/tauri.ts::invoke` + `SettingsKgTab` `role="status"` a11y scoping + this seal). Acceptance gates: **`kg-graph-off-ui-untouched` (J1) GREEN** (Playwright spy records exactly `{kg_settings_get_all}` across Settings/KG tab + Dictations page + row-click walks; positive-control flip ON lights up `kg_list_failed_filings`+`kg_queue_status`); **`kg-retrieval-correct` (J2) GREEN** via the 1C.3 Playwright spec; **`kg-failed-filing-retry-idempotent` (J3) GREEN** via the 1C.2 Rust-side throwaway + UI-side disable-on-click. Parity probe 32/32 in both default and `--persist` modes (no `kg/store` regression — 1C.5 changes are UI + test-only). Wave brief: `docs/knowledge-graph/phase-1c-brief.md`. Charter: `docs/adr/0051-kg-phase-1c-retrieval-ux-and-activation.md` (Accepted 2026-05-31, §"Phase 1C SEALED" carries the close-out). Commit chain `113e848..<seal-hash>` (Wave 1C.0..1C.5). Beads closed: `mb-j368` (epic) + `mb-plz9` (1C.0) + `mb-ucmx`/`mb-s6a8`/`mb-7w5f` (1C.1) + `mb-9ufg`/`mb-j3t1` (1C.2) + `mb-5ly5` (1C.3) + `mb-sx6p` (1C.4) + `mb-f4gn` (1C.5) + `mb-b3jy` (discharged 1C.0). **Deferred (not shipped):** `mb-oji5` (category-axis persistence — verified twice that Phase 1B schema has no queryable `category` column; ADR 0051 §"Out of scope" bans new migrations in 1C; punted to Phase 1D where the backfill path already needs a schema-touching migration). **Standing beads carried forward:** `mb-bbl2` (sonner retrofit), `mb-y6pq` (`--status-bad` token sweep), `mb-26aw` (`smoke.spec.ts` ×4 pre-1C Playwright failures). **No new `phase-*-complete` tag** — lateral epic per LESSONS P5.
- **ADR 0050 — KG Phase 1B persistence + async filing worker + dictation-tail hook** (sealed 2026-06-03 as lateral epic under ADR 0049 §"Sandbox isolation"). KG library now persists entity + tag mentions per-segment to SQLite (migration 024: `kg_entities`, `kg_canonical_tags` (v1.1 inert), `kg_entity_mentions`, `kg_tag_mentions`, `kg_filing_queue`, two concept-page VIEWs, two `BEFORE UPDATE` immutability triggers on the mention tables, `kg_graph_enabled=false` seed). Async filing worker (`kg::worker::KgFilingRuntime`) spawns at app boot when `SettingKey::KgGraphEnabled=true` (default `false`); drains FIFO with crash-recovery sweep + 30-day done-row reap. Single dictation-tail enqueue point at `dictation.rs::persist_complete` outcome-gated (`Ok` / `OkClipboardNotRestored` / `InAppNoInject`) with ignore-error semantics. `extract_entities` wired as the 5th pipeline pass in Chunk 3 (the kickoff discovered ADR 0049 §6 had been silently violated — production was 4-pass-only; closed the gap with an additive `PipelineResult.segment_entities` field). Acceptance gates: **`kg_parity` default 32/32 + `kg_parity --persist` 32/32 + `kg_graph_off_invariant` 8/8** (every InjectionOutcome variant + positive control). Wave brief: `docs/knowledge-graph/phase-1b-brief.md`. Charter: `docs/adr/0050-kg-phase-1b-persistence-and-dictation-hook.md` (Accepted 2026-06-03, §"Phase 1B SEALED" carries the close-out paragraph). Commit chain `0fed8e3..<seal-hash>`. Beads closed: `mb-bjni` (epic) + `mb-go9l` + `mb-geds` + `mb-eke8` + `mb-ryq4` + `mb-k17a`. **No new `phase-*-complete` tag** — lateral epic per LESSONS P5.
- **ADR 0049 — KG Phase 1A graduation** (sealed 2026-05-31 under the same ADR 0049 charter). Schema-driven KG pipeline graduated from `experimental/kg-validation/` to `src-tauri/src/kg/` as a callable library (no consumers wired yet — that's Phase 1C). Wave brief: `docs/knowledge-graph/phase-1a-brief.md`. Parity gate: **32/32 bit-identical** vs the Wave 0.5.4 seed-42 fixture via `src-tauri/src/bin/kg_parity.rs`. Commit chain `75485de..<seal-hash>` — Chunk 1 scaffold + fixture, Chunk 2 library translation (six commits), Chunk 3 probe + v1-slice fix to bundled SCHEMA.md (closed-vocab Move 3 list stripped per amendment A2; closed-vocab Rust wiring stays as v1.1 starting point, guarded by `closed_vocab_path_still_active_via_env_override`). Beads closed: `mb-2mc9`, `mb-qdgn`, `mb-cskk`. Per ADR 0049 §"Sandbox isolation" close-out: the graduation window for `src-tauri/**` + `migrations/**` is now closed for this epic; Phase 1B/1C/1D/1E each charter their own. No new `phase-*-complete` tag (lateral epic per LESSONS P5).
- ADR 0049 — Knowledge Graph Phase 0.5 + v1 architectural pivot (Accepted 2026-05-29). Phase 0.5 sealed across six waves on the `experimental/kg-validation/` sandbox. Two architectural keepers: SCHEMA.md as portable contract with per-model-class calibration profiles (Move 1; LESSONS P10) and entity extraction as a 5th pipeline pass (Move 4; Wave 0.5.4 ACCEPT at 54.83% / 53.40% strict Jaccard, 97.08% stability, ≥ 50% bar). Two architectural falsifications: embeddings nearest-neighbour classification (Move 2; amendment A1 — preserved as a similarity tool for entity disambiguation, retired as a classifier) and closed-vocab Move 3 (DEFERRED to v1.1 — wiring on `main` commit `8fdc7fb` is architecturally correct but blocked on two-field corpus re-labeling per LESSONS P11 "tags ≠ entities"). v1 architecture binding (amendments A1/A2/A3): pipeline segment → classify → extract → **extract_entities** → normalize, two-field entry schema (`tags:` open-vocab in v1 + `entities:` typed references), SCHEMA.md drives all passes, qwen2.5:7b-instruct-q4_K_M pinned for entity-aware operation (3b = documented tags-only degraded mode), opt-in graph guarantee, ~1 min intake latency budget. REPORT at `docs/knowledge-graph/PHASE-0-5-REPORT.md`. Beads sealed: `mb-symi` (epic), `mb-xmgs`, `mb-4xtd`, `mb-yfzy`, `mb-hnb4`, `mb-rzpd`, `mb-e10v`, `mb-o4ni`, `mb-5r1b`, `mb-qogz`. **No `phase-*-complete` tag** — lateral epic. Phase 1A (schema-driven pipeline graduates to production) awaits Dustin kickoff.
- ADR 0048 — Knowledge Graph Phase 0 validation methodology (Accepted — sealed 2026-05-29 with `docs/knowledge-graph/REPORT.md`). Seven waves shipped: Wave 0 charter + scaffold; Wave 1 corpus (32 fixtures, full taxonomy coverage); Wave 2 4-pass pipeline + run-corpus harness; Wave 3 scorer (3 sub-iterations sealing on §G7 deterministic synonym-map metric per Option E); Wave 4 6 invariant judges + run-judges rig (`phase-0-kg-start` anchor at `aad06a6`); Wave 5 IAP Wiggum loop (cap 5; 0 accepted; documented the structural ceiling); Wave 6 REPORT.md + go/no-go (§G6 strict NO-GO; defensible GO-WITH-LIMITATIONS for an assisted-filing v1 UX). Final scorecard: hard-gate `invented_dates_count=0` PASS, junk-bucket 100% PASS, segmentation 86.7% PASS, category 67.3% FAIL, entry-type 78.2% FAIL, clean-single 6.7% FAIL, tag-collapse 9.1% FAIL. Synonym map v1.1. Stability ≥95% structural agreement. v1 recommendation: lighter spec scope PART B §9 with per-entry user confirmation; draft-review pane converting filling-quality errors into 1-tap corrections; raw transcript preserved per spec §10 dual-write. **No `phase-*-complete` tag** — lateral epic. Future v1 charter ADR (provisionally 0049) inherits Q1/Q2/Q3 decisions from ADR 0048 §3 + assisted-filing-UX contract from REPORT §8. Beads sealed: `mb-4wxw`, `mb-w1lw`, `mb-i9l1`, `mb-t7w5`, `mb-901u`, `mb-i4us`, `mb-nbel`, `mb-57a1`, `mb-jz5r`, `mb-he98`, `mb-ojm5`, `mb-0baz`.
- ADR 0046 — Mobile extension via synced Obsidian vault (Accepted — sealed 2026-05-24). Four iterations shipped (desktop file ingest → outbound vault projection → inbound mobile courier → polish). User-facing surface: `+ Audio file` desktop import button, deterministic Markdown projection of dictation + meeting history to `<vault>/history/`, inbox courier auto-processing iOS-Shortcut-delivered voice memos from `<vault>/inbox/`, full Mobile Sync settings tab (8 keys + connection-health card), nested-vault detection wizard, import progress overlay, iOS Shortcut recipe (`docs/mobile/ios-shortcut.md`, 3 actions per Wave 0 Finding 5). Channel boundary preserved across 3 reuse sites: `dictation/ingest.rs` (Iter 1 ADR §3.2 amendment) consumed by IPC handler, inbox courier, and import progress overlay event-tap with zero further sealed-surface modifications. Two `sealed-phases-untouched` judges PASS (Iter 1 @ 95%, Iter 3 @ 99%); Iter 2 + Iter 4 didn't need them (greenfield + UI-side). 19 beads closed. Seal commit: HEAD of `main` at consolidation time (this STATUS update was committed in the seal commit itself; see `git log --grep='ADR 0046 SEALED'`). Wave 5 hardening matrix (`mb-qxrm`) remains open as live-corpus catch-up; not gating epic seal. **No new `phase-*-complete` tag** (lateral epic per LESSONS PINNED P5).
- ADR 0047 — Cleanup pipeline refinement (Accepted 2026-05-25). Per-pass system headers in `meetings/llm_pass.rs` (`cleaner_punctuation` no longer carries the global "Be concise" instruction — the load-bearing fix); length-ratio shrink fallback (`SettingKey::LlmShrinkFallbackThreshold`, default 0.65); Whisper `initial_prompt` wired from the user's dictionary at both dictation call sites; temperature standardized to 0.2 across casual / normal / formal / meetings (migration 019); new `DictationCleanupLevel` dial (`None` / `Light` / `Medium` / `High`; default `High` preserves prior behaviour; `Medium` uses the new `normal_v6_additive` prompt); LLM-skip-on-short-utterance (`SettingKey::LlmSkipWordThreshold`, default 12 words; gated on `!looks_listy()`; consumed `mb-cjc` / ADR 0022 Wave 3); casual mode repointed to `qwen2.5:7b-instruct-q4_K_M` (migration 021; one-liners absorbed by the skip path); opt-in Q5_K_M via `SettingKey::PreferQ5Models` with VRAM-gated runtime substitution (migration 022; defaults off); Compress Transform on `LlmPassCard` as on-demand pull-only affordance (`dictation/prompts/compress.md`); `sessions.edit_free_within_5min` instrumentation as the empirical quality signal (surfaced in Insights "Your usage"). UI surface for the dial + Q5 toggle deferred to `mb-h0nn`. Empirically validated by `docs/cleanup/eval-adr0047-cleaner-punctuation.md` (18/20 fixtures preserve all expected phrases on `qwen2.5:3b-instruct-q4_K_M`; zero over-consolidation regressions). Sealed via 13 commits `c7af486..` + this seal commit; **no `phase-*-complete` tag** per LESSONS PINNED P5.
- ADR 0045 — Dictation programmatic start/stop (Accepted 2026-05-27). Amends ADR 0037 §4: the `NoProgrammaticStart` rule is removed for Dictation; the kind now supports two start modes — Right Alt PTT (UNCHANGED) and programmatic via `dictation_start` / `dictation_stop` IPC. Both modes drive the same `HotkeyStateMachine` via a sentinel VK (`0x07`) so the FSM, orchestrator, and `dictation:state` event stream are mode-agnostic. CC Dictation tile now lands on `ShowingSessionCard{Dictation}` (closes the silent-dismiss gap `mb-ytex`). New `<DictationRecordButton>` above the search input on the Dictations page. Shipped as bead `mb-ddfx` (commit `b313742`); no new tag, Phase 10 seal unchanged. **Follow-up beads `mb-tfyp` + `mb-sowc` (2026-05-27):** added `sessions.start_mode` column (migration 017, `'ptt'` / `'in_app'`) so the in-app start path no longer incorrectly produces `ABORTED_FOCUS_CHANGED` session rows. UI list-pill now renders `IN_APP` (neutral) for programmatic sessions; detail panel shows "Push-to-talk" vs "In-app" next to the mode. Recording-pill overlay gains a primary Stop button only when `startMode === 'in_app'` (PTT pill unchanged — zero regression). Plumbed via `dictation:state` event payload (new optional `startMode` field). New `InjectionOutcome::InAppNoInject` variant (db str `"in_app"`) replaces the abort path for in-app sessions — same observable result (no paste), cleaner semantics.
- **Design System v1** — bead-only lateral epic (`mb-n455`, sealed 2026-05-26). Glass-tier semantic tokens (`--surface-glass-strong/soft/faint`), `--glass-blur-cap` (12px), canonical sticky-sidebar scroll convention (single-page scroller + `scrollbar-gutter: stable`), outline-button glass-faint default fill, full `100vh` → `100dvh` sweep, native form-control polish (themed range pill + custom select chevron + dark-pill retention inputs), Activity-page dead-token legacy bridge. 8/8 P1 + 9/12 P2 baseline-audit findings resolved (3 false-positives). 14 modified CSS files; no Rust changes. Baseline + final audits at `docs/audits/2026-05-26-design-v1-{baseline,final}/REPORT.md`. Conventions at `docs/design/conventions.md`. No ADR — work was token + CSS refinement, not architectural.

If a kickoff prompt asks you to re-execute any of the above, **STOP** and surface
the conflict before any tool call. See `.code_puppy/AGENTS.md` § "Permanently sealed".

## 🟢 Currently active

**KG Phase 1E (Obsidian as source of truth, reframed as Personal Knowledge
Engine substrate per ADR 0054) — Waves 1E.0 + 1E.1 + 1E.2 + 1E.3 + 1E.4 +
1E.5 + entity/project amendment + two hotfixes + Alignment Wave +
**1E.7 implementation (part 1: substrate + worker wiring) shipped** —
Waves 1E.6 (KG-Inbox courier) and 1E.7 refactor follow-up (worker split
under 600 LoC) unblocked; 1E.8 (iOS Shortcut docs) docs-only available
any time; 1E.9 seals both ADR 0053 + ADR 0054 together.**

### Wave 1E.7 implementation deliverables shipped (this iteration; mb-bgpt PART 1 — substrate + worker wiring)

Implements the bulk of ADR 0054 §C/§D/§E/§F/§G end-to-end. **mb-bgpt
remains OPEN** because gate #4 (worker refactor under 600 LoC, closes
`mb-5lla`) is unmet — `kg/worker.rs` is at 1669 LoC after wiring phases
5a/5b/5c and `vault/kg_layout.rs` is at 698 LoC. Both are over the hard
cap; refactor handled in a follow-up iteration so this checkpoint can
seal a known-good build for Dustin's smoke verification.

- **`vault::schema_md`** (310 LoC, prior 1E.7 session) — SCHEMA.md
  write-once user-owned renderer per ADR 0054 §C. Auto-generated on
  bootstrap; documents folder structure, page templates, frontmatter
  conventions, INDEX/LOG format specs, three-operation chat-LLM
  workflow (Ingest / Query / Lint), user-preferences seed defaults.
- **`vault::index_md`** (439 LoC main + 307 LoC sibling tests file) —
  per ADR 0054 §D. Full rebuild from DB after every filing (atomic
  temp-sibling rename). Five H2 sections (Sources / Entities / Projects
  / Tags / Concepts). Concepts section preserved verbatim across
  rebuilds (file-wins-against-chat-LLM contract). Byte-deterministic
  given fixed snapshot. **Tests split to `index_md_tests.rs` via
  `#[path]` include this iteration** to fit under 600-LoC cap; pattern
  mirrors existing `entity_pages` ⇄ `entity_pages_tests` precedent.
- **`vault::log_md`** (318 LoC, this iteration) — per ADR 0054 §E.
  Append-only operations log; format `## [YYYY-MM-DD HH:MM] kind |
  subject`. `LogOp` enum (Capture / Ingest / Query / Lint); Mockingbird
  only ever appends Capture. Atomic full-file rewrite (parity with
  every other vault writer; matches `.mb-tmp` suffix convention).
  Pipe-escapes + newline-folds subjects so malformed input can't
  corrupt line grammar. Recovers when LOG.md was deleted mid-life
  (next append re-seeds the header). 7 unit tests.
- **`vault::kg_layout`** (698 LoC; prior 1E.7 session expanded to 6
  folders + root-files bootstrap; **OVER 600-LoC cap, split deferred
  to `mb-5lla` follow-up**). New: `KgSubtreePaths.tags`,
  `bootstrap_kg_root_files` for SCHEMA/INDEX/LOG (write-once-if-missing
  for SCHEMA + LOG; minimal skeleton for INDEX so worker can rebuild
  cleanly on first filing).
- **`vault::entity_pages::ensure_tag_page`** (prior 1E.7 session) —
  tag stub generation mirroring entity/project stub pattern;
  Dataview rollup body keys on entry-frontmatter `tags:` array.
- **`vault::markdown_serializer` + `markdown_parser` type vocab
  realignment** (prior 1E.7 session) — serializer write path commits
  to nine knowledge shapes (`source` / `note` / `concept` / `entity` /
  `project` / `question` / `decision` / `reference` / `observation`);
  parser tolerates legacy `task` / `event` values by re-classifying as
  `note` on read. New `knowledge_shape_diversity.md` golden covers
  question / decision / reference / observation diversity.
- **`kg/worker.rs` phases 5a / 5b / 5c wired** (this iteration) —
  three new sequential post-seal phases after the existing 4b stubs.
  - **5a tag-stub generation**: unions `result.entries[*].topic_tags`
    into a `BTreeSet<String>` and calls `ensure_tag_page` per slug.
    Non-fatal per slug; gated on `vault_outcome.is_some()`.
  - **5b INDEX.md rebuild**: `rebuild_index_md(&conn, &vault_root)`.
    Non-fatal; always runs (rebuild is idempotent so an in-flight
    `vault_outcome=None` simply rewrites the prior bytes).
  - **5c LOG.md append**: `append_log_line(LogOp::Capture)` with the
    subject derived from the vault-relative filename slug (between
    date prefix and `__id8` suffix). Gated on `vault_outcome.is_some()`.
- **`vault/mod.rs`** — three new public modules declared
  (`index_md` / `log_md` / `schema_md`); without this declaration the
  prior 1E.7 session's `kg_layout` references would have left the build
  broken (the orphan check at this session's start confirmed exactly
  this).
- **Acceptance gates this iteration:**
  - `cargo check` (via wrapper) — GREEN, zero warnings.
  - `cargo clippy --release -- -D warnings` — GREEN.
  - `cargo fmt --check` — GREEN.
  - `cargo test --release --no-run` — GREEN (24m 47s; all 17 test
    executables linked cleanly). Live test exec deferred per
    LESSONS PINNED P-test-runner-block (sanctioned fallback).
  - `cargo build --release` — IN FLIGHT at commit time (PINNED P13;
    Dustin to verify fresh `target/release/mockingbird.exe` mtime).
- **Gates NOT met / deferred (block `bd close mb-bgpt`):**
  - Gate #4 (worker refactor closes `mb-5lla`): worker.rs at 1669 LoC
    (cap 600). The wave brief mandates the cohesive split into
    `worker/{filing,projection,archive,stubs,index_log}.rs` modules;
    that's a multi-session refactor on its own and is best done as a
    pure rearrangement separate from this functional landing.
  - `vault/kg_layout.rs` at 698 LoC (cap 600). The §C/§D/§E bootstrap
    + per-root-file render helpers want to migrate into the new
    `vault::index_md` / `vault::log_md` / `vault::schema_md` sibling
    modules. Same follow-up as worker split.
  - kg_parity 32/32, kg_source_gate 6/6, kg_graph_off 8/8 re-runs —
    require live Ollama; deferred to a dedicated invariant-gate iteration.
  - Dustin smoke verification (relaunch → vault root files present →
    fresh capture → INDEX rebuilds + LOG appends + tag pages appear →
    chat-LLM reads SCHEMA.md).

**Resume next iteration:** two parallel followups —
(a) **worker.rs + kg_layout.rs refactor** under 600-LoC cap (closes
`mb-5lla`; unblocks `bd close mb-bgpt`); (b) **1E.9 judges + dual ADR
seal** (`mb-kazi`) once Dustin smoke is GREEN. Wave 1E.6 (KG-Inbox
courier, `mb-i46v`) and 1E.8 (iOS Shortcut docs, `mb-wnsm`) remain
unblocked and orthogonal. Dispatch one-liner pattern per LESSONS P8:
`refactor kg/worker.rs into worker/{filing,projection,archive,stubs,
index_log}.rs submodules per docs/phases/phase-1e.md Amendment
2026-06-06 #2 + ADR 0054, under 600-LoC cap; do the same shape for
vault/kg_layout.rs`.

### Prior in-flight (now historical): Alignment Wave + Wave 1E.5

### Alignment Wave deliverables shipped (prior iteration; mb-rik9; zero code changes)

- **ADR 0054** (Personal Knowledge Engine substrate, Proposed) — 15
  sections (§A three-layer architecture / §B two-agent role separation
  / §C SCHEMA.md / §D INDEX.md / §E LOG.md / §F Tags subtree / §G type
  vocab realignment / §H chat-LLM operations / §I Mockingbird
  operations / §J Phase 1E wave rescope / §K Phase 2 scope / §L
  supersedes / §M references / §N consequences / §O open questions).
  Lineage: Vannevar Bush Memex (1945) → Karpathy LLM Wiki gist →
  Alvin Clark *Building a Personal Knowledge Engine with LLMs and
  Obsidian* (April 2026).
- **ADR 0053 amendment** — supersession-pointer block appended:
  §D3 (type vocab) partial; §D8 (Kanban + Dashboard seeds) partial;
  §D9 (Tasks-plugin checkbox default-on) de-emphasized to opt-in.
  Sections that carry forward unchanged enumerated explicitly
  (D1/D2/D3-shape/D4/D5/D6/D7/D10/D11/D12).
- **`docs/phases/phase-1e.md` Amendment 2026-06-06 #2** — Wave 1E.6
  unchanged; Wave 1E.7 RESCOPED (drop Kanban; add SCHEMA/INDEX/LOG/
  Tags/type-vocab); Wave 1E.8 framing updated (Shortcut delivers to
  Inbox/ Layer 1; chat-LLM crystallizes to Layer 2); Wave 1E.9
  judges semantic shift from task semantics to knowledge-link
  semantics.
- **LESSONS PINNED P14** added (above P13) — "The Karpathy/Clark
  North Star" — two-agent role separation, three-layer architecture,
  chat-LLM owns Ingest/Query/Lint, nine knowledge shapes, anti-patterns.
- **`docs/PRODUCT-STATE.md`** — snapshot date bumped to 2026-06-06;
  top "What Mockingbird is" rewritten to name the three layers
  (dictation / meeting capture / KG capture+first-pass synthesis
  layer); architecture map expanded to include the KG capture branch +
  Obsidian vault layout + chat-LLM consumer; new **§3.20 Knowledge
  Graph / Personal Knowledge Engine substrate** section covering
  ships-today / upcoming / role-separation / vocab / lineage / ADR
  trail.
- **`.code_puppy/AGENTS.md`** — top "Project context" paragraph
  expanded: Mockingbird is dictation+meeting-capture AND the capture
  layer for the Karpathy/Clark Personal Knowledge Engine pattern;
  chat-LLM = wiki author / Obsidian = IDE / vault = knowledge
  codebase / SCHEMA.md = portable contract.
- **Bead descriptions updated:** `mb-82h6` (knowledge-shape vocabulary
  in tag extractor few-shots, not "small note"), `mb-ifun` (entity
  extraction quality affects KG navigation), `mb-xqcf` (type
  classification for the nine knowledge shapes, not task/note
  binary). **Bead `mb-il83` CLOSED** — vocab drift resolved by
  ADR 0054 §G realignment.
- **New follow-up beads filed:** UI copy relabel pass; pipeline
  prompt realignment (classify + entity extraction few-shots);
  Phase 2 charter draft; `spec.md` realignment chore.
  `PLAN-mockingbird-v2.md` spot-check: **clean** (zero KG / kanban /
  task-management mentions — KG epic happened entirely as lateral
  ADR-chartered work; no follow-up bead needed).
  `docs/knowledge-graph/spec.md` spot-check: **task framing
  identified at §3 (TaskForge/Tasks plugin context), §7.2 (Layer 2
  type includes `task`), §7.4 (entire "Tasks use Obsidian Tasks
  format" section), §11 ("tasks actionable as native checkboxes"),
  §14 (Kanban + dashboard contract), §14.4 (dependency-aware boards
  framing), §15.3 (light insights "open tasks"), Appendix
  Sequencing summary** — filed as P2 bead for in-place rewrite
  follow-up; ADR 0054 supersedes by reference in the interim.
- **NO code changes anywhere** — `git diff --stat` shows only `.md`
  files modified/created. No cargo gate run (not applicable to
  docs-only).

**Resume next iteration:** `bd ready -t task` surfaces 1E.6
(`mb-i46v`) and 1E.7 (`mb-bgpt`, NOW RESCOPED — see Amendment
2026-06-06 #2 in `docs/phases/phase-1e.md` + ADR 0054 §J) as the
unblocked parallel set. 1E.7 author MUST read ADR 0054 §C/§D/§E/§F/§G
for the SCHEMA / INDEX / LOG / Tags / type-vocab contracts before
implementation. 1E.8 docs-only bead available any time. Dispatch
one-liner pattern per LESSONS P8: `implement Wave 1E.7 per
docs/phases/phase-1e.md Amendment 2026-06-06 #2 and ADR 0054`.

### Prior in-flight (now historical, deeper): KG Phase 1E Wave 1E.5 shipped (Obsidian as source of truth) — Waves 1E.0 + 1E.1 + 1E.2 + 1E.3 + 1E.4 + 1E.5 shipped + hotfix; Waves 1E.6 (KG-Inbox courier) and 1E.7 (seeds) unblocked in parallel. Charter ADR [ADR 0053](docs/adr/0053-kg-phase-1e-obsidian-as-source-of-truth.md) Proposed; phase doc at `docs/phases/phase-1e.md`; bead epic `mb-imc2` with 10 sub-beads. 1E.5 lands the reverse-watcher: Obsidian edits on `<vault>/Knowledge Graph/Entries/*.md` reconcile back into SQLite within ~3s, with SHA-256 hash-based loop-prevention against Mockingbird's own writes (`sessions.vault_file_hash` from migration 026). Entity/project pages, History sidecars, and Inbox audio are all routed to IGNORED. Wiki-link entities now emit with Obsidian's pipe-alias display form (`[[Entities/<slug>|<slug>]]`); the new parser strips the alias when extracting the slug for DB write, and tolerates both pipe-alias and legacy bare wiki-link forms.

### Wave 1E.5 deliverables shipped (this iteration)

- `src-tauri/src/vault/watcher.rs` (413 LoC) — async runtime: `ReverseWatcherRuntime` + manager thread that polls `KgGraphEnabled` + `VaultPath` every 3s and constructs an inner `notify-debouncer-full` watcher (2s quiet window, ADR 0053 §D5) only when both gates are live. Disabled-by-default installs pay zero I/O. Toggle on/off + vault-path change is reconciled within ~3s without app relaunch. Mirrors `inbox::runtime::InboxRuntime` shape.
- `src-tauri/src/vault/watcher_reconcile.rs` (401 LoC) — pure file→DB reconciler split out for the 600-line cap (see LESSONS 2026-06-06 entry). Public surface: `ReconcileOutcome`, `PathClass`, `classify_path`, `reconcile_entry_file`. Six-step reconcile pipeline (path-class filter → bytes-read → UTF-8 → parse YAML → session lookup → hash-loop-check → mention-row delete-and-reinsert in one txn + hash refresh). All failure modes are `tracing::warn!`-logged + skipped; watcher stays alive through bad input. 10 classify-path unit tests + integration via the J1 loop-prevention judge (1E.9 next iteration).
- `src-tauri/src/vault/markdown_parser.rs` (594 LoC, under cap) — reverse of the 1E.2 serializer; symmetric round-trip. Public surface: `parse_entry(content) -> Result<ParsedEntry, ParseError>` + `parse_entity_slug_from_wiki_link(raw) -> String`. CRLF tolerance, vocab validation (capture_kind / category / type / status), RFC-3339 timestamp parsing, Obsidian Tasks checkbox round-trip (`status` from YAML can be overridden by `- [x]` body line per ADR 0053 §D9). Tolerates three entity-encoding forms: pipe-alias `[[Entities/<slug>|<alias>]]`, bare `[[Entities/<slug>]]`, and legacy raw slug.
- `src-tauri/src/vault/markdown_serializer.rs` — retrofit: `write_entity_wiki_link_list` now emits `"[[Entities/<slug>|<slug>]]"` per the wiki-link alias polish. Three goldens updated + LF-normalized (`full_task.md`, `special_chars.md`, `wiki_linked_entities.md` — CRLF crept back in via `cp_create_file` per LESSONS 2026-06-05 Finding 1, fixed at gate time). Round-trip golden tests in serializer-tests + parser-tests cross-verify byte stability.
- `src-tauri/src/lib.rs` — `ReverseWatcherRuntime::spawn(shared_conn)` wired alongside the KG filing worker; manager thread + inner watcher both spawn at app boot but gate internally.
- All gates green: `fmt --check`, `clippy --release -- -D warnings`, `check`, `test --release --no-run --lib` (link-clean in 4m 20s). UI: `tsc --noEmit`, `npm test` 143/143, `npm run build`. **Invariant probes:** `kg_parity` 32/32 GREEN, `kg_source_gate_invariant` 6/6 GREEN, `kg_graph_off_invariant` 8/8 + controls GREEN — no regression. **`cargo build --release` mandatory (PINNED P13; lib.rs + worker-spawn surface touched): fresh `target\release\mockingbird.exe` rebuilt, 53.5 MB, mtime post-seal.**
- 3 P2/P3 pipeline-quality beads filed for the post-1E quality sprint (out of scope here): `mb-82h6` (P2 bug, tag extractor over-fires/hallucinates on small notes), `mb-ifun` (P3 bug, entity extraction inconsistent across phrasings), `mb-xqcf` (P3 feature, type classification for action-item-containing notes).

**Resume next iteration:** `bd ready -t task` surfaces 1E.6 (`mb-i46v`, KG-Inbox courier) and 1E.7 (`mb-bgpt`, seeds) as the unblocked parallel set. 1E.9 (`mb-kazi`, Phase 1E judges J1-J4 + ADR 0053 seal) now has its primary dependency (1E.5 hash-based dedup ledger) satisfied. 1E.8 docs-only bead is available any time. Dispatch one-liner pattern per LESSONS P8: `implement Wave 1E.6 per docs/phases/phase-1e.md`.



### Wave 1E.4 deliverables shipped (last iteration)

- `src-tauri/src/vault/history.rs` — new module (586 LoC, under cap). Phase-4 archive step (post-seal, post-mark-done; non-fatal-to-queue, mirroring Wave 1E.3's vault projection philosophy). Public surface: `archive_session_history(input, vault_root)` writes the canonical JSON sidecar via `serde_json::to_string_pretty` (LF-only by contract; see LESSONS 2026-06-05 Finding 2) + appends one trailing newline + atomic temp-sibling-rename, then moves the source audio recording to `History/<YYYY-MM>/<session-uuid>.<ext>` via rename-then-copy-then-delete (cross-volume safe). Idempotency anchor is JSON-sidecar existence; audio-move is conditional on (src exists ∧ target missing). `reconcile_history` is the on-demand scan surface — counts sealed-but-not-archived sessions + orphan sidecars; read-only (never deletes). `ARCHIVE_VERSION=1` discriminator + `.json` extension are the 1E.5 reverse-watcher's "history blob, not user content" markers per ADR 0053 §D7.
- `src-tauri/src/vault/history_tests.rs` — sibling test file (682 LoC; same `#[cfg(test)] #[path]` convention as `markdown_serializer_tests.rs` to keep impl under cap). 35 unit tests: month-bucket parsing edge cases (RFC 3339, offset TZs converted-to-UTC, milliseconds, garbage-rejection); pure serialization (field-order pin, LF-only invariant, single trailing newline, archive_version=1, embedded-newline escape); archive happy paths (audio-with-move; text-only no-audio); idempotency on re-snap; on-demand month-bucket creation; month-boundary crossing across two buckets; failure modes (history root blocked by a file, unparseable captured_at, audio source missing at archive time); reconcile scan (sealed-but-no-sidecar, sidecar-matches-session clean, orphan-sidecar flag, unsealed-ignored, unparseable started_at graceful); 3 golden-file tests.
- `src-tauri/tests/fixtures/history_golden/` — 3 LF-normalized canonical JSON fixtures + README documenting the regeneration workflow: `kg_note_with_audio.json` (full audio capture with both transcripts populated), `kg_note_text.json` (text-only capture, identical raw + cleaned per `kg::ingest_text`), `kg_note_sparse.json` (audio capture with empty `cleaned_transcript`, proving empty strings emit-not-omit). LESSONS 2026-06-05 Finding 1: file-creation tool produces CRLF on Windows; explicit LF-rewrite required for first-run byte-identity.
- `src-tauri/src/kg/worker.rs` — new `maybe_archive_history` helper (~110 LoC) called after the seal + `mark_done` transaction commits. Snapshots session (uuid, capture_kind, audio_blob_path) + transcripts (raw + final/cleaned cascade) under one short-lived mutex acquire, derives entry_filename from `outcome.vault_relative_path`, calls `vault::history::archive_session_history`. Failure is logged + swallowed (kg_filing_queue stays clean; `reconcile_history` recovers on demand). Worker file now 1261 LoC — filed `mb-5lla` (P3) to investigate a cohesive split.
- All gates green: `fmt --check`, `clippy --release -- -D warnings`, `check`, `test --release --no-run --lib` (link-clean in 4m 11s). UI: `tsc --noEmit`, `npm test` 143/143. **Invariant probes:** `kg_parity` 32/32 GREEN, `kg_source_gate_invariant` 6/6 GREEN, `kg_graph_off_invariant` 8/8 + controls GREEN — no regression from the new phase-4 step. **Live tests:** 35/35 via the LESSONS P2 throwaway-crate recipe (`vault::history` is pure-Rust; no whisper-rs/ort/cuda deps).
- 1 P3 bead filed: `mb-5lla` (kg worker.rs at 1261 LoC; investigate cohesive split).

**Resume next iteration:** `bd ready -t task` surfaces 1E.5 (`mb-qwfy`, reverse-watcher), 1E.6 (`mb-i46v`, KG-Inbox courier), and 1E.7 (`mb-bgpt`, seeds) as the unblocked parallel set — each depends only on 1E.3 (now closed). 1E.5 is the highest-leverage next pick because the J1 loop-prevention invariant judge (1E.9) blocks on its hash-based dedup ledger landing. **1E.5 author MUST read the new ADR 0053 "Amendments" section + the Wave 1E.5 "Amendment 2026-06-06" callout in `docs/phases/phase-1e.md`** — the reverse-watcher's wiki-link parser + entity-page-edit handling come from the amendment, not the original §D5. 1E.8 docs-only bead is available any time. Dispatch one-liner pattern per LESSONS P8: `implement Wave 1E.5 per docs/phases/phase-1e.md`.

### Wave 1E hotfix — 2026-06-06 (reconcile IPCs + fresh release binary)

**Trigger:** Dustin smoke-tested 1E.4 end-to-end and discovered `Knowledge
Graph/Entries/` was empty despite the dashboard's Done counter
incrementing. SQL probe against the live DB (`SELECT entry_id FROM
sessions ...`) returned `no such column: entry_id` → migration 026
never ran → the running `target/release/mockingbird.exe` was pre-1E.3.
Root cause: Waves 1E.3 + 1E.4 sealed on the `test --release --no-run`
fallback (correct for *gating* link validity per LESSONS P2) but never
produced a fresh runtime exe. **Promoted to LESSONS PINNED P13 +
AGENTS.md end-of-iteration checklist.**

Shipped:

- Rebuilt `target/release/mockingbird.exe` (6m 47s; mtime 2026-05-31
  04:42, well post-1E.4 seal commit). Dustin relaunches manually.
- `commands/kg.rs` — promoted `kg_reconcile_vault` (closes `mb-43xw`)
  and `kg_reconcile_history` (new sibling, no pre-existing bead) from
  store-layer deferred work to live `#[tauri::command]` IPC, both
  KG-toggle + vault-configured gated via a shared
  `resolve_vault_root_for_reconcile` helper (DRY in the small).
  `ReconcileReport` + `HistoryReconcileReport` gained `#[derive(Serialize)]`
  + `#[serde(rename_all = "camelCase")]` for the wire contract; pinned
  by two regression tests in `commands/kg.rs::tests`.
- `commands/mod.rs` — both commands registered in `generate_handler!`.
- `ui/src/routes/knowledge-graph/Actions.tsx` — new "Reconcile vault"
  button on the `ActionsBand` (right beside the 1D.5 "Open vault in
  Obsidian" button). Disabled-with-tooltip when KG toggle is off OR
  vault is unconfigured; runs both reconcile IPCs sequentially on
  click (not `Promise.all` — identical gates ⇒ parallel race produces
  duplicate error toasts) and renders a single combined drift banner.
- `ui/src/lib/{tauri,types}.ts` + `i18n/en.json` — typed `api.*`
  bindings + browser-fixture defaults (zero-drift) + i18n strings.
- All gates green: `fmt --check`, `clippy --release -D warnings`,
  `check`, `test --release --no-run --lib` (link surface clean,
  4m 17s), `cargo build --release` (6m 47s, fresh exe). UI:
  `tsc --noEmit`, `npm test` 143/143, `npm run build` clean.
- 1 P3 bead closed: `mb-43xw`. `mb-srvh` (timer-driven sweep) stays
  open as planned.

### Wave 1E charter amendment — 2026-06-06 (entity+project pages + wiki-link entities, `mb-08za` / ADR 0053 amendments)

**Trigger:** Dustin opened the live `Knowledge Graph/` vault in Obsidian
after 1E.4 + the two hotfixes and reported that entities rendered as
bare untyped text — no per-entity Obsidian pages, no clickable
wiki-links, no graph-view edges, no "all entries mentioning Maple"
view. The KG side felt flat compared to the Dictations side. Charter
amendment shipped as an interstitial wave between 1E.4 and 1E.5,
before the reverse-watcher locks in the bare-string shape on the
inbound side.

Shipped (one iteration, one commit reference):

- **ADR 0053 amendments appended** (`docs/adr/0053-...md`, new
  `## Amendments` section): §D1 expands subtree to 5 folders
  (`Inbox`, `Entries`, `History`, `Entities`, `Projects`); §D3 swaps
  `entities:` emission from bare strings to
  `"[[Entities/<slug>]]"` wiki-links (deduped by slug at serialize
  time); new §D11 (Entity pages) + §D12 (Project pages) define the
  auto-generated stub-page contract — write-once,
  user-owns-thereafter, atomic temp-sibling-rename, canonical-LF
  bytes, Dataview body filtering on the same `entities` field.
- **Phase 1E doc updated** (`docs/phases/phase-1e.md`):
  Wave 1E.5 "Amendment 2026-06-06" callout absorbs the
  five-folder + wiki-link parsing + entity/project-page-edits-don't-
  trigger-reverse-sync deltas. Wave 1E.7 callout tightens scope to
  the root-level seeds (Dashboard / Kanban / README) and notes the
  per-entity / per-project pages are continuous, not one-shot.
- `src-tauri/src/vault/kg_layout.rs` — expanded `KgSubtreePaths` +
  `bootstrap_kg_subtree` to include `Entities/` and `Projects/`.
  New tests pin pre-amendment three-folder → five-folder upgrade
  path. Existing tests still green.
- `src-tauri/src/vault/markdown_serializer.rs` — new
  `write_entity_wiki_link_list` helper replaces the bare
  `write_string_list` call for entities. Slug derivation shared
  with the filename `slugify_title`. Round-trip contract for 1E.5
  documented in module docs (parser accepts BOTH legacy bare-string
  AND new wiki-link shape; writes ALWAYS emit canonical wiki-link
  form). 5 existing golden fixtures updated; new
  `wiki_linked_entities.md` golden pins the dedupe + wiki-link
  emission shape. 4 new unit tests + 2 new property tests cover the
  regex `^\[\[Entities/[a-z0-9-]+\]\]$` + dedupe-by-slug invariant.
- `src-tauri/src/vault/entity_pages.rs` — NEW module (269 LoC). Public
  surface: `ensure_entity_page` + `ensure_project_page` (both take
  `&Path`, pre-slugified `&str`, `DateTime<Utc>`; return
  `StubPageReport::{Created,AlreadyExists}`; never overwrite an
  existing file). Internal: slug validator (defense-in-depth
  ASCII-kebab-case re-check), canonical-form stub renderer (LF only,
  pinned frontmatter field order, Dataview body), atomic write via
  `.mb-tmp` sibling rename.
- `src-tauri/src/vault/entity_pages_tests.rs` — sibling test file (257 LoC;
  `#[cfg(test)] #[path]` to stay under cap). 19 tests: slug validation
  matrix (empty / uppercase / path-traversal / slashes /
  leading-trailing-hyphen / overlong / valid kebab); entity-page
  write-once contract (writes when missing, no-ops when present,
  byte-identical user-content preservation, idempotency on
  back-to-back calls, LF-only canonical bytes, parent-dir
  defensive create); project-page parallel coverage + status field;
  entity + project stubs are independent files for the same slug;
  render-pure unit tests (frontmatter field order, type discrimination,
  Dataview WHERE clause pin).
- `src-tauri/src/kg/worker.rs` — new `maybe_generate_stub_pages` helper
  (~135 LoC) called after the seal + history-archive transaction
  commits. Aggregates `(slug, is_project)` across all
  `result.segment_entities`, dedupes by slug (BTreeMap for
  deterministic order), iterates: every slug gets an entity stub;
  Project-typed slugs ALSO get a project stub. Each call is
  independently non-fatal (`tracing::warn!` on error, continue).
  Worker file now 1409 LoC — `mb-5lla` (cohesive-split investigation,
  already filed last iteration) covers the refactor backlog.
- 2 P3 beads filed: `mb-zvv3` (entity slug disambiguation + merge UX,
  post-v1) and `mb-3hyp` (delete-entry UI affordance on KG screen,
  Dustin-surfaced gap).
- All gates green: `fmt --check`, `clippy --release -- -D warnings`,
  `check`, `test --release --no-run` (link clean in 4m 38s),
  `cargo build --release` (PINNED P13 — mandatory because this
  wave touched the worker pipeline; fresh `target/release/mockingbird.exe`
  mtime post-seal). UI: `tsc --noEmit`, `npm test` 143/143,
  `npm run build` clean. **Live tests:** 19/19 entity_pages tests
  via throwaway-crate recipe (LESSONS P2; `entity_pages` is pure-Rust).
  Golden suite extended by 1 fixture (`wiki_linked_entities.md`).
  Invariant probes deferred to next live capture — the amendment is
  additive (new module + new fields; no behaviour change for
  graph-off path or source-gate path).
- **Backfill deferred** per kickoff: Dustin's ~4 pre-amendment test
  entries stay in bare-string form. New entries get wiki-links;
  Dataview queries can defensively `OR` both shapes; automatic
  backfill is Phase 1F (or later) work. LESSONS entry covers the
  three findings: (1) write-once user-owns-thereafter is the only
  sane contract for auto-generated user-facing artifacts;
  (2) asymmetric wiki-link round-trip is correct and on-purpose;
  (3) deferring backfill when the user-cost is in the noise is the
  right call.

### Wave 1E hotfix #2 — 2026-06-06 (vault entry body truncation, `mb-wzui`)

**Trigger:** Dustin captured a 3-bullet grocery list; the Dictations view
showed the full bulleted cleaned transcript, but the Obsidian entry body
carried only `"... I need to get: feta cheese"` — eggs + milk vanished
(though `entities:` frontmatter listed all three). Symptom traced to
`kg::worker::maybe_commit_to_vault` writing `result.entries[0].body` to
the vault `KgEntry`; `entries[0].body` is the *segmenter's first
semantic segment* (`kg::pipeline::segment` is a reformulating chunker,
NOT a substring slicer), so any multi-bullet / multi-fact note silently
dropped segments 1..N from the vault projection.

Shipped:

- `src-tauri/src/kg/worker.rs` — `maybe_commit_to_vault` snapshot now
  includes the cleaned transcript via `load_dictation_text` (same
  final → cleaned → raw cascade the Dictations view + 1E.4 history
  archive use). New free function `pick_vault_body(transcript,
  fallback_segment)` selects body bytes: prefer non-empty trimmed
  transcript, fall back to `primary.body` only when no transcript row
  exists (defensive — KG captures always write transcripts before
  enqueueing). 4 new worker-module tests pin the helper.
- `src-tauri/src/vault/markdown_serializer_tests.rs` —
  `body_preserves_markdown_bullet_list_verbatim` regression test
  pins the serializer half of the invariant ("don't mangle bullets I
  give you"); belt-and-suspenders with the worker test ("pass the
  right body in the first place").
- All gates green: `fmt --check`, `clippy --release -- -D warnings`,
  `check`, `test --release --no-run --lib` (link clean), `cargo build
  --release` (fresh exe per PINNED P13; mtime post-fix). UI:
  `tsc --noEmit`, `npm test` 143/143, `npm run build` clean.
- **Orphan recovery:** prior session had already authored the worker
  fix (helper + snapshot extension + 4 tests) but crashed before
  commit; surfaced via `git status --porcelain=v1` as first tool call.
  Combined commit references `mb-wzui`.
- 1 P1 bead closed: `mb-wzui`. Title-quality follow-up ("Make a
  grocery list for feta cheese" omits eggs + milk) deferred — likely
  same root cause (title-generator was likely fed segment[0] too, or
  it's an LLM judgment call; not investigated this iteration).
  **Backfill of the existing broken entry:** Dustin to delete + re-
  capture; no automated force-regenerate this wave per kickoff Step 4.

### Wave 1E.3 deliverables shipped (last iteration)

- `src-tauri/src/db/migrations/026_kg_vault_linkage.sql` — additive migration. Three new TEXT columns on `sessions`: `entry_id` (KG entry UUID, sealed post-write), `vault_path` (vault-relative POSIX path), `vault_file_hash` (lowercase hex SHA-256, pre-recorded BEFORE the file write per ADR 0053 §D5). No index this wave; 1E.5 may add `idx_sessions_vault_path` when the reverse-watcher needs the event→session lookup. Pre-migration backup taken to `%APPDATA%\com.dustin.mockingbird\backup-pre-mb-k2pk\mockingbird.db`. Migrations ladder test bumped expected version 24 → 26.
- `src-tauri/src/vault/writer.rs` — new module (~600 LoC incl. tests). `commit_entry_to_vault` runs ADR 0053 §D4 steps 2-4 (pre-hash + DB-record-hash + atomic file write via temp-sibling + rename); seal (step 5) returns to the caller so it can fold seal + `mark_done` (step 6) into one transaction. `reconcile_vault` is the on-demand orphan-recovery surface (IPC wiring deferred to `mb-43xw`; timer-driven sweep deferred to `mb-srvh`). 9 unit tests cover the happy path, idempotency, both file-write failure modes (pre-hash and post-hash), reconcile against three drift signatures (orphan file matching hash, hash recorded with no file, untracked file in the entries dir), and a Tasks-checkbox round-trip.
- `src-tauri/src/db/sessions.rs` — Session struct + SELECT_ALL extended with the three new columns; `record_vault_hash` and `seal_vault_filing` helpers (the two write surfaces of the two-phase commit).
- `src-tauri/src/kg/worker.rs` — `process_one` split into three sequential transactions: (1) `apply_filed_outcome` (existing) commits the kg_* rows; (2) `maybe_commit_to_vault` does the file write outside any DB txn; (3) seal + `mark_done` commit together. Vault projection failure is logged but **non-fatal to queue** (mb-k2pk Finding 2 in LESSONS: decoupled retry budgets). Five new helpers: `kg_category_to_vault`, `kg_status_to_vault`, `kg_entry_type_to_vault` (lossy Research/Reference → Note; filed mb-il83), `parse_iso_to_utc[_opt]`, `parse_iso_to_local_date`.
- All gates green: `fmt --check`, `clippy --release -- -D warnings`, `check`, `test --release --no-run --lib` (link surface clean). UI: `tsc --noEmit`, `npm test` 143/143, `npm run build`. **Invariant probes:** `kg_parity --persist` 32/32 GREEN, `kg_source_gate_invariant` 6/6 GREEN, `kg_graph_off_invariant` GREEN. No prior gate regressed.
- 4 P3 beads filed for deferred work: `mb-il83` (vocab drift), `mb-ng1o` (multi-entry projection collapse), `mb-srvh` (timer-driven nightly sweep), `mb-43xw` (IPC for `kg_reconcile_vault`).

### Wave 1E.2 deliverables shipped (last iteration)

- `src-tauri/src/vault/markdown_serializer.rs` — new module (512 LoC impl, under the 600-line cap). Pure transformation: `KgEntry -> (filename, bytes)`. No I/O, no DB, no clock. Hand-rolled YAML emitter (not `serde_yaml`) for byte-stable output: deterministic field order, double-quoted strings even when YAML would let us bare them, block-style non-empty lists, flow-style empty lists (`tags: []`), LF line endings on every platform, RFC 3339 `Z` timestamps, conditional fields omitted (never `field: null`). YAML 1.2 §5.7 escape coverage: `\`, `"`, `\n`, `\r`, `\t`, NEL/LSEP/PSEP, plus every codepoint outside YAML's `c-printable` range (the libyaml reader's hard floor — pinned by `prop_frontmatter_is_valid_yaml`).
- `src-tauri/src/vault/markdown_serializer_tests.rs` — sibling test file (734 LoC; loaded via `#[cfg(test)] #[path]` so the impl stays under the file-cap). 41 tests total: 24 unit tests (filename slug edge cases, frontmatter field-order pin, conditional-field omission, empty-list rendering, block-list style, LF-only, escape rules, RFC 3339, wire-name discipline, Obsidian Tasks checkbox glyphs for todo/doing/done, due-date format on the checkbox line); 5 property tests via `proptest` (filename regex, frontmatter valid YAML, LF-only invariant, single trailing newline, slug invariants); 5 golden-file tests with on-disk fixtures + 1 golden-filename pin.
- `src-tauri/tests/fixtures/markdown_golden/` — 5 hand-crafted golden files + README. Cover: minimal (only required fields), full_task (every conditional field), doing_task (`- [/]` glyph), done_task (`- [x]` + 📅 date), special_chars (escape stress: `\` `"` `\t` `\n` + Unicode em-dash + café). Reproducible via `MOCKINGBIRD_UPDATE_GOLDENS=1 cargo test` — assert_golden resolves the fixtures path via `file!()` (not `env!("CARGO_MANIFEST_DIR")`) so the throwaway-crate harness writes back to the real `src-tauri/tests/...` dir, not the throwaway copy.
- Round-trip-safety documented inline in the impl module docs as a forward declaration for 1E.5 (reverse-watcher); every canonical-form choice (LF-only, quoted strings, block-vs-flow lists, omitted optionals) explicitly justified as a fixed-point for `serialize(parse(serialize(e)))`.
- All gates green: `fmt --check`, `clippy --release -- -D warnings`, `check`, `test --release --no-run --lib` (Finished release profile in 4m 24s, executable links clean). UI: `tsc --noEmit`. Live tests via LESSONS P2 throwaway-crate recipe: **41/41 pass.**

### Wave 1E.1 deliverables shipped (last iteration)

- `src-tauri/src/vault/kg_layout.rs` — new module (~330 LoC incl. tests). Pure-Rust `kg_subtree_paths` + `bootstrap_kg_subtree` helpers. `BootstrapReport` enum (`Created` / `AlreadyExists`) serialized camelCase. Seven unit tests cover all four idempotency cells from ADR 0053 §D1's contract table + partial-subtree completion + file-at-root error path + Windows-path-with-space discipline + wire-format serialization pin. All 7 verified live via the LESSONS P2 throwaway-crate recipe (cargo test runner blocked on this box).
- `src-tauri/src/commands/kg.rs` — new `kg_subtree_bootstrap` IPC. Reads `SettingKey::VaultPath` (ADR 0046 single source of truth), surfaces structured `Err(String)` for the vault-unconfigured / empty-path edge cases.
- `src-tauri/src/lib.rs` — boot-fire path. When both `KgGraphEnabled=true` AND `VaultPath` is set at app boot, fires `bootstrap_kg_subtree` best-effort with `tracing::info!` on success, `tracing::error!` on failure (non-fatal; the toggle-on IPC retries on the next user flip).
- `ui/src/pages/SettingsKgTab.tsx` — toggle-on flow now calls `api.kg_subtree_bootstrap()` after a successful `kg_settings_set`. New `bootstrapError` inline banner + `kg.settings.bootstrapError` i18n string. Toggle-off is a no-op (per ADR 0053 D1: user content lives in the subtree; never destructively cleaned).
- `ui/src/lib/{tauri.ts,types.ts}` — typed `KgBootstrapReport` union + fixture stub. UI test file gains 3 fixture-mode contract tests (143 vitest now, was 140).
- All gates green: `fmt --check`, `clippy --release -- -D warnings`, `test --release --no-run` (links clean — type system + traits + link surface all valid). UI: `tsc --noEmit`, `npm test`.

### Wave 1E.0 deliverables shipped (last iteration)

- `docs/adr/0053-kg-phase-1e-obsidian-as-source-of-truth.md` — Proposed (532 lines). Ten load-bearing decisions (D1–D10): vault subtree shape, filename format (`<date>-<slug>__<id8>.md`), YAML frontmatter shape + versioning, two-phase commit ordering (DB-first then file then DB-seal then queue-seal), reverse-watcher conflict resolution (file-wins; hash-based loop-prevention NOT mtime), KG-Inbox courier shape (sibling to ADR 0046; positional routing per Q2), History archive (per-session JSON sidecar; audio file move-out-of-Inbox), pre-built seeds, Obsidian Tasks emission, iOS Shortcut docs scope.
- `docs/phases/phase-1e.md` — 616 lines. Wave-by-wave: 1E.1 (subtree bootstrap), 1E.2 (Markdown serializer), 1E.3 (worker writes Markdown + migration 026), 1E.4 (History archive), 1E.5 (reverse-watcher; J1 invariant), 1E.6 (KG-Inbox courier), 1E.7 (seeds), 1E.8 (iOS Shortcut docs), 1E.9 (4 judges + seal).
- Bead epic `mb-imc2` + 10 sub-beads: 1E.0 `mb-nuba`, 1E.1 `mb-e16d`, 1E.2 `mb-vq8y`, 1E.3 `mb-k2pk`, 1E.4 `mb-i14b`, 1E.5 `mb-qwfy`, 1E.6 `mb-i46v`, 1E.7 `mb-bgpt`, 1E.8 `mb-wnsm`, 1E.9 `mb-kazi`. Dependency chain wired (`bd link` blocks-relationship for serial chain; `--type parent-child` for epic ↔ sub-task). Critical path: 1E.0 → 1E.1 → 1E.2 → 1E.3 → {1E.4, 1E.5, 1E.6, 1E.7} → 1E.9; 1E.8 docs-only (no code deps).
- This STATUS update.

**Resume next iteration:** `bd ready -t task` will surface 1E.4 (`mb-i14b`, History archive) as the lead actionable now that 1E.3 (`mb-k2pk`) is closed; 1E.5 (`mb-qwfy`, reverse-watcher) + 1E.6 (`mb-i46v`, KG-Inbox courier) + 1E.7 (`mb-bgpt`, seeds) unblock in parallel since each depends only on 1E.3. 1E.8 docs-only bead is available any time. Dispatch one-liner pattern per LESSONS P8: `implement Wave 1E.4 per docs/phases/phase-1e.md`.

**Standing beads carrying forward** (not blocking Phase 1E waves):

- `mb-bbl2` — sonner toast retrofit for native browser confirms.
- `mb-y6pq` — `--status-bad` design-token sweep.
- `mb-26aw` — `smoke.spec.ts` ×4 pre-1C Playwright failures.
- `mb-2wbk` — KG row → Dictations deep-link (P3, filed in 1D.4).
- `mb-0ui1` — vocab editor in Settings (P3, filed in 1D.5).




**KG Phase 0.5 narrative archived:** the full wave-by-wave in-flight
state that lived here through 0.5.1–0.5.5 is now in
`docs/knowledge-graph/PHASE-0-5-REPORT.md` §3 (scorecard journey) + §4
(load-bearing findings) + §5 (amendments). One-line summary block
below.

### Previous in-flight summary (now sealed) — KG Phase 0.5 (ADR 0049)

**Knowledge Graph Phase 0.5 + v1 architectural pivot — ADR 0049 Accepted
2026-05-29; PHASE-0-5-REPORT.md landed.** *Lateral epic per LESSONS
PINNED P5; no `phase-*-complete` tag cut.* Six waves on the
`experimental/kg-validation/` sandbox:

- **Wave 0.5.1 (`mb-xmgs` + `mb-4xtd`)** — SCHEMA.md refactor + 3b parity
  + 7b hard-gate breach + model-class calibration profiles restore the
  gate. LESSONS P10 (per-model-class calibration is necessary for
  schema-driven pipelines).
- **Wave 0.5.2 (`mb-yfzy` + `mb-hnb4`)** — embeddings classifier (Move 2)
  falsified at 32-pair corpus scale on both nearest-neighbour and
  centroid methods (-11 to -20pp across category / entry-type /
  clean-single). ADR 0049 amendment A1: embeddings infrastructure
  preserved for entity disambiguation in Move 4, retired as a
  classifier.
- **Wave 0.5.3 (`mb-rzpd` + `mb-e10v`)** — closed canonical tag
  vocabulary (Move 3) sealed as useful negative result. Wiring fix
  (`synonyms.rs` lift + `tag_validator.rs` in-band canonicalization,
  commit `8fdc7fb`) is architecturally correct and remains on `main`,
  but residual 9.1pp gap traces to corpus tag/entity conflation, not
  Move 3 architecture. LESSONS P11 ("tags ≠ entities"). Move 3 DEFERRED
  to v1.1 after two-field corpus re-labeling.
- **Wave 0.5.4 (`mb-o4ni`)** — entity extraction probe (Move 4) ACCEPT
  at qwen2.5:7b mid-confident: 54.83% / 53.40% strict Jaccard at seeds
  42 / 137 (bar 50%), 97.08% stability. 9 of 10 Wave 0.5.3 closed-vocab
  near-misses (Mrs Chen, Home Depot, brake-pads, Karen, launch, Costco)
  recover as entities — empirically validates P11. Entity layer ships in v1.
- **Wave 0.5.5 (`mb-5r1b`)** — qwen2.5:3b cross-test on pivoted
  architecture. Same SCHEMA, pass, scorer, labels → 33.21% / 35.48%
  Jaccard with 96.85% stability. 21pp cliff is structural under-extraction
  at 3b. LESSONS P12 (schema portability is 2-D: pass-type × model-class).
  v1 pins to qwen2.5:7b for entity-aware operation; 3b = documented
  tags-only degraded mode. ADR 0049 amendment A3.
- **Wave 0.5.6 (`mb-qogz`)** — this iteration. PHASE-0-5-REPORT.md
  landed at `docs/knowledge-graph/PHASE-0-5-REPORT.md`; ADR 0049
  amendments A2 (two-field schema) + A3 (7b model pin) authored; ADR
  0049 Status → Accepted. Epic `mb-symi` closed.

**v1 architecture binding (full table in PHASE-0-5-REPORT.md §6):**
pipeline segment → classify → extract → **extract_entities** → normalize;
two-field structured entry schema (`tags:` open-vocab in v1 + `entities:`
typed references with 5-bucket taxonomy); SCHEMA.md drives all passes
with per-model-class calibration profiles; qwen2.5:7b-instruct-q4_K_M
pinned for entity-aware operation; opt-in graph guarantee (existing
dictation users see zero regression); ~1 min intake latency budget;
closed-vocab Move 3 deferred to v1.1.

**Phase 1 wave plan (PHASE-0-5-REPORT.md §7):** 1A schema-driven
pipeline graduates to production → 1B SQLite entity/tag/edge tables
→ 1C retrieval UX (6 axes) → 1D migration backfill → 1E v1 beta tag.
All five waves are reference only; each gets its own brief at kickoff.

Standing work (not gating Phase 1A):
- Phase 10 live-fire Win11 smoke test (LESSONS P7 — still Dustin's post-seal step).
- Standing P1 `mb-ez9` (empirical mode-prompt iteration; picks up when fixtures land).
- Standing P2s `mb-xwi` / `mb-nc9u` / `mb-e2t8` / `mb-0n8c` / `mb-jmup`.
- Standing P3s (see below).

---

### Previous in-flight summary (now sealed) — KG Phase 0 (ADR 0048)

**Knowledge Graph Phase 0 epic — ADR 0048 Accepted, REPORT.md landed.**
*Lateral epic per LESSONS PINNED P5; no `phase-*-complete` tag cut.*
Wave 0 (charter + scaffold) landed 2026-05-28: spec imported to
`docs/knowledge-graph/spec.md` (immutable), ADR 0048 drafted (Proposed),
10-bead epic with dependency graph (`mb-4wxw` → `mb-0baz`), sandbox crate
at `experimental/kg-validation/` (standalone — its own `[workspace]`, **not**
a member of the root Mockingbird workspace, zero CUDA / whisper-rs / ort
deps so vanilla `cargo test` runs live and sidesteps LESSONS P2), schema
types + serde round-trip tests (now 5/5 passing on vanilla `cargo test`
including the corpus-files safety net).
Closed in Wave 0: `mb-4wxw`, `mb-w1lw`, `mb-i9l1`.
**Wave 1 SEALED — corpus complete (32/32 pairs, full taxonomy coverage).**
`mb-t7w5` CLOSED; `mb-901u` CLOSED (Note gap resolved inline by Wave 1
addendum, not deferred to v2). Corpus notes + capture anchor
(`2026-06-14T08:00:00Z`) + persona index + final batch ledger +
distribution + taxonomy-coverage note at
`experimental/kg-validation/corpus/CORPUS_NOTES.md`. Final persona
coverage: 01 (working-class) x6, 02 (tradesperson) x4, 03 (salaried
professional) x7, 04 (side-hustler) x5, 05 (caregiver) x5, 06 (recent
grad) x5. Difficulty: 13 clean single-item, 13 multi-item rambler
(incl. 1 five-item peak-hard at persona-05-case-03), 2 junk
(persona-01-case-05, persona-05-case-05), 4 dedicated no-date
hard-gate, 8+ ambiguous-category (incl. 3 `objective` tests), 1
`reference` type test, 2 `note` type tests (Wave 1 addendum:
persona-01-case-06 personal-note FYI vs.\ persona-03-case-07
professional-note witnessed). Calibration locks (cumulative across all
3 batches + addendum): side-hustle/Etsy/freelance = `professional`;
task-due tracks the action's deadline not the underlying event-date;
softened "I was thinking I should..." = `idea` not `task`; `objective`
= long-term identity/direction (not day-to-day logistics); `reference`
= save-info-for-later from elsewhere; `note` = firsthand-witnessed
fact or self-reminder to file (no action implied); work-adjacent
personal finance (e.g. 401k rollover) = `personal`; junk = zero
entries. **Taxonomy:** all 5 `EntryType` variants (task / idea /
research / reference / note) and all 3 `Category` variants now
exercised in fixtures; schema-level unknown-variant protection still
backed by serde deserialization. Durable safety-net tests in
`src/schema.rs`: `corpus_files_parse_as_answer_keys` (globs
`corpus/answer-keys/*.json`, deserializes each as `AnswerKey`,
asserts `expected_entry_count == entries.len()` AND the junk-bucket
invariant `is_junk → count=0 && entries.is_empty()`) plus the
promoted `corpus_exercises_full_taxonomy` (asserts all 3 Category
variants AND all 5 EntryType variants once the corpus is ≥20 keys).
Sandbox gate green (6/6 tests, fmt clean, clippy --all-targets clean). Q1 / Q2 / Q3 v1
architectural decisions (vault subtree, positional routing, files-as-
source-of-truth) are recorded verbatim in ADR 0048 for inheritance
by the future v1 charter ADR (provisionally 0049, drafted post-gate).
**Wave 2 SEALED (2026-05-28) — 4-pass pipeline + run-corpus harness
shipped.** `mb-i4us` (pipeline) and `mb-nbel` (harness binary) both
CLOSED. New surface area:

- `src/ollama.rs` — G1 carve-out: `OllamaDispatcher` trait +
  `OllamaClient` (reqwest blocking, POST `/api/generate`, stream=false)
  + `MockOllama` test double (`#[cfg(test)] pub mod testing`,
  first-substring-match-wins, records calls).
- `src/passes/{segment,classify,extract,normalize}.rs` — the four
  passes per spec §8.1. Temperature 0.2 pinned by caller (ADR 0048
  §G4), seed configurable per run for §8.5 stability. `extract` enforces
  the date hard-gate at parse time (non-null `due_iso` must be valid
  `YYYY-MM-DD`); raw model output preserved on every parse/validation
  failure for Wave 3 scoring. `normalize` is pure-Rust, conservative
  singularization (`ies→y`, `xes/zes→x/z`, trailing `s` only when
  prior char ∉ {s,x,z,u,i,o} and word doesn't end in {ss,sh,ch,us});
  compound tags singularize only the head noun.
- `prompts/{segment,classify,extract}.md` — first-cut prompts with
  2-3 few-shots each (Wave 5 iterates quality).
- `src/harness/{pipeline,runner}.rs` — orchestrator (per-segment
  failure isolation: segment-pass failure aborts dictation, classify/
  extract failure drops only that segment) + corpus walker. Persists
  `raw/<id>/{segment,classify-N,extract-N}.json` + `structured/<id>.json`
  + `SUMMARY.json`. Dry-run skips Ollama.
- `src/bin/run-corpus.rs` — hand-rolled CLI (10 flags;
  `--model/--seed/--run-id/--corpus-dir/--output-dir/--captured-iso/
  --ollama-url/--temperature/--num-ctx/--dry-run/--help`). No clap dep
  (sandbox, YAGNI).
- `.gitignore` — `target/`, `runs/`, `smoke-corpus/`.

**Sandbox gate green:** vanilla `cargo fmt --check && cargo clippy
--all-targets -- -D warnings && cargo test` from
`experimental/kg-validation/` → 44/44 tests pass (was 6/6 pre-Wave 2;
+38 new). Live-fire smoke on `qwen2.5:3b-instruct-q4_K_M` against a
3-dictation subset (persona-01-case-01 clean-single, persona-02-case-01
clean-2-item, persona-01-case-05 junk): all three succeeded with zero
parse/validation errors. Quality observations (not graded — that's
Wave 5): junk correctly returned `[]`; multi-item dictation split
cleanly into task + idea with `status` omitted from the idea
(schema discipline holds); date hard-gate worked both ways ("before
Friday" → `2026-06-19`; ambiguous "Monday morning" → `None`, i.e.
conservative); the clean-single dictation got over-split into 2
entries — Wave 5 segmenter prompt-tuning concern, not structural.

**Wave 3 (`mb-57a1` scorer + LLM tag-equivalence judge) — HALTED on
JVP Gate 3 STOP, twice. Wave 3.2 (llama3.1:8b primary) and Wave 3.3
(gemma2:9b primary after option-B swap + option-C borderline
calibration) both halt on Gate 3 with functionally identical
agreement rates (57.1% then 55.6%) but **inverted disagreement
direction** on the same three personas. Structural finding: the
tag-equivalence task as currently specified is more ambiguous than
the inter-rater reliability of LLM judges of different families
supports. Not a prompt-tuning problem (rejected); not a
judge-selection problem (empirically falsified by the swap). It is
a task-definition / metric-design problem.** Wave 3.3 details, options
forward (E/F/G/H), and Bernard's recommendation in
`docs/knowledge-graph/wave-3-results.md`. **Escalation territory —
ADR 0048 §G5 amendment required to proceed; Dustin decision needed.**

Shipped Wave 3.2 (prior session):
- Calibration v2 fix (commit `7f8ff1c`) — replaced `cal-eq-001`'s
  car-repair pair which was lexically identical to the judge prompt's
  first in-context example (would inflate Gate 1 verdict-correct on
  memorization). Replacement: `[birthday, gift]` vs `[birthday,
  birthday-gift]` — same anchored-synonym pattern, fresh vocabulary
  disjoint from all prompt examples and other calibration pairs.
  Loader round-trip test bumped to `v2`. 81/81 sandbox tests still green.
- Models pulled: `llama3.1:8b-instruct-q4_K_M` (4.9 GB) +
  `gemma2:9b` (5.4 GB) — both confirmed via `ollama list`.
- Full score-run on run-a-baseline (~50 min wall: ~40 min step 1 tag
  judge × 55 entries, ~14 min JVP 5 gates, ~4 min PCRP 13 samples).
  Run-b NOT re-scored — judge invalid ⇒ tag metric also invalid ⇒ not
  a defensible LLM budget spend.

Headline (run-a-baseline):
- ✅ Invented dates: **0** (hard gate holds)
- ✅ Junk handling: 100% (2/2)
- ✅ Segmentation (multi-item): 86.7% (13/15, ≥ 85% threshold)
- ❌ Category correct: 67.3% (37/55, < 90%)
- ❌ Entry-type correct: 78.2% (43/55, < 85%)
- ❌ Clean single-item: 6.7% (1/15) — dominant cause is
  **over-segmentation of single-item dictations** (9/15 split into 2).
- ⚠️ Tag-variant collapse: 81.8% (45/55) — **INVALID** per JVP HALT.

JVP outcome (overall **HALT**):
- Gate 1 calibration: ✅ Pass 11/12 (91.7%) — sole miss cal-eq-004
  (`[doctor-appointment]` vs `[doctor, appointment]`, borderline call).
- Gate 2 reasoning audit: ✅ Pass 70/70 (100%).
- Gate 3 cross-judge (`gemma2:9b`): 🛑 **Stop** 4/7 (57.1%, STOP < 85%).
  Two genuine `primary=Equivalent / cross=NotEquivalent` disagreements
  in the same direction → `llama3.1:8b` is more permissive than
  `gemma2:9b` on equivalence on the real corpus. Combined with Gate 4's
  64.3% equivalence rate (high end of in-band), the structural signal is
  that the primary judge's verdicts skew Equivalent in a way the cross
  doesn't corroborate, so the 81.8% tag-collapse metric is likely
  inflated. Third disagreement was a transient network error (excluded:
  4/6 = 66.7%, still STOP).
- Gate 4 distribution: ✅ Pass 64.3% equivalent (in-band 40–80%).
- Gate 5 determinism: ⚠️ Warn 0/5 byte-identical re-runs at fixed seed.
  Verdict-stable across runs (chain-of-thought prose varies); recommend
  Wave-5 to promote a parsed-verdict-only determinism check.

PCRP (run-a, reviewer `llama3.1:8b`): 8 trust-eroding / 9 trust-building.
Final-run §G6 condition triggered (≥ 5 trust-eroding AND no metric
exceeds threshold by > 5pts) → default NO-GO. Cross-persona themes:
side-hustle content miscategorized as `personal` (calibration locks
didn't propagate into classify few-shots), topic-tag drift toward
proximate-noun rather than filing vocabulary, **soft-date
under-extraction** (PCRP mis-labeled as "hallucinated" — the structural
hard-gate is correct; the failure mode is the inverse), and the
over-segmentation pattern that also shows up structurally.

**`mb-57a1` left OPEN.** Wave 4 still blocked. This is now a
**judge-validation problem, not a model-pulls problem**. Four options
forward (cheapest first; full detail in `docs/knowledge-graph/
wave-3-results.md` § "What's needed to unblock"):

- **A. Tune the judge prompt** — bias toward NotEquivalent on
  superset/decomposition disagreement; one fuzzy-NotEquivalent in-context
  example. Re-run JVP only. ~10 min iteration.
- **B. Swap primary judge** to `gemma2:9b` (already on disk) or
  `qwen2.5:14b`. Compliant with §G4 different-family rule. +30% LLM cost
  per scoring run; may resolve the asymmetry cleanly.
- **C. Add 5–8 borderline pairs to the calibration set** so Gate 1
  measures behavior on fuzzy cases, not just unambiguous ones. Pairs
  well with A or B.
- **D. Loosen Gate 3 thresholds — NOT recommended.** Documentation
  change masquerading as a fix; reject unless explicitly deferring
  judge-validity work.

Resume protocol (post-Wave-3.3): A and D were antipatterns; B + C were
shipped this iteration (commits `6565916` calibration v3 + `36f5988`
judge swap + score-run on run-a). New post-Wave-3.3 option space
(E/F/G/H) lives in `docs/knowledge-graph/wave-3-results.md` §"Options
forward — Wave 3.3 amendment". Bernard's recommendation: **option E
(replace LLM judge with deterministic exact-match + Jaccard tag
metric)** — honors AGENTS.md §6 ("if something is hard to verify,
that's the bug"), zero LLM-time-per-scoring-run, eliminates the
43-point judge-dependent gap on tag-collapse, and substantively
simplifies the Wave 4 judge bundle. Requires ADR 0048 §G5 amendment;
Dustin call.

Wave 3.3 shipped:
- Calibration v3 (commit `6565916`) — 6 borderline observational pairs
  alongside the 12 gated pairs. `tag-equivalence-v3`. JVP reports
  per-dimension match rate (`tokenization`, `specificity`,
  `coreference`, `domain-overlap`, `abstraction-level`,
  `person-specific`). 84/84 sandbox tests.
- Judge swap (commit `36f5988`) — primary `gemma2:9b`,
  cross-check `llama3.1:8b`. ADR 0048 §G4/§G5 amended with Wave 3.3
  rationale.
- Score-run on run-a-baseline (`runs/score-run-a-wave33.log`, ~26 min
  wall). Gate 1 ✅ (91.7%) + borderline 4/6 (66.7% — 100% on clear
  dimensions, 0% on coreference + abstraction-level), Gate 2 ✅
  (91/91), Gate 3 🛑 5/9 (55.6%, direction inverted vs Wave 3.2),
  Gate 4 ⚠️ 23.1% (below-band, gemma2 over-strict), Gate 5 ⚠️ 0/5.
  Tag-collapse metric shifted 81.8% → 38.2% (43-pt gap, judge-dependent
  uncertainty band).
- run-b NOT re-scored (judge invalid ⇒ not a defensible LLM budget
  spend; halt rule honored).
- PCRP themes unchanged from Wave 3.2 (same 8/9 trust ratio,
  same cross-persona patterns) — deterministic structural data,
  unaffected by the judge change.

`mb-57a1` stays OPEN. Wave 4 (`mb-he98`) stays blocked.

**Wave 3.4 (2026-05-29) — Option E shipped. Wave 3 SEALED.**

Dustin authorized Option E (deterministic exact-match-after-canonicalization
with versioned synonym map) per AGENTS.md §6 ("if something is hard to
verify, that's the bug"). ADR 0048 §G7 amendment (commit `5e8583c`).

Shipped this iteration:

- **Synonym map v1** (commit `829091a`) — `experimental/kg-validation/
  judge-calibration/synonym-map.json`, 188 canonicals / 240 variant→canonical
  entries. Sourcing: 166 auto-seed-answer-key (every answer-key tag is
  at minimum its own canonical) + 16 bernard-seed (household /
  professional / tradesperson / caregiver domain coverage) + 6
  diff-driven-codepuppy (conservative pipeline-vs-answer-key gap closure;
  `farmers-market`/`farmer's-market`, `chen`/`mrs-chen`, `roth`/`roth-ira`,
  `side-business`/`side-work`, `smith`/`the-smith`, `wholesale`/`wholesaler`).
  Discipline rules from ADR 0048 §G7 enforced: person-names NEVER collapse
  into domain tags, specificity preserved when irreducible, domain-overlap
  is NOT equivalence. Regenerator script at `scripts/generate-synonym-map.ps1`.
- **Deterministic tag-collapse metric** (commit `1b7d656`) — new module
  `src/scoring/tag_collapse.rs` with 17 unit tests covering all discipline
  rules. `score_run` signature changed from
  `<D: OllamaDispatcher>(..., Option<TagJudgeContext<'_, D>>)` →
  `(..., Option<&SynonymMap>)`. SCORE_SUMMARY.md now surfaces top-10 near-miss
  `(actual_canonical, expected_canonical)` pairs ranked by frequency — these
  are the empirical Wave 5 prompt-iteration + synonym-map-iteration
  candidates. `score-run` CLI gains `--synonym-map`; pre-G7 flags
  (`--judge-model`/`--cross-judge-model`/`--judge-seed`/`--skip-jvp`)
  hard-fail with a deprecation note. JVP architecture preserved in source
  (`src/scoring/judge_validation.rs`) for future LLM-judged metrics but
  not invoked under §G7. 99/99 sandbox tests pass; clippy clean.

**Wave 3 final scorecard (deterministic; reproducible):**

| Metric | run-a-baseline | run-b-stability | Threshold | Verdict |
|---|---|---|---|---|
| Invented dates count (HARD GATE) | **0** | **0** | 0 | ✅ PASS |
| Junk-bucket | 100% (2/2) | 100% (2/2) | ~100% | ✅ PASS |
| Segmentation (multi-item) | 86.7% (13/15) | 86.7% (13/15) | ≥85% | ✅ PASS |
| Category correct | 67.3% (37/55) | 70.9% (39/55) | ≥90% | ❌ FAIL |
| Entry-type correct | 78.2% (43/55) | 76.4% (42/55) | ≥85% | ❌ FAIL |
| Clean single-item | 6.7% (1/15) | 13.3% (2/15) | ~100% | ❌ FAIL |
| **Tag-variant collapse (G7)** | **9.1% (5/55)** | **10.9% (6/55)** | ≥80% | ❌ FAIL |

**Stability (run-a vs run-b, §8.5):** segmentation 96.9%, category 96.9%,
entry-type 98.5%, date 100%, tag-set exact 83.1%. All ≥80% (date metric
perfect); the structural pipeline is reproducible at the spec threshold.

**Headline finding on tag-collapse:** the 9.1%/10.9% number is honest
and reproducible (1.8% drift, within sampling noise). It is far lower
than the prior judge-dependent 81.8% (Wave 3.2) and 38.2% (Wave 3.3)
because Jaccard-1.0-after-canonicalization is a strict gate AND the
pipeline systematically over-emits tags relative to the answer-key
expected sets (e.g. pipeline emits `{chen, inspection, water-heater}`
vs expected `{chen, water-heater}` → J=2/3=0.67, fails 1.0 gate).
This IS the metric working as designed — misses are now mechanically
attributable to specific synonym-map gaps OR specific pipeline
over-emission patterns, not to judge-model variance. Top-10 near-miss
categories (run-a): `after-school`/`kid`, `apartment-complex`/`apartment`,
`brake`/`car-repair`, `brunch`/`rsvp`, `budget`/`meeting`,
`cake`/`bakery`, `check`/`olivia` — mix of synonym-map candidates
(apartment-complex/apartment is a clear gap) and genuine
pipeline-vs-answer-key tag-vocabulary divergence (brake/car-repair is
correctly distinct per the specificity discipline rule).

**Wave 5 inputs queued from this run:**
1. Synonym-map v2 candidates (clear gaps surfaced as near-misses).
2. Pipeline prompt iteration: extractor over-emits 3-tag sets when
   answer keys want 2-tag sets; tightening the extract prompt's
   tag-budget guidance should lift the metric materially.
3. Category prompt iteration (67% → 90% is the biggest structural
   gap; PCRP Wave 3.2 already attributed most of this to
   side-hustle-as-personal miscategorization).

**PCRP not re-run this iteration** — `PERSONA_REVIEW.md` on disk
from Wave 3.2 remains valid (PCRP reviews structured outputs which
are unchanged; the only thing that changed is the tag metric).
If Wave 5 prompt iteration changes structural outputs, PCRP re-runs
at that point.

**Wave 3 SEALED.** `mb-57a1` closeable. `mb-jz5r` (Option E task)
closeable. Wave 4 (`mb-he98`) unblocked.

**Wave 4 (2026-05-29) — 6 invariant judges + run-judges rig shipped.**
ADR 0048 §G7 retired JVP-completeness (LLM-judged) from the original
7-judge draft along with the LLM tag-equivalence metric, leaving the
deterministic 6-judge suite below. All six authored under
`experimental/kg-validation/src/judges/` with inline known-good +
known-bad fixture pairs; orchestrator at
`experimental/kg-validation/src/bin/run-judges.rs` (`cargo run --release
--bin run-judges --runs <dirs> --final-run <dir>`); operator docs at
`docs/judges/phase-0-kg/README.md`. New anchor tag `phase-0-kg-start` at
`aad06a6` (commit just before Wave 0; mirrors the `phase-mc-start`
pattern) is the default `--baseline-ref` for sandbox-isolation.

| # | Judge | Mechanism | Smoke verdict vs Wave 3 runs |
|---|---|---|---|
| 1 | `hard_gate_invented_dates_zero` | `SCORE.json::per_metric::invented_dates_count == 0` | ✅ PASS (0/55 both runs) |
| 2 | `thresholds_match_spec_8_4` | per-metric floors vs spec §8.4 | ❌ FAIL (category 67–71%, entry-type 76–78%, tag-collapse 9–11%, clean-single 7–13%; **expected — Wave 5 prompt-iteration inputs**) |
| 3 | `stability_meets_spec_8_5` | structural agreement ≥ 80%, date 100% | ✅ PASS (96.9 / 96.9 / 98.5 / 100; tag-set exact 83.1% reported but not gated per §G7) |
| 4 | `sandbox_isolation_phase0_kg` | `git diff --name-only phase-0-kg-start HEAD` | ✅ PASS post-commit (initial smoke surfaced a stale root-`.gitignore` entry for `experimental/kg-validation/runs/` — the sandbox-local `.gitignore` already covers it; redundant root entry removed) |
| 5 | `determinism_seed42_byte_identical` | live re-run via `run-corpus --seed 42`, byte-compare 3 dictations | ⚪ SKIPPED (opt-in; `--enable-determinism`. Deferred until Wave 5 ships a candidate green baseline.) |
| 6 | `pcrp_completeness_and_trust` | `PERSONA_REVIEW.md` present + (`trust_eroding ≤ 5` OR metric > floor+5pts) | ❌ FAIL §G6 NO-GO (trust_eroding=8 AND no metric > floor+5pts — **expected; canonical signal that Wave 5 prompt iteration must ship before Wave 6 attempts a seal**) |

Sandbox gate green: vanilla `cargo fmt --check && cargo clippy --all-targets
-- -D warnings && cargo test` → **124/124 passing** (was 99 pre-Wave-4;
+24 judge tests + 1 new PCRP parser fixture for the canonical
markdown-bullet emit form `- trust_eroding_failures_count: **N**`).
Real bugs fixed during smoke: (a) PCRP parser's `strip_prefix` blocked
by leading markdown bullet `- ` (real bug; aligned to actual
`persona_review::render_markdown` emit shape; new regression test);
(b) default `--baseline-ref` was `phase-mc-complete`, which predates
phase-10 + ADRs 0045/0046/0047 by ~30 commits, producing 192 spurious
violations (configuration bug; new anchor tag fixes it). The
`thresholds` + `pcrp_completeness` FAILs are not judge bugs — they
are the diagnostic surface working as designed; flipping them green
is Wave 5's job.

**`mb-he98` closeable. `mb-ojm5` (Wave 5 Wiggum loop, cap 5) unblocked.**

**Wave 5 SEALED (2026-05-29) — IAP loop ran cap, no iteration accepted; baseline UNCHANGED.**

Five iterations exhausted per the Iteration Acceptance Protocol (IAP)
documented in the kickoff brief + `experimental/kg-validation/wave-5/ITERATION_JOURNAL.md`:

| Iter | Prompt touched | Aggregate Δ | Hard-gate | PCRP Δ | Verdict | Rules tripped |
|---|---|---|---|---|---|---|
| 1 | segmenter "when in doubt keep as one entry" | +0.47 | intact | +3 | REJECT | Rule 5 (PCRP) |
| 2 | extractor tag-budget cap | +0.15 | intact | +3 | REJECT | Rules 2 + 5 |
| 3 | classifier side-hustle → professional | −3.39 | **BROKEN** | 0 | REJECT | Rules 1 + 2 + 3 (cascade `due_iso` hallucination on persona-06-case-03 "before I lose track") |
| 4 | extractor tag-vocabulary + date hardening | +0.28 | intact | 0 | REJECT | Rule 2 (entry-type −0.82pp via cascade classify-pass parse failure on persona-06-case-05) |
| 5 | extractor date soft-urgency only (minimal) | +0.24 | intact | +2 | REJECT | Rules 2 + 5 |

Four of five iterations had aggregate score > baseline; four of five held the
hard-gate; zero of five satisfied strict no-regression on all gated metrics.
The IAP correctly prevents lateral local-optimum drift; this run demonstrates
that **no single prompt change on qwen2.5:3b @ 32 dictations can ratchet the
strict-no-regression IAP**. Each iteration's changes have global cascade onto
co-metrics — iter 5 made an extract-only change with ZERO tag/category/type
language and still dropped tag-collapse 1.54pp + entry-type 0.82pp + lifted
PCRP +2 via joint-distribution shift.

**Synonym map sweep (parallel track, separate `[synonym-map]` commit `33dd5ae` +
BOM-fix `f4c1a43`):** map version v1.0 → v1.1. Three conservative ADR 0048 G7-
compliant additions: `kid` += `kids,children`; `apartment` += `apartment-complex`;
`home-maintenance` += `cleanup,home-cleanup`. Five candidates skipped per
discipline (person names never collapse, specificity preserved, domain overlap
not equivalence): `after-school` → `kid`, `cake` → `bakery|dad`, `brake` →
`car-repair`, `401k` → `retirement`, `budget` → `meeting|slide-deck`. Tag-
collapse PRIMARY (Jaccard 1.0) lift: **0pp** (5/55 → 5/55). Jaccard ≥ 0.50
lifted 26 → 27. **Finding:** tag-collapse ceiling is fundamental vocabulary
mismatch between the open-vocabulary extractor and the persona-calibrated
answer keys, not missing synonym entries.

**Wave 5 baseline (= Wave 3.4 sealed scorecard = production-ready prompt set):**
UNCHANGED. Hard-gate 0 ✅. Junk 100% ✅. Segmentation 86.7% ✅. Category 67.3% ❌.
Entry-type 78.2% ❌. Clean-single 6.7% ❌. Tag-collapse 9.1% ❌. PCRP 8 trust-
eroding (likely ~5 de-mislabeled per LESSONS PINNED PCRP pattern; reviewer
reads `captured_iso` as `due_iso`).

`mb-ojm5` closeable.

**Wave 6 SEALED (2026-05-29) — REPORT.md landed; ADR 0048 → Accepted; KG Phase 0 epic SEALED.**

Deliverable: [`docs/knowledge-graph/REPORT.md`](docs/knowledge-graph/REPORT.md)
(433 lines, 10 sections per spec §8.6 + kickoff brief).

**Verdict: NO-GO on the strict reading (§G6 trigger fires: trust_eroding=8
AND no metric exceeds threshold by >5pp). GO-WITH-LIMITATIONS for an
assisted-filing v1 UX where the user reviews each structured entry before
commit** — on the grounds that (a) the trust-critical gates (hard-gate, junk)
PASS by wide margin; (b) the PCRP count is inflated by the documented
reviewer-prompt mislabel; (c) stability is glorious across all metrics
(≥95% structural agreement); (d) the failing metrics (category 67%, entry-
type 78%, clean-single 7%, tag-collapse 9%) are filling-quality problems
that a user reviewing each draft can fix in seconds.

**v1 recommendation in §8 of REPORT.md:** ship the spec PART B §9 lighter
scope but **require explicit per-entry user confirmation** before commit;
build a draft-review pane exposing inline-editable title/category/entry_type/
due_iso/topic_tags; never expose dictation content as "filed" without
confirmation; preserve `History/` dual-write per spec §10. The 78%/67%/9%
accuracy converts from a trust-eroding silent error into a 1-tap correction.

**Future-Phase-0.5 charter implications captured in REPORT §10:** if v1 needs
to evolve toward autonomous filing, the path is EITHER a larger local model
(qwen2.5:7b / llama3.1:8b / 14b on suitable hardware — untested in Phase 0)
OR a hybrid "model drafts + heuristics layer for confident slots + tag
post-processing engine". Prompt engineering on qwen2.5:3b alone cannot close
the gaps per the Wave 5 evidence.

**ADR 0048 → Accepted** (commit this iteration). KG Phase 0 epic SEALED as
lateral epic per LESSONS PINNED P5; **NO `phase-*-complete` tag** (Phase 0 KG
is not a numbered PLAN §10 phase). The next ADR for this product surface
will be the v1 charter (provisionally ADR 0049) inheriting the Q1/Q2/Q3
decisions captured in ADR 0048 §3 + the assisted-filing-UX recommendation
from REPORT.md §8.

`mb-0baz` closeable. `mb-ojm5` closeable. KG epic SEALED.

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
