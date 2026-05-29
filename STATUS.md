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
- ADR 0048 — Knowledge Graph Phase 0 validation methodology (Accepted — sealed 2026-05-29 with `docs/knowledge-graph/REPORT.md`). Seven waves shipped: Wave 0 charter + scaffold; Wave 1 corpus (32 fixtures, full taxonomy coverage); Wave 2 4-pass pipeline + run-corpus harness; Wave 3 scorer (3 sub-iterations sealing on §G7 deterministic synonym-map metric per Option E); Wave 4 6 invariant judges + run-judges rig (`phase-0-kg-start` anchor at `aad06a6`); Wave 5 IAP Wiggum loop (cap 5; 0 accepted; documented the structural ceiling); Wave 6 REPORT.md + go/no-go (§G6 strict NO-GO; defensible GO-WITH-LIMITATIONS for an assisted-filing v1 UX). Final scorecard: hard-gate `invented_dates_count=0` PASS, junk-bucket 100% PASS, segmentation 86.7% PASS, category 67.3% FAIL, entry-type 78.2% FAIL, clean-single 6.7% FAIL, tag-collapse 9.1% FAIL. Synonym map v1.1. Stability ≥95% structural agreement. v1 recommendation: lighter spec scope PART B §9 with per-entry user confirmation; draft-review pane converting filling-quality errors into 1-tap corrections; raw transcript preserved per spec §10 dual-write. **No `phase-*-complete` tag** — lateral epic. Future v1 charter ADR (provisionally 0049) inherits Q1/Q2/Q3 decisions from ADR 0048 §3 + assisted-filing-UX contract from REPORT §8. Beads sealed: `mb-4wxw`, `mb-w1lw`, `mb-i9l1`, `mb-t7w5`, `mb-901u`, `mb-i4us`, `mb-nbel`, `mb-57a1`, `mb-jz5r`, `mb-he98`, `mb-ojm5`, `mb-0baz`.
- ADR 0046 — Mobile extension via synced Obsidian vault (Accepted — sealed 2026-05-24). Four iterations shipped (desktop file ingest → outbound vault projection → inbound mobile courier → polish). User-facing surface: `+ Audio file` desktop import button, deterministic Markdown projection of dictation + meeting history to `<vault>/history/`, inbox courier auto-processing iOS-Shortcut-delivered voice memos from `<vault>/inbox/`, full Mobile Sync settings tab (8 keys + connection-health card), nested-vault detection wizard, import progress overlay, iOS Shortcut recipe (`docs/mobile/ios-shortcut.md`, 3 actions per Wave 0 Finding 5). Channel boundary preserved across 3 reuse sites: `dictation/ingest.rs` (Iter 1 ADR §3.2 amendment) consumed by IPC handler, inbox courier, and import progress overlay event-tap with zero further sealed-surface modifications. Two `sealed-phases-untouched` judges PASS (Iter 1 @ 95%, Iter 3 @ 99%); Iter 2 + Iter 4 didn't need them (greenfield + UI-side). 19 beads closed. Seal commit: HEAD of `main` at consolidation time (this STATUS update was committed in the seal commit itself; see `git log --grep='ADR 0046 SEALED'`). Wave 5 hardening matrix (`mb-qxrm`) remains open as live-corpus catch-up; not gating epic seal. **No new `phase-*-complete` tag** (lateral epic per LESSONS PINNED P5).
- ADR 0047 — Cleanup pipeline refinement (Accepted 2026-05-25). Per-pass system headers in `meetings/llm_pass.rs` (`cleaner_punctuation` no longer carries the global "Be concise" instruction — the load-bearing fix); length-ratio shrink fallback (`SettingKey::LlmShrinkFallbackThreshold`, default 0.65); Whisper `initial_prompt` wired from the user's dictionary at both dictation call sites; temperature standardized to 0.2 across casual / normal / formal / meetings (migration 019); new `DictationCleanupLevel` dial (`None` / `Light` / `Medium` / `High`; default `High` preserves prior behaviour; `Medium` uses the new `normal_v6_additive` prompt); LLM-skip-on-short-utterance (`SettingKey::LlmSkipWordThreshold`, default 12 words; gated on `!looks_listy()`; consumed `mb-cjc` / ADR 0022 Wave 3); casual mode repointed to `qwen2.5:7b-instruct-q4_K_M` (migration 021; one-liners absorbed by the skip path); opt-in Q5_K_M via `SettingKey::PreferQ5Models` with VRAM-gated runtime substitution (migration 022; defaults off); Compress Transform on `LlmPassCard` as on-demand pull-only affordance (`dictation/prompts/compress.md`); `sessions.edit_free_within_5min` instrumentation as the empirical quality signal (surfaced in Insights "Your usage"). UI surface for the dial + Q5 toggle deferred to `mb-h0nn`. Empirically validated by `docs/cleanup/eval-adr0047-cleaner-punctuation.md` (18/20 fixtures preserve all expected phrases on `qwen2.5:3b-instruct-q4_K_M`; zero over-consolidation regressions). Sealed via 13 commits `c7af486..` + this seal commit; **no `phase-*-complete` tag** per LESSONS PINNED P5.
- ADR 0045 — Dictation programmatic start/stop (Accepted 2026-05-27). Amends ADR 0037 §4: the `NoProgrammaticStart` rule is removed for Dictation; the kind now supports two start modes — Right Alt PTT (UNCHANGED) and programmatic via `dictation_start` / `dictation_stop` IPC. Both modes drive the same `HotkeyStateMachine` via a sentinel VK (`0x07`) so the FSM, orchestrator, and `dictation:state` event stream are mode-agnostic. CC Dictation tile now lands on `ShowingSessionCard{Dictation}` (closes the silent-dismiss gap `mb-ytex`). New `<DictationRecordButton>` above the search input on the Dictations page. Shipped as bead `mb-ddfx` (commit `b313742`); no new tag, Phase 10 seal unchanged. **Follow-up beads `mb-tfyp` + `mb-sowc` (2026-05-27):** added `sessions.start_mode` column (migration 017, `'ptt'` / `'in_app'`) so the in-app start path no longer incorrectly produces `ABORTED_FOCUS_CHANGED` session rows. UI list-pill now renders `IN_APP` (neutral) for programmatic sessions; detail panel shows "Push-to-talk" vs "In-app" next to the mode. Recording-pill overlay gains a primary Stop button only when `startMode === 'in_app'` (PTT pill unchanged — zero regression). Plumbed via `dictation:state` event payload (new optional `startMode` field). New `InjectionOutcome::InAppNoInject` variant (db str `"in_app"`) replaces the abort path for in-app sessions — same observable result (no paste), cleaner semantics.
- **Design System v1** — bead-only lateral epic (`mb-n455`, sealed 2026-05-26). Glass-tier semantic tokens (`--surface-glass-strong/soft/faint`), `--glass-blur-cap` (12px), canonical sticky-sidebar scroll convention (single-page scroller + `scrollbar-gutter: stable`), outline-button glass-faint default fill, full `100vh` → `100dvh` sweep, native form-control polish (themed range pill + custom select chevron + dark-pill retention inputs), Activity-page dead-token legacy bridge. 8/8 P1 + 9/12 P2 baseline-audit findings resolved (3 false-positives). 14 modified CSS files; no Rust changes. Baseline + final audits at `docs/audits/2026-05-26-design-v1-{baseline,final}/REPORT.md`. Conventions at `docs/design/conventions.md`. No ADR — work was token + CSS refinement, not architectural.

If a kickoff prompt asks you to re-execute any of the above, **STOP** and surface
the conflict before any tool call. See `.code_puppy/AGENTS.md` § "Permanently sealed".

## 🟢 Currently active

**KG Phase 0.5 + v1 architectural pivot — ADR 0049 Proposed, Wave 0.5.0 sealed.**

Epic `mb-symi` (P1). Charter at `docs/adr/0049-knowledge-graph-phase-0-5-and-v1-architectural-pivot.md`.
Phase 0 (ADR 0048) measured a structural ceiling on qwen2.5:3b prompt-only;
ADR 0049 charters four architectural moves on the sandbox (SCHEMA.md portable
contract, embeddings classifier, closed canonical tag vocabulary, entity
extraction probe) on default `qwen2.5:7b-instruct-q4_K_M`. Six waves
0.5.1–0.5.6. v1 GO/NO-GO at 0.5.6 with mandatory Dustin sign-off gate.

Wave structure (sub-beads, ADR 0049 dependency graph in `bd`):

- `mb-xmgs` — Wave 0.5.1: SCHEMA.md refactor + 7b baseline + parity gate.
- `mb-yfzy` — Wave 0.5.2: embeddings classifier (nomic-embed-text). Blocked on 0.5.1.
- `mb-rzpd` — Wave 0.5.3: closed canonical tag vocab + new-tag-request flow. Blocked on 0.5.1.
- `mb-o4ni` — Wave 0.5.4: entity extraction probe + entity-quality metric. Blocked on 0.5.1, 0.5.3.
- `mb-5r1b` — Wave 0.5.5: qwen2.5:3b cross-test on pivoted architecture. Blocked on 0.5.2, 0.5.3, 0.5.4.
- `mb-qogz` — Wave 0.5.6: REPORT.md + GO/NO-GO + ADR 0049 Accepted. **HALT BEFORE THIS** — Dustin reviews 0.5.1–0.5.5 evidence on disk first.

Success criteria (ADR 0049): ≥ 3 of {category, entry-type, tag-collapse,
clean-single} lift ≥ 10 pts from Phase 0 baseline on 7b pivoted architecture
AND hard-gate intact AND PCRP trust_eroding ≤ Phase 0 baseline AND stability
≥ 80%. IAP per LESSONS PINNED P9: strict no-regression on trust gates;
Pareto-frontier on quality metrics.

Standing work (not gating Phase 0.5):
- Phase 10 live-fire Win11 smoke test (LESSONS P7 — still Dustin's post-seal step).
- Standing P1 `mb-ez9` (empirical mode-prompt iteration; picks up when fixtures land).
- Standing P2s `mb-xwi` / `mb-nc9u` / `mb-e2t8`.
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
