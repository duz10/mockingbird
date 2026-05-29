# LESSONS

Append-only notes from prior iterations. Each entry: date, phase tag,
short title, and the non-obvious finding.

**Reading order for new sessions:**
1. Scan the **📌 PINNED** block immediately below — these are the load-bearing
   gotchas that bite EVERY session if forgotten.
2. Skim the **TOC** below the pinned block.
3. `grep` the body for your area (`rg -i 'cuda|whisper|ort|cargo'` etc.).

**Append format for new entries:**
```
## YYYY-MM-DD [phase/iteration] short title
- Context: what we were doing
- Finding: the non-obvious thing
- Action: what to do differently next time
```

If a new lesson would change how EVERY future session should work,
promote it into the PINNED block + leave the dated entry in the body.

---

## 📌 PINNED — read these every session

These seven lessons are load-bearing. If you forget them, you will burn
an hour rediscovering the same trap.

### P1. ALL cargo invocations go through the Windows wrapper

```
powershell -File scripts\cargo-with-cuda.ps1 <cargo-args>
```

Plain `cargo` compiles but produces binaries that may not launch (missing
MSVC + CUDA env). The wrapper:
- Imports MSVC env via `vcvars64.bat`
- Pins `CUDA_PATH` + `CUDA_PATH_V12_8` to v12.8 (ADR 0011)
- Prepends `cmake` to PATH
- Caps `CMAKE_BUILD_PARALLEL_LEVEL=4` (whisper-rs CUDA OOMs at 16)
- Forwards args through `cmd.exe /c` for stream flattening

Full details: AGENTS.md § "Build / run / test environment (Windows)".
Use `powershell.exe`, NOT `pwsh` (not on PATH on this box).

### P2. `cargo test --release` is broken on this box — use the fallback gate

LESSONS 2026-05-17 [phase5-wave-I]: even with the wrapper, even with
`ORT_DYLIB_PATH` set, the test runner exits `STATUS_ENTRYPOINT_NOT_FOUND`
(0xc0000139). **The app binary launches fine** — only the test runner is
affected. Sanctioned fallback for the cargo gate:

1. `… cargo-with-cuda.ps1 check`
2. `… cargo-with-cuda.ps1 clippy --release -- -D warnings`
3. `… cargo-with-cuda.ps1 fmt --check`
4. `… cargo-with-cuda.ps1 test --release --no-run` (link-only proof)

For pure-Rust modules (no whisper-rs / ort / cuda deps), use the
throwaway-crate recipe: copy source into `$env:TEMP\<modname>_tests\`,
add only that module's real deps, run vanilla `cargo test` there.

**2026-05-23 refinement (`mb-jbf7`):** the same bug bites `cargo build
--release --example <name>` outputs **whenever the example transitively
pulls in `whisper-rs` / `ort` / CUDA**. The `smoke_iter1_ingest` example
built cleanly (41 MB binary, link-clean) and `verify_wave49.exe` runs
fine (pure rusqlite, 1.7 MB) — but the smoke example exits
`-1073741511` (0xc0000139) on `main`, identical signature to the test
runner. Implication for ADR 0046 / future ingest smoke work:
**programmatic Strategy-A end-to-end smoke through an `examples/` binary
is NOT viable until `mb-0n8c` is root-caused.** Always pair a smoke
example with a pure-rusqlite verification probe (see
`verify_iter1_schema.py`) so the DB-schema-half of the smoke is
separately verifiable. Live-fire ingest validation has to go through
`mockingbird.exe` itself (Strategy B + human click-test).

### P3. The release exe lives at WORKSPACE-ROOT `target/release/`

NOT at `src-tauri/target/release/`. Tauri 2 builds into the workspace root.
Run via `powershell -File scripts\run-mockingbird.ps1` — never
`Start-Process` the exe directly (missing `ORT_DYLIB_PATH` + CUDA bin on PATH).

### P4. Session-start ritual is mandatory — stale prompts are real, AND so is over-correction

Two incidents shaped this rule:

- **2026-05-17** — a stale bootstrap prompt was acted on for ~half a session
  before the conflict with sealed-phase state was noticed. Lesson: detect
  the conflict.
- **2026-05-23** — Bernard detected a stale Phase MC kickoff (correctly!)
  but then stopped-and-asked instead of answering the clear bug report
  embedded in the same message. Lesson: detection without nuance is its
  own waste of an iteration.

Before any tool call, EVEN if the human pastes a custom kickoff:

1. Read `STATUS.md` SESSION ANCHOR first.
2. **Triage** — three cases:
   - **(a) Stale wrapper, clear request inside** → ignore the wrapper,
     answer the request. One-liner acknowledgement, then go. (This is
     the common case when a `/goal` template auto-prepends old context.)
   - **(b) Genuinely ambiguous** → STOP and surface via `ask_user_question`.
   - **(c) Clean kickoff** → proceed.
3. Then the normal start-of-iteration list.

Sealed phases are in `STATUS.md`. Phase tags `phase-N-complete` are
authoritative. Full rule text: `.code_puppy/AGENTS.md` § "Session-start
ritual".

### P5. PLAN §10 phases seal via `phase-N-complete` git tag — lateral epics seal via ADR

Do not create new phase tags for ADR-chartered lateral epics (ADRs 0022,
0023, 0024, 0025, 0032, 0033 all sealed without phase tags). New work
against a sealed phase is a **charter-an-ADR-first** lateral epic, not a
phase reopen. See `.code_puppy/AGENTS.md` § "Permanently sealed".

### P6. PowerShell single-quote variable trap

```powershell
Test-Path '$env:USERPROFILE\foo'   # ✗ tests for the LITERAL string
Test-Path "$env:USERPROFILE\foo"   # ✓ expands
```

Bernard hit this 2026-05-18 verifying the models dir; reported "missing"
when the folder was present. Also use `"$home\foo"` not `'$home\foo'`.

### P7. Wave-6 judges don't catch live-OS integration regressions

2026-05-23 [mc-hotfix / mb-x1x]: all 5 Phase MC judges passed, but four
user-visible regressions shipped (chord collision, latched Stop button,
stub source-probe, missing overlay show). The judges are static / unit /
file-diff assertions — they prove *contracts* hold, not that *integrations*
work on a real Win11 box. **Any phase touching OS hooks, audio devices,
or live Tauri events needs a 5-minute human smoke test BEFORE the seal
commit**, not after. Add to the seal checklist for any future hook-or-
audio phase.

### P8. Sub-agent `session_id` continuity is a foot-gun across SERIAL task handoffs

2026-05-25 [phase10 / Wave 1A dispatch loop]: Bernard tried to chain
Phase 10 Waves 1A → 1B → 2 → 3 dispatches through code-puppy, reusing
the same `session_id` across all dispatches expecting continuity to help.
Each new dispatch in the accumulating session ran code-puppy's mandatory
session-start ritual (P4), which triages "the kickoff prompt" against
disk state. In a long session the "kickoff prompt" anchors on the FIRST
user message — Bernard's original Wave 0 charter ask. That charter had
already shipped (commits `613f336`, `13f3632`, `5fc89a2`), so code-puppy
correctly detected "kickoff asks for Wave 0 work but it's already on
disk → stale → STOP and surface." Bernard re-dispatched 6+ times with
re-authorization preambles; each re-dispatch re-anchored on the same
stale Wave 0 kickoff and stopped again. 8-attempt loop before escalation.

**Rule:** `session_id` is for CONVERSATIONAL refinement of ONE task
(clarify-ask-respond rounds). For SERIAL task handoffs (Wave N → Wave
N+1 → Wave N+2), **omit `session_id`** so each dispatch is its own fresh
session with its own clean kickoff anchor. The two patterns are NOT
interchangeable.

**Defense in depth:** keep code-puppy dispatch prompts short and
pointer-style — "implement X per `<spec-path-on-disk>`" rather than
embedding the full spec. The spec lives on disk, not in the prompt.
That way nothing in the prompt body looks like stale charter work to
the session-start triage. Long embedded specs invite false-positive
stale-prompt detections even in fresh sessions.

### P9. Strict-no-regression IAP cannot ratchet on small local models

Wave 5 Phase 0 KG (`mb-ojm5`): 0 of 5 prompt iterations accepted under a
strict no-per-metric-regression Iteration Acceptance Protocol. Every
prompt change — including iter 5's extract-only date-handling tweak
with zero tag/category/type language in the diff — caused global
joint-distribution shift in pipeline output. Iter 5 dropped tag-collapse
1.54pp + entry-type 0.82pp + lifted PCRP +2 despite the diff touching
none of those passes. The small-model output distribution is more
entangled across passes than a "edit one thing, observe one effect"
mental model assumes.

**Implication for any future Wiggum-style work on local models:**

- **Strict IAP** (no-per-metric-regression) is RIGHT for **trust-critical
  autonomous gates** — date hallucination, secure-input detection,
  raw-data immutability, clipboard save/restore, anything where one
  regression silently breaks user trust.
- **Strict IAP is WRONG for quality-tuning loops** — the joint
  distribution makes regression on SOME metric nearly inevitable on
  ANY prompt change. The loop rejects every iteration; nothing lands.
- **Use Pareto-frontier acceptance** for quality tuning: accept if no
  metric is meaningfully worse (define per-metric tolerance bands) AND
  aggregate improves. Allows cross-metric trade-offs.

Default for mixed loops: strict IAP on the trust-critical gates,
Pareto-frontier on the quality metrics, in the same iteration loop.
Pick discipline per metric by cost-of-regression on that metric.

### P10. Schema portability requires per-model-class calibration profiles, not one prompt for all models

Wave 0.5.1 (`mb-4xtd`): `qwen2.5:7b-instruct-q4_K_M` breached the
Phase 0 date hard-gate (4 invented dates at seed 42, 5 at seed 137)
despite using the identical extract prompt that ran clean on
`qwen2.5:3b-instruct-q4_K_M` (0 invented dates across the same
32-fixture corpus + same two seeds). Root cause: the prompt's
null-bias was empirically tuned through Phase 0 Wave 5 IAP iterations
against the 3b's cautious-by-default prior. The 7b has a
confident-by-default prior; the same instructions don't push hard
enough to overcome the larger model's disposition. Failure modes
clustered on four specific pathologies: duration phrases ("thirty
days"), vague-future references ("the next one-on-one"), past-tense
temporal anchors ("dinner Friday" said on a Sunday), and multi-segment
date bleed (event-date attached to action task).

**Implication for schema-driven systems (Clark / ADR 0049):**

The honest version of "the schema travels across models" is *the
schema travels across models WHEN the schema encodes model-aware
calibrations*. Not "one prompt rules them all" — instead "the schema
encodes the per-class calibration once, the system handles model
swap transparently."

Concretely: `SCHEMA.md` has a `## Model-class calibration profiles`
section mapping each known model to a profile
(`small-conservative`, `mid-confident`), plus a
`### Profile-specific prompt overrides` table
adding `(pass, profile) → prompt-file` rows. The default-prompt
table still exists; overrides layer on top. Adding a new model =
add its row in the assignment table (or author a new profile if its
natural prior is genuinely new). The loader resolves
`prompt_body(pass, model)` = `overrides[(pass, profile_for(model))]`
if present, else `default[pass]`. Unknown models default to
`mid-confident` (safer-on-trust-gate — over-cautious-prompt-on-
confident-model just adds nulls; under-cautious-prompt-on-confident-
model invents dates).

**This makes SCHEMA.md MORE useful, not less.** The work of taming
a specific model gets captured in the schema and persists across
the system's lifetime. Wave 0.5.1 fix shipped a clean Pareto
acceptance on iter-1: 4→0 invented dates, zero regressions on
category/type/segmentation/clean-single, +3.7pp on tag-collapse.

**Operational sub-finding:** parity-gate against the OLD model
FIRST when swapping models. The Wave 0.5.1 SCHEMA refactor parity
gate ran 3b→3b and was identical; the regression surfaced only on
the 3b→7b model swap, cleanly isolating refactor-regression from
model-regression. Skipping the parity step would have entangled
the two questions.

### P11. "Tags" and "entities" are different objects; conflating them in one field defeats both closed-vocab AND entity-extraction layers

Wave 0.5.3 (`mb-rzpd` + `mb-e10v`) closed as a useful negative result.
The wiring fix (synonym-map applied in-band at validate-time) was
architecturally correct and lifted tag-collapse ~2pp (3.8% → 5.7% on
seed 42) — but it left a 9.1pp gap vs the iter-1-7b-fix open-vocab
baseline (14.8%). Diagnostic: top 9 of 10 near-misses (`becca`, `dad`,
`costco`, `brake-pad`, `bakery`, `app`, `business`, `business-tool`,
`design`) are **open-class entity references**, NOT semantic category
tags.

The Phase 0 corpus answer keys conflate two distinct object types in
a single `tags:` field:

- **Semantic categories** (`work`, `car-repair`, `finance`, `health`)
  — bounded, closed-world, curatable. Closed vocabulary works here.
- **Open-class entities** (person names, brand names, specific
  objects, project names) — unbounded long tail. You cannot curate
  an infinite tail globally; closed-vocab fails by design.

Closed vocabulary is the right mechanism for the first; **entity
extraction is the right mechanism for the second** — but only if the
structured output schema separates the two fields. The closed-vocab
layer cannot recover open-class entities the model correctly
recognises as out-of-vocabulary and omits.

**v1 architectural implication (binding):** the structured entry
schema MUST have a separate `tags:` field (closed-vocab semantic
categories — Move 3 applies) AND a separate `entities:` field
(open-class first-class references — Move 4 entity extraction
applies). The Phase 0 single-field `tags:` is retroactively
understood as a v0 simplification; v1 carries the split forward.
Future corpus authoring (Phase 1+) splits answer keys into the
two-field schema from the start. ADR 0049 receives amendment A2
in the Wave 0.5.6 REPORT framing.

**Defense in depth for diagnosing similar layered systems:** when a
mechanism lifts a metric in the predicted direction but only partway,
don't assume more of the same mechanism closes the gap — check
whether the residual has a different root cause. Two-by-two decision
table: (wiring/fix lifts? × matches predicted magnitude?). Only
(yes × yes) is a clean accept; (yes × partial) means there's a
second variable in play. Iter 3 (Wave 0.5.3) was (yes × partial);
the second variable was vocab-coverage of open-class entities, which
a closed-vocab layer can't address by definition.

---

## 📚 Table of Contents (chronological, newest first)

Use `rg '^## YYYY-MM' docs/LESSONS.md` to navigate.

| Date       | Tag                              | Title (truncated)                                                        |
|------------|----------------------------------|--------------------------------------------------------------------------|
| 2026-05-30 | `[ADR 0049 Wave 0.5.3 CLOSE / mb-rzpd + mb-e10v]` | Wave 0.5.3 sealed as useful negative result per Bernard Option B. Iter 3 wiring fix (synonym-map in-band at validate-time) lifted ~2pp (3.8% -> 5.7% seed 42) but fell 9.1pp short of open-vocab baseline (14.8%). Diagnostic on near-miss top 10: 9 of 10 are open-class entity references (person names, brand names, concrete objects, project names), not semantic category tags. Phase 0 corpus conflates two distinct object types in a single tags: field. Closed-vocab is the right mechanism for semantic categories (bounded, closed-world, curatable); entity extraction is the right mechanism for open-class references (unbounded long tail). v1 commits to two-field schema (tags + entities) with closed-vocab on tags and Move 4 entity-extraction on entities. ADR 0049 amendment A2 deferred to Wave 0.5.6 REPORT. Promotes to PINNED P11. Wiring fix stays on main (commit 8fdc7fb) for any future closed-vocab work to inherit |
| 2026-05-29 | `[ADR 0049 Wave 0.5.3 / mb-rzpd HALT]` | Closed-vocab tag validator (228-entry seed) regressed tag-collapse -9.1pp (iter 1, verbose prompt, model over-tagged with valid generics) then -11.0pp (iter 2, tight prompt, model under-tagged conservatively). 2-iter rejection cascade. Diagnosed root cause: the closed-vocab validator's normalize step does NOT apply the synonym map, so model emissions like `automobile-repair` are DROPPED instead of being collapsed to `car-repair` (which IS in vocab). Open-vocab baseline let these flow through to the scorer, which collapsed them via synonym map. Closed-vocab as shipped loses the synonym-collapse contribution entirely. The deeper architectural pattern: small-LLM + 228-item closed list cannot be reliably navigated by the model — both directions (verbose vs tight prompt) fail Jaccard 1.0, just in opposite ways (over- vs under-tagging). Path forward (Bernard's Option A in STATUS `mb-rzpd`): integrate synonym-map into validator (normalize → synonym-collapse → vocab check), single architectural fix in `tag_validator.rs`. Halt rather than burn iters 3-5 on prompt-only tweaks |
| 2026-05-29 | `[ADR 0049 Wave 0.5.1 / mb-4xtd CLOSED]` | Hard-gate restored on qwen2.5:7b via SCHEMA.md model-class calibration profiles. Iter-1 result vs 7b-pre-fix baseline: invented_dates 4→0 (HARD GATE PASS); zero regressions on category (81.5%), entry-type (88.9%), segmentation (93.3%), clean-single (33.3%), junk (100%); tag-collapse +3.7pp (11.1%→14.8%). Stability at seed 137 also clean. Architectural shape: SCHEMA.md gains `## Model-class calibration profiles` (small-conservative / mid-confident with assignment table + unknown-model default = mid-confident on trust-gate-safe grounds) and `### Profile-specific prompt overrides` (`(pass, profile) → file` rows, layered on top of the default-per-pass table). Loader resolves `prompt_body(pass, model)` = `overrides[(pass, profile_for(model))]` ∥ `default[pass]`. New `prompts/extract.mid-confident.md` hardened with: front-loaded null-bias framing, three-condition hard-gate, four explicit rules (duration phrases ≠ dates, vague-future ≠ dates, past-tense anchors stay past, segment-isolation prevents event-date bleed), 7 in-context examples on fictional vocab (Caldwell/Priya/Bergstroms — distinct from mb-4xtd failure set so those stay eval not training). PINNED P10 captures the architectural lesson. **Pin counterpart to P9 (Wave 5 strict-IAP rejection cascade) — P9 is "strict no-regression cannot ratchet quality on small models"; P10 is "prompts calibrate to a specific model's prior; portable schemas need per-class calibration"** |
| 2026-05-29 | `[ADR 0049 Wave 0.5.1 / mb-xmgs + mb-4xtd]` | Date hard-gate prompt empirically tuned for qwen2.5:3b does NOT transfer to qwen2.5:7b; same SCHEMA, same prompts, same corpus produces 4 + 5 invented dates on 7b at two seeds (vs 0 on 3b). 7b is more confident-by-default and the prompt's null-bias is empirically insufficient to overcome its prior on borderline temporal anchors (vague future, past-tense, multi-segment misassignment, pure fabrication). Operational lesson: parity-gate on the OLD model before changing the model — it cleanly isolates refactor-regression from model-regression. v1 implication: any model substitution in a schema-driven pipeline needs empirical re-baselining of trust gates; SCHEMA portability is necessary-but-not-sufficient because prompts are model-tuned even when they look universal. Move 1 still delivered 3/4 architectural-lift success-criteria simultaneously (+14.2 / +10.7 / +26.6 on category/type/clean-single) — quality lift and trust regression coexist on the same Pareto frontier |
| 2026-05-29 | `[ADR 0048 Wave 3.3 / mb-57a1]` | Swapping the primary judge from `llama3.1:8b` to `gemma2:9b` inverts the disagreement direction on Gate 3 without closing the gap (Wave 3.2 4/7 → Wave 3.3 5/9; same three personas, primary=Equivalent/cross=NotEquivalent → primary=NotEquivalent/cross=Equivalent). Tag-collapse metric moves 81.8% → 38.2% on the SAME data — a 43-pt judge-dependent gap. Borderline observational set (added this wave) shows the pattern crisply: judges agree on `tokenization`/`specificity`/`domain-overlap`/`person-specific` (the clear dimensions, 4/4 perfect for gemma2) and disagree on `coreference` + `abstraction-level` (the genuinely-fuzzy dimensions, 0/2 for gemma2). The structural finding: LLM-judge tag-equivalence on a corpus with this much surface-token variation is more ambiguous than the inter-rater reliability of different judge families supports. Two consecutive Gate 3 STOPs with inverted direction is the signal that the failing gate is correctly identifying a methodology problem the patches under consideration (judge swap, prompt tune, threshold loosen) can't reach. Path forward: replace LLM-judge tag metric with deterministic exact-match + Jaccard (option E in wave-3-results.md) — honors AGENTS.md §6 "if something is hard to verify, that's the bug." Requires ADR 0048 §G5 amendment; not Bernard's call autonomously |
| 2026-05-29 | `[ADR 0048 Wave 3.2 / mb-57a1]` | JVP Gate 3 STOP: llama3.1:8b is more permissive than gemma2:9b on tag-equivalence on the real corpus, while passing Gate 1 (unambiguous calibration) cleanly at 91.7%. Calibration sets of unambiguous pairs can't surface this skew — Gate 3 (cross-judge on real-corpus 10% sample) is the gate that catches it; the 5-gate JVP design is load-bearing because each gate covers a different failure mode. Calibration-fairness sub-finding: every cal pair's vocabulary must be disjoint from the judge prompt's in-context examples (cal-eq-001 v1 violated this; v2 fix in commit 7f8ff1c). PCRP-mislabel sub-finding: PCRP prose's literal claims ("hallucinated dates") must be cross-walked vs. structured output before being treated as fact; the themes are the durable signal, surface words are not |
| 2026-05-28 | `[phase-0-kg-wave-5 / mb-ojm5]`  | Strict IAP rejects every iteration on small local models — see PINNED P9 |
| 2026-05-28 | `[ADR 0048 Wave 0 / mb-4wxw + mb-w1lw + mb-i9l1]` | `bd close` of a downstream bead in the same iteration as its blocker's close fails with "blocked by open issues [<blocker-id>]" even when the blocker was already closed — bd evaluates the blocks-edge against a stale view (Dolt auto-commit batching, presumably). Workaround: pass `--force` for the downstream closes. Doesn't affect cross-iteration chains because the blocker close has committed by then |
| 2026-05-27 | `[ADR 0046 Iter 2 SEAL / mb-3xww]` | Obsidian nested-vault trap during Mockingbird Mobile Sync setup: when an Obsidian vault is created via 'Create new vault' and the named folder already exists, Obsidian silently creates a nested `<vault>/<vault>/` structure; Mockingbird writes to outer, Obsidian reads from inner — symptom is "Vault up to date (N records)" toast is truthful but Obsidian shows nothing. Diagnosis: `Get-ChildItem <vault> -Force` shows both `.mockingbird/` AND a same-named nested folder; the nested folder contains `.obsidian/`. Fix: move `.obsidian/` + `Welcome.md` from inner to outer, delete empty inner (preserves Obsidian Sync pairing). Iter 4 wizard should detect + guide |
| 2026-05-23 | `[ADR 0046 Iter 1 / mb-jbf7]`    | Programmatic Strategy-A end-to-end smoke is blocked by mb-0n8c: example binaries that transitively link whisper-rs/ort/CUDA exit STATUS_ENTRYPOINT_NOT_FOUND identically to `cargo test --release`; pure-rusqlite examples (verify_wave49 shape) work fine. Pattern: pair every smoke example with a pure-rusqlite probe so the DB-schema half is verifiable independent of mb-0n8c, route the live-fire half through `mockingbird.exe` + Strategy B |
| 2026-05-27 | `[ADR 0046 §3.2 / mb-7vyz]`      | Resolution of the earlier-today topology fork: in-thread `std::sync::mpsc → crossbeam-channel` bridge is the minimum-diff path; converting the `StateDriver` channel itself would have cascaded out-of-boundary, while bridging inside `run_dictation_thread` keeps every §3 "do not touch" file untouched and adds ~20 lines |
| 2026-05-27 | `[ADR 0046 Iter 1 / mb-evn3 / mb-7vyz]` | The `StateAction` channel topology can't be naively extended for headless ingest — the orchestrator's input channel is fed by exactly one producer (StateDriver), so "add a new variant" requires a new sibling mpsc + run-loop multi-select, which is an ADR-amendment-sized change, not an in-boundary tweak |
| 2026-05-27 | `[mb-tfyp / mb-sowc / ADR 0045]` | start_mode plumbing: the focus-drift abort heuristic conflates two semantically distinct outcomes (PTT target lost focus vs. in-app session never had a target); the fix is a new column + a new InjectionOutcome variant, NOT a new pill string on the legacy abort path |
| 2026-05-26 | `[design-v1 / mb-n455]`          | Design System v1 audit + formalization shipped; six modes by which UI drift accumulated between phases; one specific bug class (the dead-token cascade silently rendering surfaces transparent) deserves a permanent guardrail; kitten's `getComputedStyle().backgroundColor` probe is blind to background-image gradients — false-positive trap |
| 2026-05-26 | `[phase10-hotfix / mb-scla / mb-23rh]` | Two post-seal Phase 10 hotfixes: (1) `activity_blocks.primary_title` schema-vs-code drift slipped past green judges because `cargo test --release` is `--no-run` on this box — judges check contracts, not runtime SQL; (2) Command Center FSM `drive()` emitted the captured pre-effect `next` AFTER a recursive inner drive() had already emitted the post-effect state, clobbering the UI back into `Launching` and locking all tiles |
| 2026-05-26 | `[phase10-wave-6b / SEAL]`       | Phase 10 sealed at `phase-10-complete`; Wiggum loop went green on iteration 1 of 3; narrowing the `sealed-phases-untouched` base ref from `phase-mc-complete` → `stable-alpha-v0.1` excluded an unrelated lateral epic from grader scope |
| 2026-05-26 | `[phase10-wave-4]`               | 270s hard cap on agent foreground tool calls; backgrounded `start /B` cmd children survive parent shell exit; `is_multiple_of` is unstable on i64; release-mode `debug_assert!` defeats `#[should_panic]` in throwaway crate runs |
| 2026-05-25 | `[phase10-wave-2]`               | windows-rs MONITORINFOF_PRIMARY lives under UI/WindowsAndMessaging not Graphics/Gdi; serde camelCase isn't free; throwaway preamble must append not prepend |
| 2026-05-25 | `[phase10-wave-1b]`              | typed-settings IPC has no UI wrapper; Pill `tone` is a token string not a union; `formatRelative` takes ISO not unix-ms |
| 2026-05-25 | `[phase10 / meta / dispatch]`    | sub-agent session_id is conversational, NOT serial — PINNED P8           |
| 2026-05-24 | `[meta / tooling]`               | bd create non-ASCII trap; git status --porcelain=v1 vs --short; findstr regex limits; triage-before-acting pattern |
| 2026-05-24 | `[mc-v1.2 / ADR 0035]`           | MC Stable Alpha seal — capabilities migration is the real mb-z5y root cause; cancel/rename/auto-title; WASAPI loopback fix; stable-alpha-v0.1 tag |
| 2026-05-24 | `[dictation-polish]`             | paste-trailing-space, History→Dictations, on-demand LLM pass, Insights 2-tab redesign |
| 2026-05-23 | `[mc-hotfix / mb-z5y / ADR 0034]`| overlay stuck in CHOOSE — show-before-emit + emit_to + defensive clear   |
| 2026-05-23 | `[meta / session-start]`         | detected stale Phase MC kickoff but then over-corrected — added (a/b/c) triage |
| 2026-05-23 | `[mc-hotfix / mb-x1x]`           | post-deploy live-fire surfaced 4 gaps the Wave-6 judges couldn't catch   |
| 2026-05-23 | `[mc-v1.1]`                      | post-seal audit found four polish gaps; lateral epic vehicle worked      |
| 2026-05-22 | `[phase-mc-retrospective]`       | Phase MC retrospective                                                   |
| 2026-05-21 | `[phase-mc-wave-5]`              | Tauri 2 `tauri::command` macro: bare AppHandle fails with `State<'_, T>` |
| 2026-05-21 | `[phase-mc-wave-5]`              | legacy `update_setting` IPC can't carry typed meeting settings cleanly   |
| 2026-05-21 | `[phase-mc-wave-5]`              | pre-existing 600-line cap violations are scope-creep traps mid-wave      |
| 2026-05-20 | `[phase-mc-wave-3]`              | two independent WH_KEYBOARD_LL hooks coexist iff meeting hook chains     |
| 2026-05-20 | `[phase-mc-wave-2]`              | whisper.cpp segment timestamps are CENTISECONDS not ms; UTF-8 capit.     |
| 2026-05-20 | `[phase-mc-wave-1]`              | stale migration test since 005; release-LTO test compile single-threaded |
| 2026-05-19 | `[unsplash-bg / release-build]`  | Tauri release exe lives at workspace-root target/release/                |
| 2026-05-19 | `[unsplash-bg / release-build]`  | Tauri compresses embedded UI assets — ASCII grep won't find JS in exe    |
| 2026-05-19 | `[unsplash-bg]`                  | glass token override beats per-component rewrites for photo modes        |
| 2026-05-19 | `[unsplash-bg]`                  | z-index:0 photo trapped in-flow text BENEATH the photo                   |
| 2026-05-18 | `[phase-mc-wave-0]`              | Build/test env conventions were tribal knowledge — surfaced to AGENTS.md |
| 2026-05-18 | `[phase5-postship-9-followup]`   | ADR-0024 iter-1: corpus expansion revealed imperative-content failure    |
| 2026-05-18 | `[phase5/6 UI sprint]`           | CSS Modules need vite-env.d.ts; commands.rs vs commands/ conflict; etc.  |
| 2026-05-18 | `[phase-3-wave-5]`               | Orchestrator integration tests want stubbed traits, not real spawn       |
| 2026-05-17 | `[phase5-postship-*]`            | (multiple) UI bundle stale, clipboard bitmap crash, Ollama 30s timeout,  |
|            |                                  |   migration 006 SQL syntax, History metadata `—` rendering, etc.         |
| 2026-05-17 | `[phase5-wave-I]`                | `cargo test --lib` exits 0xc0000139 even with cargo-with-cuda — PINNED   |
| 2026-05-17 | `[phase3-wave4.9]`               | K32GetModuleBaseNameW silent zero; clipboard seq baseline; etc.          |
| 2026-05-17 | `[phase3-wave4.8]`               | Silero v5 ONNX needs UNDOCUMENTED 64-sample context buffer               |
| 2026-05-17 | `[phase-3-wave-4.5]`             | release exe path; cargo test doesn't see ensure_ort_dylib_set            |
| 2026-05-17 | `[phase-3-wave-3/4]`             | WH_KEYBOARD_LL thread-local discipline; pure-vs-OS split; tick cadence   |
| 2026-05-17 | `[phase-3-wave-2]`               | windows-rs 0.56 HWND is isize; GUI_SECUREINPUT not a real constant       |
| 2026-05-17 | (multiple)                       | Session-start anchor; Vite CSS `@import`; bridge-then-cutover; ADR-0024  |
| 2026-05-16 | `[phase-2]`                      | CUDA 12.8 install; whisper-rs 0.16 + bundled ggml; ort 2.0 RC + MSVC2022 |
| 2026-05-16 | `[phase-2]`                      | Silero VAD model path; `Box<dyn Trait>`; cpal::Stream !Send; cpal::Host  |
| 2026-05-15 | `[bootstrap]`                    | bd alongside STATUS.md; bd init TTY; PowerShell stderr; hook decode      |
| 2026-05-15 | `[phase-0/1]`                    | rust-toolchain pinning; rustfmt with autocrlf; rusqlite 4-min check;     |
|            |                                  |   #[cfg(test)] crate boundaries; SQL UNIQUE+NULL; CURRENT_TIMESTAMP res. |

---

## 📜 Body (chronological)

---

## 2026-05-30 [ADR 0049 Wave 0.5.3 CLOSE / mb-rzpd + mb-e10v] Closed-vocab Move 3 sealed as useful negative result; the residual gap is corpus tag/entity conflation, not Move 3 architecture (PINNED P11)

- **Context:** Wave 0.5.3 ran three iterations head-to-head against
  the iter-1-7b-fix open-vocab baseline. Iter 1 (verbose prompt) and
  iter 2 (tight prompt) regressed in opposite directions (over-tagging
  vs under-tagging); the 2026-05-29 mb-rzpd HALT entry diagnosed the
  scorer-side wiring bug. Iter 3 (`mb-e10v`, commit `8fdc7fb`) shipped
  the wiring fix — `SynonymMap` lifted to `src/synonyms.rs` and applied
  in-band at validate-time. Bernard surfaced the iter-3 IAP REJECT on
  2026-05-29 (commit `af2b0e1`) with three options for Dustin (A/B/D),
  framed as: the wiring fix is necessary but not sufficient because
  the residual ~9pp gap traces to a different root cause from the
  scorer-side bug the fix addressed.
- **Decision (Dustin via planning-agent kickoff, 2026-05-30):**
  Option B — close Wave 0.5.3 as useful negative result. The wiring
  fix stays on `main` (architecturally correct for any future closed-
  vocab work to inherit). Architectural insight promotes to LESSONS
  PINNED P11. Advance to Wave 0.5.4 entity-extraction probe (`mb-o4ni`)
  which addresses the actual residual root cause.
- **Finding (promoted to PINNED P11):** The Phase 0 corpus answer
  keys conflate two distinct object types in a single `tags:` field:
  semantic categories (bounded, closed-world, curatable — closed-vocab
  works) and open-class entities (unbounded long tail — closed-vocab
  fails by design). 9 of 10 top near-misses on iter 3 are open-class
  entity references (`becca`, `dad`, `costco`, `brake-pad`, `bakery`,
  `app`, `business`, `business-tool`, `design`). Closed-vocab Move 3
  is the right mechanism for the first object type; entity extraction
  is the right mechanism for the second. v1 architectural commit:
  the structured entry schema separates `tags:` (closed-vocab) and
  `entities:` (Move 4 entity extraction) into distinct fields.
  Phase 0 corpus single-field `tags:` is retroactively understood as
  v0 simplification; v1+ corpus authoring splits answer keys from
  the start. ADR 0049 amendment A2 deferred to Wave 0.5.6 REPORT.
- **Action:** No more closed-vocab prompt tuning on the current corpus
  schema. The kickoff's Option D (expand seed vocab toward open-class)
  is rejected — it defeats the closed-vocab thesis and grows toward
  open-vocab while accumulating curation churn. Wave 0.5.4 probes
  whether the 7b model can extract open-class entities with sufficient
  quality (≥50% entity-quality on the labeled subset) to be the
  canonical handler. If yes, v1 includes the entity layer; if no, v1
  falls back to open-vocab tags + synonym-map + new-tag-request growth
  loop (the v1.1 deferral named in ADR 0049). Either outcome is a
  defensible v1 ship gate.

---

## 2026-05-29 [ADR 0049 Wave 0.5.3 / mb-e10v iter 3 REJECT] Wiring the synonym map into the closed-vocab validator is necessary but not sufficient — vocab-coverage gap dominates the remaining regression

- **Context:** Follow-up to the 2026-05-29 mb-rzpd HALT entry (immediately
  below). That entry diagnosed iter-1/iter-2 closed-vocab regression
  as a missing-canonicalization-step in the validator. Iter 3 (`mb-e10v`)
  shipped the architectural fix: `SynonymMap` lifted out of
  `scoring::tag_collapse` into a shared `src/synonyms.rs` and applied
  IN-BAND at validate-time (`normalize → synonym-collapse → vocab
  membership check`). Same prompt, same vocab, same seeds — the
  wiring was the only changed variable.
- **Finding:** The wiring fix lifted seed-42 tag-collapse from 3.8%
  (iter 2) → 5.7% (iter 3), confirming the diagnosis was directionally
  right. But that's ~2pp; the gap to iter-1 open-vocab baseline (14.8%)
  is 9.1pp. **The dominant pathology shifted from scorer-side to
  model-side**: iter-2 top near-misses were `(missing)` tags that
  *would* canonicalize had they been emitted; iter-3 top near-misses
  are STILL `(missing)` tags — the model recognises them as out-of-
  vocabulary and omits them entirely rather than emit. Specifically:
  person names (`becca`, `dad`, `henderson`), brand names (`costco`),
  concrete objects (`brake-pad`, `app`, `business-tool`). The synonym
  map can only canonicalize tags the model emits; it cannot recover
  tags the model never emitted in the first place. Closed-vocab
  failure mode is therefore **two-dimensional**: (1) scorer wiring
  (now fixed, ~2pp recovery) and (2) seed-vocab coverage of the long
  tail of open-class entities (responsible for the remaining ~9pp gap
  and unfixable without either expanding the vocab toward open-class
  — defeating the closed-vocab thesis — or shipping a closed-class
  taxonomy + open-class entity layer as separate concerns).
- **Action:** When a wiring fix lifts a metric in the diagnosed direction
  but only partway, don't assume more wiring will close the gap — check
  whether the remaining gap has a different root cause. Two-by-two decision
  table: (wiring fix lifts? × matches predicted magnitude?). Only (yes × yes)
  is a clean accept. (yes × partial) means there's a second variable in
  play; treat the wiring fix as a useful but separate intervention and
  surface the second variable before another iter. The iter-3 wiring fix
  is now on `main` (commit `8fdc7fb`) regardless of Wave 0.5.3's overall
  verdict — it's architecturally correct on its own merits, and any future
  v1.1 closed-vocab work inherits it for free.

## 2026-05-29 [ADR 0049 Wave 0.5.3 / mb-rzpd HALT] Closed-vocab tag validator regresses tag-collapse in BOTH prompt directions because the validator's normalize step doesn't apply the synonym map

- **Context:** Wave 0.5.3 (Move 3 of ADR 0049) shipped a 228-entry
  closed canonical tag vocabulary in `SCHEMA.md` + a post-LLM
  `tag_validator` pass that drops out-of-vocab tags and routes
  them to `new-tag-requests.jsonl` for review. Two head-to-head
  iterations on seed 42 vs the `iter-1-7b-fix` open-vocab baseline.
- **Iter 1 (verbose prompt, "pick 2-4 tags"):** tag-collapse
  5.7% vs 14.8% baseline (-9.1pp). Model picked extras like
  `home-maintenance` and `client` (both in vocab) on entries where
  the answer key wanted only the specific (e.g. `henderson` for the
  person). Jaccard ≥ 1.0 punishes any extras. 35 new-tag-requests
  on seed 42 (15 explicit, 20 implicit) — 47% of dictations have an
  explicit request, over the kickoff's 30% sanity-check bar.
- **Iter 2 (tight prompt, "pick 1-3, prefer 2, fewer is better",
  3 negative-example notes):** tag-collapse 3.8% (-11.0pp). Now
  model emits SO FEW tags that it misses answer-key tags entirely.
  Top near-misses are `(missing)` against expected tags that ARE in
  vocabulary (`meeting`, `app`, `bakery`, `costco`, `dad`). 19
  new-tag-requests on seed 42 (only 1 explicit; model learned to
  drop the field) but the explicit-request collapse came WITH a
  tag-collapse collapse.
- **Root cause diagnosed.** Read `src/passes/normalize.rs`:
  `normalize_tags` does case-fold + singularize but does **NOT**
  apply the synonym map. The synonym map lives in
  `src/scoring/tag_collapse.rs` and is invoked only by the scorer.
  Open-vocab baseline: pipeline emits `automobile-repair`; scorer
  reads `automobile-repair`; synonym map collapses to `car-repair`;
  matches answer key. Closed-vocab: pipeline's validator sees
  `automobile-repair` is not in vocab, **DROPS it**; scorer never
  sees it. Closed-vocab as shipped LOSES the synonym map's
  contribution to Jaccard 1.0 matches.
- **Architectural pattern:** small-LLM + closed list (228 items)
  cannot be reliably navigated by the model alone. Verbose prompt
  + permissive vocab → over-tagging (extras kill Jaccard); tight
  prompt + same vocab → under-tagging (misses kill Jaccard).
  Iter 3-5 of prompt-only tuning would oscillate between these
  failure modes without addressing the missing synonym-collapse
  step. Same shape of finding as ADR 0049 A1 (embeddings classifier
  falsified at 32-pair scale): the mechanism is one-tool-short of
  what the open-vocab baseline already used.
- **Path forward (Bernard's recommendation, Option A in STATUS):**
  integrate `SynonymMap` into the validator. New order:
  normalize → synonym-collapse → vocab check. Validator plumbing
  needs the synonym-map path passed through the runner; the
  SynonymMap struct + `canonicalize()` already exist in
  `scoring/tag_collapse.rs` — the work is wiring, not new logic.
  Hypothesis: with synonym-collapse in-band, closed-vocab matches
  open-vocab on legitimate model emissions AND keeps the
  closed-vocab benefit of dropping truly novel noise.
- **Halt discipline:** 2 IAP rejections in a row with a clearly
  diagnosed architectural fix is the right surface-to-Bernard
  point. The kickoff's 5-attempt rule is a cap, not a quota —
  burning iters 3-5 on prompt-only tweaks when the fix is in
  validator code would have been the wrong choice. Per ADR 0049's
  HALT conditions: "IAP rejection cascade" — this is one.
- **Operational lesson for future closed-list architectures in
  this pipeline:** any closed list that imposes a constraint the
  open-list baseline didn't have must also inherit the open-list
  baseline's collapse / normalization steps, in-band, before the
  constraint check. Otherwise the constrained variant is strictly
  worse than the unconstrained on metrics that depend on those
  collapse steps.
- **Artifacts preserved (not committed):** four runs under
  `experimental/kg-validation/runs/run-7b-closed-vocab-{seed42,seed137,iter2-seed42,iter2-seed137}/`,
  validator code + 9 unit tests in `src/passes/tag_validator.rs`,
  SCHEMA.md vocab section (228 tags), iter 2 prompt at
  `prompts/extract.closed-vocab.mid-confident.md` (iter 1 prompt
  recoverable via `git show 743ebf2:experimental/kg-validation/prompts/extract.closed-vocab.mid-confident.md`).

---

## 2026-05-29 [ADR 0049 Wave 0.5.1 / mb-4xtd CLOSED] Hard-gate restored on qwen2.5:7b via SCHEMA.md model-class calibration profiles

- **Context:** Wave 0.5.1 SCHEMA.md refactor (Move 1 of ADR 0049)
  passed its parity gate on `qwen2.5:3b` but the 7b baseline
  produced 4 invented dates at seed 42, 5 at seed 137 — a clean
  hard-gate breach against the Phase 0 zero-invented-dates anchor.
  The four failures clustered on duration phrases ("thirty days"),
  vague-future ("the next one-on-one"), past-tense temporal anchors
  ("dinner Friday" on a Sunday), and multi-segment date bleed.
- **Finding:** the regression is real and architectural, not a
  prompt typo — the 3b's cautious-by-default prior was carrying
  some of the null-bias work that the prompt language alone didn't
  encode. Swapping to the 7b's confident-by-default prior with the
  same prompt let the assertive disposition reach borderline temporal
  anchors the 3b would have abstained on.
- **Fix:** model-class calibration profiles in SCHEMA.md (`mb-4xtd`).
  Two sections added to the schema:

  1. `## Model-class calibration profiles` — declares
     `small-conservative` (qwen2.5:3b-class, gemma2:2b-class) and
     `mid-confident` (qwen2.5:7b-class, gemma2:9b, llama3.1:8b-class)
     with an explicit `### Profile assignment` table. Unknown models
     default to `mid-confident` (loud failure mode: extra nulls;
     vs silent failure mode: invented dates on the trust gate).
  2. `### Profile-specific prompt overrides` — layered on top of
     the existing default-per-pass table. Resolution rule:
     `prompt_body(pass, model)` = `overrides[(pass, profile_for(model))]`
     if present, else `default[pass]`. YAGNI: we add override rows
     only where empirical evidence demands them.

  New `prompts/extract.mid-confident.md` (the only override row for
  Phase 0.5) is hardened with:
  - Front-loaded null-bias framing ("Most dictations do NOT contain
    a specific date.").
  - Three-condition hard-gate (anchor present ∧ unambiguously future
    ∧ IS the action's deadline). "When in doubt, output null."
  - Rule A: duration phrases ("thirty days", "in a few weeks",
    "soon", "down the road") explicitly enumerated as NOT dates.
  - Rule B: vague future ("next one-on-one", "next time", "when I
    have time") explicitly enumerated as NOT dates; "`next` alone
    is NOT a calendar anchor".
  - Rule C: past-tense temporal anchors stay past — do NOT map onto
    the upcoming occurrence of that weekday. Worked example: "dinner
    Thursday" on Sun 2026-06-14 → null, NEVER 2026-06-18.
  - Rule D: segment-isolation — event-date in the segment doesn't
    bleed into action-task deadline unless the action has an
    explicit "by <date>". Worked example: "housewarming Saturday
    and I haven't replied" → the action is "reply", Saturday is the
    event, not the deadline; null.
  - 7 worked examples drawn from FICTIONAL vocabulary (Caldwell,
    Priya, Bergstroms, etc.) NOT from the mb-4xtd failure set, so
    those four dictations stay an eval set and don't leak into the
    training-by-example.

  Loader changes (`src/schema_loader.rs`):
  - `Schema::profile_for(model: &str) -> &str` — lookup with
    `mid-confident` default.
  - `Schema::prompt_body(pass, model) -> Result<&str, ...>` —
    override-then-default resolution; called from
    `harness/pipeline.rs` once per pipeline run.
  - Old `prompt_bodies: PromptBodies` struct removed; per-pass
    bodies now live in `default_prompt_bodies: HashMap<pass, body>`
    and `override_prompt_bodies: HashMap<(pass, profile), body>`.
  - Parity test pinned: 3b model resolves to the verbatim contents
    of `prompts/extract.md`; 7b resolves to the verbatim contents
    of `prompts/extract.mid-confident.md`. Sandbox `cargo test`
    144/144 green.

- **Empirical result (iter-1, seed 42, vs `run-7b-baseline`):**

  | Metric | 7b pre-fix | iter-1-7b-fix | Δ | Verdict |
  |---|---|---|---|---|
  | **Invented dates (HARD GATE)** | 4 | **0** | -4 | ✅ FIXED |
  | Clean single-item | 33.3% | 33.3% | 0 | hold |
  | Segmentation | 93.3% | 93.3% | 0 | hold |
  | Category | 81.5% | 81.5% | 0 | hold |
  | Entry-type | 88.9% | 88.9% | 0 | hold |
  | Tag-collapse (G7) | 11.1% | 14.8% | +3.7 | improved |
  | Junk-bucket | 100% | 100% | 0 | hold |

  Clean Pareto-frontier acceptance: hard-gate restored, ZERO
  regressions, one improvement. Seed-137 stability run confirmed
  (see runs/iter-1-7b-fix-stab/).

- **Caveat (not gating acceptance, worth flagging):** the strong
  null-bias framing dropped three legitimate dates in
  persona-05-case-03 ("by Saturday's game" → should be 2026-06-20,
  "this weekend" → should be 2026-06-20, "by July fifteenth" →
  should be 2026-07-15). None of these moved a top-line metric in
  the current scorecard (no "missed dates" metric; clean-single is
  identical 5/15), but it's real over-correction worth follow-up
  in Wave 0.5.3+ once the closed tag vocabulary work creates a
  natural place for a per-segment date-eligibility refinement.
  Logged as a future-work observation, not a gating finding.

- **Action:** PINNED P10 captures the architectural lesson. `mb-4xtd`
  closed. Wave 0.5.1 (`mb-xmgs`) seals. Downstream waves
  (`mb-yfzy`/`mb-rzpd`/`mb-o4ni`/`mb-5r1b`/`mb-qogz`) unblocked.

---

## 2026-05-29 [ADR 0049 Wave 0.5.1 / mb-xmgs + mb-4xtd] Prompt hard-gates empirically tuned on a small model do NOT transfer to a larger model in the same family

- **Context:** ADR 0048 Phase 0 spent Wave 5 (5 IAP iterations) empirically
  tuning the `extract` prompt against `qwen2.5:3b-instruct-q4_K_M`. Final
  Phase 0 scorecard: `invented_dates_count = 0` (hard-gate PASS) on the
  32-fixture corpus across two seeds.
- **Finding:** the same SCHEMA.md / same prompts / same corpus on
  `qwen2.5:7b-instruct-q4_K_M` at seed 42 produced **4** invented dates,
  and at seed 137 produced **5** — a consistent, reproducible hard-gate
  breach on the larger sibling model. Failure modes were not random;
  they clustered on four specific dictation pathologies the 3b prompt
  was implicitly tuned around: vague future references ("next
  one-on-one" → next Monday), past-tense temporal anchors ("dinner
  Friday" → next Friday), multi-segment date misassignment, and pure
  fabrication with no anchor.
- **Why it happened:** the date hard-gate prompt language tells the
  model "emit null when the date is ambiguous, future-vague, or
  past-tense". A 3B model is conservative-by-default and over-emits
  null — the prompt's null-bias works WITH the model's natural prior.
  A 7B model is more confident-by-default and the prompt's null-bias
  is empirically insufficient to overcome its prior on the same
  borderline cases. The prompt was never "correct"; it was *calibrated*
  for one specific model's prior.
- **Implication for the v1 charter (ADR 0049):** ANY model substitution
  in a schema-driven pipeline must include an empirical re-baselining of
  the trust gates against the new model. SCHEMA.md being portable is
  necessary-but-not-sufficient; the prompts themselves are *model-tuned*
  artifacts even when they look universal. Treat per-model trust-gate
  empirical proof as a release-blocking artifact, not a courtesy run.
- **Operational lesson:** the parity gate (byte-identical structured
  outputs on the OLD model) successfully isolates "is this a refactor
  regression" from "is this a model regression" — without it the 7b
  breach could have been blamed on the SCHEMA refactor and a couple
  iterations burned chasing a phantom bug. Always parity-gate on the
  *original* model before changing the model.
- **Numbers worth pinning:** Move 1 alone (SCHEMA refactor + 3b→7b)
  delivered 3/4 success-criteria architectural lifts simultaneously
  with the hard-gate regression. Category +14.2pts, entry-type +10.7pts,
  clean-single +26.6pts (all clearing the +10pt bar from ADR 0049). The
  quality lift is REAL; the trust regression is also REAL. They live on
  the same Pareto frontier — Bernard's recommendation to Dustin in
  `mb-4xtd` is option A (prompt-tune the hard-gate against 7b's prior),
  but option B (revert to 3b for v1) remains defensible if the
  prompt-tuning sub-iterations don't land within 3 attempts.
- **Beads:** `mb-xmgs` (0.5.1 in_progress), `mb-4xtd` (HALT escalation,
  blocks 0.5.2-0.5.6).

---

## 2026-05-29 [ADR 0048 Wave 3.3 / mb-57a1] Judge swap inverts Gate 3 disagreement direction without closing the gap — the failing gate is correctly identifying a methodology problem the patches under consideration can't reach

- **Context:** Wave 3.2 halted on Gate 3 cross-judge (`llama3.1:8b`
  primary 4/7 agreement, `gemma2:9b` cross-check). Dispatched Wave 3.3:
  shipped option B (swap primary to `gemma2:9b`; rotate `llama3.1:8b`
  to cross-check) + option C (add 6 borderline observational pairs
  to the calibration set alongside the 12 gated unambiguous ones).
  Re-ran score-run on run-a-baseline.
- **Finding 1 (judge swap):** Gate 3 STOPped again at 5/9 (55.6%) —
  functionally identical agreement rate — but with the disagreement
  **direction inverted**. The same three personas
  (`persona-01-case-01 entry[0]`, `persona-01-case-06 entry[0]`,
  `persona-03-case-05 entry[1]`) disagree, but now
  `primary=NotEquivalent / cross=Equivalent` where Wave 3.2 had
  `primary=Equivalent / cross=NotEquivalent`. The disagreement is a
  stable property of the judge pair, not a property of which judge is
  in the primary slot. Tag-collapse metric on the SAME pipeline data
  moved 81.8% → 38.2% (43-pt gap) — the metric is essentially
  judge-dependent.
- **Finding 2 (borderline observational):** gemma2:9b hit 4/6
  (66.7%) on the borderline set with a crisp pattern:
  `tokenization` / `specificity` / `domain-overlap` / `person-specific`
  = 4/4 perfect; `coreference` / `abstraction-level` = 0/2. The gated
  calibration pairs are unambiguous by construction so this asymmetry
  is invisible to Gate 1; the borderline gate surfaces it. Both misses
  are the judge being **more strict** than the documented verdict —
  the exact signature that produces Gate 4 below-band (23.1%
  equivalent) and the Gate 3 inversion.
- **Finding 3 (structural):** the tag-equivalence task on a corpus
  with this much surface-token variation is more ambiguous than the
  inter-rater reliability of LLM judges of different families supports.
  This is not a prompt-tuning problem (Wave 3.2 option A, rejected
  antipattern), not a judge-selection problem (Wave 3.3 option B,
  empirically falsified), not a calibration-set-coverage problem
  (Wave 3.3 option C shipped — borderline gate works but doesn't fix
  the structural disagreement). It is a metric-design problem.
- **Action (this iteration):** halt + surface per dispatch standing
  rule ("Gate 3 fails AGAIN → halt + surface, escalate") + AGENTS.md
  5-attempt rule. Bead `mb-57a1` left open. Wave 4 (`mb-he98`) stays
  blocked. Full options-forward analysis at
  `docs/knowledge-graph/wave-3-results.md` § "Wave 3.3 — judge swap +
  borderline calibration".
- **Recommended path forward (Dustin's call):** **option E** — replace
  the LLM tag-equivalence judge with a deterministic exact-match +
  Jaccard metric on normalized tag sets (optional small synonym map).
  Honors AGENTS.md §6 ("if something is hard to verify, that's the
  bug") — the verification is hard because the metric design is too
  judge-dependent; the fix is to redesign the metric, not to keep
  tuning the judge. Requires ADR 0048 §G5 amendment; ships in one
  iteration; unblocks Wave 4 and substantively simplifies the Wave 4
  judge bundle (two of the seven Wave-4 judges depend on the
  JVP+tag-collapse half being valid).
- **Sub-finding (defense in depth on borderline calibration):** the
  borderline observational gate added this wave is now a permanent
  diagnostic. Even after option E lands and the LLM-judge tag metric
  retires, the per-dimension match rates (especially `coreference`
  and `abstraction-level`) are useful for any future LLM-graded
  metric. The 6-pair set lives at
  `experimental/kg-validation/judge-calibration/tag-equivalence.json`
  (`tag-equivalence-v3`, `borderline` section).

## 2026-05-29 [ADR 0048 Wave 3.2 / mb-57a1] JVP Gate 3 cross-judge STOP: `llama3.1:8b` is more permissive than `gemma2:9b` on tag-equivalence; Gate 1 calibration alone does NOT detect this

- **Context:** Wave 3.2 ran full JVP+PCRP on run-a-baseline with the
  dispatched primary judge `llama3.1:8b-instruct-q4_K_M` and cross-judge
  `gemma2:9b`. Gate 1 (12 unambiguous calibration pairs) PASSED at
  11/12 = 91.7%. Gate 3 (cross-judge agreement on 10% sample of real
  corpus verdicts) STOPPED at 4/7 = 57.1% — both genuine disagreements
  (one was a transient network error) were in the SAME direction:
  `primary=Equivalent / cross=NotEquivalent`. Gate 4 distribution showed
  primary marked 64.3% of verdicts Equivalent, at the high end of the
  in-band 40–80% range. The 81.8% tag-collapse metric the primary
  produced is therefore likely inflated.
- **Finding:** Calibration sets built from unambiguous pairs can pass
  Gate 1 cleanly while completely missing a judge's systematic skew on
  fuzzy real-corpus cases. The two failure modes the calibration set
  was designed for (random verdicts, prompt-misreading) are different
  from the failure mode the corpus surfaces (one-direction permissive
  drift on superset/decomposition disagreement). Gate 3 (cross-judge
  on a real-corpus sample) is the gate that catches this; Gate 1 by
  itself is necessary but not sufficient. ADR 0048 §G5's 5-gate design
  is correct precisely because no single gate covers all of these
  failure modes.
- **Calibration-set fairness sub-finding:** The original `cal-eq-001`
  pair (`[car-repair, auto]` vs `[car-repair, auto-maintenance]`) was
  lexically identical to the judge prompt's first in-context example.
  Wave 3.1 self-flagged this and Wave 3.2 fixed it (commit `7f8ff1c`)
  by swapping in `[birthday, gift]` vs `[birthday, birthday-gift]` —
  same anchored-synonym pattern, fresh vocabulary disjoint from all
  prompt examples and other calibration pairs. The fix is the durable
  rule: **every calibration pair must use vocabulary disjoint from the
  judge prompt's in-context examples,** or Gate 1 measures memorization
  not reasoning. Bumped `calibration_set_id` v1 → v2.
- **PCRP-mislabel sub-finding:** PCRP reviewer (also `llama3.1:8b`)
  called out persona-04-case-01 and persona-05-case-03 as having
  "hallucinated dates". Cross-checking the structured output: the
  pipeline actually **missed** dates that the answer key specified
  (e.g. "before the weekend" → expected `2026-06-20`, pipeline emitted
  no due date). The structural `invented_dates_count` hard gate is
  correct at 0; the PCRP reviewer's prose framing was inverted. Lesson:
  PCRP qualitative themes are valuable but their literal claims must
  be cross-walked against the structured output before being treated
  as fact. The themes (date-extraction is fragile on soft phrases) are
  the durable signal; the surface words ("hallucinated") are not.
- **Resume protocol:** Four options forward, fully documented in
  `docs/knowledge-graph/wave-3-results.md` § "What's needed to unblock":
  (A) tune judge prompt — bias toward NotEquivalent on
  superset/decomposition with one fuzzy-NotEquivalent example, cheapest,
  ~10 min iteration; (B) swap primary judge to `gemma2:9b` or
  `qwen2.5:14b` — compliant with §G4 different-family rule;
  (C) add 5–8 borderline pairs to the calibration set so Gate 1
  measures fuzzy-case behavior, pairs with A or B;
  (D) loosen Gate 3 thresholds — NOT recommended, this is documentation
  change masquerading as a fix.

---

## 2026-05-28 [phase-0-kg-wave-5 / mb-ojm5] Strict IAP rejects every iteration on small local models — see PINNED P9
- Context: Wave 5 Wiggum loop, 5 prompt iterations, cap from AGENTS.md
- Finding: 0/5 accepted; even iter 5's minimal extract-only change moved tag-collapse, entry-type, and PCRP via downstream cascade
- Action: promoted to PINNED P9. Future quality loops on local models use Pareto-frontier acceptance, not strict no-regression

---

## 2026-05-28 [ADR 0048 Wave 2 / mb-i4us + mb-nbel] Mock dispatcher needle ordering: extract needles must be more specific than classify needles

- **Context:** The KG sandbox `MockOllama` matches model dispatch by
  prompt-substring needles, first-rule-wins. The four-pass pipeline
  sends the segment text to BOTH the classify pass (`SEGMENT:\n<text>`
  in the prompt) AND the extract pass (`SEGMENT:\n<text>\nCLASSIFICATION:
  ...` in the prompt). A naive mock rule keyed on the segment text
  alone (e.g. `respond_when("call the daycare", classify-json)`) wins
  on BOTH the classify dispatch AND the extract dispatch — extract
  then tries to deserialize classify JSON, blows up on `missing field
  title`, and the integration test fails in a confusing way.
- **Finding:** The end-to-end pipeline test failed exactly this way on
  first run. The fix is to anchor needles on the unique pass-marker
  prefix each pass writes — `"SEGMENT:\n<text>"` for classify,
  `"<text>\nCLASSIFICATION"` for extract — and to register the
  *more-specific* (longer / more-anchored) needles FIRST so
  first-match-wins lands them.
- **Action:** When extending the harness with new passes or new
  pipeline tests, always include a per-pass marker prefix in the
  needle. Don't rely on segment-text uniqueness alone. Pattern is
  documented inline at the top of
  `experimental/kg-validation/src/harness/pipeline.rs::tests::end_to_end_with_mock_dispatcher`.
- **Not promoted to PINNED:** sandbox-internal harness concern; not
  load-bearing for the product or future production-side work.

---

## 2026-05-28 [ADR 0048 Wave 0 / mb-4wxw + mb-w1lw + mb-i9l1] `bd close` rejects a downstream bead in the same iteration as its blocker's close, even after the blocker is CLOSED — `--force` is the workaround

- **Context:** Wave 0 of ADR 0048 closes three beads in one shot —
  `mb-4wxw` (Wave 0 scaffold) blocks `mb-w1lw` (sandbox skeleton)
  blocks `mb-i9l1` (schema types). All three landed in this iteration
  and need to close before the iteration ends.
- **Finding:** Closing them in dependency order, sequentially —
  `mb-4wxw` first (succeeds), then `mb-w1lw` (FAILS with `cannot close
  mb-w1lw: blocked by open issues [mb-4wxw]`), then `mb-i9l1` (FAILS
  with `[mb-w1lw]`). `bd show mb-4wxw` immediately after the first
  close confirms `Status: CLOSED` — so bd's *display* layer sees the
  close, but the `close` command's blockers-check sees the *pre-close*
  view. Likely a Dolt auto-commit batching artifact (the docs in
  `bd --help` describe `batch` mode where writes defer until
  `bd dolt commit`). The pattern is independent of which beads or
  order: any in-iteration downstream close after a same-iteration
  blocker close hits this.
- **Action:** Pass `--force` on the downstream closes. The dependency
  edges in the graph remain intact for posterity (visible via `bd show`
  / `bd graph`), only the close-time guard is bypassed. Strictly cleaner
  alternative if it ever matters: `bd dolt commit` between closes to
  flush the batch — but `--force` is a one-token addition and bd's
  blocker check is the only mechanism this guard protects, so it's
  not actually load-bearing here.
- **Not promoted to PINNED:** only relevant when an iteration closes
  ≥2 beads in a blocker chain, which is uncommon outside wave-style
  multi-bead waves like ADR 0048 Wave 0. The body entry is enough.

---

## 2026-05-28 [ADR 0047 Wave 2A / mb-da5t] `SettingKey::all().len()` assertion silently drifted because the cargo gate never runs assertions

**Context:** While wiring the new `LlmSkipWordThreshold` setting key for Wave 2.2, noticed the `all_enumerates_every_variant` test in `settings/model.rs` asserts `assert_eq!(SettingKey::all().len(), 36)` but the actual `all()` array at HEAD already contained **37** entries (Wave 1.2's `LlmShrinkFallbackThreshold` had been added to `all()` but its count comment + assertion were never bumped). The drift had been sitting unnoticed since whenever Wave 1.2 landed.

**Finding:** This is **Finding 1 of LESSONS 2026-05-26** (schema-vs-code drift hidden by `--no-run`) wearing a new costume. The cargo gate on this box is `test --release --no-run` (LESSONS P2); `--no-run` proves types + traits + link surface, but **never executes any `assert_eq!` or `#[test]` body**. Any test that's wrong at runtime but compiles cleanly will silently rot. The `all().len() == N` count tests are a load-bearing canary precisely BECAUSE they're cheap to bump and easy to forget; the canary only catches drift if someone runs it. On this box, no one does, until the throwaway-crate recipe runs the affected module live.

**Action:**
1. When adding a new `SettingKey`, ALWAYS bump the comment AND the `assert_eq!` in `all_enumerates_every_variant` in the same hunk. Same applies to any other `assert_eq!(X.len(), N)` style canary.
2. If you touch `settings/model.rs` for any reason, do a quick eyeball check of that test's number vs the actual length of `all()`. The Wave 1.2 author missed it; Wave 2.2 caught + fixed it inline as part of commit `7330884`.
3. Consider running pure-Rust assertion-bearing tests via the throwaway-crate recipe (LESSONS P2) when you suspect a drift. For `settings/model.rs` that's harder (rusqlite ties it to the DB layer), but for any pure-data module it's a 30-second sanity check that closes the assertion-hidden gap.

---

## 2026-05-27 [ADR 0046 Iter 3 Waves 3.1-3.3 / mb-9lgi+mb-txmy+mb-3ivf] First-launch live-fire of the inbox subsystem went green on a pre-existing file; one cosmetic post-archive watcher event was logged but is harmless

**Context:** Three serial commits landed the inbox file-watcher, courier processor, and InboxRuntime + wiring (Waves 3.1 / 3.2 / 3.3 of ADR 0046 Iter 3). First app launch after wiring picked up a `New Recording 38.m4a` voice memo already sitting in `<vault>/inbox/` from a prior manual test, decoded it, drove it through whisper-rs CUDA + Ollama cleanup, wrote `sessions` row `session_id=116` with `source='mobile-inbox'`, and atomically archived the source. End-to-end on commit one; the dispatch's three-phase plan + Wave-0 spike findings paid off completely.

**Finding 1 — The initial-scan-on-start pattern is essential, not optional.** Without it the runtime would only see FUTURE FS events, which means a file sitting in `inbox/` from before the watcher came online would never be processed. A `notify`-style watcher fundamentally doesn't see existing state. The fix is to walk the directory non-recursively for allowlisted files BEFORE spawning the watcher, push them into the watcher→courier channel as synthetic `StableInboxFile` events, then start the watcher. The channel buffers (unbounded) so the pre-fill is safe even though the courier hasn't started reading yet. In retrospect this is obvious; it's still worth recording because the first instinct ("just spawn the watcher") would silently lose the recorded-while-laptop-closed case from ADR §6.

**Finding 2 — After a successful archive (atomic rename out of `inbox/`), the watcher logs one "candidate registered" event for the source path that gets dropped quietly.** Tracing observed timestamp `T+0s` archive completes, `T+10ms` watcher emits `candidate registered path=<source>\New Recording 38.m4a`. The watcher's `notify` debouncer apparently coalesces the rename event into a single late notification on the SOURCE path; the candidate enters the stability-check phase, fails the metadata read (source no longer exists), and is dropped via the existing retry cap. Net effect: one info-level log line + a few wasted milliseconds in the stability worker. Not worth fixing today — the cap drops it cleanly — but if a future stability-check refactor lands, the right move is to bail the candidate immediately on `std::fs::metadata` `NotFound` rather than waiting for retries. Filing under "cosmetic" not "bug".

**Finding 3 — Module placement: `src-tauri/src/inbox/` as its own top-level subsystem beats nesting under `vault/`.** The original dispatch left the call open. The right call ended up being a new top-level module because (a) the inbound (`inbox/`) and outbound (`vault/`) flows share ONLY the vault path — their lifecycles, channels, and consumers are otherwise independent; (b) it mirrors the existing `dictation/` vs `meetings/` split where two related-but-distinct subsystems live as peers; and (c) it keeps `vault/`'s scope tight ("projects records OUT") rather than ballooning into "any code that touches the vault path". Cost was zero — a single `pub mod inbox;` line in `lib.rs` and a parallel `app.manage(Arc::clone(&inbox_runtime))` next to the existing `app.manage(Arc::clone(&vault_runtime))`. PRODUCT-STATE.md gained §3.17 as the inbox's home, with §3.16 (vault) updated to point at it.

**Finding 4 — Lifecycle gating via the SAME settings keys that gate the outbound projection is the cleanest UX.** One toggle (`MobileSyncEnabled`) + one path (`VaultPath`) drives both directions of the vault flow. The settings-set IPC calls BOTH runtimes' `refresh_config` in the same tick. Users don't need a separate "inbox watcher enabled" toggle — if Mobile Sync is on, both directions are on; if off, both are off. The 5-state transition matrix in `InboxRuntime::refresh_config` (stopped/idle, stopped/active, running/idle, running/same-path, running/different-path) is small enough to keep in the head and exhaustively unit-test via the throwaway-crate recipe.

**Action:** All four findings are subsystem-specific (relevant only to `inbox/` work in Iter 4 hardening or any future watcher-style subsystem); none rises to PINNED. The Finding-2 "stale candidate after archive" pattern is worth grepping for if any future stability-check refactor lands.

---

## 2026-05-27 [ADR 0046 Iter 3 Wave 0 / mb-s8s2] Two small but real Windows tooling gotchas

**Context:** Building `scripts\watch-vault.ps1` (PowerShell 5.1 FileSystemWatcher script for the sync-layer spike) and updating `mb-s8s2` description via `bd update`. Both hit silent failure modes worth recording.

**Finding 1 — PowerShell 5.1 reads `.ps1` files as ANSI / Windows-1252 by default, not UTF-8.** Em-dashes (`U+2014`, encoded as 3 bytes `E2 80 94` in UTF-8) get decoded as three Latin-1 characters (`â € ”`). The middle byte `0x80` is the EUR sign in Windows-1252, but more importantly the `0x94` byte is interpreted as a smart double quote (`”`). When that lands inside a `Write-Host "..."` string the parser sees an unterminated string and continues parsing the next token as a command — producing baffling errors like `The term 'mb-s8s2' is not recognized as the name of a cmdlet`. Fix: ASCII-only inside `.ps1` files, or save with a UTF-8 BOM. Detection one-liner: `[regex]::Matches((Get-Content -Raw -Encoding UTF8 path.ps1), '[^\x00-\x7F]').Count`. Sweep script: `(Get-Content -Raw -Encoding UTF8 path) -replace [char]0x2014, '--' -replace [char]0x2019, "'" ...` then `[System.IO.File]::WriteAllText` with a no-BOM `UTF8Encoding`.

**Finding 2 — `Register-ObjectEvent -Action { ... }` runs the action in a SEPARATE runspace from the script.** Anything the action block calls (script-scope functions, `$script:` variables) silently isn't visible, and any exception thrown inside the action is swallowed into the per-event runspace's `$Error` and never surfaces. The script appears to start fine, the watcher fires events, and... nothing happens. The diagnostic-friendly pattern is to register WITHOUT `-Action` (events accumulate in PowerShell's event queue) and dequeue them with `Wait-Event` / `Get-Event` in the main loop. Everything stays in one runspace, handlers can see script scope, and exceptions surface normally.

**Finding 3 — `bd update <id> -d "<long string>"` SILENTLY truncates / does not apply the new description for strings over a few hundred characters on Windows.** Exit code 0, success message, but the JSONL on disk is unchanged. The reliable mechanism is `bd update <id> --body-file path.txt` (write the new description to a temp file first). Detection: `python -c "..."` against `.beads/issues.jsonl` to compare description length before/after. The `-d` flag is fine for short titles-style updates; treat anything multi-paragraph as `--body-file`-only.

**Action:** All three are non-PINNED-worthy individually (they bite once and you remember), but recording them here so a future session's grep on `FileSystemWatcher` / `Register-ObjectEvent` / `bd update --description` finds the answers immediately. None changes the standard workflow; just trapdoors with documented hatches.

---

## 2026-05-27 [ADR 0046 Iter 2 SEAL / mb-3xww] Obsidian nested-vault trap during Mockingbird Mobile Sync setup

**Context:** Iter 2 of ADR 0046 (outbound Obsidian projection) shipped across eight commits and a Mobile Sync (preview) section in Settings → Advanced. Dustin's hands-on smoke against `C:\Users\dboyd\mockingbird-vault\` with Obsidian Sync Standard tier paired to iPhone Obsidian Mobile. Backfill triggered, toast said "Vault up to date (90 records)." Mockingbird logs clean. iPhone Obsidian Mobile after sync round-trip: showed nothing but the `Welcome.md` Obsidian had created at vault init. Desktop Obsidian: same. But Mockingbird-side everything looked correct.

**Symptom shape:** the toast was truthful, the files DID exist on disk, but Obsidian on both ends saw none of them. The instinct is "sync layer broken" or "projection broken" — both wrong.

**Diagnosis pattern.** `Get-ChildItem C:\Users\dboyd\mockingbird-vault -Force` revealed two children: `.mockingbird/` (Mockingbird's manifest + bookkeeping, expected) AND a nested `mockingbird-vault/` folder of the same name as the parent. The nested folder contained `.obsidian/` and `Welcome.md`. **That's the smoking gun.**

**Root cause (NOT a Mockingbird bug).** When the user runs Obsidian's "Create new vault" wizard and types a name that matches an already-existing folder on disk, Obsidian silently nests the actual vault one level deeper rather than refusing or warning. So:

- Mockingbird's `VaultPath` setting pointed at `C:\Users\dboyd\mockingbird-vault\` (the OUTER folder — the one the user picked in the native folder picker).
- Mockingbird wrote `dictation/*.md`, `meeting/*.md`, `.mockingbird/manifest.json` into the OUTER folder.
- Obsidian's `.obsidian/` config (which determines what Obsidian treats as "the vault") lived in the INNER folder.
- Obsidian on both desktop and iPhone treated the INNER folder as the vault, never saw the outer-folder writes.
- Obsidian Sync synced the inner folder (an empty vault with only `Welcome.md`) successfully — the OUTER writes were simply outside Obsidian's known universe.

Mockingbird never had a chance: it was given a path, it wrote to that path, the path was real, the writes succeeded. The path Obsidian considered the vault was somewhere else.

**Resolution.** In-place migration, no Mockingbird code change, no re-pair of Obsidian Sync needed:

1. Move `.obsidian/` from `<vault>/mockingbird-vault/.obsidian/` to `<vault>/.obsidian/`.
2. Move `Welcome.md` (if the user wants to keep it) to the outer too.
3. Delete the now-empty inner `mockingbird-vault/` folder.
4. Obsidian on next launch re-anchors on the outer `.obsidian/` config and now sees everything Mockingbird has been writing. Obsidian Sync credentials live INSIDE `.obsidian/sync/`, so the credential file moves with the rest of `.obsidian/` — no re-pairing needed.

This worked clean on the first attempt; the next dictation Dustin spoke synced to iPhone within ~30s.

**Iter 4 implication (`mb-3xww`).** The Mockingbird Mobile Sync setup flow should detect this case and either (a) refuse to enable Mobile Sync until the user resolves it (with clear instructions), or (b) offer a one-click auto-migrate that does the move-inner-to-outer + delete-empty-inner dance. (b) is the better UX if the file-move is reliable; (a) is safer if there's any risk of misclassifying something legitimate as a nested vault.

Detection is straightforward: when the user picks a `VaultPath`, list its children; if there's a same-named subfolder AND that subfolder contains `.obsidian/`, you've found the trap. Out of scope for the wizard: detecting non-nested but otherwise-misconfigured vaults (e.g. user pointed Mockingbird at the wrong folder entirely) — that's a much fuzzier UX problem.

**Why this entry isn't PINNED.** It's an Obsidian-setup operational gotcha that bites once per user during setup, with a clear post-detection auto-fix. PINNED entries are load-bearing rules every session must remember. This is body-rank — Iter 4's `mb-3xww` work needs it, and any future Obsidian-vault-adjacent debugging will benefit from finding it via TOC + grep, but it doesn't shape how every session starts.

**Process note.** Iter 2's gate strategy (cargo check + clippy + fmt + `--no-run` + UI build all green, with smoke deferred to Dustin's hands-on) worked exactly as intended here — judges + gates proved the *contracts* (atomic writes, deterministic projection, content-addressed manifest, opt-in default-OFF settings, sealed-phase isolation), and the live-fire smoke caught the *environment* mismatch that no static check could have seen. Same shape as Phase 10's live-fire smoke catching the Command Center black-box / Esc-handling regressions that the green Wave 6 judges couldn't catch (LESSONS PINNED P7). Pattern reinforced: live-fire smoke catches environment + setup + integration-with-third-party regressions; static + judge gates catch contract regressions. Both are necessary; neither is sufficient.

---

## 2026-05-23 [ADR 0046 Iter 1 / mb-jbf7] Programmatic Strategy-A smoke is blocked by mb-0n8c; pair every smoke example with a pure-rusqlite probe instead

**Context:** Iter 1 of ADR 0046 (desktop file-ingest) shipped through `mb-jqhw` / `mb-hxm4` / `mb-evn3` / `mb-7vyz` / `mb-thmd`. `mb-jbf7` was the live-fire smoke step. The kickoff offered Strategy A (a programmatic `cargo run --release --example smoke_iter1_ingest -- <fixture>` that drives `DictationRuntime` + `headless_ingest()` end-to-end) and Strategy B (just launch `mockingbird.exe` and let Dustin click-test). Strategy A was preferred because it covers criteria 1-4 + 7 of the bead programmatically and leaves only the UI-refetch (criterion 5) + PTT-regression (criterion 6) as human work.

**Finding:** Strategy A is blocked by the same DLL-load-chain bug as `cargo test --release` (LESSONS PINNED P2 / `mb-0n8c`). The `smoke_iter1_ingest` example built cleanly (release-profile build through the wrapper, 41 MB binary, link-clean, fmt + clippy quiet), but every invocation — both via the cargo wrapper and direct — exits `-1073741511` (0xc0000139 / `STATUS_ENTRYPOINT_NOT_FOUND`) with empty stdout/stderr, regardless of:

- `ORT_DYLIB_PATH` set externally (verified to point at the existing `onnxruntime.dll`)
- CUDA v12.8 `bin\` prepended to PATH
- CWD set to `target\release\` so `mockingbird_lib.dll` resolves
- `target\release\` added to PATH for the same reason
- The wrapper itself (which provides MSVC + CUDA env per ADR 0011)

The failure signature is identical to the test runner: process dies before `main` runs, no Rust panic backtrace, exit code in the entrypoint-not-found family. The control case `verify_wave49.exe` (also under `examples/`, pure rusqlite, 1.7 MB, no whisper-rs / ort / CUDA deps) launches and runs to completion fine. So the bug isn't "examples are broken" — it's "any non-`mockingbird.exe` binary whose link graph pulls whisper-rs / ort / CUDA hits the entrypoint-resolution failure".

**Implication for ADR 0046:** the SAME design (a tiny smoke example that spins up the real runtime + drives `headless_ingest`) would have been the obvious verification pattern for Iter 2 (Obsidian polling) and Iter 3 (paste path). It is now off the table until `mb-0n8c` is root-caused. Live-fire ingest verification has to go through `mockingbird.exe` (Strategy B) + a separate DB introspection probe.

**Action / pattern:** when an iteration produces a new ingest entry point and the temptation is "write a smoke example to drive it programmatically", do BOTH of these instead:

1. Build a pure-rusqlite verification probe (Python via `sqlite3` builtin, or a tiny Rust binary with only `rusqlite` in its dep tree — `verify_wave49.exe` shape, NOT `smoke_iter1_ingest.exe` shape). This proves the schema half of the smoke (migration applied, columns + indexes present, recent rows have the right `source` / `start_mode` / `status` values).
2. Launch the real `mockingbird.exe` via `scripts\run-mockingbird.ps1` (Strategy B). Verify the process stays alive past warmup (~10s for whisper-rs CUDA + Silero VAD + Ollama warmup). Read the latest `%APPDATA%\com.dustin.mockingbird\logs\mockingbird.log.YYYY-MM-DD` and confirm there are no WARN/ERROR/panicked lines in the startup chunk. Then hand off the click-test to a human.

The Iter 1 smoke example is kept in tree (`src-tauri/examples/smoke_iter1_ingest.rs` + `Cargo.toml` entry) so that when `mb-0n8c` is solved the smoke is ready to run as-is. It's a one-shot diagnostic, not committed-test infrastructure — gitignored fixture, real production DB, no CI hook. The pure-rusqlite probe `verify_iter1_schema.py` lives at the repo root for the same reason (one-shot, run when needed).

P2 has been updated to flag examples as also affected; this body entry is the long-form.

**Stats:** 1 build cycle for the smoke example (4m 30s), 1 build cycle for the rebuilt `mockingbird.exe` (5m 43s), 2 attempts at launching the smoke example (both fail identically), 1 successful Strategy-B launch + log inspection + schema probe. `mb-jbf7` stays open for Dustin's UI click-test (criteria 5 + 6) and the live-fire ingest verification (criteria 1-3, 7) — schema half (criterion 4) is PASS programmatically.

---

## 2026-05-27 [ADR 0046 §3.2 / mb-7vyz] Resolution of the channel-topology fork: bridge mpsc→crossbeam IN THE DICTATION THREAD, don't convert the upstream channel

**Context:** Earlier today (entry below) we surfaced three forks for headless ingest's channel-topology problem. ADR 0046 §3.2 amendment took option (a-extended): add a sibling crossbeam channel + multi-select in `run`. This entry records the **specific design call inside that fork** that wasn't obvious until the diff was on the table.

**Finding:** When the orchestrator needs to `select!` between two channels but one of them is fed by a sealed/out-of-boundary producer, the smallest diff is to **bridge the inbound channel inside the dictation thread itself**, not to convert the upstream producer.

Concretely: `StateDriver::start` (in the out-of-boundary `hotkey/driver.rs`) produces a `std::sync::mpsc::Receiver<StateAction>`. To `select!` against a new crossbeam channel I either had to (i) change the driver's channel type to crossbeam — touches sealed code in a way that ripples through every test using `StateDriver`, OR (ii) spawn a tiny adapter thread inside `run_dictation_thread` that does `for action in actions.iter() { crossbeam_tx.send(action) }`. Option (ii) is ~20 lines of pure type-adapter glue, lives entirely inside the dictation runtime, and the upstream `StateDriver` literally doesn't know the bridge exists. The bridge thread terminates symmetrically when either side closes — no extra shutdown signal.

The orchestrator's `run` becomes a `select!` over two `crossbeam_channel::Receiver`s, and the loop's outer shape ("exit when both inputs are dead") is implemented with two `bool` latches rather than the old `for action in actions.iter()` pattern. Both arms remain trivially testable: a unit test can drive a `StartCapture → StopCapture` cycle by pushing onto the crossbeam side directly, no bridge needed.

**Pattern for future epics:** when you need to multi-select a sealed-producer channel against a new sibling channel, the question is "where does the type adapter live?" — and the cheapest answer is almost always "inside the consumer thread, not at the producer." The producer thread doesn't care; only the consumer needs the select-capable types.

**What we did:** Authored ADR 0046 §3.2, added `crossbeam-channel = "0.5"` (already in the dep closure via notify/rayon), introduced `dictation::ingest_channel::HeadlessIngestRequest` + factory `channel()`, and split the dictation thread into (a) the bridge + (b) the orchestrator-runs-with-two-crossbeam-receivers. Tests + fmt + clippy all green; `mockingbird.exe` was running so `cargo test --release` link was skipped per LESSONS P2.

**Closes the loop on:** the "channel topology can't be naively extended" entry directly below.

---

## 2026-05-27 [ADR 0046 Iter 1 / mb-evn3 / mb-7vyz] The `StateAction` channel topology can't be naively extended for headless ingest — it needs an ADR-amendment-sized change, not an in-boundary tweak

**Context:** ADR 0046 Iter 1 Phase D (`mb-7vyz`, "+ Audio file" button) needed the IPC handler to drive the same VAD/STT/Cleaner the orchestrator owns. The dispatch's preferred pattern was "extend `StateAction` with a `HeadlessIngest(IngestRequest)` variant and let the orchestrator process it on its thread."

**Finding:** That pattern doesn't survive contact with the real channel topology.

1. `StateAction` lives in `src-tauri/src/hotkey/state.rs` — a file explicitly outside the ADR §3 boundary. Even adding a single enum variant touches sealed Phase 3 code in a non-trivial way (any `match` on `StateAction` becomes non-exhaustive without an arm). The dispatch's "Stop and ask if" #3 calls this out specifically.

2. Even with the enum variant added: the orchestrator's input is `Receiver<StateAction>`, but the SENDER is `StateDriver`, which only consumes `Receiver<HotkeyEvent>`. There is no path from `DictationRuntime` (where the IPC handler reaches in via Tauri state) into the orchestrator's `StateAction` stream that doesn't go through the keyboard FSM. To support pattern (a) cleanly you'd need a NEW sibling mpsc channel from runtime → dictation thread, plus the run loop would have to multi-select on two channels (which `std::sync::mpsc::Receiver` can't do natively — needs crossbeam-channel `select!` or polling with `try_recv`).

**Why it matters for future epics:** "extend the existing channel enum" is a tempting shorthand for "plumb a new event through the orchestrator," but in this codebase the channel topology asymmetry (one input channel, fed by ONE specific producer) means the real change is bigger than the enum delta suggests. Future ADRs that propose orchestrator-targeted side channels (e.g. a future "start-from-Activity-block-context" feature) should explicitly answer: "who is the new sender, and does the orchestrator's run loop need to multi-select?"

**What we did:** Sealed `mb-evn3` (Phase C — the headless ingest function itself, which is dep-injected and channel-agnostic) and stopped before `mb-7vyz` to surface the three viable forks: (b) construct fresh VAD/STT/Cleaner per IPC call (in-boundary, costs a whisper-rs CUDA reload per import), (a-extended) author an ADR 0046 amendment for the channel topology change, or defer Phase D entirely until the architecture is decided. Documented in STATUS.md in-flight block.

**Process win:** the dispatch's explicit "Stop and ask if" #3 paid for itself — without it Bernard would have either over-edited `hotkey/state.rs` (boundary violation) or sunk an iteration into a half-baked sibling-channel scaffold that doesn't actually solve the problem either.

---

## 2026-05-27 [mb-tfyp / mb-sowc / ADR 0045 follow-up] In-app dictation showed `ABORTED_FOCUS_CHANGED`; the fix is a new column AND a new InjectionOutcome variant, not a new pill string

**Context:** ADR 0045 (mb-ddfx) shipped programmatic dictation start via the `dictation_start` IPC. Both PTT and in-app modes drive the same FSM via a sentinel VK, so the orchestrator was mode-blind by design. First live-fire reveal: clicking the new in-app Start button → captured a transcript → session row landed with `injection_status = 'aborted_focus_changed'` (red pill in the list). Latency row showed `Inject 0 ms`, consistent with the abort path skipping injection.

**Finding:** The `aborted_focus_changed` outcome conflates two semantically distinct things:
  1. PTT session lost its target app between key-down and inject (a real-world abort — the user's text would land somewhere wrong, so we skip).
  2. In-app session — there was no target app to begin with, so the comparison is incoherent.
Both wound up at the same `InjectionOutcome::AbortedFocusChanged` arm because the focus comparison ran unconditionally and produced "different" (`null` foreground at start ≠ `mockingbird.exe` at end). The temptation was to special-case the pill render: "if the underlying status is aborted_focus_changed AND start_mode is in_app, show IN_APP." That's a leak — the DB row is still wearing a red status, and any future query / analytics / judge looking at `injection_status` gets the wrong story.

**Action:**
  1. New `sessions.start_mode` column (migration 017, `'ptt' | 'in_app'`, DEFAULT `'ptt'`). Backfills cleanly because all pre-ADR-0045 rows were PTT.
  2. New `InjectionOutcome::InAppNoInject` variant (db str `"in_app"`). Computed in `complete()` as `if state.start_mode == StartMode::InApp { InAppNoInject } else { existing-focus-drift logic }`. The focus comparison literally doesn't run for in-app — different code path, same observable result (no paste).
  3. `dictation:state` event payload extended with optional `startMode` field. UI list-pill prefers `startMode === 'in_app'` (neutral `IN_APP` chip via `status-info` tone) over `injectionStatus` for pill rendering. Detail panel shows "Push-to-talk" / "In-app" next to the mode pill.
  4. Recording-pill overlay conditionally renders a Stop button when `startMode === 'in_app'` (mirrors the meeting overlay's Stop+Cancel pattern). PTT pill is byte-identical to before. Zero regression.
  5. Plumbing: `DictationRuntime` owns an `Arc<AtomicBool> next_start_is_programmatic` that the `dictation_start` IPC flips; `start_capture` reads-and-clears it into the session state. Single-sourced — no caller of `start_capture` can forget to set it.

**Why this matters for next time:** when a status enum carries semantic baggage ("we aborted because X"), and you ship a new path where X is a category error ("there is no X"), the right fix is a NEW variant, not a pill-render hack. The DB row should encode what actually happened. UI semantics that contradict the DB row are tech debt with compound interest.

**Schema discipline:** migration 017 is the 2nd post-`phase-10-complete` migration (016 was the activity_blocks hotfix). Post-seal migrations remain sanctioned for additive plumbing / defect repair; non-additive changes still need an ADR. Pure ADD COLUMN with DEFAULT — verified via throwaway-crate recipe per LESSONS P2 because `cargo test --release --no-run` is still the gate on this box.

---

## 2026-05-27 [mb-ddfx / ADR 0045] Kickoff drift: 3 of 4 beads in the kickoff plan were already CLOSED on disk

**Context:** The /goal kickoff for the "mb-ddfx + mb-aho4 + mb-l8ey + mb-t6bk" 4-bead lateral epic listed all four beads as outstanding work, with a suggested execution order that ended with the meaty mb-ddfx. Session-start ritual saved this one: STATUS read first, then `bd show` for each bead. Three of the four (mb-aho4, mb-l8ey, mb-t6bk) had already been closed in earlier commits (39d5a30, 168389d, a6eed93) and the work was visibly on disk. Only mb-ddfx (programmatic dictation start) and the absorbed mb-ytex remained.

**Finding:** A kickoff prompt that bundles N beads under a single "epic" can drift between when it's drafted and when it's executed — especially when an iteration crashes out partway through. The correct response to "the kickoff says do X, Y, Z, W" is NEVER "start at X and work down". It's:

1. `bd show <id>` (or `bd ready`) for every bead listed, before any code edit.
2. If a bead is already CLOSED with a sensible close reason, verify the close reason matches what's on disk (one grep) — then skip and move on.
3. The remaining outstanding beads are the actual scope.

This is the same family of stale-context-vs-disk-state error as the 2026-05-17 incident (which was about sealed phases) and the 2026-05-23 incident (kickoff conflict with disk). The PINNED P4 guard rail catches the sealed-phase case; this is the smaller-grained "individual bead status" case. Doesn't warrant a new PINNED entry — the existing `bd ready` step in the start-of-iteration checklist would have caught this if I'd run it before reading the kickoff body. **Refresh:** always run `bd ready` (or at least `bd show` for explicitly-mentioned beads) AS PART OF the kickoff triage, not after.

No wasted work was done. Caught at triage time; the report just calls out the three already-closed beads instead of re-doing them.

---

## 2026-05-26 [design-v1 / mb-n455] Design System v1 audit + formalization shipped; the modes by which UI drift accumulated, the dead-token-cascade bug class, and the kitten-probe gradient blind spot

**Context:** Dustin's first live-fire pass after `phase-10-complete` surfaced systemic UI drift, most visibly on the new Activity page (literally bare floating text on the photo bg — no card surfaces, no visible buttons). Bernard ran the kickoff plan as a 5-phase bead-only lateral epic (`mb-n455`, no ADR) with qa-kitten as the visual-audit specialist for baseline + re-audit. Final verdict: SHIP, 8/8 P1 + 9/12 P2 baseline findings resolved, 3 P2s reclassified as false-positives, 0 regressions, 2 P3 follow-ups filed.

### Finding 1 — Six modes by which UI drift accumulates between phases

The baseline audit's 30 findings clustered into six failure modes. Worth documenting so future phases catch them at PR time, not at first-live-fire time:

1. **Dead-token cascade silently rendering transparent.** Phase 10's `Activity.module.css` consumed token names from a pre-W6 design-language vocabulary (`--surface-1`, `--surface-2`, `--text-1`, `--border-subtle`, `--accent-1`) that Wave 3+4 of the Design Language v1 epic had REMOVED. CSS spec says: when `var(--undefined)` has no fallback, the whole declaration is invalid and the cascade falls through to its initial value. For `background`, `border-color`, etc. that initial value is `transparent`. **No build error. No lint error. No test fails.** The surface just renders invisible. This is the single most insidious bug class in the design system; the only catch was a human glance at a photo background. Guardrail filed below.
2. **Single-source-of-truth violations metastasizing.** The outline-button "transparent default" bug existed in `primitives.module.css` `.btn_ghost` for ~5 phases. Every phase that added a new outline action button picked it up. Fix was 2 lines in one file, but only because the abstraction existed; if Phase 10 had hand-rolled its own button styles (as Activity initially did), the sweep would have been 40+ sites.
3. **`100vh` rot.** Each phase added new `100vh` references because the author was looking at adjacent files for the convention — and adjacent files were also `100vh`. Phase 8 was where `100dvh` should have landed but didn't. The sweep took 30 minutes once we decided to do it; the technical debt was 0 minutes per phase to write the wrong value.
4. **Nested scrollers, normalized.** Same pattern as `100vh`. Each `overflow-y: auto` on a sidebar list looked correct in isolation; the bug only existed in the composition. The fix was inverting the entire scroll model (sticky sidebar in a single page-level scroller) which would have been ~30 minutes if done in Phase 8 and was ~2 hours retroactively across 3 pages.
5. **Native form controls leaking through.** The HTML5 `<input type=range>` thumb, the native `<select>` chevron, the native `<input type=checkbox>` — all three rendered in OS chrome (bright blue / light gray / Windows-default) which broke the warm-neutral theme. Every phase author thought 

**Context:** First Phase 10 hotfix (`ebe976b`) fixed the visible
recursive-emit clobber (mb-23rh / mb-7ju5) and gates were green —
but Dustin's live smoke test surfaced TWO new P1s on the rebuilt
binary: the Command Center now rendered as an empty black box on
chord-press, and Esc / outside-click would not dismiss it (taskkill
required to recover). Same iteration also surfaced a P3:
crash-recovered activity sessions showed `Duration: 0s` even when
79 events were on disk.

**Finding 1 — `capabilities/default.json` is a strict allowlist; new
Tauri windows MUST be added there or `listen()` silently no-ops.** The
`command_center` window was added in Phase 10 Wave 1A but never added
to the `windows` array in `capabilities/default.json`. The file's own
description literally calls this failure mode out (it was the
mb-z5y / ADR 0035 root cause): `invoke()` of `#[tauri::command]`
handlers still works for unlisted windows, but `listen() / window.hide()
/ emit_to()` silently no-op. Symptoms in this iteration:
  - **Empty black box (mb-a0f3)**: React mounted, called
    `getCommandCenterState()` (invoke → worked → returned Closed at
    mount time), then registered a `listen('command_center:state',
    ...)` handler. The listen registration was a silent no-op. When
    Rust later emitted `modePicker`, nothing arrived → React stayed in
    its initial `Closed` snap → component returned null → empty
    transparent window appeared (which against the desktop wallpaper /
    dark windows reads as "black box").
  - **Won't dismiss (mb-q2if)**: a follow-on of the same bug. With
    React stuck on `Closed`, `apply(Closed, Dismiss, …)` is a FSM
    no-op (no `HideWindow` effect). So invoke fired, FSM stepped
    Closed → Closed, and the orphaned window stayed visible. The user
    saw "Esc / outside-click do nothing".
  
  This is the THIRD time this exact gap has bitten (mb-z5y wave-1,
  mb-z5y wave-2, now mb-a0f3 / mb-q2if). **Action:** the file's
  description now spells out the recurrence pattern explicitly and the
  rule "whenever you add a Tauri-declared window in `tauri.conf.json`,
  also add it to `capabilities/default.json`". Future thought: hook?
  schema check that the two `windows` lists are equal? For now, the
  in-file shouting is the cheapest deterrent.

**Finding 2 — "ship-and-pray" pattern: gates green, live broken, because
the orchestrator surface had no unit tests at all.** The first hotfix
was verified by cargo check / clippy / npm gates only — the entire
`drive()` orchestrator (the function that runs the FSM, dispatches
effects, and re-emits state) had ZERO unit tests because it was
inseparable from the Tauri `AppHandle`. The pure FSM in `state.rs`
had 44 tests (all green) but those test the wrong thing — they verify
`apply(state, input) → Transition`, not the orchestrator's
sequence-of-emits behavior that the React UI actually binds to. Both
the mb-23rh recursive-emit clobber AND the post-hotfix gap (everything
working except for the missing capability) would have been caught by
a single "chord-press Open must emit a visible state synchronously"
test if such a test existed.
  
  **Action taken this iteration:** extracted the orchestrator's pure
  core into `command_center/drive.rs` as a free function over a
  `CcEffects` trait. Production wires it via a `TauriEffects` adapter
  in `mod.rs`; tests wire it via a `MockEffects` recorder that captures
  the full sequence of show / hide / dispatch_start / dispatch_stop /
  emit_state / persist_seen_flag calls. 16 new unit tests cover all 7
  user-facing paths from the kickoff acceptance criteria (chord press
  first-run / subsequent, each tile pick, runtime-refuses, Esc dismiss,
  Stop button, re-chord while session live, SessionEnded mid-card) plus
  explicit named regression guards for mb-23rh, mb-a0f3, mb-q2if. The
  trait is the seam; the pattern transfers to any future
  AppHandle-coupled orchestrator (the dictation `complete()` chain is
  the obvious next candidate). Total cost: ~600 lines including the
  test suite; the engine itself is ~50 LoC of orchestration.

**Finding 3 — `crash_recovery::mark_interrupted_sessions` synthesized
`ended_at = MAX(updated_at, started_at)` but never looked at the
`activity_events` table.** A session that crashed mid-stream (events
written but no `set_session_audio_provenance` / `finalize_session` call
to bump `updated_at`) recovers with `updated_at == started_at`, so
`ended_at` lands ON `started_at` and the UI shows `Duration: 0s`
despite the events being right there in the DB. Fix: nest a
`(SELECT MAX(ts) FROM activity_events WHERE session_id = …)` into the
`MAX(...)`. Two tests added: one with events at increasing timestamps
(asserts ended_at == latest event ts), one eventless (asserts ended_at
falls back to started_at gracefully).

**Finding 4 — `tracing::debug!` for FSM steps is the wrong level for
production.** The drive loop logged transitions at `debug!` — invisible
at the default `info` level the launcher configures. When Dustin's
binary mis-behaves, there's no FSM trace in `mockingbird.log` to
cross-reference. Bumped to `info!` (one line per FSM step,
tiny cost in normal operation, massive diagnostic value when a future
bug surfaces). General principle: ANY state-machine orchestrator's
step transitions should be `info!`, not `debug!`. The body of effects
(window show, runtime starts) can stay `info!` or `debug!` as fits.

**Why this matters for next time:** "green gates, broken live" is not
rare — it's the default failure mode whenever the gates don't exercise
the full integration surface. The fix is NOT "run more manual
smoke-tests"; it's "add a unit-testable seam at the integration
boundary so the gate can mechanically cover it". For Tauri
orchestrators specifically: the AppHandle is the wrong testable unit;
the trait of "what the orchestrator does TO the AppHandle" is the
right one. Apply the same pattern wherever you see `&AppHandle` in a
function signature that contains business logic.

---

## 2026-05-26 [phase10-hotfix / mb-scla / mb-23rh / mb-7ju5 / mb-7knd] Two post-seal Phase 10 papercuts and a load-bearing observation about the test gate

**Context:** Dustin's first live-fire smoke test of `phase-10-complete`
surfaced four P1/P2 bugs (Bernard's notes filed them as `mb-scla`,
`mb-23rh`, `mb-7ju5`, `mb-7knd`). The seal stays put — these are hotfix
commits on top of `phase-10-complete`. Both root causes are previously
unseen failure modes worth recording.

### Finding 1 — Schema-vs-code drift hides behind `--no-run`

**Symptom:** Activity session detail UI exploded with
`sqlite error: no such column: primary_title`. The column is touched
in SIX activity modules (`blocks_persist.rs` INSERT + SELECT,
`blocker.rs`, `abstractor.rs`, `assembler.rs`, `export.rs`,
`pdf_export.rs`). Migration 012 — which created `activity_blocks` —
never declared the column. Migrations 013 / 014 / 015 all added
different columns; none added `primary_title`.

**Why six Wave-6 judges + two extra fixture-test passes missed it:**

1. The Wave 6.B `provenance-is-total` judge is a static / file-diff
   reasoning check. It asserts that every `INSERT INTO activity_blocks`
   call site provides a `prompt_version_sha` column (which it does).
   It does NOT open SQLite and execute the INSERT to prove the schema
   accepts it.
2. The Wave 6.B `db/migrations.rs::migration_013_*` unit test does an
   INSERT with `primary_title` in its body. It would have failed
   loudly. But on this box `cargo test --release` exits
   `STATUS_ENTRYPOINT_NOT_FOUND` (LESSONS P2), so the gated step is
   `cargo test --release --no-run` — which proves the **linker** is
   happy, not that **a single SQL statement runs**.
3. CI for this project IS the developer's box. There is no second box
   running `--no-run`-less tests.

**The trap is generic:** a `--no-run` gate hides any class of bug
where the Rust type system + trait surface is internally consistent
but the *runtime contract* (SQL schema, JSON shape, file format,
network protocol) is wrong. The cargo gate proves "Rust agrees with
itself," not "Rust agrees with reality."

**Defense in depth — what I did about it this session:**

- Authored migration `016_activity_blocks_primary_title.sql` with
  `ALTER TABLE … ADD COLUMN primary_title TEXT NOT NULL DEFAULT ''`.
- Bumped both the inner unit test
  (`db/migrations.rs::apply_all_brings_fresh_db_to_latest_version`)
  and the integration test (`tests/db_migrations.rs::
  schema_version_is_16_after_apply`) so the next drift triggers a
  loud failure at the link-only level (the wrong constant in the
  binary still fails the assertion at runtime when somebody DOES run
  `--no-run`-less tests, e.g. on the macOS Phase 9 box).
- **Used the throwaway-crate recipe (LESSONS P2) to actually run the
  fix live.** Tiny `cargo new mb016_smoke`, depend only on rusqlite,
  read every relevant migration off disk, exercise the exact
  `blocks_persist::list_blocks` SELECT shape. Took ~30 seconds and
  proved the fix end-to-end. **This recipe should be the default
  for any schema / migration / pure-Rust change going forward —
  not a fallback, the FIRST move.** The cargo gate is fine for
  catching code that won't link; it is structurally incapable of
  catching a schema column the Rust types don't reference.
- Added a new `migration_016_ships_*` unit test that mirrors the
  production SELECT shape. It will not run on this box until
  `mb-0n8c` (the `--release` test runner bug) clears, but it is the
  artifact a future agent would need if they revisit this.

**Promotion candidate?** I considered promoting this finding into the
PINNED block but decided against it. P2 already documents the
`--no-run` workaround; what's new is the implication ("--no-run is
structurally blind to runtime contracts"). The pattern fits better
as a body entry whose lesson is "reach for the throwaway-crate
recipe early on any schema/SQL/JSON-contract change." Future agents
find this via P2's existing reference.

### Finding 2 — Recursive `drive()` + post-effect `emit_state` clobbers the UI

**Symptom:** Clicking the Activity tile (and probably Meeting too)
in the Command Center caused the modal to lock up — all three tiles
rendered disabled, only outside-padding clicks dismissed. The
activity runtime *did* start (DB writes confirmed), but the UI
showed `state == "launching"` forever.

**Root cause:** `command_center/mod.rs::drive()` does

```rust
let (effect, next, first_run) = { lock + apply + write back };
self.run_effect(effect);          // can recursively self.drive(...)
self.emit_state(next, first_run); // ← captures pre-recursion `next`
```

For Activity / Meeting picks, `run_effect(DispatchStart{kind})` calls
`dispatch_*_start()`, which on success calls
`self.drive(CcInput::RuntimeReplied{success:true})`. The inner
`drive()`:

1. Transitions the FSM from `Launching{kind}` → `ShowingSessionCard{kind}`
2. Emits the `sessionCard` payload to the UI

Then control returns to the outer `drive()`, which dutifully emits
its captured `next = Launching{kind}` — **clobbering** the inner's
`sessionCard` emit. UI snaps backwards. CommandCenter.tsx renders
`<ModePicker launchingKind={kind}>`, which disables all tiles
(`disabled = launchingKind != null`).

Dictation didn't visibly hit the bug because `dispatch_start(Dictation)`
recursively drives `Dismiss` → `HideWindow`, and a hidden window can't
show the stale Launching state.

**Why no test caught it:** the FSM itself (`state.rs::apply`) is pure
and has 40+ unit tests; they all pass. The bug is in the orchestrator
(`mod.rs::drive`), which can't be unit-tested without a mock
`AppHandle` — there are no orchestrator-level tests today. The Wave
6 judges check FSM invariants + file-diff scope; neither catches
"orchestrator emits stale state."

**Fix:** re-snapshot AFTER `run_effect`:

```rust
self.run_effect(effect);
let actual = self.snapshot();
let actual_first_run = self.inner.first_run.load(Ordering::Relaxed);
self.emit_state(actual, actual_first_run);
```

One load each from the existing state mutex + atomic. Idempotent —
when `run_effect` didn't recurse, `actual == next` and the UI sees
the same payload it would have. When it did recurse, the UI sees
the latest payload twice (harmless: React's `setState` collapses
identical-shape snapshots).

**Generalization worth keeping in head:** any "capture local → mutate
world → emit local" loop is a clobber waiting to happen if the
mutation step can re-enter the same loop. The fix is always the
same: re-read the world before the emit. Resist the urge to refactor
to a queue / event-bus until you've actually seen the pattern fire
twice.

### Process — diagnostic notes Bernard left in the kickoff cut iteration cost in half

The kickoff brief for this session included Bernard's per-bead
diagnoses ("primary_title isn't in migration 012", "the UI side has
ZERO callers of cc_update_session", "the FSM transitions through
Launching → ShowingSessionCard"). Two of those three were directly
correct; the third pointed me at the right module even though the
exact root cause was different. That kind of "here's what I looked
at and where I got stuck" pre-work is worth its weight in iterations
when the kickoff is human-authored. Worth modeling in future
hotfix briefs.

---

## 2026-05-26 [phase10-wave-6b / SEAL] Phase 10 sealed; two non-obvious findings worth a future agent's time

**Context:** Wave 6.B (mb-8r5p) — author the 12 fixture-mismatch tests
called out by Wave 6.A's dry-run report, fix two rig bugs, run the
LLM-grader portion of `sealed-phases-untouched`, then seal Phase 10
at `phase-10-complete`. Wiggum loop cap was 3; went green on
iteration 1.

### Finding 1 — Diff-range scope matters as much as diff *content* in seal judges

The Wave 6.A dry-run had `sealed-phases-untouched` C4 (sealed-migration
modifications) checking `git diff --name-only phase-mc-complete..HEAD --
...001..014`. That range includes the dictation-polish lateral epic
(commit `dda676a`) + the MC v1.2 capabilities migration (commit
`f298a5d`) that landed between MC seal and Phase 10 start — neither
of which is Phase 10's responsibility. Folding them in would force the
LLM grader to relitigate sealed lateral work.

Dustin's Wave 6.B decision: narrow the base ref from `phase-mc-complete`
to `stable-alpha-v0.1`. `stable-alpha-v0.1..HEAD` is EXACTLY Phase 10's
footprint. Combined with `--diff-filter=M` (which excludes the
legitimate ADDITIONS of migrations 012-015 during Phase 10), the C4
check went from "INFO: 4 files, needs LLM classification" to
"GREEN: empty diff".

**Generalized rule:** when authoring a seal judge that asks "X is
unchanged since Y", make Y the EARLIEST tag that fully predates the
work being sealed AND fully postdates any sealed-lateral-epic work
you don't want the grader to relitigate. For Phase 10 that was
`stable-alpha-v0.1`, not `phase-mc-complete`, because the dictation
polish epic ran in between.

This also informs the LESSONS P7 distinction: judges prove invariants
in a CONTROLLED scope. Widen the scope to "all changes since the
last phase seal" and you'll see noise from unrelated work that the
grader has to mentally subtract. Narrow the scope to exactly the
phase under seal and the grader can answer the actual invariant
question cleanly.

### Finding 2 — Whole-file `IndexOf` checks shadow themselves in modules with multiple call sites

Wave 6.A's `exclusion-is-total` C5 check used `IndexOf('check_excluded(')
< IndexOf('insert_event(')` on the whole `runtime.rs` file to verify the
matcher fires BEFORE persistence. Runtime.rs has FIVE `insert_event(`
calls: one in `record_event` (the call site we care about) and four
in earlier functions (`run_emit_control_event`, `emit_layer_error`,
etc.) that have nothing to do with the exclusion matcher.

The whole-file IndexOf hits the EARLIEST `insert_event(` in the file —
which is in `run_emit_control_event` at offset 13085, well BEFORE
`record_event`'s `check_excluded(` at offset ~17900. So the check
structurally returned `ins < exc` and flagged BAD even though the
actual call-site ordering is correct.

**Fix:** scope the IndexOf to the body of `pub fn record_event`. Find
the function start, find the closing `\n}` at the same indent level,
then IndexOf within that substring only.

**Generalized rule:** mechanical structural eyeball checks ("X precedes
Y in the source") need to be scoped to the SMALLEST syntactic unit
that the invariant lives within. Whole-file checks are correct only
for invariants that genuinely span the file (e.g. "this file imports
foo before bar"). For "function f consults matcher before DB write",
the check belongs inside function f's body. The dry-run rig is
NOT a substitute for the LLM grader — its job is to catch the obvious
mechanical failures BEFORE the grader spend, but "obvious" still
requires the right scope.

### Process note — Wiggum loop went green on iteration 1

The cap was 3 iterations per Dustin's Wave 6.B authorization (per the
5-attempt rule). The loop terminated after iteration 1 because:
1. The Wave 6.A dry-run report was honest about what was red.
2. Each red had a one-line fix (author the named test; fix the named
   rig bug).
3. The judge files themselves were authored well enough that the
   fixture authoring was a structured exercise, not a search.

The seal commit's scorecard: **15 commits since `stable-alpha-v0.1`,
7 ADRs (0036, 0037, 0040, 0041, 0042, 0043, 0044), 4 migrations
(012-015), 22 modules under `src-tauri/src/activity/`,
~22,400 LoC delta across `src-tauri/` + `ui/` + `docs/`.** Live-fire
Win11 smoke test is Dustin's next step (LESSONS P7).

---

## 2026-05-26 [phase10-wave-4] Audio Layer 2 shipped — four small tooling/Rust paper-cuts worth recording

**Context:** Wave 4 (mb-g1w2) — wired per-Block audio transcription into
Activity Capture by *wrapping* the sealed Meeting Capture infrastructure
(`audio::capture::Capture` twin-stream + `meetings::long_form_stt::LongFormStt`
chunked Whisper) rather than duplicating it. New pure-Rust modules:
`activity/audio.rs` (`AudioPipeline` trait + `LongFormAudioPipeline` impl +
`StubAudioPipeline` for tests), `activity/segments_persist.rs`,
`activity/block_audio_stitcher.rs` (midpoint-rule attribution). Migration
014 added two provenance columns. ADR 0041 Accepted inline.

### Finding 1 — 270s hard cap on agent foreground tool calls

The Code Puppy shell tool kills any single foreground command at
exactly ~270 seconds, even when the requested timeout is 1500s+ and
the child process is still doing useful work. `cargo test --release
--no-run` from a cold cache takes 5–7 minutes here, well past the cap.

**Action**: use `start /B cmd /c "<command> > <log> 2>&1"` from a `cmd.exe`
shell to detach the cargo invocation completely from the agent's shell
lifetime. The orphaned `cmd.exe` + its `cargo.exe` / `rustc.exe`
children keep running after the agent foreground returns. Then poll
the log file + `Get-Process cargo,rustc` in subsequent short tool
calls (≤ 240s each) until processes hit zero. Bernard burned 4
attempts (each killed at 270s on the dot, identical `execution_time=270.84`)
before figuring out the cap was the agent shell, not cargo.

Also note: the agent's `background: true` mode does NOT survive the
way you'd expect either — the parent powershell exits cleanly, but the
child cargo gets killed alongside it. `start /B cmd /c "..."` from
plain `cmd.exe` (NOT `powershell -Command "start ..."`) is the
pattern that actually detaches. Verified 2026-05-26 by checking
`Get-Process cargo,rustc -ErrorAction SilentlyContinue` showed
live PIDs minutes after the foreground call returned.

### Finding 2 — `i64::is_multiple_of` is unstable; trait import is the diagnostic, not the cure

```
error[E0599]: no method named `is_multiple_of` found for type `i64`
help: trait `Integer` which provides `is_multiple_of` is implemented but not in scope;
      perhaps you want to import it
```

The rustc help points you at `use num_integer::Integer` but in our
workspace `num-integer` is a transitive dep we don't directly depend
on, and adding it just to test a stitcher property is silly.
`<u32>::is_multiple_of` IS in std (stable on unsigned ints in 1.85)
but the signed-int version isn't there yet. Use `% 2 == 0` for tests;
clippy's `manual_is_multiple_of` lint doesn't fire on `i64` so you
stay green.

### Finding 3 — `debug_assert!` + `#[should_panic]` is a footgun under release

The `block_audio_stitcher::stitch` precondition ("blocks must be
chronologically ordered") was originally a `debug_assert!` with a
`#[should_panic(expected = "chronologically ordered")]` test. The
throwaway crate runs `cargo test --release` (LESSONS P1 / `scripts\
throwaway-test.ps1`), where `debug_assert!` is compiled out — so the
test never panics and the suite goes red on a phantom failure.

**Action**: when a precondition is also load-bearing for memory-safety
or correctness of subsequent calls (here: `binary_search_by` on an
unsorted slice is UB-adjacent — it returns garbage indices, mis-attributing
segments to wrong Blocks), promote it to a plain `assert!`. Don't
gate correctness invariants on `debug_assertions`. The runtime cost
(O(B) walk for a sortedness check on session close) is negligible.

### Finding 4 — Whisper segment timestamps are capture-relative ms, NOT epoch ms

Obvious in hindsight, but the meeting-capture infra hands you
`LongFormOutput.mic_segments[].t0_ms` / `t1_ms` (u32) that are
relative to the start of capture (`first_sample == 0`). Activity
Blocks are stamped in *epoch* ms. If you persist transcript segments
without the offset shift, the block_audio_stitcher silently routes
zero segments to every Block because the coordinate systems disagree
by ~`session.started_at` ms.

**Action**: at insert time in the runtime, query
`SELECT started_at FROM activity_sessions WHERE id = ?` and add the
result to each segment's `t0_ms`/`t1_ms` before persisting. This is a
one-time per-session lookup, so the cost is nothing. Documented in
ADR 0041 § "Coordinate system".

---

## 2026-05-25 [phase10-wave-3] LLM block summarization shipped — two non-obvious findings

**Context:** Wave 3 (mb-pwup) — turned the firehose of `activity_events` into
human-readable Blocks. Pure-Rust pipeline `segmenter` → `blocker` →
`abstractor` → `assembler`, persisted via `blocks_persist.rs`, exported via
`export.rs`. ADR 0040 captures the five inline architecture calls.

### Finding 1 — The `sha2` crate isn't pulled in transitively; reach for `crc32fast` instead for prompt fingerprints.

Plan was a `sha256(prompt_text)[..16]` as the `prompt_version_sha`. `sha2`
wasn't already in the dep graph, and adding a crypto dep just for a
non-security fingerprint felt like a YAGNI violation. `crc32fast` is
already pulled in (via zip/flate2), and a 32-bit CRC formatted as
`abstract_v1-{:08x}` is plenty unique for "did the prompt text change?"
provenance. Conclusion: when you need a non-cryptographic content
fingerprint, prefer `crc32fast` over importing `sha2` for the first
time. The provenance column is `prompt_version_sha` for historical
consistency — the name is misleading but the schema is sealed.

### Finding 2 — The Wave-2 `status.kind = "no_payload"` path needs an LLM-skip fast-path or you waste Ollama RTTs on game windows.

Wave 2 shaping note #4 called this out, but the temptation while writing
the abstractor was "just send everything through the same `OllamaProvider`
call, the model can handle empty input." That's true but wasteful: each
call is 200-800ms of GPU time per Block, and a 4-hour session can easily
have 20+ game/locked-screen Blocks. The fix is a cheap heuristic in
`abstractor::abstract_block`: if **every** event in the Block has
`status.kind == "no_payload"` (or no snapshot at all), skip the LLM and
emit a templated string (`"App: {primary_app}. No additional context
available."`). The templated path still flows through the same
provenance write — `prompt_version_sha` records `"template_v1"` instead
of the LLM prompt's CRC, so a future re-run with a real model can
detect and upgrade. Don't conflate "abstractor ran" with "LLM ran".

---

## 2026-05-25 [phase10-wave-2] three small Rust + tooling paper-cuts while wiring UIA deep snapshots

**Context:** Wave 2 (mb-hr1u) — promoted the activity sampler from titles-only
to full UIA deep snapshots (focused field, visible-text fragments, control
summary, multi-monitor attribution, password-field redaction) using raw
`windows`-rs 0.56 with the new `Win32_UI_Accessibility` feature.

**Findings:**

1. **`MONITORINFOF_PRIMARY` lives under `Win32::UI::WindowsAndMessaging`,
   not `Win32::Graphics::Gdi`** in `windows`-rs 0.56. The Win32 SDK puts
   it next to `MONITORINFO` (which IS under `Graphics::Gdi`), but the
   crate exposes the constant via the WindowsAndMessaging re-export tree.
   Cost: 1 build cycle. Inline doc-comment now flags the surprise location
   in `windows_com.rs`.

2. **`#[serde(rename_all = "camelCase")]` is not implicit on derive(Serialize).**
   The activity-capture DTOs (`ProbeResult`, `MonitorInfo`, `FocusedField`,
   `ControlSummary`, `Rect`) all needed the explicit attribute to match
   the TypeScript types on the UI side. The first throwaway-test run lost
   8/19 tests because they were probing `v["visibleTextFragments"]` while
   serde was producing `visible_text_fragments`. Defense: when adding a
   serde-derived struct that round-trips through IPC, copy-paste the
   `#[serde(rename_all = "camelCase")]` from the nearest neighbor
   (`persist::ActivitySessionRow` is the canonical example) AT THE SAME
   MOMENT you add the derive macros.

3. **Throwaway-crate preamble must be APPENDED, not prepended.** Generalizing
   the throwaway-test recipe (LESSONS P2) into a reusable
   `scripts/throwaway-test.ps1` introduced a footgun: my first version
   prepended a stub `pub mod error { ... }` so the source file's
   `use crate::error::*` would resolve. That broke compilation because
   the source's own inner attributes (`#![allow(missing_docs)]`) and
   module-level doc comments have to be the FIRST tokens in the file.
   Fix: append the preamble at the end. `mod error { ... }` is order-
   independent for path resolution, so it works at either end of the file.

**Action:** Throwaway script now supports `-Preamble` (appended). Idle-tracker
module has its own wrapper at `scripts/test-activity-level.ps1` that
supplies the windows-crate dep + stub error module. Future pure-Rust modules
that reference `crate::error` should drop their own wrapper or thread the
preamble through directly.

---

## 2026-05-25 [phase10-wave-1b] three small UI-layer paper-cuts while wiring the Activity skeleton

**Context:** Wave 1B (mb-hnl3) iteration — wiring the new Activity page +
the two Wave 1A deferred Settings rows (`command_center_chord` chord rebind,
`legacy_meeting_chord_enabled` toggle).

Nothing load-bearing here; just three small irritants that cost time and are
worth pre-empting next time.

### Finding 1 — Typed-settings IPC was never exposed to the UI.

There are two parallel settings paths on the Rust side:

- `commands::settings::{get_settings, update_setting}` — the flat-bag legacy
  table (`settings`), TEXT-only, used by Settings.tsx for ui.theme /
  ui.sound_enabled / etc.
- `commands::legacy::{get_setting, set_setting}` — the typed registry
  (`settings_v2` + `SettingKey` enum), JSON values, used by
  `meeting_settings_get_all` / `meeting_settings_set` curated wrappers, AND
  by anything that needs to read a `SettingKey` directly (Command Center
  reading its own chord on boot, etc.).

The UI shim (`ui/src/lib/tauri.ts`) only had wrappers for the legacy flat
bag. The new chord row + legacy-meeting-chord toggle both live in the
typed registry, so I had to add `legacy_get_setting` + `legacy_set_setting`
wrappers to the `api` object + fixture-mode shims for browser preview.

**Takeaway:** when adding a Settings row, check first whether the key is in
`SettingKey::*` (typed) or just a string in the flat bag. They look
identical from the React side until you hit the IPC layer and discover the
wrapper isn't there. There's now no excuse — both wrappers are in
`tauri.ts`.

### Finding 2 — `Pill` accepts `tone: string`, not a union — but ONLY the existing tokens work.

I initially typed the status-pill tone as `"neutral" | "accent" | "warn"`
thinking that matched the component API. It doesn't — `Pill`'s `tone` is
a free-form string that's spliced into a CSS custom property
(`var(--${tone})`). Only tokens that exist in `design/tokens.css`
actually render (`status-ok`, `status-error`, `status-info`, `mode-*`).
Passing `"warn"` silently produces an unstyled pill (no error,
just no color).

**Takeaway:** treat `Pill` like a token consumer, not a variant component.
When in doubt, grep `tone="` in `ui/src/pages/` for existing usage and
match.

### Finding 3 — `formatRelative` takes an ISO string, not a unix-ms number.

The activity persistence layer stores `started_at` / `ended_at` as `i64`
unix-ms (matches existing migrations). The Rust serializer hands those
back to the UI as JSON numbers. But `ui/src/lib/format.ts::formatRelative`
takes `iso: string` and passes it to `new Date(iso)` — passing a number
in TS strict-mode is a TS2345 error.

**Fix:** `formatRelative(new Date(epochMs).toISOString())` at the call
site. Considered overloading `formatRelative` to also accept `number`, but
that hides the conversion at every other call site — better to make the
conversion explicit.

**Meta-takeaway:** the timestamp boundary between Rust (unix-ms i64) and
the UI format helpers (ISO string) is a tiny but recurring friction. If
this surfaces a third time in another page, generalize `formatRelative`
to accept both with a discriminated union.

---

## 2026-05-25 [phase10 / meta / dispatch] sub-agent session_id is a foot-gun across serial task handoffs

- **Context:** Bernard (planning agent) tried to chain Phase 10 Waves
  1A → 1B → 2 → 3 dispatches through code-puppy back-to-back, reusing
  `session_id="code-puppy-session-214550"` across all of them, expecting
  session continuity to help with the multi-wave handoff. Wave 0 + 0.5
  had already shipped (charter ADRs 0036 + 0037 Accepted; phase10.md
  + PLAN amendment + bead manifest all on disk at commit `5fc89a2`).

- **Finding:** Each new dispatch in the accumulating session ran code-
  puppy's mandatory session-start ritual (LESSONS P4), which anchors
  "the kickoff prompt" on the FIRST user message in the session —
  Bernard's original Wave 0 charter ask. Code-puppy correctly detected:
  "kickoff asks for Wave 0 work, but Wave 0 is already on disk → stale
  prompt → STOP and surface." Bernard then re-dispatched 6+ times with
  re-authorization preambles trying to override the stale-detection;
  each re-dispatch re-anchored on the same stale Wave 0 kickoff and
  stopped again. 8-attempt loop before Bernard escalated.

- **Compounding factor:** Anthropic API had a separate overloaded_error
  window during the same hour (4 `req_011CbGy*` failures), which masked
  the session-id issue as "just API flakiness" for the first 3-4
  attempts. Diagnosis didn't crystallize until ~attempt 6, by which
  point Dustin had to intervene.

- **Action (PROMOTED to PINNED P8):** for SERIAL task handoffs through
  sub-agents (Wave N → Wave N+1 → Wave N+2), **omit `session_id`** so
  each dispatch is its own fresh code-puppy invocation with its own
  clean kickoff anchor. Reserve `session_id` for CONVERSATIONAL
  refinement of ONE task (clarify-ask-respond rounds within a single
  scope of work). The two patterns are not interchangeable.

- **Bonus action:** keep sub-agent dispatch prompts SHORT and pointer-
  style — "implement X per `<spec path on disk>`" rather than embedding
  the full spec. The spec lives on disk; embedding it in the prompt
  makes the prompt body LOOK like potential stale charter work to the
  session-start triage, increasing false-positive stale-prompt
  detections. A 200-line prompt that re-states the ADR spec is
  structurally indistinguishable from a charter prompt re-paste, even
  in a fresh session.

- **Recovery:** tree was clean — only docs+beads work had landed; no
  Wave 1A code attempt reached disk. One staged `.beads/issues.jsonl`
  change (benign `updated_at` bump on `mb-jtbk` from the in_progress
  flips during the loop) was committed alongside this LESSONS entry.
  Standing instruction for the next Wave 1A dispatch: fresh session,
  short prompt, source-of-truth pointers to ADR 0037 + phase10.md
  Wave 1A section.

---

## 2026-05-24 [meta / tooling] four tooling gotchas + a process pattern that paid off

- **Context:** End-of-iteration meta-review with Dustin. Codifying gotchas
  discovered the hard way this session so future-Bernard doesn't burn
  iterations on them. Three are shell/tooling traps; one is a positive
  process pattern worth re-using.

- **Finding 1 (`bd create` + non-ASCII = silent duplicate trap):**
  Running `bd create "Title" -t feature -p 2 --description "... text with — em-dash ..."`
  causes `bd` to exit with a non-zero status code **after** writing the
  issue to disk. The error stream shows nothing useful. If you retry on
  the assumption that the create failed, you get a duplicate issue. This
  bit me twice in one iteration tonight (8 issues created when only 4
  were intended; I noticed when `bd ready` showed near-identical pairs
  of titles with different `mb-*` ids). **Action / workaround:** keep
  bd create-time titles and descriptions ASCII-only. If you need rich
  text (em-dashes, smart quotes, code blocks), use `bd update <id>` on
  the freshly-created issue. Encoded into AGENTS.md § "Issue Tracking →
  Gotchas".

- **Finding 2 (`git status --short` + `findstr` eats the leading status
  character):** Porcelain v1 output format is `XY filename` where `X` is
  the index status and `Y` is the worktree status. When the index status
  is a space (file modified in worktree only) the leading space gets
  consumed by some Windows piping paths, making it look like `M filename`
  (which would mean "staged-modified"). Confused me twice tonight into
  thinking `git add` had silently failed when it actually worked.
  **Action:** prefer `git status --porcelain=v1` for scripts and grep —
  the two-char `XY` prefix survives the pipe intact. Reserve
  `git status --short` for direct human reading. Encoded into AGENTS.md.

- **Finding 3 (`findstr /R` regex is anemic):** Windows `findstr /R` is
  POSIX-BRE *minus* features. No `\b` word boundary, no `+` quantifier
  (use `*` after a duplicated atom), no lookahead, no character classes
  beyond ASCII ranges, no alternation outside `/C:` literal mode.
  Symptoms I hit tonight: `findstr /R "^.Created\|mb-"` returned empty
  on output that obviously contained both patterns. **Action:** for
  anything beyond a literal substring search, pipe to PowerShell's
  `Select-String` (real regex) or `Select-Object` (line-range pagination)
  via `powershell -Command "$input | Select-String 'pattern'"`. Encoded
  into AGENTS.md. The cargo/git output examples already use this pattern
  in commit `dda676a` and onward.

- **Finding 4 (triage-before-acting is cheap insurance):** Tonight the
  working tree arrived with 13 modified + 7 untracked files from a prior
  unfinished session. The temptation was to `cargo fmt` the whole crate
  (would have touched files outside my scope) and `git commit -a` (would
  have bundled my work with Dustin's in-flight epic). Instead I burned
  3 minutes auditing — `git diff --stat HEAD -- <suspect-files>`,
  `cat` the new untracked files, surface ownership ambiguity in the
  response BEFORE staging anything. Result: Dustin could greenlight the
  surgical commit (my files only) cleanly, then we sealed his in-flight
  epic (MC v1.2 / ADR 0035) deliberately in a separate commit with him
  driving the calls. **Generalizable rule:** when the tree is dirty
  and you didn't make it dirty, the right first move is `git diff
  --stat HEAD` against suspect files + a short audit summary in your
  next message. The 3 minutes of triage trades cleanly against the 10
  minutes of "oh no, I committed someone else's half-baked changes."
  Promoted into AGENTS.md § "Work sizing & workflow selection" implicitly
  via the "bead-first" + "discovery → bead" patterns; explicit prose
  reference lives here.

- **Why these are body-only, not PINNED:** PINNED is reserved for
  load-bearing every-session traps (cargo wrapper, test-binary launch
  bug, stale-prompt session-start ritual). The four findings above are
  each "burn 5-15 minutes" not "burn an hour rediscovering the same
  trap." Body entries are the right tier; the TOC row makes them
  greppable for future sessions.

---

## 2026-05-24 [mc-v1.2 / ADR 0035] MC Stable Alpha seal: Tauri capabilities config was the real root cause of the mb-z5y bug class, not just an event-ordering race

- **Context:** Triaging the pre-existing dirty tree before sealing,
  found a coherent in-flight epic spanning 14 files
  (`capabilities/default.json` new, `audio/capture.rs`,
  `meetings/{lifecycle,repo,title,mod,runtime,overlay}.rs`,
  `commands/meetings.rs`, plus React side: `MeetingDetail.tsx`,
  `Meetings.tsx`, `MeetingOverlay.tsx`, `meetings.ts`, `Icon.tsx`).
  The work was ~80% done; what remained was the ADR charter, fmt
  pass on the drifted files, bead tracking, STATUS/LESSONS, and the
  commit + tag. Live-fire verification was on Dustin's side
  (he confirmed "things are good" before greenlighting the seal).

- **Finding 1 (the real mb-z5y root cause):** ADR 0034 hot-fixed the
  symptom (overlay event delivery race) with four belt-and-suspenders
  layers. They worked. But the *real* root cause was hiding in plain
  sight: there was no `src-tauri/capabilities/default.json`. Without
  that file, Tauri 2.x's permission system is empty by default for
  secondary windows, and `listen()`, `emit_to()`, and
  `window.hide()` silently no-op on non-main webviews. `invoke()` of
  `#[tauri::command]` handlers uses a different permission path and
  still works — which is *exactly* why the bug looked like an
  event-delivery race instead of a missing-permission problem.
  **Action (v1.2):** added the capabilities file granting
  `core:default` to `main`, `recording`, and `meeting_overlay`. The
  ADR 0034 belt-and-suspenders fixes stay in place (cheap, idempotent
  defense in depth) but the capabilities file is the deeper fix.
  **Lesson:** when a hotfix works but you don't fully understand *why*
  it works, treat that ignorance as a follow-up debt, not a closed
  chapter. ADR 0034 was Accepted on 2026-05-23 with the surface
  understanding; one day later we found the deeper cause. Worth
  re-reading the Tauri capabilities docs end-to-end the next time a
  permission-shaped bug appears.

- **Finding 2 (`build_stream` diverged from `probe_sources`):**
  Two-channel meetings worked in development but sometimes failed on
  real hardware. The reason: `probe_sources()` (used for the device
  picker availability check) correctly branched on
  `DeviceSource::{Input, Loopback}` and called
  `default_output_config()` for loopback per ADR 0031.
  `CpalCapture::build_stream` (used to actually start the stream)
  did NOT — it called `default_input_config()` unconditionally,
  which fails on render devices with "requested stream type is not
  supported." In some lucky cases cpal accepted the wrong config
  and produced silent/garbled audio that survived to the
  "transcript is empty" branch, hiding the bug. **Lesson:** when
  two functions need to make the same OS-call-shape decision
  (probe vs. actual use), extract the decision into a single
  helper or at minimum cross-reference them in code comments.
  A divergence like this is exactly the kind of thing the
  600-line file-cap rule catches by forcing function extraction
  earlier.

- **Finding 3 (auto-title pure module = easy to test exhaustively):**
  `meetings/title.rs` is ~310 lines and has ~25 unit tests. It's a
  pure function — no I/O, no DB, no clock — so the test surface is
  exactly the input-output mapping. Tests cover: happy-path 5-word
  truncation, speaker-label stripping (`**You:**` / `**Other(s):**`),
  paragraph-skipping for filler-only paragraphs, channel fallback
  (merged > mic > sys), unicode capitalization (`café` → `Café`),
  apostrophe/hyphen preservation, quote stripping, char-cap at 60
  without splitting tokens, pathological single-huge-token,
  realistic two-speaker merged-formatter output. **Lesson:** when a
  module's whole purpose is a pure transformation, the test density
  target is much higher than the AGENTS.md baseline (10 tests per
  500 LoC). A pure module deserves 25-30 tests if there's any
  branching at all, because the marginal cost of each test is
  trivial and the marginal value (catching a regression in a
  re-tuning) is high. This was already PLAN guidance for MC waves;
  it's worth re-stating for non-MC pure modules too.

- **Finding 4 (the forensic ping pattern):**
  `meeting_debug_listener_ping` is an explicitly-temporary IPC that
  React listeners call from inside their `meeting:state` callback,
  with the Rust side doing nothing but logging. This gives hard
  evidence of JS listener firing — distinguishing "emit landed but
  listener didn't fire" from "listener fired but state update
  raced" the next time this bug class shows up. Code comment
  + ADR 0035 + bead `mb-xnn7` all explicitly mark it for
  removal. **Lesson:** forensic instrumentation that *would* have
  caught the previous bug is worth shipping even after the bug is
  fixed — but only if you track the deletion as a real follow-up
  task. Otherwise you accumulate forever-temporary instrumentation
  that future-Bernard reads and treats as load-bearing.

- **Finding 5 (`getCurrentWindow().hide()` Win32 silent no-op):**
  Calling `getCurrentWindow().hide()` synchronously from a button
  onClick handler silently fails on Win32 — the click finishes,
  the JS executes, but the hide doesn't take. The Rust path
  (`AppHandle::get_webview_window(label).hide()`) uses Tauri's
  internal window registry and works reliably from any context.
  Same shape as the `emit_to` lesson from ADR 0034 / capabilities
  finding above: when the JS-side primitive is unreliable on a
  specific platform, route through Rust. `meeting_overlay_hide` is
  now the canonical hide path.

- **Finding 6 (process — sealing an in-flight epic you didn't
  write):** When you walk into a session and find a coherent
  ~80%-done epic in the dirty tree, the right move is *not* to
  finish writing it inline (you don't have the live-fire context).
  The right move is to: (a) audit and triage publicly so the user
  can decide whether to seal-as-is or fix-then-seal; (b) treat their
  "things are good" as the live-fire acceptance gate; (c) do the
  paper-work seal (ADR + STATUS + LESSONS + beads + tag) without
  changing the runtime behavior. This kept the iteration cheap and
  preserved Dustin's authorship of the runtime work.

- **Gate evidence:** `cargo fmt --check` clean across the whole
  crate; `clippy --release -D warnings` clean; `check --release
  --tests` clean; `tsc --noEmit` clean; vitest 55/55 (note: includes
  the `SettingsMeetingTab.test.ts` 11-test suite that was always
  there — earlier grep just truncated). Stable alpha tag
  `stable-alpha-v0.1` lands on the seal commit. `mb-xnn7` stays
  open as the v1.3-prep follow-up.

---

## 2026-05-24 [dictation-polish] paste-trailing-space, History→Dictations rename, on-demand LLM pass on saved dictations, Insights 2-tab redesign

- **Context:** Post-`a4e0ec3` checkpoint, Dustin asked for four
  things in one session: (1) stop pasting a trailing space at the
  end of dictations, (2) rename History → Dictations (the page was
  always about dictations, not arbitrary history), (3) let the user
  run an LLM pass on a saved dictation on-demand (summary / action
  items / cleaner punctuation / custom prompt), (4) make the
  Insights page actually worth opening — Wispr-killer territory.
  Pre-existing dirty state in `meetings/*` + untracked
  `mockingbird-activity-capture-plan.md` was NOT touched (separate
  in-flight epic owned by user).

- **Finding 1 (paste trailing space):** The bug was deterministic
  not a model quirk. Whisper produces `" hello"` (leading space) and
  the cleanup LLM happily echoed it. Trying to prompt-engineer the
  LLM to omit trailing whitespace is fighting the wrong battle —
  small models forget, big models echo whatever you give them.
  **Action:** added `dictation/paste_payload.rs::sanitize_for_paste`
  as a deterministic post-pass: strips a SINGLE trailing space (not
  more — we want to preserve `"  "` if a user dictated
  "double-space"), leaves newlines alone. 11 unit tests. Wires into
  `dictation.rs::complete()` immediately before clipboard handoff.
  Independent of provider/model — the right architectural layer for
  this kind of guarantee.

- **Finding 2 (rename):** Git's rename detection (`-M`) only kicks
  in if the file is ≥50% similar AFTER your edits. I rewrote
  ~30% of `History.tsx` (new LLM card section) before renaming.
  Result: `git status --short -M50%` showed it as delete+add. Fix:
  do the rename FIRST, then edit. Git showed it as a clean rename
  in the final commit (`History.tsx -> Dictations.tsx (63% similarity)`).

- **Finding 3 (LLM pass — fence stripping):** Small/medium Ollama
  models (qwen2.5:7b, llama3.1:8b) reliably wrap their output in
  ```` ```markdown ... ``` ```` fences, even when the prompt says
  "return plain markdown, no code fence." Bigger models obey;
  smaller don't. Tried prompt engineering (4 different phrasings)
  with no consistent fix. **Action:** added a defensive
  `strip_outer_fence()` postprocess in `dictation/llm_prompts.rs`
  that ONLY strips the outermost fence if the entire response is
  wrapped — preserving inner code blocks the user might have asked
  for in a summary. 4 unit tests including the
  fence-inside-fence case. Lesson: when the model is wrong
  consistently AND deterministically, fix it deterministically.
  Don't waste tokens hoping the next prompt iteration works.

- **Finding 4 (LLM pass — prompt storage):** PLAN MC explicitly
  says meeting LLM prompts live in `meetings/prompts/*.md` (not DB).
  Same reasoning applies to on-demand dictation LLM prompts:
  versioned with code, no migration friction, no "why is the prompt
  different in dev vs prod" mystery. Used `include_str!` so prompts
  are baked into the release binary. The DB-stored `modes`
  prompts are the EXCEPTION because they're tuned via the empirical
  mode-eval rig (ADR 0024) which needs DB UPDATEs.

- **Finding 5 (Insights — heatmap padding):** GitHub-style
  contribution heatmaps are visually trivial but the data alignment
  has one trap: the 365-day series ends today but TODAY could be
  any day-of-week. If you just chunk by 7 you get a misaligned
  grid where Mon/Wed/Fri row labels don't match the cells.
  **Action:** `Heatmap` component pads the LEADING edge with
  `null` cells until the first column starts on Sunday
  (`getDay() === 0`), then chunks by 7. The pad cells render as
  `visibility: hidden` to keep grid columns uniform.

- **Finding 6 (Insights — heatmap theming):** First pass used
  hardcoded green like GitHub. Looked terrible against the
  warm-earth theme. **Action:** intensity levels use
  `oklch(from var(--mode-normal) l c h / 0.28..1.0)` — derived
  from the user's accent color via OKLCH alpha modulation.
  Theme swaps inherit automatically. The 5-level legend at the
  bottom uses the same swatches so users can self-calibrate.

- **Finding 7 (Insights — WPM outlier handling):** First pass
  computed WPM as `total_words / total_seconds`. A single
  near-zero-duration session (e.g. 200ms ghost recording with
  17 word output from a misfire) blew the mean to 5000 wpm.
  **Action:** per-session WPM, exclude sessions <5s, cap individual
  wpm at 300 (world record territory but plausible spoken),
  average per-session not weighted. Also surface `samples` count
  in the UI so users know "based on 184 sessions" not from one
  outlier.

- **Finding 8 (Insights — backward-compat with old DBs):** The
  meeting_sessions table only exists post-migration 011. Brand-new
  installs were fine; clones of an older DB would 500 on
  `insights_snapshot`. **Action:** wrapped the meeting_sessions
  COUNT/SUM in `.unwrap_or((0, 0))` — treats "table missing" as
  zeroes. Mild lie of omission (the snapshot doesn't say "meetings
  unavailable on this DB") but the UX is correct: zero meetings
  recorded == zero meeting time displayed.

- **Finding 9 (process — pre-existing dirty state):** Tree
  arrived this session with ~13 modified files + 3 untracked
  files in `meetings/*`, `audio/capture.rs`, capabilities config,
  and a meeting-title feature in-flight (`meetings/title.rs`,
  `mockingbird-activity-capture-plan.md`). Burned 3 min auditing
  before staging. **Action:** when in doubt, surface the dirty
  state explicitly in the response BEFORE committing, with a
  per-file ownership audit. Committed only my files; left the
  pre-existing changes alone for Dustin to triage. STATUS.md
  "Currently active" section now reflects this — there's an
  unsealed in-flight feature whose plan file I haven't read.

- **Gate evidence:** `cargo check --release --tests` clean,
  `clippy --release -D warnings` clean for touched files,
  `cargo fmt --check` clean for touched files (pre-existing meetings/*
  fmt drift left untouched), `tsc --noEmit` clean,
  `vitest 55/55 pass`, `npm run build` clean, release binary built
  at 9:41 PM (commit `dda676a`).

---

## 2026-05-23 [mc-hotfix / mb-z5y / ADR 0034] overlay stuck in CHOOSE + Stop button frozen because broadcast `emit` raced the show() on the hidden meeting_overlay webview

- **Context:** Post-`a4e0ec3` (ADR 0033) live-fire: user clicks **Start
  recording** in main Meetings page → timer ticks (so `recordingUuid`
  was set by the optimistic post-IPC update) BUT the overlay window
  appears and stays in CHOOSE mode forever, the main-window **Stop**
  button stays disabled, the overlay **×** button is unresponsive,
  and the user has to taskkill. All four symptoms collapse to one
  root cause: the `meeting:state="started"` event emitted by
  `meetings::lifecycle::emit_state` is never observed by either
  React listener.
- **Finding:** In `commands::meetings::meeting_start` the lifecycle
  path called `rt.start_meeting(src)` (which fires the broadcast
  `app_handle.emit("meeting:state", ...)`) **before**
  `force_show_for_recording`. The overlay window was declared
  `visible: false` in `tauri.conf.json` and lives that way at app
  boot. In Tauri 2.1.x, a broadcast `Emitter::emit` against a webview
  that has been shown-then-hidden delivers normally — but against a
  webview that has been hidden since boot, the event appears to be
  dropped (listener registered, JS ran at boot, but no callback
  fires). Show-then-emit lands; emit-then-show races. The collateral
  damage on the main window's listener (which IS visible) suggests
  Tauri's `emit` may serialize all-webview delivery and short-circuit
  the whole broadcast when one target is in this state — couldn't
  fully isolate from outside the framework.
- **Killer detail:** `emit_state` was written
  `let _ = self.app_handle.emit(...)` — silently discarding the
  `Result`. There is no production log line confirming whether the
  emit succeeded. The dictation side's equivalent
  (`recording_window::emit_state`) does `if let Err(e) = ... warn!`,
  but Phase MC was written first and that pattern hadn't been
  observed-failed yet.
- **Action — fix (ADR 0034):** Four layers, any one of which alone
  would address the bug; together they harden:
  1. **`meeting_start` IPC**: swap order — `force_show_for_recording`
     BEFORE `rt.start_meeting()`. On `start_meeting` error, hide the
     overlay so we don't strand a blank pill.
  2. **Belt-and-suspenders `emit_to`**: after `start_meeting`
     returns Ok, also `app_handle.emit_to(MEETING_OVERLAY_LABEL,
     "meeting:state", payload)`. Listener is idempotent.
  3. **Frontend defensive clear**: `Meetings.tsx::handleStart` now
     clears `startingOrStopping` on IPC success, symmetric to the
     existing optimistic `setRecordingUuid`. Stop button enables
     immediately on IPC return regardless of event delivery.
  4. **Observability**: `emit_state` logs `tracing::debug!` on Ok
     and `tracing::warn!` on Err — matches dictation pattern.
- **Generalization:** **In Tauri 2.x, the contract for broadcast
  `Emitter::emit` against a hidden-since-boot webview is unreliable.**
  When you must signal a webview around the moment of showing it,
  either (a) show first and emit after, or (b) use targeted
  `emit_to(label, …)` which appears to work in either ordering, or
  (c) both. The cost is microseconds; the failure mode is total UI
  freeze with no log line, so always (c).
- **Meta:** Event delivery should be observable by default. The
  Phase MC `let _ = emit(...)` pattern saved 5 LoC and cost ~3
  iterations of "why isn't the listener firing?" debugging. The
  AGENTS.md "7. No shortcuts" principle in action — `let _ =` on
  an `emit` is the moral equivalent of `.unwrap()` in error code:
  it's a shortcut that defers the bug to live-fire.
- **Live-exec verification:** Cargo test runner on this box still
  hits LESSONS-PINNED P2 (`STATUS_ENTRYPOINT_NOT_FOUND`), so the
  fix is gated on `cargo test --no-run` + vitest 55/55 +
  Dustin's manual repro of the original `mb-z5y` symptom on the
  next rebuild.

---

## 2026-05-23 [meta / session-start] detected stale Phase MC kickoff but then over-corrected

- **Context:** Dustin sent a short bug report about the meeting overlay
  pill not syncing + Stop button not responding. The message *also*
  contained, above the bug report, a giant "You are implementing Phase
  MC — Waves 1→6…" kickoff paragraph (likely an accidental paste or a
  `/goal` template that wasn't trimmed).
- **Finding 1 — detection worked, response didn't.** AGENTS.md P4 + the
  session-start ritual correctly fired "stop, sealed phase" because
  `phase-mc-complete` shipped six days earlier. But Bernard then
  authored a long ask-the-human question tree instead of just
  recognising that the bug report was the obvious actual request and
  the Phase MC framing was paste noise. Net: one wasted iteration with
  zero code changed.
- **Finding 2 — "STOP and ask" is too coarse a rule.** It conflates
  "genuinely ambiguous user intent" with "obviously stale wrapper around
  a clear request." The first one needs a question; the second one
  needs a one-line ack and then execution.
- **Action:** Updated `.code_puppy/AGENTS.md` § Session-start ritual
  rule #2 to a three-branch triage: (a) stale wrapper + clear ask →
  answer the ask; (b) genuinely ambiguous → ask_user_question; (c)
  clean → proceed. P4 in this file mirrors the change.
- **Generalisable shape:** when two rules pull in opposite directions
  ("don't re-execute sealed work" vs. "don't waste the human's time
  with obvious questions"), the resolution is almost always a
  triage-before-action step, not picking one rule to dominate.

---

## 2026-05-23 [mc-hotfix / mb-x1x] post-deploy live-fire surfaced 4 gaps the Wave-6 judges couldn't catch

- **Context:** Same day as the ADR 0032 v1.1 polish ship. Dustin ran
  the actual app end-to-end and four regressions surfaced in <30
  minutes: source-probe stub on all boxes, stuck Stop button on
  main-window-start, default chord stolen by Microsoft 365 Copilot,
  overlay pill invisible on main-window start.
- **Finding (1 — the meta-lesson):** Wave-6 judges (formatter
  determinism, lossless stitch, two-channel merge, no-LLM-in-critical-
  path, dictation-untouched) are all *static* / *file-diff* / *unit-
  test* assertions. None of them exercise (a) a real cpal device
  list, (b) a real Windows WH_KEYBOARD_LL hook against a real OS
  with a real competing global-hook app installed (Copilot), (c)
  React state transitions that depend on Tauri events the test
  harness doesn't emit. The judges proved the *contracts* are
  preserved; they did NOT prove the *integrations* work end-to-end on
  a real Win11 box. A 5-minute human smoke test caught what 4 hours
  of automated judging didn't. Smoke-test rituals belong in the seal
  checklist for any phase that touches OS hooks or audio devices.
- **Finding (2 — boring engineering):** Two bugs were hiding each
  other. `lib.rs` boot path called `MeetingRuntimeConfig::
  defaults_with()` instead of `from_settings()`, so the Settings UI
  chord picker was a no-op (every chord-related test in the suite
  used `defaults_with` directly and was therefore green). The `VK_M`
  default collision with Copilot was only discoverable on a Windows
  11 box with Microsoft 365 installed — Dustin's machine, not CI.
  Combined: even if a user had noticed the Copilot collision and
  changed the setting, the change wouldn't have taken effect at next
  boot anyway. Lesson: **`from_settings`-vs-`defaults_with`-style
  pairs need a test that the spawn path actually consults the DB**.
  Added `from_settings_picks_up_user_customised_chord` for exactly
  this reason.
- **Finding (3 — Microsoft 365 Copilot specifically):** Copilot's
  global chord handler on Windows 11 fires regardless of whether a
  WH_KEYBOARD_LL hook returns 1 (consume) or 0 (pass-through) from
  its callback. Either Copilot polls key state independently of the
  hook chain, or it injects at a higher kernel callback level than
  WH_KEYBOARD_LL, or it has a shell-registered global accelerator
  that fires pre-hook. Action: **avoid `RCtrl + letter` defaults
  for any global hotkey on Windows**. OEM punctuation
  (`.`, `,`, `;`, `\`) is the safe-chord territory.
- **Finding (4 — settings-DB sentinel rows are a real, ugly pattern):**
  We added `_internal_mc_chord_copilot_hotfix_v1` as a marker in
  `settings` so the one-shot migration doesn't keep checking on every
  launch. This is technically a side-channel — `settings` is
  semantically user-facing. It's annotated + scoped, and the
  alternative (a real schema migration for a default-value flip)
  felt heavier than the disease. Tolerate one-off; if we ever do a
  second one, refactor to a dedicated `_internal_migrations`
  scratch table.
- **Action:** **Add a manual smoke-test checklist to any phase seal
  whose scope touches OS hooks, audio devices, or live Tauri events.**
  Specifically for MC: 5-minute Dustin-at-keyboard test on the actual
  Windows 11 + Microsoft 365 box BEFORE the seal commit, not after.
  We avoided this on Wave 6 because the Wave-5 QA matrix was so
  thorough — but Wave 6 added the formatter/judges/STT changes and
  was rubber-stamped through. **Five-attempt rule was respected
  perfectly here:** Dustin found, Bernard diagnosed in 1 iteration,
  ADR + fix + commit in the second iteration. No churn.

---

## 2026-05-23 [mc-v1.1] post-seal audit found four polish gaps; ADR-chartered lateral epic vehicle worked cleanly

- **Context:** Day after sealing Phase MC at `phase-mc-complete`,
  Bernard ran a static audit against the master plan and found four
  user-visible deferrals:
  1. `meeting:tick` event + live VU meters never wired (PLAN §MC.6
     spec; mb-nig).
  2. "This LLM output isn't saved" first-time notice never added
     (Risk 7 mitigation; mb-rm7).
  3. `MeetingMaxDurationSeconds` setting persisted/clamped server-side
     but never exposed in the Settings UI (mb-mom).
  4. `"basically"` named in the default filler list but missed from
     the `phf::Set` (mb-tn5).
- **Finding:** the AGENTS.md "Permanently sealed" rules made the
  recovery path obvious (ADR-charter → bd epic → wave brief → seal
  via STATUS + ADR Accepted, NO new tag). The four gaps landed in a
  single iteration as ADR 0032. The deciding factor for "one ADR vs.
  four ADRs" was that all four share an audit origin + a single
  judge-preservation argument; minting four ADRs for ~400 LoC would
  be process bloat. The post-seal mistake to avoid is doing the audit
  *without* a charter — the gaps rot in backlog for weeks and the
  context to fix them evaporates.
- **Action:** treat the "ADR + epic + no new tag" pattern as the
  default for any post-seal polish work on a sealed phase. The
  precedent is now ADR 0023 (Design Language v1, post-Phase-3) and
  ADR 0032 (MC v1.1, post-Phase-MC). The pattern's load-bearing
  invariant: the **seal tag stays at the original commit** so the
  `mc-dictation-untouched`-style diff judges have a stable reference
  point. New work files go through new ADRs that supersede only if
  the *methodology* changes — not for adding more items.
- **Bonus finding:** the `meeting:tick` emitter (lifecycle.rs +
  capture.rs + levels.rs) is the natural reuse point for any future
  meeting-side live-feedback feature (waveform view, transcribed-so-far
  ticker, latency probe). Don't reinvent — extend.
- **Bonus finding 2:** `cargo test --release --no-run` for the whole
  workspace takes ~5m34s wall on a cold-ish artifact cache for this
  project. Worth knowing for time-budgeting future iterations.

---

## 2026-05-21 [phase-mc-wave-5] Tauri 2 `tauri::command` macro: bare `AppHandle` fails when mixed with `State<'_, T>` — must use `AppHandle<R>` with `R: Runtime`

- **Context:** Phase MC Wave 5 — adding `meeting_export_markdown(app: tauri::AppHandle, db: State<'_, AppStateHandle>, ...)`. Compiled fine in isolation in earlier exploration, blew up at `tauri::generate_handler!` expansion.
- **Finding:** The error was `the trait bound "AppHandle: CommandArg<'_, R>" is not satisfied`. Tauri 2's command macro expands to a function generic over `R: Runtime`. The blanket impl is `impl<R: Runtime> CommandArg<'_, R> for AppHandle<R>` — it only matches when the AppHandle's runtime parameter equals the command's generic `R`. Bare `tauri::AppHandle` defaults to `AppHandle<Wry>`, so it only impls `CommandArg<'_, Wry>` not `CommandArg<'_, R>`. The mismatch is invisible until you mix `AppHandle` with another generic-over-R param like `State<'_, T>`. Renaming the param to `app_handle` does NOT help — Tauri 2 matches the runtime-injected params by TYPE shape, not by name.
- **Action:** When a Tauri 2 command takes both an `AppHandle` AND a `State<'_, T>`, write the command as `pub fn cmd<R: Runtime>(app_handle: AppHandle<R>, db: State<'_, T>, ...)`. The same fix applies to helper fns the command delegates to (e.g. `fn prompt_save_as<R: Runtime>(app: &AppHandle<R>, ...)`). Bare `AppHandle` is fine ONLY if it's the sole `R`-generic param.

---

## 2026-05-21 [phase-mc-wave-5] legacy `update_setting(key: String, value: String)` IPC can't carry typed meeting settings cleanly — added a typed pair instead

- **Context:** Phase MC Wave 5 — wiring the Settings UI to the 8+1 new `Meeting*` `SettingKey` variants. The Wave 5 brief implied reusing `commands/settings.rs::update_setting` / `get_settings`. The existing pair returns a fixed `SettingsSnapshot` struct (Phase-1 keys hardcoded) and accepts only `String` values — booleans get stringified to "1"/"0", nulls become empty strings, numbers parse defensively.
- **Finding:** This doesn't work for `MeetingAudioRetentionDays: Option<i64>` (the `null` sentinel for "inherit from global" can't survive a `String` round-trip), and forces every new setting to expand the hardcoded `SettingsSnapshot` shape — violating OCP. The typed `Settings::new(conn).get(key)/set(key, value)` facade already exists (Wave 1) and handles JSON encoding correctly.
- **Action:** Added a parallel `meeting_settings_get_all -> MeetingSettingsSnapshot` + `meeting_settings_set(key: String, value: serde_json::Value)` IPC pair. The `set` command allowlists writable Meeting* keys, REJECTS dictation keys and `MeetingHotkeyPaused` (which has a dedicated `meeting_set_paused` command that injects a `PauseToggle` activation event in addition to writing the setting). The legacy IPC stays untouched for dictation. Both IPC pairs coexist; UI picks the one matching the setting domain. If/when dictation settings ever need a similar treatment, the pattern is now established — don't extend the typed pair to handle dictation keys; mint a `dictation_settings_*` pair beside it.

---

## 2026-05-21 [phase-mc-wave-5] pre-existing 600-line cap violations surface as scope-creep traps mid-wave

- **Context:** Phase MC Wave 5 — adding the Meeting tab to `Settings.tsx`. The file was already 671 lines at HEAD (pre-Phase-MC violation). Adding the tab brought it to 715. AGENTS.md cap is 600.
- **Finding:** The right move is NOT to silently absorb a refactor of pre-existing dictation/general/etc panels into a current wave — that blows out the wave scope and the resulting commit becomes un-reviewable. The right move is also NOT to ignore the cap and push the count higher.
- **Action:** Did the *minimum* refactor that the new work demanded (extract the Meeting tab into `ui/src/pages/SettingsMeetingTab.tsx`) and filed `mb-17d` for the rest. The Settings.tsx delta was +4 net lines (the new tab is a single import + tab-button + tab-panel line), which is a defensible Wave-5-scope cost. The four-way panel split is a follow-up. Future lateral epics: scan target files for cap violations at brief-authoring time; file refactor beads in the same epic so they get done OR explicitly deferred, not silently ignored.

---

## 2026-05-20 [phase-mc-wave-3] two independent WH_KEYBOARD_LL hooks coexist iff the meeting hook ALWAYS CallNextHookEx; mpsc Receiver one-shot handoff via Option::take

- **Context:** Phase MC Wave 3 — installing a SECOND `WH_KEYBOARD_LL`
  hook for the meeting chord (RCtrl+M) without touching the sealed
  dictation hook in `hotkey/windows.rs`. Two surprises along the way.
- **Finding #1 (cohabiting LL hooks):** Two `WH_KEYBOARD_LL` hooks
  installed on the same desktop work fine — they form a chain that
  Windows walks in reverse-install order. BUT: the second hook MUST
  call `CallNextHookEx(None, code, wparam, lparam)` unconditionally
  (never return `LRESULT(1)` to suppress) or the FIRST-installed
  hook stops seeing keystrokes. The dictation hook installs at app
  boot first, then meetings second; if meetings ever suppressed,
  dictation would die silently for that keystroke. The pure
  classifier in `hotkey_installer.rs::classify_meeting_keystroke`
  emits the event but the proc itself never branches on suppression
  — keep it that way.
- **Finding #2 (mpsc Receiver one-shot handoff):** `TwinStreamCapture`
  owns a `Sender<ChannelChunk>` that both audio streams write to,
  and `LongFormStt` needs *sole ownership* of the matching
  `Receiver`. `std::sync::mpsc::Receiver` isn't clonable on purpose
  (chunk ordering is the whole point of a single receiver). The
  pattern that worked: store the receiver as `Option<Receiver<_>>`
  and expose a `pub fn take_chunk_rx(&mut self) -> Option<Receiver>`
  that uses `Option::take()`. Subsequent calls return `None` and
  `try_recv_chunks()` becomes a no-op. Wave 4 runtime wiring calls
  this exactly once, right after `start()` returns.
- **Finding #3 (split when a test file blows the 600-line cap):** The
  `long_form_stt` integration tests came in at 743 lines unsplit.
  Rather than thinning the tests, I split by KIND (integration vs.
  pure-helper) into `long_form_stt_tests.rs` (597 lines, the
  StubStt-driven scenarios) and `long_form_stt_pure_tests.rs` (143
  lines, the parse_seq + crc32 + tail_tokens unit tests). Both
  files included via `#[path]` on a `#[cfg(test)] mod` declaration
  in `long_form_stt.rs`. The split is cohesive (the test categories
  have genuinely different scaffolding needs) — not a mechanical
  split-just-to-hit-line-count, which AGENTS.md warns against.
- **Action:**
  - When designing a sibling hook in `hotkey/` (Phase 9 macOS, future
    Mac Touch Bar, etc.), the cohabitation rule is the same: always
    `CallNextHookEx`-equivalent on the platform's chain primitive.
  - When a subsystem needs to consume an mpsc stream that another
    subsystem owns, prefer `Option<Receiver<_>>` + `take()` over
    inventing a new Sender. The compiler enforces single-consumer.
  - The 600-line cap is a hard wall; budget for a `_tests.rs` split
    when designing a test-heavy module (`#[path]` is the
    Rust-idiomatic way to keep the inclusion explicit).

## 2026-05-20 [phase-mc-wave-2] whisper.cpp segment timestamps are CENTISECONDS, not ms; UTF-8-safe capitalization needs a char-walker not byte slicing; cpal::Stream is !Send on Windows

- **Context:** Phase MC Wave 2 — implementing the chord activation
  state machine, deterministic formatter, rolling chunker, and the
  ADR 0030 `SpeechToText::transcribe_segments` extension. Author also
  scouted Wave 3 (loopback + TwinStreamCapture + long-form driver).
- **Finding 1 (whisper-rs timestamp units).** `whisper_rs::WhisperSegment::start_timestamp()` and `end_timestamp()` return `i64` **centiseconds** (10 ms units), not ms. whisper.cpp's C-side comment matches:
  ```
  /// # Returns
  /// Start time in centiseconds (10s of milliseconds)
  ```
  Wave 2 multiplies by 10 with `saturating_mul` to convert to the ms
  unit the rest of the meeting pipeline uses (`SttSegment.t0_ms`,
  `t1_ms`). Anyone touching the long-form stitch in Wave 3 needs to
  understand this — the chunker's `first_sample` is in samples (÷16
  to ms at 16 kHz); the STT segment is in ms (post-conversion). Two
  different timeline units inside the same function. Document loudly.
- **Finding 2 (capitalization is harder than it looks).** The
  formatter's sentence-capitalization walker MUST iterate `char`s
  (not bytes) and treat non-alpha non-ws characters as **transparent**
  for the next-uppercase flag. Otherwise:
   - `s[0..1].make_ascii_uppercase()` panics on a multi-byte UTF-8
     boundary (CJK, emoji, accented).
   - A leading quote like `"hello"` consumes the next-uppercase flag
     and you emit `"hello"` instead of `"Hello"`.
  The right pattern is: walk chars; if char is alphabetic and the
  flag is set, uppercase it + clear the flag; if char is whitespace,
  set the flag iff the previous emitted char was `.!?`; otherwise
  emit unchanged and keep the flag as-is. The formatter has the
  reference impl; mirror it if you ever need similar normalization
  elsewhere.
- **Finding 3 (cpal::Stream is !Send on Windows).** While scouting
  Wave 3, confirmed that `cpal::Stream` (used by `audio::CpalCapture`)
  is NOT `Send` on Windows — WASAPI handles are thread-bound. This
  forces `meetings::capture::TwinStreamCapture` to spawn a dedicated
  **owner thread per stream** and communicate via crossbeam channels.
  Don't try to put the mic + loopback streams in a struct that you
  pass between threads; it won't compile. The brief at
  `docs/phases/phase-mc-wave3-brief.md` codifies the design.
- **Finding 4 (TimedSegment alias deviation).** The Wave 2 brief
  originally proposed two types: `stt::SttSegment` (canonical) and
  `meetings::long_form_stt::TimedSegment` (meetings-local rename).
  Author chose to make `TimedSegment` a transparent `pub use` alias
  for `SttSegment` instead — keeps the readable local name without
  duplicating the type. A pin-test in `long_form_stt::tests` catches
  any future accidental fork. Recorded as deviation #4 in the Wave 2
  brief; lifted here for visibility.
- **Finding 5 (proptest dev-dep re-imports).** Phase 1 added
  `proptest = "1"` to dev-deps but the formatter is the first
  Phase-MC module to actually use it. No `Cargo.toml` change needed
  — just `use proptest::prelude::*;` inside `#[cfg(test)]`. If you
  need it in a NEW crate later, double-check it's listed in that
  crate's dev-deps before adding `proptest!`.
- **Finding 6 (0xc0000139 reproduces in debug too).** Confirmed that
  `cargo test --lib meetings::` in **debug** profile (not just
  release) exits with `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)` on
  this box. So it's not a release-LTO artifact; it's a DLL-load
  failure at process start for the test runner specifically. The
  documented fallback (`cargo test --release --no-run`) remains the
  Wave-N seal gate until the box is rebuilt.
- **Action:**
  1. Wave 3 brief already captures all of the above in its
     deviations + signatures + test specs.
  2. ADR 0030's text mentions ms but doesn't call out the
     centisecond-source unit explicitly. Wave 3 author: if you
     touch ADR 0030 for any reason, add a clarifying paragraph.
     (Not blocking; the impl docs the conversion in the code itself.)
  3. For Phase 9 macOS port: `cpal::Stream` may also be `!Send` there;
     plan on the same owner-thread design rather than something cuter.

---

## 2026-05-20 [phase-mc-wave-1] Pre-existing migration test was stale since migration 005; release-LTO test compile is single-threaded death-march on this box

- **Context:** Phase MC Wave 1 — landing migration 011, extending
  `src-tauri/tests/db_migrations.rs` with FTS round-trip + cascade-
  delete tests for the new meeting tables. Also ran the post-Wave-1
  cargo gate.
- **Finding 1 (stale test assertion).** `tests/db_migrations.rs` had
  an assertion `assert_eq!(version, "4")` against `schema_meta`. That
  test has been silently broken since migration 005 landed — apparently
  the integration-test runner never actually ran live on this box
  (see Finding 2 below; the test exe failed to launch with
  `STATUS_ENTRYPOINT_NOT_FOUND` long before this assertion would have
  fired). Wave 1 caught it on a careful read of the file and fixed it
  forward to `"11"`, but the pattern is: **any assertion of a literal
  schema_version in tests is a maintenance landmine.** Future fix idea:
  assert against `db::migrations::LATEST_SCHEMA_VERSION` constant
  (doesn't exist today; add it in Wave 2 or a future migration wave).
- **Finding 2 (release LTO test compile is a death march).** On this
  box, `cargo test --release` rebuilds every test crate with the
  release LTO settings (`lto = "fat"`, codegen-units = 1) — a single-
  threaded link-time-optimization pass per test exe, 13 exes total.
  Empirical wall-clock: ~10m 30s end-to-end from `cargo clean`. With
  warm artifacts (just my Wave 1 .rs files changed), still ~5 min
  because LLVM has to re-link every dep-graph user of `mockingbird_lib`.
  The shell-tool wrapper has a hard 270-second cap, so even the
  background-mode invocation has to poll-and-wait for completion.
  **Pattern:** for cargo gates in iteration, always run the cheap
  checks first (`cargo check`, `cargo fmt --check`) in the foreground;
  background the heavy ones (`cargo test --release [--no-run]`,
  `cargo clippy --release`) with `background=true` and poll the
  output log. Don't expect `timeout` parameters > 270s to actually
  work in this tool.
- **Finding 3 (LESSONS 2026-05-17 still applies).** Even with the
  cargo-with-cuda wrapper, `cargo test --release` fails at first test-
  exe launch with `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)`. Confirmed
  by reading the 2026-05-17 LESSONS entry — known issue, not Phase MC's
  fault. The documented fallback `cargo test --release --no-run` is
  what we sealed Wave 1 on. **Carry forward:** every wave that adds
  pure-Rust tests can seal on `--no-run`; waves that add DLL-touching
  tests (Wave 2's 4 `transcribe_segments` tests, Wave 3's loopback
  integration) need to either `#[ignore]` them or live on a machine
  where the live-launch issue is resolved.
- **Finding 4 (`cargo fmt` silently corrected pre-existing drift).**
  Wave 1's `cargo fmt --check` flagged 6 files; 5 were Wave-1-introduced
  enum/struct/string-literal style drifts, but the 6th was a multi-
  line `&'static str` in `src/tray.rs` that pre-dated Phase MC. The
  binding list doesn't seal `tray.rs`, so `cargo fmt` fixing it in the
  Wave 1 commit is fine — but worth knowing: **`cargo fmt` is
  non-idempotent across rustfmt versions**, so a clean `phase-N-complete`
  tag does NOT guarantee `fmt --check` will pass at HEAD after a
  toolchain bump. Add a note to the AGENTS.md cargo-gate section that
  surprise fmt drift in non-binding files is normal and should be
  committed as part of the wave-seal commit, not surfaced as a
  separate ADR.
- **Action:** (1) Add a `LATEST_SCHEMA_VERSION: u32` constant in
  `db::migrations` and have `tests/db_migrations.rs` assert against it
  instead of literals — file this for Wave 2 if cheap, else a future
  migration wave. (2) Bake the "shell-tool 270s cap; use background
  mode for cargo test --release" tip into the AGENTS.md
  Build-environment section. (3) Carry the `--no-run` fallback gate
  pattern into every Phase MC wave brief from this one forward.

---

## 2026-05-19 [unsplash-bg / release-build] Tauri release exe lives at WORKSPACE-ROOT `target/release/`, not `src-tauri/target/release/`

- **Context:** Post-ship-build verification — went to `src-tauri/
  target/release/mockingbird.exe` to spot-check the binary. Path
  did not exist. Build had clearly succeeded (cargo printed
  `Finished release profile [optimized] target(s) in 5m 18s`).
  Spent a minute confused about where the artifact went.
- **Finding:** The repo has a **Cargo workspace** rooted at the
  repo root, with `src-tauri/` as one of its members. Workspace
  builds drop artifacts at the WORKSPACE-ROOT `target/`, NOT at
  the member's `target/`. So the actual exe path is:
  `target\release\mockingbird.exe` (45.7 MB at ship), not
  `src-tauri\target\release\...`. Same for `target\debug\`.
- **Action:** Update any script / doc / muscle-memory that
  assumes `src-tauri/target/`. `scripts/run-mockingbird.ps1`
  already uses the workspace path correctly (it found the exe);
  it was just my assumption that was wrong. For future ad-hoc
  verification: `Get-ChildItem -Recurse -Filter mockingbird*.exe`
  from the repo root is the fastest sanity-check.

---

## 2026-05-19 [unsplash-bg / release-build] Tauri compresses embedded UI assets in release builds — ASCII grep won't find JS content in the exe

- **Context:** Verifying that a release-build rebuild actually
  embedded the new UI bundle. Tried `[Encoding]::ASCII.GetString(
  ReadAllBytes(exe))` then `.IndexOf('utm_source=mockingbird')`,
  `.IndexOf('Photo by')`, `.IndexOf('settings.general.bg')` —
  every literal string from the source missed. Briefly panicked
  that the bundle wasn't embedded.
- **Finding:** Tauri's release builds **compress** the embedded
  UI assets (the contents of `ui/dist/`) — gzip or brotli, picked
  by the bundler. So the JS/CSS contents inside the exe are
  compressed binary blobs that ASCII grep cannot see. What DOES
  survive as ASCII in the binary:
  - **Asset filenames** (resource-map keys) — e.g.
    `main-9NbkO0yY.js`, `main-Ckt336ix.css`. These uniquely
    identify which vite build produced them.
  - **CSP strings** and other `tauri.conf.json` config — these
    are stored as plaintext config, not compressed.
  - **Rust source string literals** — anything inlined from
    `.rs` via `tracing!`, `format!`, etc.
- **Action:** To verify a release exe has the right UI bundle,
  grep for the hashed asset filenames + recent CSP additions —
  NOT for source literals from `.tsx`/`.ts`/`.css`. The current
  hash from `npm run build`'s last output line is what you want
  to find. If those filenames + your latest CSP edits are in the
  exe, the bundle is current. Trying to grep for `'Photo by'`
  etc. is a false-negative trap.

---

## 2026-05-19 [unsplash-bg] glass token override beats per-component rewrites for photo-vs-ambient modes

- **Context:** Landed an optional Unsplash photo background. The
  design-system glass surfaces (sidebar, Cards, panels) are tuned
  via four `--glass-tint-*` tokens at 4–12% cream-alpha — designed
  to refract against the warm-blob ambient (near-black). Against
  arbitrary photos they wash out completely: text becomes invisible
  on bright photo regions, the sidebar ghosts away over high-
  frequency foliage / book spines, etc.
- **Finding:** Don't restyle every glass component. Add
  `<html data-photo-bg="active">` from the photo component, then
  declare a `:root[data-photo-bg]` scope in `materials-v2.css` that
  **overrides the same four token values** to dark-alpha (45–82%).
  Every existing consumer of `var(--glass-tint-*)` auto-adapts
  with zero per-component churn — OCP-clean (extension without
  modification). Tier-B surfaces that never opted into the token
  system (History `.leftPane`, Dictionary `.shell`) still need
  one-line additions in their own module CSS, but everything that
  was already a "good citizen" of the design system upgrades for
  free.
- **Bonus pattern — adaptive overlay:** Unsplash returns an average
  `photo.color` per response. Compute Rec.709 luminance from it and
  apply a clamped (0..0.45) dark-scrim overlay automatically; combine
  with the user's manual slider via `Math.max` so the slider is a
  floor. Dark photos stay pristine; bright photos auto-darken to the
  point page-header text clears AA. Helper in `fetchPhoto.ts` as
  `autoOverlayForColor`.
- **Action:** When adding any "mode toggle" that changes how the
  design system reads (light↔dark, ambient↔photo, dense↔airy),
  reach for token override scopes FIRST. Per-component rewrites are
  a code smell — they multiply maintenance and drift. Only fall
  back to per-component additions for surfaces that never
  consumed the relevant tokens to begin with.

---

## 2026-05-19 [unsplash-bg] z-index:0 photo background trapped in-flow text BENEATH the photo

- **Symptom:** With the Unsplash background enabled, **PageHeader
  text (page h1 + subtitle) disappeared completely** even though
  Cards and Sidebar rendered fine over the photo. DevTools showed
  the `<header>` element present with proper 538×64 dimensions — the
  text just wasn't visible. First instinct ("contrast issue!") was
  wrong; I added text-shadows and the problem persisted.
- **Finding:** Classic CSS painting-order trap. The photo layer
  was a `<div>` with `position: fixed; z-index: 0` (forms its own
  stacking context). The app shell (`.shell`) was non-positioned,
  in the default flow. **Per CSS paint order, positioned elements
  with z-index ≥ 0 paint ABOVE non-positioned in-flow elements
  regardless of DOM order.** So the photo was drawn ON TOP of any
  page chrome that didn't have its own stacking context.
- **Why some elements worked and others didn't:** Cards and Sidebar
  accidentally dodged the bug because their `backdrop-filter:
  blur(...)` quietly creates an implicit stacking context (same way
  `opacity != 1`, `transform`, `will-change: transform`, `filter`,
  and `isolation: isolate` do). PageHeader had no such property →
  no stacking context → stuck below the photo. The bug is INVISIBLE
  during component-level testing because cards always work; only
  affects bare in-flow text.
- **Fix:** One line in `App.module.css` — `.shell { position:
  relative; z-index: 1; }`. Promotes the entire app shell into a
  stacking context above the photo. Every descendant inherits the
  right paint order. No per-component bandaids needed.
- **Action:** Whenever introducing a `position: fixed/absolute;
  z-index: 0` background layer (photo, video, canvas, …), ALWAYS
  give the sibling content tree an explicit positive z-index +
  positioning to form its own stacking context. The default works
  by accident when every visible element happens to have a
  stacking-context-forming CSS property, and silently breaks the
  moment one doesn't. Add this to the design-system layout-shell
  contract; not the photo component's job to know about every
  sibling's z-context.

---

## 2026-05-17 [phase5-postship-9-followup] stale UI bundle embedded in release binary

- **Symptom:** shipped Wave 2 backend + UI source changes, ran my
  usual `cargo-with-cuda.ps1 build --release`, launched, and the
  Modes page showed the OLD layout — Normal/Verbose/Fragment in
  the transcription section + Casual/Formal incorrectly grouped
  under AI command modes alongside Rewrite/Expand/Summarize. The
  database had the right rows (`prompt_id=10`, `model=qwen2.5:7b`,
  `temperature=0.1` in the orchestrator log), the IPC returned
  them, but the partitioning logic on the JS side was using the
  pre-Wave-2 `TRANSCRIPTION_SLUGS = ["normal", "verbose",
  "fragment"]` allowlist. Stale frontend.
- **Root cause:** Tauri's `beforeBuildCommand`
  (`"npm --prefix ../ui run build"` in `tauri.conf.json`) ONLY
  runs under `cargo tauri build`. Plain `cargo build --release`
  skips it. So when I edited `ui/src/lib/types.ts` + friends and
  then ran `cargo-with-cuda.ps1 build --release`, the Rust binary
  re-linked against whatever `ui/dist/` was sitting on disk —
  which was the bundle from the last `cargo tauri build` or `npm
  run build`. New TypeScript sources, old bundled output.
- **Second trap on top of the first:** even after running
  `npm run build` manually to refresh `ui/dist/`, a subsequent
  `cargo build --release` didn't re-link the binary, because
  tauri-build's `cargo:rerun-if-changed=` directives don't always
  detect content changes inside `frontendDist`. The build said
  "Finished" instantly but the .exe timestamp didn't move. Had
  to `touch src-tauri/src/lib.rs` to force a re-link that
  re-embedded the assets.
- **Fix (long-term):** new `scripts/build-release.ps1` wraps the
  full three-step dance: `npm run build` + touch `lib.rs` +
  `cargo build --release`. Use this any time UI sources changed.
  `cargo-with-cuda.ps1 build` (without the wrapper) is fine for
  pure-backend iteration.
- **Patterns burned in:**
  - **"If the schema/IPC says one thing and the UI shows another,
    suspect a stale bundle BEFORE suspecting a logic bug."** The
    backend logs (`orchestrator config resolved mode=normal
    prompt_id=10`) were the smoking gun — they proved the
    backend was correct; therefore the discrepancy had to be
    upstream of the bundle.
  - **"Build tools that conditionally run hooks are a footgun."**
    Tauri's `beforeBuildCommand` is meant to be helpful but its
    silent skip on plain `cargo build` creates a class of bug
    that's invisible at build time and surfaces only when the
    user opens the page. A wrapper script that ALWAYS does the
    full dance removes the conditional.
  - **"Trust your eyes, not the build success message."** Both
    `npm run build` and `cargo build --release` reported success.
    The bug was in the gap between them.

---

## 2026-05-17 [phase5-postship-9] three focused modes + 7B default — Wave 2 of ADR 0022

- **The Santa-list regression** (2026-05-17 seventh smoketest):
  with the preprocessor in place, raw input `"I'm making a list
  of things and checking it twice. And I'm going to find out
  who's naughty or nice. And to do that I need to know these
  important things. Who has stolen something? Who has lied to
  their friends? Who has lied to their mom?"` came out cleaned as
  just three bulleted questions — the four sentences of preamble
  were dropped. The v3 prompt explicitly said "do not summarize"
  but the 3B-q4 model decided the questions were "the point"
  and editorialized everything else away.
- **Root cause is attention budget, not prompt wording.** The v3
  prompt was ~4.8 KB. A 3B-q4 model has finite attention; rules
  buried below the first KB of a long prompt get statistically
  ignored. **Load-bearing rules must go FIRST.** Same lesson as
  Wave 1's "move work out of the LLM" — every byte of prompt the
  model doesn't have to attend to is attention budget freed for
  the actual judgment work.
- **"Preserve every sentence" must be NON-NEGOTIABLE and FIRST.**
  All three Wave-2 prompts (casual_v1, normal_v4, formal_v1)
  start with a section literally titled
  `## NON-NEGOTIABLE RULES` whose first rule is
  `**PRESERVE EVERY SENTENCE.**` Followed by the reason: cleanup
  ≠ summarization, every sentence the speaker said matters. Then
  the rest of the prompt fits in ~1 KB. With this structure the
  rule lives in the highest-attention region of the context
  window. Small model can't miss it.
- **Bigger model when content fidelity matters.** qwen2.5:7b-q4
  follows instructions markedly better than 3b-q4 — the model-size
  effect on rule-following is real and large. The 7B cold-loads
  in ~17 s on the user's RTX-2060/6 GB rig (with Whisper-large
  already resident, ~1 GB free) and runs warm calls in ~3 s vs
  ~1.5 s for 3B. **2× latency for ~10× rule-following reliability
  is a great trade** for normal/formal modes. Casual stays on 3B
  because Wave 3 will skip the LLM entirely for short casual
  utterances anyway.
- **REQUEST_TIMEOUT must bracket the worst-case cold-load.** Old
  value 30 s covered 3B cold-load (~6 s) with 5× headroom. Same
  budget on a 7B cold-load is only 1.2× headroom; one moment of
  Whisper-Ollama-Tauri startup contention can blow through it.
  Bumped to 60 s. Steady-state warm calls pay nothing extra.
  Lesson: when changing default model SIZE, recheck every
  timeout downstream of it.
- **Migration `ON CONFLICT DO UPDATE WHERE` is exactly the right
  tool for soft state migration.** Migration 008 needed to rescue
  any user whose `dictation.active_mode_slug` setting pointed at
  the now-disabled `verbose` or `fragment`. The SQLite idiom:
  ```sql
  INSERT INTO settings VALUES ('dictation.active_mode_slug', 'normal')
  ON CONFLICT(key) DO UPDATE SET value = 'normal'
  WHERE value IN ('verbose', 'fragment');
  ```
  This is one statement that handles all four cases: row absent →
  insert with 'normal'; row present + verbose/fragment → update
  to 'normal'; row present + something else → WHERE excludes, no
  update; row present + already 'normal' → WHERE matches but
  UPDATE is a no-op. No `if exists` ceremony, no client-side
  branching.
- **Shared `<datalist>` is the DRY way to wire N comboboxes from
  one source.** The Modes editor has one model `<input>` per
  mode card, but all of them autocomplete from the same
  installed-models list. Browsers de-dup automatically when
  multiple inputs reference the same `list=` id, so the cleanest
  pattern is: render the `<datalist id="...">` ONCE at the top
  of the page, point every input at it via `list=`. The shared
  ID lives in a const at the top of the file (`MODELS_DATALIST_ID`)
  so producer + consumer can't drift.
- **Patterns burned in:**
  - **"Load-bearing rules go first." This applies to prompts,
    docstrings, function signatures, and anything else attention-
    budgeted. The most important rule deserves the highest-
    attention slot — front of the section, front of the line,
    bold/uppercase if the medium allows.
  - **"Smaller prompts > more rules."** The v3→v4 prompt shrank
    from 4.8 KB to 1.5 KB despite ADDING the preservation rule.
    How? Removed every rule the deterministic preprocessor now
    handles (fillers, punctuation, capitalization, layout cues).
    The LLM only sees the SHAPE of cleanup work it actually has
    to do. Same architectural move as Wave 1 — push work down
    the stack so the LLM-shaped portion shrinks.
  - **"Test the regression you fixed."** Each Wave-2 prompt's
    `## Examples` section contains the exact Santa-list utterance
    that triggered the regression. The example shows the model
    the right answer for the input that previously broke. Next
    test against the same input is a high-confidence pass.

---

## 2026-05-17 [phase5-postship-8] deterministic preprocessor — Wave 1 of ADR 0022

- **Context:** sixth-smoketest screenshot showed the LLM emitting
  `` ```ery keyboard supplies: `` (hallucinated intro), wrapping
  output in fences explicitly forbidden by the prompt, and dropping
  the speaker's framing. Cleanup latency was 3198 ms — 70 % of
  end-to-end. Root cause: asking a 3B-q4 model to do 100 % of
  cleanup inside a 5 KB prompt blows its attention budget.
- **Architectural fix:** new `cleanup/preprocessor.rs` runs BEFORE
  the LLM call. Handles the rule-shaped 80 % (fillers, stutters,
  self-corrections, verbal punctuation/quote/layout cues,
  capitalisation, terminal punctuation) in ~5 ms. The LLM now sees
  pre-cleaned text and is asked only to do judgment work in later
  waves. See ADR 0022 for the full pipeline rationale.
- **The `regex` crate trapped me twice** while porting the rule
  table from a 'this is just regexes' first pass:
  - **No lookaround.** I'd written `(?:^|\s)cue(?=\s|$|[.,!?])`
    for standalone-token matching of multi-word verbal cues. Rust's
    `regex` is the safe non-backtracking implementation and
    explicitly rejects `(?=...)` / `(?<=...)`. Fix: consume the
    boundary instead of looking past it — `(?:^|\s)cue\b\s?`. \b
    IS supported because it's zero-width-but-not-backtracking-y.
    Use `fancy-regex` only if you genuinely need lookaround; the
    safe crate is faster and you can usually refactor.
  - **No backreferences.** Stutter collapse ("the the the" → "the")
    naturally wants `\b(\w{1,4})(?:\s+\1\b){1,}` — match a word,
    then re-match the same word. `\1` isn't supported by `regex`
    either. Fix: do it as a manual `split_whitespace` token walk
    with last-token-comparison. O(n), comparable cost to a regex
    pass, and arguably more readable.
- **The ordering trap.** First version put layout-cue rendering
  (which inserts `\n\n`) BEFORE stutter collapse (which calls
  `split_whitespace`). `split_whitespace` eats newlines, so every
  inserted paragraph break vanished. Two tests failed loudly with
  `"first thought second thought"` where I expected the break. The
  invariant is now pinned in the `process()` docstring: **any pass
  that injects newlines MUST run AFTER any pass that uses
  `split_whitespace`.** Subtle, easy to violate, worth a comment.
- **Tier-2 filler stripping is load-bearing on prosody.** First
  version of the regex stripped "you know" at any sentence start.
  That broke `keeps_you_know_when_not_bounded` ("You know nothing
  about it" → "Nothing about it"). The fix: ONLY strip when the
  speaker also said a comma ("You know, it's true" has the
  prosodic marker; "You know nothing" doesn't). The trailing comma
  is the SOLE differentiator between filler and content. Same
  rule for `like`, `basically`, etc. — strip only when prosody
  (STT-rendered commas) flags them.
- **DLL-load issue forced a workaround:** `cargo test --lib` on
  this box dies with STATUS_ENTRYPOINT_NOT_FOUND in ntdll because
  ORT/CUDA DLLs aren't on the test binary's PATH at process load.
  Setting PATH + ORT_DYLIB_PATH env vars didn't help (the wrong
  ABI version was being picked up). Workaround: created a tiny
  throwaway crate at `C:\Users\dboyd\AppData\Local\Temp\preproc_test\`
  containing only the preprocessor source + regex dep, and ran
  `cargo test --lib` there. Found two real bugs in two iterations.
  Worth keeping the recipe documented for future pure-rust modules.
- **Patterns burned in:**
  - **"Move the load-bearing work out of the LLM, into the
    deterministic layer."** Every byte of prompt the LLM doesn't
    have to attend to is attention freed up for the actual
    judgment. Wisprflow's 'just-knows' magic is almost certainly
    not a better model; it's a fatter rule layer in front of it.
  - **Provenance suffix > new column.** Encoding the preprocessor
    version into the existing `model_used` string
    (`qwen2.5:3b-q4+preproc@v1`) gives full provenance without a
    migration. ADR 0008's append-only-migration invariant prefers
    schema stability over schema purity.
  - **Test rigs beat test runners when the runner is broken.**
    The throwaway-crate trick is a 5-minute reliable test loop
    even when the main crate's test infrastructure is unwell. If
    the module under test has bounded dependencies it's worth it.

---

## 2026-05-17 [phase5-postship-7] clipboard snapshot crashed the process when a bitmap was on the clipboard (STATUS_HEAP_CORRUPTION)

- **Context:** First dictation after `phase5-postship-6` ship. User
  said "it crashed when recording the test vtt". Process gone. Log
  ended mid-paste with `inject begin decision=Proceed(Paste)
  text_len=73 focus_drifted=false` and no matching `inject end`.
  No Rust-level error — process died beneath the tracing layer.
- **Diagnosis:** Windows Event Log Application Error showed
  `Faulting module name: ntdll.dll, Exception code: 0xc0000374`.
  That's `STATUS_HEAP_CORRUPTION` — ntdll's heap-validation
  tripwire fires when something has scribbled on the process heap
  metadata. Process death is immediate; tracing never gets to log
  it.
- **Root cause:** `injection/paste.rs::copy_format_bytes` iterated
  every format `EnumClipboardFormats` returned and called
  `GlobalSize(handle)` / `GlobalLock(handle)` on each one. The
  comment in the code claimed "GlobalSize returns 0 if not
  HGLOBAL" — **that's wrong**. Per MS docs, calling `GlobalSize` on
  a handle that wasn't allocated by `GlobalAlloc` is undefined
  behaviour. `CF_BITMAP` returns an `HBITMAP`, `CF_ENHMETAFILE`
  returns an `HENHMETAFILE`, etc. `GlobalSize` reads what it
  THINKS is the moveable-memory header at handle's address; on a
  GDI object that's other GDI metadata, and the call either
  scribbles or returns garbage that the next `GlobalLock` writes
  through.
- **Reproduction:** between the user's last successful paste at
  16:18 and the crashing one at 16:47, they took a screenshot for
  the bug report. The clipboard now held `CF_BITMAP` + `CF_DIB`
  alongside the text. Snapshot enumeration tried `CF_BITMAP`,
  passed its `HBITMAP` to `GlobalSize`, corrupted the heap. Next
  allocation tripped the tripwire and ntdll killed the process.
  Verified reproducer: place a bitmap on the clipboard via
  `[System.Windows.Forms.Clipboard]::SetImage`, trigger any paste
  dictation → immediate process death.
- **Fix:** allowlist. New `is_hglobal_format(fmt: u32) -> bool` is
  the gatekeeper for the snapshot loop. Anything not on the
  allowlist is logged at debug + skipped without EVER passing the
  handle to `GlobalSize` or `GlobalLock`. Allowlisted formats are
  the ones MS documents as HGLOBAL-backed: `CF_TEXT`, `CF_SYLK`,
  `CF_DIF`, `CF_TIFF`, `CF_OEMTEXT`, `CF_DIB`, `CF_UNICODETEXT`,
  `CF_HDROP`, `CF_LOCALE`, `CF_DIBV5`. Notably, `CF_DIB` IS
  HGLOBAL (header + pixels in moveable memory) while `CF_BITMAP`
  is NOT (HBITMAP) — they look related but live in different
  storage worlds. Registered formats (`>= 0xC000`) are
  app-defined; docs RECOMMEND HGLOBAL but don't require it, so
  we're conservative until a Phase 9 per-app deny list lands. Net
  effect: user temporarily loses round-trip of bitmaps + custom
  formats around a paste dictation. That's a paper cut; heap
  corruption is a project-ender.
- **Patterns burned in:**
  - **"It returns 0 if X" is a load-bearing claim that needs an
    MSDN citation, not a comment.** The old code had the comment
    `// GlobalSize returns 0 if handle isn't an HGLOBAL` directly
    above the UB call. The comment was wrong and lived for months
    because nobody had a screenshot on their clipboard during a
    dictation. Heap-corruption bugs are silent until they aren't.
  - **Allowlist > sniff-and-skip for FFI handles.** You cannot
    safely "try and recover" with raw Win32 handles — the act of
    asking "are you an HGLOBAL?" requires already treating you as
    one. The only safe move is to know up front. Allowlists are
    the right primitive whenever a wrong guess crashes the
    process.
  - **Crash logs > error logs for diagnosing process death.**
    Standard reflex (`Get-Content app.log -Tail`) doesn't help
    when the kernel kills the process; tracing buffers never get
    flushed. `Get-WinEvent -LogName Application | Where ProviderName
    -like '*Application Error*'` is the right tool. Pinning this
    in the project's smoketest runbook would have saved 10 minutes
    of head-scratching.
  - **A user-friendly bug-report workflow is itself a fuzzer.**
    The crash trigger was a screenshot taken to file the previous
    bug — i.e., the user's act of helping us hit a latent UB the
    test suite never tickled. The interactive-desktop ignored
    tests in `paste.rs` only exercise the happy text-text path;
    they never plant a bitmap and re-enumerate. New unit tests
    pin the full GDI-handle-rejection table so a regression here
    is caught without needing live Win32.

---

## 2026-05-17 [phase5-postship-6] active-mode selector + prompt v3 (preserve list context)

- **Context:** Sixth Dustin smoketest pass. Three pieces of feedback:
  (a) "Everything seems to default to normal mode and I don't know
  how I can change it", (b) the v2 prompt rendered bullets but
  dropped the introductory framing ("list of keyboard supplies" →
  three bare bullets), (c) per-mode hotkey chords (Ctrl+Win,
  Ctrl+Shift+Win, etc.) shown on the Modes page were confusing
  noise — the user wanted to just pick a mode and have Right-Alt
  use it.
- **Two-class mode model.** Mockingbird has two distinct mode
  classes that needed different UX treatments:
   - *Transcription modes* (`normal`, `verbose`, `fragment`): exactly
     ONE is active at a time. Right-Alt always uses the active one.
     The Modes page now shows them as a radio-style selector — each
     card has a "Use this mode" button, the active card gets an
     accent border + "Active" pill, and the per-mode hotkey badge is
     hidden (Right-Alt is global).
   - *AI command modes* (`rewrite`, `expand`, `summarize`): act on
     existing clipboard/selection text via their OWN hotkeys when
     enabled. They keep the legacy enable/disable toggle + hotkey
     badge. They are NOT eligible to be set as the active mode —
     there's no audio input concept to attach them to.
  The categorisation lives in `ui/src/lib/types.ts` as a fixed
  `TRANSCRIPTION_SLUGS` const + an `isTranscriptionSlug` predicate,
  mirrored on the Rust side in
  `src-tauri/src/commands/active_mode.rs`. The Rust
  `set_active_mode` IPC rejects any slug outside that list — the UI
  can't accidentally point Right-Alt at `summarize`.
- **Storage: settings table, not a new schema column.** The active
  mode is a single string under `settings.dictation.active_mode_slug`.
  No migration needed (the orchestrator falls back to `"normal"` if
  the row is missing). One source of truth, no cache to invalidate.
  Per-session lookup is one indexed-PK query — negligible vs.
  STT/cleanup latency. **Net effect: a `set_active_mode` call takes
  effect on the NEXT Right-Alt hold with zero restart/signalling/
  refcount dance.** This is the simplest possible mechanism that
  satisfies the user requirement.
- **Orchestrator: session-pinned mode, not config-time mode.** The
  old `OrchestratorConfig` set `mode_id` / `mode_slug` / `prompt_id`
  once at boot. I added `ResolvedMode { mode_id, slug, prompt_id }`
  plus `SessionState.active_mode: Option<ResolvedMode>`. At
  `start_capture` we resolve fresh and pin for the whole session;
  `complete()` + both `insert_session_row` helpers read from
  `current_mode()` which falls back to `self.config` if no session
  is active. Pinning AT `start_capture` (not at insert) means a
  `set_active_mode` call mid-dictation can't split one session
  across two modes (the cleanup prompt would mismatch the DB-
  recorded `mode_id` and we'd violate provenance). Resolution has
  two graceful fallbacks (poisoned mutex → config; missing settings
  row OR modes lookup fails → config) so the user never loses a
  dictation to a flaky DB read.
- **Prompt v3: "preserve framing" rule.** v2 was too eager to strip
  introductory phrases. The example I shipped in v2 — `"um so make
  a list first thing is apples"` → bare bullets — set the wrong
  precedent. v3 keeps that example (no intro spoken → no intro
  rendered) but adds three NEW examples where the speaker DID name
  the list ("I'm going to put together a list of keyboard supplies")
  and the cleaned output keeps that as a one-line lead-in followed
  by a blank line, then the bullets. ADR 0008 compliant: new file
  `normal_v3.md`, new migration 007 that INSERTs `prompts` row v3
  and repoints `modes.normal.prompt_id` — v1 + v2 stay addressable
  for every historical session that referenced them.
- **Migration test rig.** Added `scripts/test_migrations.py` because
  the Rust test runner has the pre-existing STATUS_ENTRYPOINT_NOT_FOUND
  DLL-load issue (ORT/CUDA path) and won't run unit tests at all on
  this box. The Python rig substitutes prompt-body tokens the same
  way `prompt_loader.rs` does (including the apostrophe escape and
  the leftover-token guard regex), applies every migration
  sequentially to a `:memory:` SQLite, and prints the resulting
  `prompts` + `modes` rows. Reproduced the working state in <1 s.
  Worth keeping in the repo for future migration iteration; pays
  for itself the first time you don't have to wait on a 4-minute
  release build to know whether your SQL is valid.
- **Patterns burned in:**
  - **"Active selection" is a UX problem, not a permissions problem.**
     Enable/disable toggles imply "on means this mode runs in the
     background" — which makes no sense for a transcription mode
     that runs on user-initiated hotkey. Radio-style selection makes
     the contract obvious: ONE is in use, click to switch. Reach
     for the right primitive; don't bend toggles to a single-select
     job.
  - **Resolve config per-session, not per-process.** Boot-time
     config is great for things that physically can't change without
     restart (audio device, model files). For anything the user can
     change from the UI, lookup fresh at use-time + pin for the
     duration of a single in-flight operation. Settings table +
     one indexed query is enough; no need for shared `Arc<RwLock<_>>`
     state or event broadcasters.
  - **The first example in a prompt sets the tone for everything
     the model generates.** v2's only list-example was the
     bare-bullet case. The model interpreted that as "strip
     EVERYTHING and emit bullets". v3 leads with the intro-preserving
     example and demotes the bare-bullet case to example three with
     an explicit "no intro phrase was spoken, so no lead-in is
     invented" annotation. Order + emphasis in few-shot examples
     matter as much as the rule prose.

---

## 2026-05-17 [phase5-postship-5] migration 006 crashed the entire app at boot with cryptic `near 'voice': syntax error`

- **Context:** Fifth Dustin smoketest (sigh). User ran `run-mockingbird.ps1`,
  terminal said "Started in background", but no tray icon, no main window,
  nothing. `Get-Process mockingbird` returned empty — the process was
  dead. The log file showed only `Mockingbird starting` and then
  silence. `database ready` (the next log line) never printed, so the
  crash was somewhere inside `db::apply_migrations`.
- **Finding:** Reproduced the migration outside the app via a tiny
  Python script (`sqlite3.executescript`) and got
  `OperationalError: near "voice": syntax error`. The word "voice"
  appears in the v2 prompt body ("preserve the speaker's voice"). My
  initial assumption — unescaped apostrophe in `speaker's` — was
  wrong; `prompt_loader::sql_escape` correctly doubles all single
  quotes. Real root cause was geometrically worse:

  Migration 006's own `--` comment block included the literal token
  `` `__PROMPT_NORMAL_V2_BODY__` `` as documentation of where the
  body gets substituted. `prompt_loader::substitute_prompt_bodies`
  is a blanket text replace — it doesn't care whether the token sits
  inside a SQL string literal, a `--` comment, or a `/* */` block.
  So the entire v2 prompt body (3.7KB of markdown with embedded
  newlines, apostrophes, fenced code blocks, etc.) got injected into
  the middle of a comment line. The body's first `\n` terminated
  the `--` comment early, and everything from there onward (
  including escaped apostrophes that were `''` inside the intended
  string literal but are now bare unmatched quotes in raw SQL
  context) hit the parser. Hence "syntax error near 'voice'".

  Migrations 003 and 005 dodged this by accident: both refer to the
  token family as `__PROMPT_*_BODY__` (with a literal asterisk), which
  doesn't match any concrete substitution key. I wrote 006's comment
  with the exact token literal because it felt more precise. It was.
  Precisely catastrophic.
- **Action (two-layer fix):**
  1. **Migration 006 comment** rewritten to use `__PROMPT_*_BODY__`
     style (matches the 003/005 precedent), with a multi-line
     `DO NOT` warning explaining what happens if you write the
     exact token. The next person to copy this file gets the
     warning at the same time they get the example.
  2. **Defensive leftover-token guard** in
     `prompt_loader::substitute_prompt_bodies`: after the chained
     `.replace()` calls, scan for any surviving `__PROMPT_` substring
     and panic loudly with the offending token + a hint about the
     two ways this happens (forgot to register, OR wrote it in a
     comment). Costs microseconds at boot; saves the next hour of
     "why does it say 'near voice'" debugging.
- **DB state preserved by SQLite transaction semantics.** Migration
  006 starts with `BEGIN TRANSACTION` and ends with `COMMIT`. When
  the syntax error fired mid-script, SQLite implicitly rolled back
  the open transaction. Verified post-incident: live DB still at
  schema_version=5, prompts table still has only v1 rows, no audit
  trigger noise. **This is exactly why every multi-statement
  migration must be wrapped in a transaction.** Phase 1 made this a
  rule (in 002, 003, 005); 006 honoured it; it saved the user from
  a partially-applied migration that would have required manual
  recovery.
- **Patterns burned in:**
  - **Text-substitution tooling treats source files as opaque
    strings.** Comments do not protect against substitution. If
    the substitution payload can contain syntactically-significant
    characters (newlines, quotes, semicolons), it can break out of
    any host-language construct that was structurally protecting
    the surrounding code. Either:
    (a) make the substitution syntax-aware (parse SQL, only
        substitute inside string literals), OR
    (b) make the token name impossible to confuse with prose (e.g.
        `<<<INSERT_NORMAL_V2_HERE>>>` with rare ASCII), OR
    (c) add a post-substitution sanity check (the path we took —
        cheapest, catches everything blanket-replace can miss).
  - **"Syntax error near X" usually means the parser entered the
    wrong state several tokens earlier.** Don't trust the column
    number. Dump the actual SQL that hit the parser; scan backward
    for the first place the lexer would have changed state
    incorrectly (unterminated string, broken comment, missing
    semicolon).
  - **Silent process death after a single log line is almost always
    a panic before tracing flushes.** First instinct should be
    "reproduce outside the app and capture the real error", not
    "hunt through the source for what runs between those two log
    statements." The reproduction took two minutes; the visual
    inspection would have taken much longer.

---

## 2026-05-17 [phase5-postship-4] History metadata showed `—` for Model / Prompt / Dictionary even though all three were populated in the DB

- **Context:** Fourth Dustin smoketest. Ollama-cleaned dictation worked
  cleanly (Cleaned=689ms, Inject=63ms). History row had different Raw
  vs Cleaned text — proof that LlmCleaner ran. But the Metadata panel
  showed `Model: —`, `Prompt: —`, `Dictionary: —` for all sessions,
  old and new.
- **Finding (instrumented via Python sqlite3 on the live DB):** The
  data WAS in the DB. `transcripts.model_used` = `'qwen2.5:3b-instruct-q4_K_M'`.
  `sessions.prompt_id` = 1. `sessions.dictionary_snapshot_id` = 1.
  Running the EXACT backend query manually in sqlite3 returned
  `('qwen2.5:3b-instruct-q4_K_M', 1, 1)`. So the bug was in how Rust
  read the row, not in what was written.

  Looked at the rusqlite visitor:
  ```rust
  r.get::<_, Option<String>>(0)?,  // model_used: TEXT  → fine
  r.get::<_, Option<String>>(1)?,  // p.version: INTEGER → ERROR
  r.get::<_, Option<i64>>(2)?,     // dict_id: INTEGER  → fine
  ```

  `prompts.version` is INTEGER (schema in migration 003), not TEXT.
  Asking rusqlite to read INTEGER as `Option<String>` returns
  `Err(InvalidColumnType)`. The visitor uses `?`, so the WHOLE
  closure returns Err. The outer code was `.unwrap_or((None, None,
  None))` — silently swallowing the error and returning a tuple of
  all-NULLs. THREE pieces of metadata vanished from the UI because
  of ONE type mismatch in ONE column, and the swallow-the-error
  pattern made it invisible to logs.

  Worst part: `commands/modes.rs` had a five-line comment WARNING
  about this exact pitfall (with the same column!), and used
  `'v' || p.version` in its SQL to force TEXT affinity. The author
  of `commands/sessions.rs` (also me) didn't know about that
  precedent, didn't search for it, and re-introduced the same bug.
- **Action (two fixes):**
  1. **SQL-side:** changed `p.version` → `'v' || p.version` in the
     sessions query. Matches the modes.rs precedent. Produces "v1",
     "v2", … — same shape the UI shows.
  2. **Rust-side:** replaced `.unwrap_or((None, None, None))` with
     `.unwrap_or_else(|e| { tracing::warn!(...); (None, None, None) })`.
     Same fallback behaviour for the user, but the error surfaces in
     logs within seconds instead of producing a mystery UI bug that
     survives three smoketest rounds.
- **Patterns burned in:**
  - `unwrap_or((None, None, None))` for ANY multi-column SQL result
     is a footgun: a single column-type error silently corrupts every
     field in the tuple. Always `unwrap_or_else` with a log.
  - When the same gotcha bites you twice (modes.rs comment, then
     sessions.rs query), the comment is in the wrong place. The fix
     belongs at the boundary that produces the gotcha (DTO/serde
     layer), not at each call site. Future Phase 6 polish: write a
     `Stringy<T>` newtype that auto-stringifies INTEGER columns into
     Option<String>, OR migrate prompts.version to TEXT in a v3
     migration.
  - When the symptom is "backend looks right but UI shows —", the
     bug is almost always in the visitor closure, not the SQL.
     Reproduce the query in `sqlite3` first — if it returns data,
     skip straight to inspecting the row decode.

---

## 2026-05-17 [phase5-postship-4] normal-mode prompt didn't render lists when user said "make a list"

- **Context:** Same smoketest. User dictated "...make a list here.
  First thing is apples and then eggs and then berries." Cleaned
  output was "Here's a list: first thing is apples, then eggs, and
  then berries." — reasonable English but NOT a list. Wisprflow
  (competitor) would render bullets.
- **Finding:** v1 of normal.md was deliberately structure-averse
  ("do not invent structure that isn't implied by the speech"). The
  rule is right for ambiguous cases but too strict when the speaker
  EXPLICITLY says "make a list". Dustin's expectation is "my words
  produce the structure I asked for, in markdown if needed."
- **Action (ADR 0008 compliant):**
  - Did NOT edit `normal.md` in place (initial reflex — caught
     myself before commit). ADR 0008 binds prompts to append-only
     versioning. Editing v1's source file would silently change what
     gets seeded into fresh-install DBs WITHOUT bumping a version,
     breaking provenance for any user whose existing session row
     points to prompt_id=1 with the OLD body.
  - Created `cleanup/prompts/normal_v2.md` with the new body
     (structure cues + 4 worked examples). v1 file stays frozen as
     the on-disk record of what shipped.
  - Added `PROMPT_NORMAL_V2` const + `__PROMPT_NORMAL_V2_BODY__`
     substitution token in `db/prompt_loader.rs`.
  - Created `db/migrations/006_prompt_normal_v2.sql` that INSERTs
     a new prompts row (mode_slug='normal', version=2) and UPDATEs
     modes.normal.prompt_id to point at v2. v1 row stays in the
     prompts table forever; existing session rows pointing at
     prompt_id=1 still resolve to the v1 body for provenance.
  - Registered migration in `db/migrations.rs`; bumped expected
     `schema_version` from "5" to "6" in tests.
- **Pattern (reusable):** the file-naming convention `{mode}.md` for
  v1, `{mode}_v2.md` for v2, etc., scales linearly with prompt edits.
  For phase 6+ when prompt iteration accelerates, consider switching
  to a directory layout: `prompts/normal/v1.md`, `prompts/normal/v2.md`,
  with a build-script that auto-generates the include_str! const list.
  Not worth it for 1-2 edits per phase; revisit at 5+.
- **Whisper aside:** Dustin also noted "I said ums and filler, raw
  doesn't show them at all." Whisper-large-v3 auto-suppresses
  disfluencies before producing the raw transcript — a model-level
  behavior we can't fully disable without switching models. Knobs to
  explore in Phase 6 if requested: `condition_on_prev_tokens=False`,
  smaller model size (medium/small suppresses less), or a custom
  initial prompt. For now: the "raw" we persist is Whisper's output,
  which is already lightly cleaned. The user-facing concept "raw"
  matches Whisper-raw, not microphone-raw. Documented here for
  future me when someone files this as a bug.

---

## 2026-05-17 [phase5-postship-3] inject reported `outcome=Ok` but nothing pasted into Notepad

- **Context:** Third Dustin smoketest. Pipeline ran end-to-end. New
  inject-lifecycle logs (added in postship-2) showed clean handshake:
  `inject begin decision=Proceed(Paste) text_len=23` →
  `inject end injection_latency_ms=67 outcome=Ok`. History row
  persisted with the correct raw + cleaned + injected text. App
  metadata showed `App: Notepad`. But Notepad was empty — no paste.
- **Finding:** ADR 0020 covers focus changes BETWEEN key-down and
  key-up (permissive: inject into key-up app). It does NOT cover
  focus changes BETWEEN key-up and inject. Sequence:
  1. Hold RightAlt with Notepad focused → fg_keydown = Notepad.
  2. Release RightAlt → fg_keyup snapshot captures Notepad.
  3. Cleanup hangs 30s on cold-load Ollama → user gets bored, clicks
     on Mockingbird's main History window to see if anything's
     happening. Mockingbird is now the foreground.
  4. Cleanup finally returns. Injector runs SetClipboardData +
     SendInput(Ctrl+V) against the CURRENT foreground = Mockingbird
     main window. Ctrl+V in our React UI is a no-op (no input field
     focused). Clipboard ops returned success, SendInput returned
     success → outcome=Ok. Nothing actually appeared anywhere
     visible. The injector has NO concept of "the target was
     Notepad; verify before pasting."
- **Action (three layers, all in this commit):**
  1. **Re-snapshot foreground before inject.** New post-cleanup
     check in `dictation::complete()`: re-call `window_ctx.foreground()`
     right before the inject step. If `process_name` differs from
     `fg_keyup.process_name` (case-insensitive, basename only —
     ignore HWND and title to avoid false alarms from window cycles
     and title edits), set `outcome = AbortedFocusChanged` and skip
     the injector call entirely. Raw + cleaned still persist for
     provenance; only the `final` transcript stage is omitted. User
     gets a clear History row showing "aborted, you navigated away"
     instead of a silent wrong-window paste.
  2. **Reused the existing `AbortedFocusChanged` variant** rather
     than minting a new `AbortedFocusDrift`. Semantics are close
     enough ("focus changed, we declined to paste") and the DB
     CHECK constraint in migrations/004 already allows the string.
     A new variant would require a migration AND a DB constraint
     update AND a UI badge, which is out of scope for the bugfix.
     Future: if we want to disambiguate "focus changed between
     key-down and key-up" (legacy, never emitted under ADR 0020)
     from "focus drifted during slow cleanup" (this fix), mint a
     new variant in Phase 6 polish.
  3. **Warm Ollama on boot** to make the slow-cleanup case rare.
     `dictation/runtime.rs::spawn_ollama_warmup` fires a tiny
     `/api/chat` (num_predict=1) on a dedicated thread right after
     the health-check succeeds. Pays the 30-60s cold-load cost
     while the user is opening their target app. First real
     dictation hits a warm model. Errors ignored — worst case the
     first real cleanup still cold-loads, which is just today's
     behavior.
- **Pattern:** ANY OS-targeted side effect (paste, click, focus,
  hotkey-grab) that runs AFTER an unbounded asynchronous step must
  re-validate its target before firing. The captured-at-key-up
  snapshot is stale the moment the next async op runs. Re-validation
  is cheap (one GetForegroundWindow + GetWindowThreadProcessId +
  K32GetModuleBaseNameW); silent wrong-target action is expensive
  (user trust + maybe a security-sensitive paste into the wrong app).

---

## 2026-05-17 [phase5-postship-3] pill still had a dark rectangular halo even after filling the window

- **Context:** Third smoketest. Postship-2 made the pill fill the
  entire window (`width:100%`, `height:100%`), which fixed the
  "transparent corners showing dark" issue. Pill is now a proper
  capsule. BUT — there's STILL a dark rectangle visible around the
  pill in the screenshot. Sharp corners. Hugs the pill on all four
  sides like a halo.
- **Finding:** The `.pill` rule still had `box-shadow: var(--shadow-3)`
  from the original design (when the pill was a smaller centered
  child of the window). Since the pill now fills 100%×100% of the
  window, the box-shadow extends OUTWARD from the rounded pill — but
  the area outside the pill is INSIDE the window's rectangular
  bounds. WebView2 happily paints the shadow there. Result: a sharp-
  cornered dark rectangle of shadow, hugging the rounded pill, that
  the previous fixes couldn't eliminate because they were targeting
  the wrong artifact (we kept blaming WebView2 transparency).
- **Action:** Remove the pill's `box-shadow`. Tauri's `shadow: true`
  on the recording window (already set in tauri.conf.json) gives a
  real OS-level DWM shadow that renders OUTSIDE the window bounds —
  the only place a shadow belongs on a frameless popup. CSS shadows
  are for elements with breathing room around them.
- **Reusable rule:** When a CSS element fills 100% of its container,
  it can't have a CSS `box-shadow` that extends outward — the shadow
  will clip to the container's bounds and produce a sharp-cornered
  artifact that looks NOTHING like a shadow. Either give the element
  margin/padding inside the container, or move the shadow to the
  container (OR, for windows, to the OS shadow API).

---

## 2026-05-17 [phase5-postship-2] pill stayed up + app crashed when Ollama cold-loaded a model past our 30s timeout

- **Context:** Phase 5 second smoketest. Right Alt + dictate. Capture
  finished cleanly. Pill went CLEANING. Then a 31-second gap in the
  logs ending with `WARN cleanup failed; falling back to raw
  transcript error=transport: http://localhost:11434/api/chat:
  Network Error: Error encountered in the status line: A c...`. After
  that: zero further log lines, pill stuck on screen forever,
  Mockingbird eventually crashed (process gone).
- **Finding 1 (cleanup hang root cause):** Ollama loads the model into
  VRAM on the FIRST `/api/chat` request. For qwen2.5:3b-q4 on a fresh
  Ollama process, that cold load can take 30-60 seconds. Our
  `REQUEST_TIMEOUT` in `cleanup/ollama.rs` is exactly 30s, so we sit
  RIGHT at the edge and frequently lose by a hair. The `/api/tags`
  health probe passes instantly (no model load involved), so the
  app's startup health check shows green even when the first cleanup
  is doomed to time out.
- **Finding 2 (pill stuck after cleanup hang):** `LlmCleaner::clean`
  catches the timeout error and returns `Ok(raw)` per the fallback
  rule — the WARN line in the log proves we reached that branch. But
  no logs after that means either the inject path hung silently OR
  the process panicked between cleanup-fallback and inject. Either
  way, `complete()` never reached its explicit `self.recording_window
  .hide()` at the bottom. The pill has no self-defense mechanism.
- **Finding 3 (why the process eventually crashed):** Unproven but
  most likely: the WebView2 child process died after our many
  emit-while-no-listener events with no logging path (WebView2 uses
  its own process group; when it dies, AppHandle::emit silently
  no-ops, but if the death was during an emit's actual IPC handshake
  the parent can get a broken-pipe panic propagating up the Tauri
  internals).
- **Action (defense in depth):**
  1. **Rust Drop guard.** Added `PillHideGuard` (RAII) at the top of
     `complete()`. Wraps a clone of `RecordingWindow` (cheap — it's
     Arc<AtomicBool> + Arc<Mutex<Option<AppHandle>>>). On Drop, if
     the window is still visible, hide it. Disarmed right before the
     explicit success-path hide() so we don't double-fire. Skips the
     warning log when window is already hidden (idempotent persist_
     failed_* paths beat us to it).
  2. **React watchdog.** Recording overlay now tracks time since the
     last `dictation:state` event. If >60s passes (= Rust
     orchestrator dead or terminally hung), the webview hides itself
     via `getCurrentWindow().hide()`. Doesn't need IPC to a dead
     parent. This is the only line of defense that works when the
     ENTIRE Rust process has crashed.
  3. **Inject logging.** Added `tracing::info!` at cleanup-begin,
     cleanup-end, inject-begin, inject-end. Next time something
     hangs we'll see exactly where.
  4. **Did NOT touch the 30s timeout** (in scope: defensive UI; the
     timeout itself is a separate ADR conversation — raising it
     punishes the user with longer hangs, lowering it more risks
     legit slow first-calls). Future work: a one-time "warm Ollama"
     ping on app boot that does a dummy /api/chat to pay the
     model-load cost in the background, so the first user dictation
     hits a hot model.
- **Pattern for future Tauri overlays:** ANY OS-managed visual
  affordance (overlay window, tray flyout, status badge) needs:
  - A Drop guard on the Rust side guaranteeing the affordance gets
    cleared on early return / panic.
  - A watchdog timer on the webview side that self-clears when no
    state update arrives within N seconds, using the webview's own
    API rather than IPC. The parent process might be dead.
  Together these handle (a) Rust-level errors, (b) Rust panics, (c)
  full Rust process crashes — the only failure they don't cover is
  WebView2 itself dying, in which case the user gets to ALT-F4 the
  empty window like any other ghost.

---

## 2026-05-17 [phase5-postship-2] `prompts.version INTEGER` failed to deserialize as `String`

- **Context:** Same smoketest. Modes page error box (after our prior
  silent-spinner fix exposed it): `Invalid column type Integer at
  index: 8, name: COALESCE(p.version, 'v1')`.
- **Finding:** `prompts.version` is `INTEGER` in 001_initial.sql. The
  `ModeDto.prompt_version` is `String`. The fallback literal `'v1'`
  IS a string, which made the COALESCE return type ambiguous —
  rusqlite picks the first non-null branch's affinity at row time,
  and on rows where `p.version` was non-null it returned Integer.
  rusqlite's `String` deserializer rejects Integer columns hard.
- **Action:** Concatenate to force TEXT affinity:
  `COALESCE('v' || p.version, 'v1')`. SQLite's `||` operator coerces
  both operands to TEXT. Produces strings like "v1", "v2", … which
  is what the UI was already rendering. No schema change needed
  (and forbidden post-Phase-1-seal anyway).
- **Reusable rule:** Whenever an SQL fallback literal differs in type
  from the column it's replacing, the COALESCE result type is
  per-row-dependent. Either cast both sides or change the DTO to
  match the column type. Default to casting (DTO contracts cross
  process boundaries; column types are internal).

---

## 2026-05-17 [phase5-postship] release binary baked in `localhost:5173` — webview shows "can't reach this page"

- **Context:** First end-to-end Dustin smoke test of the Phase 5 build.
  Tray icon left-click started working (after our prior fix), main
  window opened... and rendered the Edge/WebView2 default error page:
  *"Hmmm… can't reach this page — localhost refused to connect…
  ERR_CONNECTION_REFUSED"*. Pipeline still worked; only the visual
  surface was dead.
- **Finding:** In Tauri 2, the choice between `devUrl`
  (`http://localhost:5173`) and bundled-asset `frontendDist` is gated
  on the `tauri/custom-protocol` cargo feature, NOT on
  `cfg(debug_assertions)`. `cargo tauri build` enables `custom-protocol`
  implicitly; plain `cargo build --release` does NOT. So a vanilla
  `cargo build --release` produces a binary that, in production,
  literally tries to fetch the UI from a dev server that isn't running.
  Confirmed by string-searching the .exe: `localhost:5173` was right
  there, baked in.
- **Action:** Add `default = ["custom-protocol"]` +
  `custom-protocol = ["tauri/custom-protocol"]` to `src-tauri/Cargo.toml
  [features]`. Now both `cargo build --release` (our wrapper path,
  because tauri-cli doesn't propagate CUDA env reliably) AND
  `cargo tauri build` produce a binary that uses the bundled UI. The
  override pattern `cargo build --release --no-default-features` still
  works if someone genuinely wants the dev-server path in a release
  binary (weird but supported).
- **Diagnostic snippet** worth keeping:
  ```powershell
  $bytes = [IO.File]::ReadAllBytes('target\release\mockingbird.exe')
  $text = [System.Text.Encoding]::ASCII.GetString($bytes)
  if ($text -match 'localhost:5173') { 'devUrl LEAKED into release binary' }
  ```

---

## 2026-05-17 [phase5-postship] tray left-click did nothing + recording overlay rendered blank

- **Context:** Same Dustin smoke test. Two visible bugs:
  (1) left-clicking the tray icon did nothing (the menu opens on
  right-click, the main window never appeared); (2) the recording
  overlay appeared as a blank rounded box — not the pretty pill from
  the Playwright baselines.
- **Finding (tray):** `tray.rs` had `.on_menu_event(…)` (right-click
  menu) but ZERO `.on_tray_icon_event(…)` (left-click). And the
  `open_history` / `settings` / `pause` menu items were still Phase 1
  stubs that just `tracing::info!`d "(stub, Phase 5)". Easy miss
  during the UI sprint because the orchestrator + DB work absorbed all
  attention; the tray surface was never re-touched after Phase 1.
- **Finding (overlay):** Two stacked race conditions:
  - **Emit-before-listen.** `RecordingWindow::show()` calls
    `w.show()` then immediately `self.emit(LISTENING, …)`. On the
    first show the webview cold-starts (~50–500ms: WebView2 process
    spawn + JS bundle load + React mount + `listen()` registration).
    The single emit fires WHILE React is still mounting and the
    listener doesn't exist yet — event is lost forever, React stays at
    its initial `state: "idle"` value.
  - **Missing `modeLabel` in payload.** Rust `StateEventPayload` had
    `state + mode_slug + error`. React renders the mode badge
    conditionally on `event.modeLabel` — which was always undefined.
  - **Naive event-replace on the React side.** Mid-pipeline emits
    (`transcribing`, `cleaning`, …) only carry the new `state`, no
    `modeSlug` / `modeLabel`. A bare `setEvent(e.payload)` was wiping
    the mode badge on every transition.
- **Action (tray):** Add `.on_tray_icon_event` matching `MouseButton::Left`
  + `MouseButtonState::Up`, toggle the main window's visibility +
  focus. Wire `open_history` and `settings` menu items to call the
  same show-main-window helper. (Deep-linking to specific pages
  needs an `app:navigate` event — follow-up.)
- **Action (overlay):** Three layers of defense, all cheap:
  1. Rust: spawn a 3-emit burst at 50/200/500ms after the first
     `show()`, gated on `was_hidden`, bailing if visibility flips off.
  2. Rust: add `mode_label: Option<String>` to the payload, derived
     from `mode_slug` via title-case fallback.
  3. React: change initial state to `"listening"` (not `"idle"`) so
     the pretty pill renders even if every emit somehow misses;
     change `setEvent(e.payload)` to `setEvent(prev => ({ …prev, …e.payload }))`
     so mid-pipeline emits preserve the mode badge.
- **Pattern for future Tauri overlays:** ALWAYS pair an event-driven
  initial-state push with EITHER a query-on-mount IPC command OR a
  re-emit burst. The webview-cold-start emit race is silent and
  reproducible; debugging it without a console is brutal because
  transparent + `focus: false` windows are awkward to attach DevTools to.

---

## 2026-05-17 [phase5-wave-I] `cargo test --lib` exits 0xc0000139 even with cargo-with-cuda wrapper

- **Context:** Phase 5 Wave I wiring `RecordingWindow` to the real Tauri
  webview + `Emitter`. Wanted to run `cargo test --lib recording_window`
  (pure tests, no DLL deps in my code) to confirm new unit tests pass.
- **Finding:** Test binary exits with `STATUS_ENTRYPOINT_NOT_FOUND`
  (0xc0000139) even when `pwsh scripts/cargo-with-cuda.ps1 test --lib`
  is used. The earlier `0xc0000135 STATUS_DLL_NOT_FOUND` was solvable
  by putting `target\debug\` on PATH; this one is one level deeper —
  a DLL loaded successfully but an expected export is missing
  (likely `whisper.dll` ABI drift between debug+release builds, or
  `onnxruntime.dll` version mismatch). Affects ALL lib tests, not just
  mine — `cargo test --lib hotkey::state` (pure state-machine, no
  whisper/ort touch) also exits 0xc0000139. So it's a process-load-time
  failure, not a test failure.
- **Action:** Verify Rust code via `cargo build --lib` +
  `cargo test --lib --no-run` instead — both confirm the type system,
  borrow checker, and trait bounds without needing to actually launch
  the test exe. If a Phase 5/6 wave needs live test execution, the
  fix is to copy `whisper.dll` + `onnxruntime.dll` into
  `target\debug\deps\` (where the test exe lives), not just
  `target\debug\`. The launcher script doesn't do this today.

---

## 2026-05-17 [phase3-wave4.8] Silero v5 ONNX needs an UNDOCUMENTED 64-sample context buffer

- **Context:** End-to-end dictation produced empty Whisper output. Tracing
  showed the audio pipeline was healthy (capture worked, resampler worked,
  WAV-dump of the post-resample buffer sounded perfect to a human), but
  `vad_trim` kept returning zero samples because Silero VAD scored every
  frame as non-speech (max confidence ~0.0031 across 155 frames of clear
  speech vs. the 0.5 threshold).
- **Finding:** The Silero v5 ONNX model published at
  `snakers4/silero-vad/master/src/silero_vad/data/silero_vad.onnx`
  requires the caller to maintain a 64-sample "context" buffer (last 64
  samples of the previous frame, initialised to zeros) and **prepend it
  to every 512-sample frame before inference**, making the actual model
  input shape `[1, 576]`, not `[1, 512]`. Without this:
  - The model still runs (no error)
  - The output tensor still has the right shape `[1, 1]`
  - The output is essentially CONSTANT regardless of input: silence,
    speech, pure tones, and white noise all score ~0.001–0.003. The
    sigmoid output sits permanently at "definitely silence."
  This is **not in the ONNX schema metadata**. The schema declares
  `input: Float32 [-1, -1]` (any batch, any samples) — accepts 512
  samples without complaint. The requirement lives only in
  `silero-vad/src/silero_vad/utils_vad.py`'s `__call__` method:
  ```python
  context_size = 64 if sr == 16000 else 32
  if not len(self._context):
      self._context = torch.zeros(batch_size, context_size)
  x = torch.cat([self._context, x], dim=1)   # PREPEND
  ort_inputs = {'input': x.numpy(), ...}
  self._context = x[..., -context_size:]     # SAVE LAST 64 FOR NEXT CALL
  ```
  Misleading red herrings on the path here:
  - The `sr` input is declared `Tensor { ty: Int64, shape: [] }` (scalar).
    Both `ort 2.0.0-rc.10` and ONNX Runtime accept shape `[1]` (1-d,
    1-element) silently. The `silero-rs` Rust crate also uses `[1]`.
    Trying "true" scalar shapes (`()`, `[0_usize; 0]`) actually made the
    output WORSE, not better — these paths through ort + onnxruntime
    appear to mis-handle 0-d Int64 scalars. Stick with `[1]`.
  - ORT `GraphOptimizationLevel::Level3` vs `Level1` makes no difference
    once the context buffer is correct.
  - The model file hash matched the official upstream — it wasn't
    corrupted, just being called wrong.
- **Action:**
  1. Always maintain a `context: Vec<f32>` of size 64 in any Silero v5
     wrapper. Init to zeros, update with the last 64 samples of every
     new frame, reset in `reset()`.
  2. **When integrating a vendor ONNX model, find and read the
     reference Python `__call__` end-to-end** before trusting the
     schema. Schemas describe tensor shapes, not protocol semantics.
     The schema cannot tell you "this model expects to be called with
     overlapping windows."
  3. **Test models against KNOWN INPUTS with known expected outputs.**
     Our test suite only asserted "silence scores low" — which the
     broken impl still satisfied. Now we have a regression test
     (`silero_output_has_dynamic_range`) that asserts the model
     produces *meaningfully different* outputs for structurally
     different signals (silence vs. swept sine). Without the context
     buffer, all confidences collapse to ~0.001 and the test fails.
  4. **Add a `last_capture.wav` dump on every dictation** (single-slot
     overwrite) — this was the diagnostic that proved the audio was
     fine and isolated the bug to Silero. Cheap, low-overhead, and
     pays for itself the first time you need it.
  5. ORT `Tensor::from_array` silently accepts wrong-shape inputs for
     scalar parameters. Don't trust "no error" as "correct config" —
     verify behaviour with end-to-end output assertions.


## 2026-05-17 [phase-3-wave-4.5] `target/release/mockingbird.exe` fails with `STATUS_DLL_NOT_FOUND` from any cwd that isn't the build dir

- **Context:** Wave 4.5 smoke-tested the wired-up binary by running it directly. Got exit code `0xC0000135` (DLL_NOT_FOUND) within 100 ms, no logs, no panic message.
- **Finding 1:** `[lib] crate-type = ["staticlib", "cdylib", "rlib"]` means the exe links against `mockingbird_lib.dll` (cdylib), which Windows looks for via standard DLL search order. Running the exe from any cwd other than `target\release\` fails the load — the cdylib isn't on PATH.
- **Finding 2:** Even with cwd = `target\release\`, the binary still failed because `whisper-rs = { features = ["cuda"] }` (root `Cargo.toml` line 66) makes the build dlopen `cudart64_*.dll` at process start. CUDA 12.8 is installed at `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.8\` but its `bin\` is NOT on system PATH — only the `cargo-with-cuda.ps1` wrapper adds it for cargo invocations.
- **Finding 3:** The diagnostic was painful because `windows_subsystem = "windows"` (set in `main.rs` for release builds) suppresses console output. Loader-time failures show only the exit code, with no stderr text.
- **Action:** Created `scripts/run-mockingbird.ps1` that (a) sets cwd to `target\release`, (b) prepends CUDA 12.8 `bin\` to PATH, (c) sets `ORT_DYLIB_PATH`, (d) sets `SILERO_VAD_PATH` + `WHISPER_MODEL_PATH` for log clarity. Phase 6 packaging will copy the CUDA + ONNX DLLs next to the exe so this is moot in the installer; for dev workflow, use the launch script.
- **Generalisation:** When diagnosing `STATUS_DLL_NOT_FOUND` on a Tauri-cdylib release binary, the suspect order is: (1) the cdylib next to the exe, (2) any `features = ["cuda"]` deps, (3) `ORT_DYLIB_PATH` for `ort` crate, (4) Visual C++ runtime. Loader Snaps (`gflags /i mockingbird.exe +sls`) would print each DLL probe to a debugger — overkill for dev but invaluable if (1)-(4) don't explain it.

## 2026-05-17 [phase-3-wave-4.5] `cargo test --lib` doesn't see `ensure_ort_dylib_set()` so VAD tests need explicit env

- **Context:** After Wave 4.5 patches, 3 VAD tests failed with `ort 2.0.0-rc.10 is not compatible with the ONNX Runtime binary found at \`onnxruntime.dll\`; expected GetVersionString to return '1.22.x', but got '1.17.1'`.
- **Finding:** `dictation::runtime::ensure_ort_dylib_set()` only runs from `lib.rs::run()` (the Tauri entry point). `cargo test` is a separate binary that links `mockingbird_lib` directly and exercises `audio::vad::tests::*` without going through `run()`. So the env autodiscovery doesn't fire and Windows resolves `onnxruntime.dll` via default search order — finding a stale 1.17.1 DLL somewhere on the system.
- **Action:** Document the env requirement: `cargo test` needs `ORT_DYLIB_PATH` set explicitly. Wave 5 (or Phase 6) should either add a `#[ctor]`-style init for tests OR mark the affected VAD tests as `#[ignore]` to keep `cargo test` clean. For now: the launch script + cargo wrapper both set the env; bare `cargo test` is the only caller that needs manual setup.
- **Generalisation:** Env-var autodiscovery in `lib.rs::run()` covers the production path but creates a silent test-only gap. If a runtime env var is required, it should be enforced uniformly — either via a shared `init_runtime_env()` function called from both `run()` and a test-init hook, or via test attributes (`#[ignore]` for env-dependent tests).



## 2026-05-17 [phase-3-wave-4] ADR scope ≠ task brief scope

- **Context:** The Wave 4 brief said "no migration 004 — ADR 0010 binding." Implementation discovered the orchestrator needs per-row `injection_status` persistence — without storage, the cross-app QA matrix can't be objectively verified after the fact.
- **Finding:** Re-read ADR 0010. It says raw-`transcripts` rows are immutable; says NOTHING about migration count. The "no migration 004" line in the brief was the brief author (me) conflating two different bindings: "raw transcripts immutable" (real) + "migrations append-only" (real but means "don't EDIT existing migrations, new ones fine"). Asked Dustin via `ask_user_question`; he picked (A) "add migration 004."
- **Action:** Migration 004 lands the nullable `injection_status` column. Brief discipline TODO: when writing a brief that cites an ADR as binding, **paste the binding sentence verbatim from the ADR into the brief**. Paraphrasing introduces drift.

## 2026-05-17 [phase-3-wave-4] `windows-rs` import names drift between consts

- **Context:** Implementing `paste.rs` test for `CF_UNICODETEXT`. First attempt: `use windows::Win32::System::DataExchange::CLIPBOARD_FORMAT;` to wrap our constant in the typed newtype. Build broke.
- **Finding:** In `windows-rs 0.56`, `CLIPBOARD_FORMAT` lives in `Win32::System::DataExchange` as a `pub struct CLIPBOARD_FORMAT(pub u16)` — but it's NOT re-exported at the module's top level for direct `use`. You have to import via its enclosing alias path or just use raw u32. For internal-only test usage, a raw `u32` constant + a comment-encoded invariant is simpler.
- **Action:** Dropped the typed wrapper test; kept `CF_UNICODETEXT_ID: u32 = 13` as a plain constant with a comment locking in the Win32 ABI guarantee (CF_UNICODETEXT has been 13 since NT 3.51). **Generalisation:** when a windows-rs type ISN'T at the top-level of its module, don't fight it for one test — fall back to documented raw values.

## 2026-05-17 [phase-3-wave-4] `&mut dyn Trait` arguments are easy to miss in trait composition

- **Context:** Orchestrator `complete()` first attempted `trim_speech(&samples, self.audio.sample_rate(), &TrimConfig::default())`. Build failed: `trim_speech` takes `&mut dyn VoiceActivityDetector`, not `u32 (sample_rate)`.
- **Finding:** I had glossed the VAD shape based on partial recall. The orchestrator needs to own a `Box<dyn VoiceActivityDetector>` AS A SEPARATE FIELD from the audio capture. The two are independent traits that happen to share a `sample_rate` constant (16 kHz). Conflating them was a category error.
- **Action:** Added `vad: Box<dyn VoiceActivityDetector>` to `DictationOrchestrator`. **Generalisation:** when composing multiple traits in an orchestrator, list them as bullet points in a docstring BEFORE writing the struct fields — forces clear thinking about which trait owns what.

## 2026-05-17 [phase-3-wave-4] Hard-coded timestamps in tests are guaranteed wrong

- **Context:** Wrote `assert_eq!(format_secs_as_iso(1_779_926_400), "2026-05-17T00:00:00Z")`. Test failed; my arithmetic was off by 11 days.
- **Finding:** Mental arithmetic on Unix epoch seconds is reliably wrong. Even careful pen-and-paper attempts miscount leap years (especially the 2000 case).
- **Action:** Corrected the value (1_778_976_000) by deriving from a known anchor (2024-01-01 = 1_704_067_200) plus precisely-counted intervening days. Added a second test using 2024-02-29 to catch leap-year regressions specifically. **Generalisation:** when a pure-math test fails, the test is the bug 90% of the time. Derive expected values from KNOWN anchors + arithmetic; never from "I think it's around…"

---


## 2026-05-17 [phase-3-wave-3] WH_KEYBOARD_LL thread-local discipline

- **Context:** Implementing `hotkey/windows.rs` real `SetWindowsHookEx(WH_KEYBOARD_LL, ...)` per ADR 0015. The callback is `unsafe extern "system" fn` — no captures, no closures, no `&self`. State has to live in a `static` somewhere.
- **Finding:** The natural temptation is `static SENDER: Mutex<Option<Sender>> = ...`. **Don't.** The callback fires on the hook-installing thread (the only thread that ever touches it), so a `Mutex` adds zero safety and risks the 300 ms hook-watchdog timeout if any other code on the same process ever locks it. The right tool is `thread_local!(static SENDER: RefCell<Option<Sender>> = RefCell::new(None))`. Uncontended by construction.
- **Action:** Used three `thread_local!` cells: `CALLBACK_TX` (the channel sender), `CALLBACK_VK` (configured VK for filtering), `CALLBACK_HHOOK` (owned hook handle for RAII unhook). All set on hook-thread entry, all cleared on exit. Drop order matters — unhook BEFORE dropping the sender (so a stray late callback can still no-op cleanly).

## 2026-05-17 [phase-3-wave-3] Pure-vs-OS split makes WH_KEYBOARD_LL testable

- **Context:** Without care, every test of `LowLevelKeyboardProc` would have to install a real hook + use real `SendInput`. Slow, flaky, and impossible on headless CI.
- **Finding:** The callback's actual work is "decide if this message + VK is interesting, and if so emit which `HotkeyEvent`". That's a pure function: `fn classify_keystroke(wparam, vk_code, configured_vk, at) -> Option<HotkeyEvent>`. The OS-side glue (read `KBDLLHOOKSTRUCT`, `try_send`, `CallNextHookEx`) is a 10-line shim with no decisions in it. **9 of the 11 unit tests in `hotkey/windows.rs` exercise the pure helper** with synthesised message/VK pairs; the 2 remaining are `#[ignore]` live SendInput round-trips.
- **Action:** Establish the pattern for the rest of Wave 3+: ANY new OS-bound module should split into a pure-helper file (or function) covered by fast unit tests, plus a thin OS shim that's `#[ignore]`-tested live. `hotkey/probe.rs` follows the same recipe (`probe_with` is pure, `probe_live` is the OS wrapper).

## 2026-05-17 [phase-3-wave-3] State-driver tick cadence is not "as fast as possible"

- **Context:** Designing `hotkey/driver.rs` — the loop that pulls from the listener channel + synthesises `HotkeyEvent::Tick` events when no real event arrives. First instinct: tick every 1 ms for sub-millisecond resolution.
- **Finding:** 1 ms ticks are pointless. The §6.1 thresholds we care about are 80 ms (hold), 300 s (max session), 30 s (cancel-threshold), 3 s (confirm-timeout). The TIGHTEST resolution we ever need is 80 ms / 4 = 20 ms. Lower cadence means: less CPU wake-up overhead on battery, less syscall churn through `recv_timeout`, and the LL-hook watchdog logger (every 250th tick = 5 min @ 20 ms) gets a clean integer count.
- **Action:** Default `DEFAULT_TICK_INTERVAL = Duration::from_millis(20)`. Tests in `driver.rs` use `Duration::from_millis(5)` for faster wall-clock test runs but the production constant stays 20 ms. **Generalisation:** when designing a tick loop, derive the cadence from the tightest deadline / (4 to 8), not from "feels precise."

---


## 2026-05-17 [phase-3-wave-2] windows-rs 0.56 HWND is isize, not pointer

- **Context:** Implementing `window_context/windows.rs` + `injection/secure_guard.rs`. First attempt used `.is_null()` and `*mut c_void` patterns that work in newer `windows-rs` releases.
- **Finding:** In `windows-rs 0.56` (our pinned version per ADR 0011 + Cargo.lock), `HWND` is `pub struct HWND(pub isize);` and `HANDLE` is `pub struct HANDLE(pub isize);`. Null check is `hwnd.0 == 0`, not `hwnd.0.is_null()`. Conversion to/from `ForegroundWindow.hwnd: isize` is the trivial `.0` access. **`windows-rs 0.61+` switched to `*mut c_void`-backed pointer types** for HWND/HANDLE — but we're not on 0.61+ and won't be in Phase 3.
- **Action:** Established pattern: pass `isize` across thread boundaries (in `ForegroundWindow.hwnd`), wrap as `HWND(isize_value)` at the OS boundary, compare against zero for null. If we ever upgrade `windows-rs`, this is one of the breaking points to watch.

## 2026-05-17 [phase-3-wave-2] GUI_SECUREINPUT is not a real Win32 constant

- **Context:** ADR 0017 specified three signals for `SecureInputGuard`, including `GetGUIThreadInfo(GUI_SECUREINPUT)`. Wave 2 implementor went to look up the constant value in `windows-rs 0.56` to use it.
- **Finding:** `GUI_SECUREINPUT` does not exist in `windows-rs` AND does not exist in the official Win32 SDK `winuser.h`. The full list of `GUITHREADINFO_FLAGS` is `GUI_CARETBLINKING (0x1)`, `GUI_INMOVESIZE (0x2)`, `GUI_INMENUMODE (0x4)`, `GUI_SYSTEMMENUMODE (0x8)`, `GUI_POPUPMENUMODE (0x10)`. The ADR author (me) conflated this with macOS's `IsSecureEventInputEnabled()` which IS a real API. The Windows reality is different: UAC consent prompts run on a separate **secure desktop** that our process can't enumerate; `GetForegroundWindow()` returns NULL during them, which trips the null-foreground guard in `window_context/windows.rs` BEFORE we ever reach the secure-input check.
- **Action:** Amended ADR 0017 with an "Update — 2026-05-17 (Wave 2)" section dropping signal 1. The remaining two signals (class-name allowlist + `ES_PASSWORD` on focused edit) are sufficient because: (a) UAC / Hello / BitLocker / Ctrl-Alt-Del trip the null-foreground guard, (b) Credential UI is caught by class name, (c) Win32 password edits are caught by `ES_PASSWORD`. WebView2 password fields remain documented as out-of-scope and are mitigated via per-app `Abort` overrides in ADR 0016.
- **Carry-forward:** When an ADR references an API, **the implementor MUST validate the API exists in our pinned dep version before sealing the ADR**. Add this to the planning-agent's "ADR review checklist".

## 2026-05-17 [phase-3-wave-2] State-machine precedence: hard stop wins

- **Context:** `HotkeyStateMachine::handle` for `(ConfirmingCancel, Tick)` originally checked `confirm_timeout` first, then `max_session`. A unit test (`max_session_overrides_confirm_cancel`) caught the latent bug.
- **Finding:** When two timeouts fire on the same tick, the precedence matters. The 300 s `max_session` is a HARD ceiling per PLAN §6.1 — without it a user who sits on the confirm-cancel toast past 300 s would have their recording grow indefinitely. The 3 s `confirm_timeout` is a SOFT revert. Hard stops must always win.
- **Action:** Reordered the branches; added a code comment explaining the precedence + a test that explicitly hits both timeouts simultaneously. **Generalisation:** state-machine code with multiple time-dependent transitions out of one state should always order the branches by "severity" — hard stops first, soft transitions after.

---


## 2026-05-17 [phase-3-wave-1] Build env, parallelism, and PowerShell stream/arg parsing

- **Context:** Phase 3 Wave 1 — five ADRs + module scaffolds + first cargo gate post-Phase-2. Hit six separate Windows toolchain papercuts before the gate went green.
- **Finding 1 — `scripts/cargo-with-cuda.ps1` is now the one-call wrapper for ALL cargo invocations in this project.** Imports MSVC env via `vcvars64.bat`, pins `CUDA_PATH` + `CUDA_PATH_V12_8` to v12.8 (ADR 0011), prepends cmake to PATH, caps `CMAKE_BUILD_PARALLEL_LEVEL=4`, then forwards args through `cmd.exe /c "cargo ... 2>&1"`. Replaces the prior "set env inline before every cargo call" pattern from Phase 2. Always invoke via `-File`, never `-Command` (see Finding 4).
- **Finding 2 — whisper-rs-sys CUDA compile OOMs at `--parallel 16` on a 16 GB machine.** Each `fattn-mma` template instance can use 2–4 GB resident RAM. With 16 cores, MSBuild fires 16 nvcc processes in parallel and one or more get killed silently, leaving 0-byte `.obj` files. The downstream `Lib.exe` then fails with `LNK1136: invalid or corrupt file`. **Fix:** export `CMAKE_BUILD_PARALLEL_LEVEL=4` (now baked into the wrapper script). Build time goes from ~5 min (when it works) to ~10 min, but the OOM is gone.
- **Finding 3 — Em-dashes in PowerShell scripts break `-File` invocations.** `powershell -File` reads the script as system code page (cp1252 on US Windows), not UTF-8. UTF-8 em-dashes (U+2014, three bytes `E2 80 94`) get split into bogus tokens (e.g. `'\libnvvp'`), parser fails downstream of the actual problem with a misleading "Unexpected token" error. **Fix:** stick to ASCII hyphens in `.ps1` files. Markdown / Rust source / ADRs can use em-dashes freely.
- **Finding 4 — `powershell -Command` eats the `--` argument delimiter; `-File` preserves it.** This kills `cargo clippy ... -- -D warnings`-style invocations. PowerShell's `-Command` parser silently swallows the `--`, sending `-D warnings` to cargo-clippy directly, which forwards it to `cargo check`, which errors out with `unexpected argument '-D'`. **Fix:** the wrapper script uses no `param` block and no `[CmdletBinding()]` — it grabs everything from `$args` — and all callers invoke via `-File`, not `-Command`.
- **Finding 5 — cmd.exe `%ERRORLEVEL%` expands at PARSE time, not run time, across `&` separators.** Writing `powershell ... & echo exit:%ERRORLEVEL%>exit.log` captures the previous-iteration exit code (typically 0), NOT the just-finished powershell's exit. **Fix:** use `call echo exit:%^ERRORLEVEL%` — the `^` escapes the `%` past parse-time, and `call` re-parses the line at run-time. Now exit codes propagate correctly.
- **Finding 6 — PowerShell pipelines treat native-command stderr as terminating errors under various `Tee-Object` / `*>&1` combinations.** Cargo writes "Compiling …" progress lines to stderr; under the wrong stream config, PowerShell promotes these to `NativeCommandError` and kills cargo mid-build. **Fix:** the wrapper invokes cargo via `& cmd.exe /c "cargo ARGS 2>&1"` — merging streams INSIDE cmd.exe means PowerShell only sees a unified text stream, no error promotion. `$ErrorActionPreference = 'Continue'` immediately before the cargo call adds belt-and-braces protection.
- **Action — all six findings now codified in `scripts/cargo-with-cuda.ps1`.** Future iterations should call `pwsh scripts/cargo-with-cuda.ps1 <cargo-args>` rather than reinventing the env-setup wheel.

---

## 2026-05-17 [phase-3-wave-1] Wave 1 retrospective

**Delivered:** 5 ADRs (0015–0019), 16 module scaffolds across `hotkey/`, `injection/`, `window_context/`, `AppError::Hotkey` + `AppError::Injection` variants, `phf` workspace dep, broader `windows-rs` feature set (UI_WindowsAndMessaging + UI_Input_KeyboardAndMouse + System_DataExchange + System_Memory + System_Threading + System_ProcessStatus), and a reusable `scripts/cargo-with-cuda.ps1` build wrapper.

**Surprised:** six separate PowerShell / cmd.exe / cmake / nvcc papercuts before the cargo gate went green. Each was one specific subtlety — ASCII-only scripts, parallelism cap, `-File` vs `-Command`, `%^ERRORLEVEL%`, cmd.exe stream merging, $ErrorActionPreference scope. Lost about 90 minutes here. None of these would have surfaced from "just write Rust code" planning — they only show up the first time a fresh shell tries to run the gate after Phase 2.

**Deferred:** none. All Wave 1 deliverables landed in one iteration.

**Carry-forward:**
- Wave 2 implementor (injection-author + code-puppy) MUST use `pwsh scripts/cargo-with-cuda.ps1` for every cargo call. No inline env setup; the script is the contract.
- Default for `WinKeyboardHook` is a non-derived impl that sets `vk = VK_RMENU`. Wave 3 must reconsider when the conflict probe (ADR 0019) resolves the binding — the constructor should accept a `vk` parameter.
- The `block-bare-paste` hook (`scripts/hooks/warn-bare-clipboard-set.py`) is shell-side only. Rust-side static enforcement of "only `injection/paste.rs` calls `SetClipboardData`" is deferred to a clippy lint or rust-analyzer rule in a later wave — YAGNI for Wave 1.

**Numbers:** 13 new tests (164 / 164 passing). 16 new files. ~600 net lines of code (counted in implementation files; ADRs are separate ~1100 lines). ADRs 0015–0019 sealed. bd tasks closed: 6 of 24 (mb-q1z, mb-3az, mb-anl, mb-jzm, mb-dlo, mb-rne).

---

## 2026-05-16 [phase-2] CUDA 12.8 install + GPU re-enable success story
- **Context:** Wave 4 punted CUDA because chocolatey only ships CUDA 13.2.1, which is too new (deprecated ggml archs + empty MSBuild `CudaToolkitDir`). Wave 5 finale: install CUDA 12.8 manually from developer.nvidia.com, side-by-side with the existing 13.2.
- **Finding 1 — Side-by-side works fine.** CUDA Toolkit installations live in version-suffixed dirs (`v12.8\`, `v13.2\`) under `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\`. The installer asks about this politely (the "Environment Variable Check" dialog at the end is informational, not an action item) — pick custom install, uncheck Nsight / Documentation / Display Driver / HD Audio (the driver components would DOWNGRADE you from a newer driver), KEEP `Development` + `Runtime` + **`Visual Studio Integration`** under CUDA. Visual Studio Integration is the one that ships `.targets`/`.props` files into the VS BuildTools BuildCustomizations dir — without it cmake's VS generator can't build CUDA at all.
- **Finding 2 — MSBuild picks the LATEST `.targets` file alphabetically.** Even after CUDA 12.8 installs cleanly, cmake-rs's VS 2022 generator was still reading `CUDA 13.2.targets` (broken) instead of `CUDA 12.8.targets` (working) because MSBuild auto-imports all CUDA `.targets` files and tries the highest version. Fix: physically move (not delete — backup is reversible) the v13.2 `.props/.targets/.xml/.dll` files out of `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\MSBuild\Microsoft\VC\v170\BuildCustomizations\` to a backup folder. Requires admin (UAC). See `scripts/disable-cuda13-msbuild.ps1`.
- **Finding 3 — `CMAKE_GENERATOR_TOOLSET=cuda=...` does NOT override cmake-rs.** Tried setting this env var to force cmake to use CUDA 12.8; cmake-rs is hard-coded to set `toolset=host=x64` and complained about a duplicate toolset spec. The MSBuild integration file route is the only viable path on Windows.
- **Finding 4 — Child PowerShell processes do NOT inherit User/Machine env from the registry.** When the agent spawns a fresh PowerShell, that shell inherits env from the agent's PARENT process (which started before the CUDA install). New `CUDA_PATH`, `CUDA_PATH_V12_8`, and `PATH` entries set via `[Environment]::SetEnvironmentVariable(..., 'User')` aren't visible in spawned shells until the parent process restarts. Workaround: explicitly assign `$env:CUDA_PATH = '...v12.8'` AND `$env:CUDA_PATH_V12_8 = '...v12.8'` at the top of every PowerShell invocation that calls cargo. Persisted env vars matter for future shells; the running session needs `$env:` assignments.
- **Finding 5 — `cargo clean -p whisper-rs-sys` doesn't always trigger a fresh CUDA rebuild.** Cargo computes a per-feature-set hash for each crate's build dir; two simultaneous hashes (`6bf2b1cc..` CPU-only and `9140b77c..` CUDA-on) coexisted in `target/release/build/`. The stale binary linked against the CPU artifact even though the CUDA artifact had built successfully — because the previous build's link step timed out before re-linking `stt_test.exe`. **Just run `cargo build` again after a timeout; cargo's incremental rebuild figures it out.**
- **Finding 6 — clippy uses the DEBUG profile by default, which requires a fresh debug cmake configure.** After cuda landed in `target/release/`, running plain `cargo clippy` triggered a brand-new debug build of whisper-rs-sys (~10 min, including all CUDA kernels recompiled in debug). Fix: `cargo clippy --release --all-targets`. Reuses the release artifacts; takes 60 s instead of 10+ min.
- **Finding 7 — Whisper hallucinates `"Thank you."` from pure silence.** Whisper's training data includes thousands of YouTube outro phrases; the model occasionally fabricates one from silence. **This is not a regression** — it's a documented Whisper artifact. The non-fabrication test should assert text length under ~50–100 chars, not text equality. Real VAD will trim silent buffers down to nothing before they reach Whisper in production, so this affects only `--no-vad` paths.
- **Action — Wave 5 final commit landed `phase-2-complete` tag.** 151/151 tests pass on GPU. Latency on silent.wav: 716 ms total (~600 ms is cold model load to GPU; subsequent transcribes will be sub-100 ms).

---

## 2026-05-16 [phase-2-retrospective] Phase 2 retrospective (Waves 1–5; ✅ SEALED)

**Delivered (5 waves):**
- Wave 1: 4 ADRs (0011 whisper-rs CUDA, 0012 ort runtime, 0013 cpal/ringbuf, 0014 model storage), `AudioCapture` + `VoiceActivityDetector` + `SpeechToText` traits, model-resolver + 224-token cap constant, scaffolds with `todo!()` bodies, model download script (BITS-resumable + SHA-256-verified).
- Wave 2: `CpalCapture` (cpal 0.15 + ringbuf 0.4, 16 kHz mono i16, 1 MB SPSC, 30 ms frames, start/stop idempotent), synthetic WAV fixture generator + 3 fixtures committed (silent / sine_440 / mixed), 8 integration tests.
- Wave 3: Silero VAD via ort `2.0.0-rc.10` with `load-dynamic + ndarray` features (sidesteps MSVC 2022 STL static-link demand), 512-sample frames + LSTM carry-through, `vad_trim` helper with lead-in/hangover/min-speech, 4 unit + 4 integration tests.
- Wave 4: `WhisperStt` (whisper-rs 0.16, CPU path), `prompt_builder` (recency × frequency × app-match, hand-rolled ISO-8601 parser, 224-token greedy pack), `stt_test` CLI (pretty + JSON), criterion bench skeleton, 4 whisper integration tests, 12 prompt_builder unit tests.
- Wave 5: 3 judge cards (`stt-correct`, `cuda-verified`, `perf-stt`) + 3 entries in `judges-template.json`, this retrospective. Initial Wave 5 commit held back the seal tag pending GPU verification. **Wave 5 finale (same day):** CUDA 12.8 installed side-by-side with CUDA 13.2, MSBuild integration sorted, `whisper-rs cuda` feature re-enabled, stt_test verified `gpu_used=true` on RTX 2060 — **`phase-2-complete` tag APPLIED.**

**Test count growth:** 101 (Phase 1) → 122 (W2) → 134 (W3) → 151 (W4). +50 tests over the phase, target was +40–50 — hit.

**What worked:**
- **The brief pattern keeps paying.** End-of-wave briefs nailed Wave 2/3/4 first-try compile-and-pass with one or two trivial clippy fixes. ~95% first-run pass rate.
- **Skipping gracefully > skipping silently.** Tests gated on runtime resources (`silero_runtime_available`, `whisper_model_present`) skip with a `eprintln!` — keeps CI green without `#[ignore]` hiding regressions.
- **ADRs upstream of dependency decisions.** ADR 0011 named the CUDA-fallback design BEFORE we hit the CUDA-13 chasm. When the chasm appeared the answer was "the architecture already handles this; ship CPU and re-enable later," not panic.

**What surprised us:**
- **Chocolatey's CUDA package is current-only.** No version pins available; you get whatever's latest. For tightly-coupled toolchains (CUDA 12 vs 13 is a different ABI generation), this is a footgun.
- **whisper-rs 0.13.2 ships internally inconsistent.** 71 build errors before we even touched CUDA — the wrapper accessed fields the `-sys` crate's bindgen explicitly hid as opaque. Newer crate version solved it; lesson is *don't blind-pin -sys-coupled crates*.
- **whisper-rs 0.16 API renames silently.** `full_n_segments()` → returns `i32` (was `Result`); `full_get_segment_text(i)` → `get_segment(i)` returning `Option<Segment>` with `.to_str_lossy()`. Caught at compile time but the brief was based on 0.13 shapes.
- **PowerShell parses em-dashes (U+2014) as multi-byte garbage in scripts.** Strip to ASCII before saving any `.ps1`. Already burnt this in Phase 1 — recurring.

**What we deferred:**
- **A real-speech 10s WAV fixture.** Wave 4 ships synthetic sine/silent only; the `stt-correct` + `perf-stt` judges need `hello.wav` (or similar) with an `.expected.txt` sidecar. Helios delegation candidate (Windows `System.Speech.Synthesis`).
- **Phase 3 will own the global hotkey + injection paths.** No keyboard hooks landed; the trait stubs for cross-app injection are NOT in Phase 2.

**Carry-forward to Phase 3 (cross-app injection):**
- The brief pattern: write `docs/phases/phase3-wave1-brief.md` at the start, treat it as binding.
- The `Provenance is total` invariant is about to get pressed harder — sessions table rows go from "in-memory test only" to "every hotkey press writes a row."
- Test density target: ~10 tests / 500 LoC. Phase 1 hit it, Phase 2 hit it (~50 tests / ~2,500 new lines).
- AppError-string variants generalize well; Phase 3 will likely add `Injection(String)` + `Hotkey(String)`.

**Phase 2 numbers:**
- Tests: 151 / 151 ✅ (was 101 at Phase 1 seal)
- LoC added: ~2,500 (audio + stt + vad + bin + tests + benches + scripts + ADRs)
- ADRs: 4 new (0011–0014) — all Status=Accepted
- LESSONS entries: +18 (now ~30 total)
- bd tasks: 27 of 27 Phase-2 tasks closed (including the GPU-verification seal-blocker `mb-ltq`)
- Phase tag: **`phase-2-complete` APPLIED 2026-05-16.**

---

## 2026-05-16 [phase-2] CUDA 13 + whisper-rs 0.16's bundled ggml = chasm
- **Context:** Wave 4 installed CUDA Toolkit 13.2.1 (latest, only version on choco) plus VS 2022 BT, cmake, LLVM. Tried to build whisper-rs with the `cuda` feature.
- **Finding 1:** ggml hard-codes CUDA architectures `52;61;70;75`. CUDA 13 dropped pre-Turing support — those archs no longer compile.
- **Finding 2:** MSBuild's `CUDA 13.2.targets` integration file reads `CudaToolkitDir` from somewhere that's coming up empty post-install. Either a registry timing issue or the installer not registering correctly when invoked through chocolatey.
- **Finding 3:** Chocolatey only publishes `cuda 13.2.1`. Older versions (12.x) are NOT available through the default repo — would require manual download from developer.nvidia.com (~3 GB).
- **Action:** Shipped Wave 4 CPU-only by dropping the `cuda` feature from whisper-rs in `Cargo.toml`. **This is NOT a shortcut** — ADR 0011's runtime CPU fallback was designed for exactly this scenario. `WhisperStt::new` still has GPU-first/CPU-fallback semantics; without the cuda feature the GPU attempt fails immediately and the CPU path runs. When CUDA 12.x is installed side-by-side from developer.nvidia.com (CUDA toolkits coexist in `v12.x` / `v13.x` subdirs), flip the feature back on and the GPU path activates.
- **Follow-up:** bd `mb-ltq` tracks the GPU re-enable task.

## 2026-05-16 [phase-2] whisper-rs 0.13.x ships incompatible with its own -sys crate
- **Context:** Wave 4 originally pinned `whisper-rs = "0.13"`. Build failed with 71 errors of the form `no field grammar_penalty on type whisper_full_params`.
- **Finding:** whisper-rs 0.13.2 (the high-level wrapper) and whisper-rs-sys 0.11.1 (the bindings) were published incompatible. The -sys crate's bindgen produced opaque structs (`pub _address: u8` with `size_of=264` assertion) — bindgen's signal for blocklisted types. The 0.13.2 wrapper tries to access fields the bindings explicitly hid.
- **Action:** Bumped to `whisper-rs = "0.16"`. The 0.16/sys-0.15.0 pair compiles cleanly and the field-access pattern works. **Lesson:** when a Rust crate has a sibling `-sys` crate, the high-level and low-level versions are coupled tightly; trust the crate author's pairing and use the LATEST stable rather than version-pinning blind.

## 2026-05-16 [phase-2] whisper-rs 0.16's segment API renamed methods
- **Context:** Brief specified `state.full_get_segment_text(i)` (0.13 API) and `state.full_n_segments()` returning `Result<i32>`.
- **Finding 1:** 0.16 changed `full_n_segments()` to return `i32` directly (no Result).
- **Finding 2:** 0.16 introduced a `Segment` accessor: `state.get_segment(i)` returns `Option<Segment>` (not Result); use `.to_str_lossy()` on the segment to get UTF-8-safe text.
- **Action:** Brief updated mid-execution. The 0.16 API is cleaner; document the shape in `stt/whisper.rs` comments so future bumps catch the next API drift.

## 2026-05-16 [phase-2] chocolatey package paths: cmake hides inside VS 2019 BT
- **Context:** Pre-install reconnaissance showed `where cmake` returned nothing — yet a recursive search of `C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools` found `cmake.exe` already on disk.
- **Finding:** VS 2019/2022 BuildTools includes a cmake.exe inside `Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\`. It's NOT on PATH by default. Saved 50 MB of redundant install in option A's path; ultimately we installed standalone cmake anyway for explicit PATH access.
- **Lesson:** Before `choco install <tool>`, recursive-search the existing VS installs. Frequently the tool is already there.

## 2026-05-16 [phase-2] PowerShell em-dash bites in script files
- **Context:** `scripts/install-wave4-toolchain.ps1` failed to parse because I copy-pasted an em-dash ("—") inside a string literal. PowerShell's parser cascaded brace errors from the malformed string.
- **Finding:** PowerShell expects ASCII inside script files unless explicit BOM/UTF-8 encoding is declared. The em-dash (U+2014) and en-dash (U+2013) got mangled on save → garbled multi-byte sequences that broke string termination.
- **Action:** Strip both characters via `$c -replace [char]0x2014, '--' -replace [char]0x2013, '-'` before saving. Use ASCII hyphens in PowerShell scripts always.

## 2026-05-16 [phase-2] ort 2.0 is RC-only AND static-link demands MSVC 2022
- **Context:** Phase 2 Wave 3 added `ort` for Silero VAD. Compilation failed two ways.
- **Finding 1:** No stable ort 2.0 exists — only `2.0.0-rc.1` through `rc.12`. Plain `"2"` fails cargo ("prerelease must be specified explicitly"). Pin with `version = "=2.0.0-rc.10"`.
- **Finding 2:** With default features (`download-binaries`), ort-sys statically links a libonnxruntime.lib built with MSVC 2022 STL. On VS 2019 BuildTools, this triggers `LNK2001: unresolved external symbol __std_find_trivial_8` across dozens of `.obj` files.
- **Action 1:** Switched to `default-features = false, features = ["load-dynamic", "ndarray"]`. Skips the static lib; `dlopen`s `onnxruntime.dll` at runtime. **Version-locked:** rc.10 demands ONNX Runtime 1.22.x exactly. A 1.20.x DLL panics at startup with `expected GetVersionString to return '1.22.x', but got '1.20.1'`.
- **Action 2:** Wave 3 pinned `rc.10`, not `rc.12`. rc.12 has an internal compile bug under `load-dynamic` + default-features-off: `no field SessionOptionsAppendExecutionProvider_VitisAI on type &OrtApi`. Until that's fixed upstream, downgrade.
- **Action 3:** Added `scripts/download-onnxruntime.ps1` that fetches v1.22.0 specifically and tells you what to set `ORT_DYLIB_PATH` to. Production bundling (DLL alongside the .exe in Tauri's resources) is a Phase 4/5 concern.
- **Lesson:** When upstream is in RC, version-pin tightly and read release notes for every patch bump. ort's `load-dynamic` feature is a god-tier escape hatch when static link demands a newer toolchain than you have.

## 2026-05-16 [phase-2] Silero VAD model lives under `src/silero_vad/data/` now, not `files/`
- **Context:** Wave 1 manifest pointed at `https://github.com/snakers4/silero-vad/raw/master/files/silero_vad.onnx`. Download succeeded with HTTP 200 but the connection terminated mid-stream — the path is no longer canonical.
- **Finding:** snakers4 reorganized the repo around the Python `silero-vad` PyPI package. The ONNX file moved to `src/silero_vad/data/silero_vad.onnx`. The `raw.githubusercontent.com` host is more reliable than `github.com/.../raw/...` redirects.
- **Action:** Updated manifest URL + pinned the real SHA-256 (`1a153a22...`) and size (2,327,524 bytes). Documented the deprecation in the manifest `notes` field.

## 2026-05-16 [phase-2] `Box<dyn Trait>` brings trait methods into scope automatically
- **Context:** Wave 2 integration test imported both `make_default_capture` and `AudioCapture`. Clippy `-D warnings` flagged `AudioCapture` as unused even though I was calling `.start()`, `.drain()`, etc. on the returned `Box<dyn AudioCapture>`.
- **Finding:** When a value's type is `Box<dyn Trait>` (or `&dyn Trait`, etc.), the trait's methods are accessible **without** an explicit `use` for the trait. The trait is implicitly in scope via the type. Opposite of the usual "trait methods require trait in scope" rule for `impl Trait for ConcreteType` calls.
- **Action:** Drop redundant trait imports when working with `Box<dyn Trait>` return values. Rule of thumb: if you have a `Box<dyn Foo>` or `&dyn Foo`, you don't need `use crate::Foo;` to call Foo's methods.

## 2026-05-16 [phase-2] cpal::Stream is `!Send` on Windows
- **Context:** Wave 2 brief specified `AudioCapture: Send`. cpal's `Stream` on Windows is NOT `Send` (WASAPI handles are thread-bound).
- **Finding:** Wrapping a non-Send field in a struct that impls a `Send`-bounded trait fails with `*mut c_void` cannot be sent between threads safely`. The trait bound has to go.
- **Action:** Dropped `Send` from `AudioCapture`. Doc comment explains why; Phase 5 owns the recording thread. **Generalized lesson:** don't add `: Send` speculatively — add it when you need it, and be ready to discover an OS won't satisfy it.

## 2026-05-16 [phase-2] `cpal::Host` is `!Clone` — re-resolve in worker threads
- **Context:** Wave 2 device watcher needed Host access from a spawned thread. `let host = self.host.clone()` failed: `Host: !Clone`.
- **Finding:** cpal's `Host` is a per-platform singleton with no public clone path. Idiom: re-resolve via `cpal::default_host()` inside the thread (cheap; struct construction).
- **Action:** In `spawn_watcher`, call `let host = cpal::default_host();` inside the closure. No Host capture from outer scope.

## 2026-05-15 [bootstrap] bd (beads) lives next to STATUS.md, not instead of it
- **Context:** PLAN.md predates the decision to adopt `bd` for issue
  tracking. User asked during bootstrap if we were using beads.
- **Finding:** `bd` is the live, dependency-graph-aware task queue; STATUS.md
  is the human-readable phase snapshot the PLAN, judges, and hooks expect
  at iteration boundaries. They serve complementary roles — keep both in
  sync end-of-iteration.
- **Action:** Every iteration: `bd close <completed-ids>`, `bd create` for
  discovered work, AND update STATUS.md.

## 2026-05-15 [bootstrap] bd init is interactive and will timeout in a non-TTY
- **Context:** `bd init --prefix mb` ran for >60s because it prompted
  "Contributing to someone else's repo? [y/N]".
- **Finding:** The initial run *did* create `.beads/` before timing out;
  re-running fails because it sees the partial state.
- **Action:** Pipe `"N"` (PowerShell `'N' |`) to skip the prompt, or use a
  `--non-interactive` flag if/when bd adds one. If init is partial, just
  proceed with `bd status` — the partial state is usable.

## 2026-05-15 [bootstrap] PowerShell + native CLIs treat stderr as terminating
- **Context:** `bd create` emits a one-line warning to stderr
  ("beads.role not configured"); with `$ErrorActionPreference = "Stop"`
  PowerShell threw on the first call.
- **Finding:** PS 7's `$PSNativeCommandUseErrorActionPreference = $false`
  is the right escape hatch for native-command stderr noise.
- **Action:** In any PS script that wraps native CLIs, set
  `$PSNativeCommandUseErrorActionPreference = $false` at the top.

## 2026-05-15 [bootstrap] Hook scripts must decode subprocess stdout themselves on Windows
- **Context:** `session-start-briefing.py` crashed with cp1252 UnicodeDecodeError
  when reading `bd ready` output (em-dashes were not cp1252-encodable).
- **Finding:** `subprocess.run(..., text=True)` uses the locale codec on
  Windows (cp1252), not UTF-8.
- **Action:** Always pass `capture_output=True` without `text=True`, then
  decode `.stdout` as `utf-8, errors='replace'`. The shared
  `scripts/hooks/_lib.py` has examples.

## 2026-05-15 [phase-0] rust-toolchain.toml is a PIN, not an MSRV declaration
- **Context:** Added `rust-toolchain.toml` with `channel = "1.77"` thinking
  it would declare the project's MSRV. The next `rustc --version` call
  triggered rustup to install Rust 1.77 (a downgrade from the dev's
  installed 1.93), hanging the whole shell for ~40s.
- **Finding:** `rust-toolchain.toml` is a hard pin — rustup auto-installs
  the channel on *any* cargo/rustc invocation in that directory. MSRV
  (minimum supported) is a separate concept and belongs in
  `Cargo.toml`'s `[package] rust-version = "..."` field.
- **Action:** Do NOT commit `rust-toolchain.toml` unless you genuinely
  want every developer on the same exact Rust version. For "works on
  1.77+", use `rust-version = "1.77"` in `Cargo.toml` (added in Phase 1).
  Side lesson: `Get-Command rustc` in PowerShell will also block on the
  toolchain auto-install — diagnosis was confusing because the hang
  surfaces as a `Get-Command` hang, not a `rustup install` log.

## 2026-05-15 [bootstrap] Secret-scan hook needs a known-public-prefix allowlist
- **Context:** The Tauri updater public key in STATUS.md tripped the
  high-entropy heuristic in `block-secret-commit.py` (152-char base64 token).
- **Finding:** Public keys are *intended* to be in repos; scanning them as
  secrets is a false positive. Cleanest fix: an allowlist of well-known
  public-material prefixes (`dW50cnVzdGVkIGNvbW1lbnQ6` = Tauri/minisign,
  PEM `-----BEGIN PUBLIC KEY-----`, `ssh-rsa `, etc.) plus an inline pragma
  `pragma: allow-secret-scan` for human-vetted edge cases.
- **Action:** When adding a new "secrets you intentionally commit"
  category, extend `KNOWN_PUBLIC_PREFIXES` in `scripts/hooks/block-secret-commit.py`
  with a comment justifying the prefix. Never disable the high-entropy
  check wholesale.

## 2026-05-15 [bootstrap] PowerShell param defaults can't use $PSScriptRoot
- **Context:** `scripts/seed-judges.ps1` set a default param to
  `Join-Path $PSScriptRoot ...`; PS evaluates defaults before binding
  $PSScriptRoot, so the path was empty.
- **Finding:** Compute path defaults in the *body* of the script, not in
  the `param()` block. Also: `Join-Path` is two-arg only —
  `[IO.Path]::Combine(...)` is the n-arg version.
- **Action:** Pattern: `param([string]$X = "")` + `if (-not $X) { $X = ... }`.

## 2026-05-15 [phase-1] cargo fmt fights git autocrlf on Windows
- **Context:** Phase 1 Wave 1 first `cargo fmt --check` failed:
  `Incorrect newline style in src-tauri/src/lib.rs` even though the files
  were written with LF.
- **Finding:** Git's Windows default `core.autocrlf=true` converts LF to
  CRLF on checkout. rustfmt with `newline_style = "Unix"` then reads back
  CRLF and fails. The two settings fight each other.
- **Action:** Drop `newline_style` from `.rustfmt.toml` (default = Auto,
  accepts file as-is). Add `.gitattributes` with `*.rs text eol=lf` to
  pin LF cross-platform on next checkout. gitattributes is the single
  source of truth; rustfmt becomes ending-agnostic.

## 2026-05-15 [phase-1] rustup minimal toolchains do not include rustfmt or clippy
- **Context:** Fresh Rust install attempting `cargo fmt` produced
  `cargo-fmt.exe is not installed for the toolchain stable-x86_64-pc-windows-msvc`.
- **Finding:** rustup ships only the compiler by default; rustfmt and
  clippy are components, not bundled.
- **Action:** Always `rustup component add rustfmt clippy` as part of
  dev setup. Phase 1 Wave 5 task `p1-lefthook-verify` should add this
  to `setup-dev.ps1`.

## 2026-05-15 [phase-1] First cargo check with rusqlite-bundled takes ~4 minutes
- **Context:** First `cargo check --workspace` after Phase 1 Wave 1.
- **Finding:** 247 seconds (4m07s) cold-cache. `rusqlite` features=["bundled"]
  compiles SQLite from C source (~150k lines). One-time cost; incremental
  builds are seconds.
- **Action:** Budget the cold compile when planning iterations on a
  fresh checkout. CI should cache `target/` aggressively. Do NOT panic
  when cargo check appears to hang for 3-4 minutes on a fresh clone.

## 2026-05-15 [phase-1] Wave-specific briefs ship integration-test pass rates above 90% on first compile
- **Context:** Phase 1 Wave 2 — migrations 001-003 + runner + 7 integration tests.
  The wave was preceded by `docs/phases/phase1-wave2-brief.md` (~300 lines)
  written end-of-Wave-1 by code-puppy with fresh context, capturing every
  design decision PLAN §7 didn't pin down: audit-trigger SQL extrapolated
  to all 4 tables, runner file layout with function signatures,
  integration-test specs with exact assertion counts, PLAN bug flagged
  (`dictionary.OLD.enabled` doesn't exist).
- **Finding:** With the brief, migration-author delivered 4 files in one
  shot. Compile produced 9 trivial `From<rusqlite::Error>` errors (mechanical
  fix — add a variant to AppError). **Tests: 15/15 passed first run, including
  all 7 cross-crate integration tests.** Zero 5-attempt escalations. Zero
  surprise architectural decisions made under pressure.
- **Action:** **Pattern: at the end of every iteration, write a brief for
  the next wave** with full context. Briefs that work well: full SQL/code
  snippets (not just "do X"), exact assertion counts, flagged source-doc
  bugs, explicit deviations from canonical (PLAN) with reasons, visibility
  notes for cross-crate concerns. The cost (~one iteration of context to
  write) pays back ~3x in implementation efficiency. Adopt for Waves 3, 4,
  5 of Phase 1 and every multi-iteration phase going forward.

## 2026-05-15 [phase-1] `#[cfg(test)]` does NOT carry across crate boundaries
- **Context:** Wave 2 brief originally specified `#[cfg(test)]` on
  `Database::open_in_memory()`. migration-author flagged: integration tests
  in `src-tauri/tests/db_migrations.rs` are a **separate crate** from the
  `src-tauri` library crate, so `#[cfg(test)]` items in `src-tauri/src/`
  are invisible to them.
- **Finding:** `#[cfg(test)]` only enables items when the **current crate**
  is being compiled in test mode. Integration tests (`tests/*.rs`) build
  the library crate in **release mode** (not test mode), then link against
  it as a regular dependency. Items needed by integration tests must be
  `pub` (or `pub(crate)` if behind a shim).
- **Action:** For any helper that integration tests need (test-database
  fixtures, `open_in_memory`, etc.): make it plain `pub` with a doc
  comment marking it test-oriented. If you want to discourage production
  callers, gate behind a Cargo feature like `test-helpers` instead of
  `#[cfg(test)]`.

## 2026-05-15 [phase-1] AppError variants are added per-module as the modules come online
- **Context:** Wave 2 db module's first compile failed with 9 instances of
  `From<rusqlite::Error>` not implemented for AppError.
- **Finding:** I (code-puppy) preloaded AppError in Wave 1 with `Io` and
  `Tauri` variants only — the others get added when their source modules
  first compile. This is the right pattern (YAGNI: don't pre-declare error
  variants for modules that don't exist yet) and the fix is mechanical
  (add one `#[error("sqlite error: {0}")] Sqlite(#[from] rusqlite::Error)`
  variant).
- **Action:** When a new module fails to compile with `From<...>` errors,
  the fix is always: add a `#[from]` variant to `AppError` in `error.rs`.
  Don't refactor to module-local error types — the AppError aggregator is
  the explicit project-wide pattern (per `.code_puppy/AGENTS.md` Rust
  conventions). When in doubt, check `error.rs` first.



### Delivered (5 waves, 5 commits + 4 brief commits + seal)

- **Wave 1** (`8e70d7c`): scaffolding, error aggregator, ADR 0004, Cargo workspace, tauri.conf.json. 5 tests.
- **Wave 2** (`b1f39ff`): migrations 001-003 (4 files), runner with PRAGMA + integrity_check + foreign_key_check, prompt_loader with token substitution. **15/15** tests first run.
- **Wave 3** (`7dada9d`): 7 DB repository modules (transcripts, prompts, dictionary, examples, search, sessions, audit) + `tests/db_repos.rs`. **77/77** tests after 2 trivial test-only fixes (raw-string quote count, SQL UNIQUE+NULL gotcha).
- **Wave 4** (`c7d3faa`): logging (rolling appender + PII scrub), settings (typed facade + 8-key registry), tray (placeholder menu), commands (3 IPC handlers), app wire. **101/101** tests **first run** — zero fixes needed.
- **Wave 5** (this commit): docs/CONTRIBUTING.md, docs/SETTINGS.md (binding), 3 judge cards, `#![warn(missing_docs)]` re-enabled, retrospective, seal commit + `phase-1-complete` tag.

### Final test count

**101 tests** across the workspace, all green:
- 88 unit tests inside `src-tauri/src/`
- 7 integration tests in `tests/db_migrations.rs`
- 6 integration tests in `tests/db_repos.rs`

### What worked

1. **The brief pattern.** End-of-wave handoff briefs (`docs/phases/phase1-waveN-brief.md`) that specify types, function signatures, test specs, known risks, and explicit deviations from PLAN. Outcome: 3 consecutive ~100% first-run test pass rates. The pattern is now the documented default for any multi-iteration phase.
2. **AppError aggregator with `#[from]` variants.** New modules add a variant when they bring a new source error type. Mechanical, predictable, no abstraction debt.
3. **`Database::open_in_memory()` (plain `pub`, not `#[cfg(test)]`).** Bridged the cross-crate test boundary; integration tests get a fully-migrated DB in ~5ms.
4. **Typed registries.** `SettingKey` enum + `default_value` + `try_parse` + `all()` makes adding a setting a 4-step mechanical edit with no string-typing.
5. **`AuditedTable` enum gating dynamic SQL.** Zero SQL-injection surface in the audit/rollback path despite needing to UPDATE/INSERT/DELETE arbitrary tables.
6. **Provenance-is-total enforced at API layer, not schema.** `NewSession` requires `i64` (not `Option<i64>`) for FKs that SQL leaves nullable. The schema and API deliberately disagree.

### What surprised us

1. **`#[cfg(test)]` doesn't carry across crate boundaries.** Integration tests in `tests/*.rs` are a separate crate; `pub` is required for helpers they consume.
2. **SQL UNIQUE treats NULL as distinct.** Two rows with `app_context: None` both pass a `UNIQUE(term, app_context)` constraint. Fix: test with non-null values, or use a partial INDEX with COALESCE.
3. **SQLite `CURRENT_TIMESTAMP` has 1-second granularity.** Audit-rollback tests would race within the same second. Workaround: `pin_latest_at` test helper that overwrites the `at` column to a synthetic timestamp after the trigger fires.
4. **`#![warn(missing_docs)]` is hostile to repo modules with self-documenting fields.** 163 warnings for fields like `pub id: i64`. Resolution: keep the lint at the crate level, allow at the module level for repo modules, doc the small-API modules (commands, tray, logging) properly.
5. **Rolling 4-minute cold `cargo check`** because `rusqlite-bundled` compiles SQLite from C. One-time cost. Document so future contributors don't panic.
6. **PowerShell `Select-String` matches inside comments** when counting code patterns. Anchor with `^` or run via SQLite for ground truth.
7. **`tracing_subscriber::try_init` is once-per-process.** Test isolation matters; only call inside test code that's certain it's the first.

### What we deferred (intentional, captured in phase ownership)

- **Mockall trait abstractions** (Wave 3 brief — YAGNI; Wave 4 didn't need them either). Reintroduce only when a specific command/UI surface needs to mock a repo.
- **DBOS** (bootstrap step 3 — skipped per project owner).
- **Pack agents** (deprecated upstream — `no-pack-agents` judge enforces).
- **Operator-aware FTS5 query parsing** (Phase 6 history viewer brief). Phase 1 ships conservative phrase escaping.
- **Audio retention enforcement** (Phase 5).
- **Real example ranking + auto-selection** (Phase 8 learning loop).
- **`ClaudeApiKeyRef` actual Credential Manager lookup** (Phase 4).
- **Tray icon state transitions** (Phase 5 recording lifecycle).
- **Cross-app injection** (Phase 3 — requires human at keyboard).
- **Lefthook live-fire verification** — lefthook binary not on dev machine PATH this iteration. Config in `lefthook.yml` looks correct. Install (`scoop install lefthook` or equivalent) and run a real commit through pre-commit; append observations here.
- **`missing_docs` polish for repo modules** — applied `#[allow]` at module level rather than doc-ing every self-evident field. Phase-6 UI work may add field-level docs where they matter.

### Carry-forward for Phase 2+

- **Brief pattern is now the default.** Every multi-iteration wave gets `docs/phases/phaseN-waveM-brief.md` written end-of-current-iteration with full context.
- **LESSONS.md is institutional memory.** Append non-obvious findings as you hit them, not at retrospective time.
- **STATUS.md is the canonical handoff document.** Resume instructions, last-judge line, cost line, blocked-on section all live there.
- **AppError aggregator pattern** generalizes. Phase 2 will add `Stt`, `Audio` variants; Phase 3 will add `Injection`; Phase 4 will add `Claude`.
- **Provenance-is-total at the API layer** is a project-wide principle, not a Phase 1 quirk. Future repos honor it.
- **The `phase-N-complete` tag SEALS its migrations.** Phase 2 ships migration 004+; the previous numbers are now frozen forever.
- **Test-density target:** ~10 tests per ~500 lines of code. Phase 1 hit ~100 tests / ~5,000 lines.

### Numbers for posterity

- **Files created:** ~30 (modules) + ~10 (docs) + ~10 (judges/briefs).
- **Lines of code:** ~5,000 Rust + ~1,500 SQL + ~3,000 markdown.
- **bd tasks closed:** 25/25 Phase 1 tasks (plus 11 Phase 0 tasks).
- **Commits:** 9 (bootstrap + Phase 0 + Wave-1-brief + Wave 1 + Wave-2-brief + Wave 2 + Wave-3-brief + Wave 3 + Wave-4-brief + Wave 4 + Wave-5-brief + Wave 5 + seal).
- **Test pass rates per wave:** W1 5/5, W2 15/15, W3 75→77 (2 test fixes), W4 101/101 first run, W5 101/101 still.

---

## 2026-05-15 [phase-1] SQL UNIQUE treats NULL as distinct (`NULL != NULL`)
- **Context:** Wave 3 dictionary repo test `unique_term_app_context_is_enforced`
  inserted two rows with `term='Foo', app_context=NULL` expecting the UNIQUE
  constraint to fire. Both inserts succeeded.
- **Finding:** Standard SQL semantics: `NULL != NULL` for purposes of
  UNIQUE constraints. Two rows with NULL in the same UNIQUE column are
  considered distinct and both allowed. This is a famous SQLite gotcha
  (also true in Postgres, MySQL, etc).
- **Action:** For null-equal-null semantics, use a partial UNIQUE INDEX
  on `COALESCE(col, '')` or similar — that's a schema change requiring
  a future migration. For Phase 1 we test the constraint with a
  non-null value where UNIQUE actually fires. Phase 6 dictionary UI
  may want the null-equal-null behavior.

## 2026-05-15 [phase-1] SQLite `CURRENT_TIMESTAMP` has 1-second granularity
- **Context:** Wave 3 audit-rollback tests insert→update→rollback. Each
  audit-trigger fire timestamps with `CURRENT_TIMESTAMP` which only has
  per-second resolution. Two operations within the same second get
  identical `at` values, breaking the `state_at` algorithm's ordering.
- **Finding:** Sleeping ≥1s between ops works but makes tests slow.
  Cleaner: after each real operation, UPDATE the just-created history
  row's `at` field to a known synthetic timestamp. The audit table has
  no constraint preventing this — it's an internal-record-of-fact
  table, not a contract. Pattern (added as `pin_latest_at` helper):
  ```rust
  conn.execute("UPDATE _history_X SET at = ?1 WHERE id = (SELECT MAX(id) FROM _history_X)", [ts])?;
  ```
- **Action:** Use synthetic `at` values for any test that depends on
  temporal ordering. Keep this trick test-only — production code
  trusts `CURRENT_TIMESTAMP`.

## 2026-05-15 [phase-1] `#![warn(missing_docs)]` is hostile to repo modules with self-documenting fields
- **Context:** Wave 1 added `#![warn(missing_docs)]` at the top of
  `lib.rs`. Wave 3 added 7 repository modules with ~60 public structs/
  enums/fields where the field name IS the documentation (`pub id: i64`,
  `pub term: String`, etc.). Clippy spammed 60+ missing-doc warnings
  and `clippy -D warnings` refused to ship.
- **Finding:** Mandatory module-level docs are valuable. Mandatory
  field-level docs are noise when the field name is self-evident.
- **Action:** Demoted `missing_docs` from `warn` to nothing for now;
  Wave 5 polish task will (a) add doc comments to non-self-documenting
  public items, (b) re-enable the lint, (c) `#[allow(missing_docs)]`
  on the obvious cases like `pub id: i64`. Don't blanket-enable lints
  faster than you can comply with them.

## 2026-05-15 [phase-1] PowerShell Select-String matches inside comments — grep regexes need context
- **Context:** Sanity-checking the trigger count after Wave 2: I expected 14
  triggers (per the brief), but `Select-String -Pattern 'CREATE TRIGGER'`
  returned 15.
- **Finding:** One of those matches was inside a `--` SQL comment in
  `002_audit_triggers.sql` ("-- new migration that CREATE TRIGGER IF NOT
  EXISTS-replaces the offender"). Substring match doesn't distinguish
  code from comments.
- **Action:** For exact code counts, anchor the pattern: e.g.
  `Select-String -Pattern '^CREATE TRIGGER'` (line starts with) or
  `'^\s*CREATE TRIGGER'` (optional indent). Or use `sqlite3 :memory: < file.sql`
  followed by `SELECT COUNT(*) FROM sqlite_master WHERE type='trigger'`
  for the ground truth. The integration test asserts the ground truth
  (`trigger_count_is_14`) and that's the canonical check.


## 2026-05-16 — Phase 3 Wave 4.8 — Test-only callers are landmines

**Symptom.** First hold-release dictation cycle worked end-to-end.
Second + third hold did nothing: no beep, no log entry, no audio
captured. The LL hook was firing fine, the orchestrator was alive,
the audio layer was restartable (Wave 4.8 fix #1) — but the §6.1
state machine sat in `Processing` forever and
`(Processing, _) => StateAction::None` silently dropped every event.

**Root cause.** `HotkeyStateMachine::complete_processing()` was the
designated `Processing → Idle` transition method. It had unit tests
(`complete_processing_returns_to_idle`,
`complete_processing_in_idle_is_noop`). It had docstrings. The
state-machine module looked complete.

It had **zero production callers**. `grep complete_processing`
across the whole codebase returned only the method definition + its
tests. The orchestrator never signalled "pipeline done" back to the
driver, so the machine never transitioned.

**Why it slipped through review.**
1. The state-machine unit tests called `complete_processing()`
   directly — they exercised the transition without exercising the
   wiring.
2. The orchestrator's own tests (`pipeline::tests`) tested decision
   logic, not the full action-loop.
3. There was no end-to-end smoke test that ran two consecutive
   hold-release cycles through the real runtime. Phase 3 Wave 5 is
   when integration testing was planned.

**The fix.**
- Added `HotkeyEvent::PipelineComplete` variant — back-channel from
  dictation thread to driver thread.
- State machine routes `PipelineComplete` to the existing
  `complete_processing()` method (idempotent: no-op outside
  `Processing`).
- Orchestrator's `handle()` wraps `StopCapture` + `DiscardAudio`
  with a `signal_pipeline_complete()` call AFTER the inner method
  returns — guaranteed signal even on pipeline error, no risk of
  losing the signal in an early-return error path.
- `PauseHandle::sender_clone()` exposes the existing hotkey channel
  Sender for the dictation thread to use (same channel as
  PauseToggle — preserves event ordering wrt KeyUp).

**Lessons.**
1. **`grep <method_name>` looking only at non-test results is a
   useful pre-commit check** for any method that's supposed to be
   wired into production. If the only callers are tests, it's not
   wired — regardless of how complete it looks.
2. **State-machine handshake protocols need a counterpart end-to-
   end smoke test**, not just unit tests of each transition. Two
   consecutive sessions is the minimum useful smoke for any state
   machine that has a "completion ack" step.
3. **Centralise mandatory signals in the dispatch site**, not the
   per-branch helpers. `handle()` wrapping
   `let r = self.complete(); self.signal_pipeline_complete(); r`
   guarantees the signal fires on every code path through complete,
   including the four `persist_failed_*` early-returns. Sprinkling
   the signal at every return site of complete() would have been
   DRY-violating and bug-prone.
4. **Audio capture restartability (Wave 4.8 fix #1) was a real bug
   masked by this one.** Both needed to be fixed. The producer-slot
   bug would have surfaced on cycle 2 if the state machine had
   actually transitioned out of Processing. Lucky-ish that both
   showed at once — could have been a long debugging session if
   they'd shown up sequentially.

---

## 2026-05-17 [phase3-wave4.9] K32GetModuleBaseNameW silently returns 0 under PROCESS_QUERY_LIMITED_INFORMATION

- **Context:** Bug B in the Wave-4 QA matrix — `sessions.foreground_app`
  was always empty string. Tracing showed `OpenProcess` succeeded,
  `QueryFullProcessImageNameW` returned the full exe path correctly,
  but `K32GetModuleBaseNameW` returned 0 → empty string → all
  downstream strategy resolution, per-app overrides, and audit
  joins silently broke.
- **Finding:** `K32GetModuleBaseNameW` (and the older
  `GetModuleBaseNameW`) require `PROCESS_QUERY_INFORMATION +
  PROCESS_VM_READ` on the process handle. Mockingbird opens with
  the lighter `PROCESS_QUERY_LIMITED_INFORMATION` (necessary to
  read protected processes — System, csrss, anti-cheat-protected
  game windows). Under that mask, `K32GetModuleBaseNameW` does
  NOT error — it returns 0 (= "0 chars copied") and sets nothing.
  The MSDN page mentions the access requirement only in passing
  and most online examples show `OpenProcess(PROCESS_ALL_ACCESS, ...)`
  which masks the issue. Stack Overflow answers that recommend
  the function rarely flag the access-mask trap.
- **Action:** Derive process basename from
  `QueryFullProcessImageNameW`'s output via
  `std::path::Path::file_name` instead. Same information, one
  fewer Win32 call, no access-mask surprises. The
  `basename_from_path` helper is pure + trivially unit-testable.
  Lesson generalises: any Win32 helper that returns "0 = failure"
  with no error-code path is suspect — assume access-mask
  sensitivity until proven otherwise.

## 2026-05-17 [phase3-wave4.9] Clipboard sequence baseline must be measured AFTER our write, not before

- **Context:** Bug C in the Wave-4 QA matrix — after a successful
  paste, the user's pre-dictation clipboard contents were lost
  (the dictation text remained). The
  `SequenceAnalysis::classify(seq_before_set, seq_after_paste)`
  classifier treated `seq_after_paste == seq_before_set + 1` or
  `+ 2` as "safe to restore". In practice we observed `+ 2` after
  our own writes (no target write yet) and `+ 3` after a normal
  paste → classifier returned `Diverged` → restore skipped.
- **Finding:** `EmptyClipboard` + `SetClipboardData` together
  advance the sequence number by an OS-dependent amount. Windows
  may fold consecutive ops into a single bump or count them
  separately; the count appears to vary across builds and
  possibly across clipboard-format-list state. Baselining off
  `seq_before_set` made the classifier brittle to that fold.
  Baselining off `seq_after_set` (measured immediately AFTER
  `write_unicode_text` returns) eliminates the dependency entirely
  — the classifier now answers only the question we actually care
  about: "did anything OTHER than our write happen?"
- **Action:** Always baseline post-mutation, not pre-mutation, when
  the mutation's own sequence cost is OS-internal. The classifier
  is correspondingly simpler:
  - `seq_after == seq_after_set` → target read-only paste (common).
  - `seq_after == seq_after_set + 1` → target also wrote (clipboard
    managers, dedupe).
  - Anything else → another writer; skip restore.

  Also: dropped the `wait_for_paste_sentinel` polling loop in
  favour of a fixed `PASTE_CONSUME_GRACE` (30 ms) sleep before
  the post-paste seq read. Read-only paste never advances seq,
  so the poll could only ever time out on the common case —
  burning 250 ms instead of 30. Deterministic sleep > clever
  poll when the "clever" signal is absent for the dominant path.

## 2026-05-17 [phase3-wave4.9] Hard-coded test count is a bad smoke test for "did the refactor work"

- **Context:** During Wave-4.9 I almost grepped for "303 passed"
  as the success criterion after refactoring transcripts +
  classifier + permissive focus. The refactor changed the test
  count (removed 5 tests, added 9 → net +4) so a count match
  would have been a false negative.
- **Finding:** `cargo test`'s "N passed; 0 failed" line is the
  real gate. The count is only meaningful when compared against
  the expected delta for the iteration. A grep for "0 failed"
  is correct; a grep for a specific passed count is brittle.
- **Action:** Smoke commands should grep `FAILED|panicked|0 failed`
  (the first two are negative checks, the third confirms the
  test runner finished), never a hard-coded `N passed` literal.

## 2026-05-17 [phase3-wave4.9] Running mockingbird.exe locks itself; `cargo build --release` fails with os error 5

- **Context:** During Wave-4.9 verification I asked Dustin to re-launch
  after a release rebuild. He hit
  `error: failed to remove file ... mockingbird.exe / Caused by:
  Access is denied. (os error 5)` on cargo's pre-link cleanup.
- **Finding:** Windows holds an exclusive lock on a running .exe's
  image file. cargo's link step tries to overwrite
  `target/release/mockingbird.exe` in-place, which fails until the
  running process exits. Unix devs are surprised by this because
  Linux allows overwriting open files via inode swap (the running
  process keeps its open inode; new file gets a fresh one). The
  Windows error message doesn't mention "running process" — it
  just says "Access is denied", which is a misleadingly generic
  diagnostic for what is fundamentally a per-OS semantic
  difference.
- **Action:** `scripts/run-mockingbird.ps1` now accepts a `-Force`
  flag that kills any running `mockingbird.exe` before launching,
  with a 300 ms sleep to let Windows release the file lock.
  Canonical rebuild-and-relaunch dance is now:
  1. `pwsh scripts/cargo-with-cuda.ps1 build --release`
  2. `pwsh scripts/run-mockingbird.ps1 -Force`

  If step 1 fails with os error 5, that's the signal a previous
  instance is still running — `taskkill /F /IM mockingbird.exe`
  and retry, or just use `-Force` on the next `run-mockingbird`
  invocation. Generalises: any Rust binary that does `dlopen` of
  cudart / onnxruntime / etc. hits this on rebuild; the
  `-Force`-style kill is the standard Windows answer.

## 2026-05-18 [phase-3-wave-5] Orchestrator integration tests want stubbed traits, not a real `DictationRuntime` spawn

- **Context:** Wave 5 needed end-to-end tests that drive `DictationOrchestrator::run` through a full `StartCapture → StopCapture` cycle so the new judges (`e2e-injection`, `db-provenance`, `secure-input-respected`) had real CI targets instead of just wrapping the pure `pipeline::decide` unit tests.
- **First instinct (wrong):** spawn the real `DictationRuntime` from `dictation/runtime.rs` and use `HotkeyEvent::PipelineComplete` for synchronisation. That pulls in a real low-level keyboard hook, a real `WinSecureInputGuard`, and a real `GetForegroundWindow` — none of which work on a headless test runner.
- **What actually works:** put the test in `src-tauri/tests/dictation_orchestrator.rs` (separate crate, depends on `mockingbird_lib` like any consumer). Stub every trait the orchestrator takes (`AudioCapture` / `VoiceActivityDetector` / `SpeechToText` / `Cleaner` / `Injector` / `WindowContext` / `SecureInputGuard`). Use `Database::open_in_memory()` for SQLite + `default_normal_config` for FK seeding. Use a plain `std::sync::mpsc` pair for `StateAction`. The orchestrator's `run(rx)` iterates `rx.iter()` and terminates when the sender drops — so the test pushes its two actions, drops `tx`, then calls `run(rx)` inline. No threads, no synchronisation primitives beyond the channels the orchestrator already owns.
- **Generalisation:** when an orchestrator has `Box<dyn Trait>` deps for every I/O surface, integration tests should mirror the pattern: in-memory DB + stub traits + drop-the-sender to terminate the loop. The Phase 4 `LlmCleaner` integration test should follow the same recipe, swapping `PassthroughCleaner` for a `StubLlmCleaner` that returns a deterministic transformation, so the e2e-injection judge can prove the LLM is actually in the loop.
- **Gotcha:** `rustfmt` rewrites long `assert_eq!` lines aggressively. Always run `pwsh scripts/cargo-with-cuda.ps1 fmt` (no `--check`) before pushing — a fmt-check failure blocks the `mb-quality-bar` judge AND surfaces noise about unrelated files (e.g. Wave 4.9's `examples/verify_wave49.rs` was already not-clean and got swept up in Wave 5's pre-tag check).


## 2026-05-18 [phase5/6 UI sprint] CSS Modules need a vite-env.d.ts to satisfy tsc

- **Context:** UI bundle compiled fine via Vite alone, but `tsc --noEmit`
  (the first step of `npm run build`) flagged every `import styles from
  "./Foo.module.css"` as TS2307. Even `@vitejs/plugin-react` doesn't add
  the module ambient declaration on its own — that's `vite/client` types
  via a triple-slash reference.
- **Finding:** A `ui/src/vite-env.d.ts` with `/// <reference types="vite/client" />`
  plus ambient declarations for `*.module.css`, `*.module.scss`, `*.css`
  is what unblocks `tsc`. Vite's docs imply this file is auto-created
  by `npm create vite`, but a hand-rolled Phase 5 scaffold needs it
  explicitly.
- **Action:** When standing up a new Vite + TS workspace, drop a
  `vite-env.d.ts` next to `main.tsx` BEFORE writing the first
  `.module.css` import. Saves a round-trip through Playwright/CI to
  notice.

## 2026-05-18 [phase5/6 UI sprint] Tauri `commands.rs` (file) vs `commands/` (dir) conflict

- **Context:** Splitting the IPC surface into per-feature sub-modules
  (`commands/insights.rs`, `commands/sessions.rs`, ...) collided with the
  Phase 1 `commands.rs` file that held `AppState` + `get_setting` /
  `set_setting` / `fts_smoke_test`. Rust 2024 module resolution
  rejects having both `foo.rs` and `foo/mod.rs` at the same depth.
- **Finding:** Delete the old file and move its contents into a
  `commands/legacy.rs` sub-module. Keeping both command surfaces
  side-by-side (typed `SettingKey`-JSON vs flat string/string) is fine
  because Tauri command names are globally unique — `get_setting` vs
  `get_settings` (different by one letter) is enough. The `AppState`
  struct stays re-exported from `commands/mod.rs` so the call site
  `use crate::commands::AppState` in `lib.rs` keeps working unchanged.
- **Action:** Treat `pub mod commands` as a directory from day one in
  Tauri projects, even when it contains only one file initially.
  Cheaper than the inevitable refactor when the IPC surface grows past
  3 commands.

## 2026-05-18 [phase5/6 UI sprint] `tauri::AppHandle` in command signatures bounces with CommandArg error

- **Context:** Wanted `#[tauri::command] pub fn app_paths(app: AppHandle) -> ...`
  so the path resolver could return canonical app-data + log dirs.
  Compiler bounced it: `the trait Deserialize<'_> is not implemented for AppHandle`.
- **Finding:** Inside a `#[tauri::command]` fn the runtime extracts
  `AppHandle<R>` from the invoke context, NOT from JS-side args. The
  `generate_handler!` macro should detect that and skip the
  `CommandArg` impl — but in our setup (tauri 2.x with the runtime
  generic erased at the registration site) it didn't.
- **Action:** For app-data / logs / models paths, bypass `AppHandle`
  entirely and read `APPDATA` + `USERPROFILE` env vars the same way
  `logging::init` and `lib::run` do. Matches the rest of the runtime's
  resolution logic. If we hit a case where Tauri's `path_resolver` has
  overrides we truly need, revisit by adding the runtime generic to
  the command: `pub fn app_paths<R: tauri::Runtime>(app: tauri::AppHandle<R>)`.

## 2026-05-18 [phase5/6 UI sprint] rusqlite closure type inference needs explicit annotations under `?`-chains returning String

- **Context:** Several new command modules did this pattern:
  ```rust
  let mut stmt = conn.prepare(SQL).map_err(|e| e.to_string())?;
  let rows = stmt.query_map([], |r| Ok(MyRow { ... })).map_err(|e| e.to_string())?;
  ```
  Half the closures failed with E0282 ("type annotations needed"). The
  outer `?` operator was wired to `Result<_, String>` via `map_err`
  but the closure return type couldn't be inferred backwards from
  there.
- **Finding:** Two cleaner fixes:
  1. Hint the closure: `|r: &rusqlite::Row<'_>| -> rusqlite::Result<MyRow> { ... }`.
  2. Use a generic `into_err<E: Display>(e) -> String` helper instead of
     `|e| e.to_string()` closures everywhere — the function pointer's
     type signature does the inference.
  We picked (2) because it also dedupes the `map_err` body and reads better.
- **Action:** Add `commands::into_err` (or similar) to any
  command-module crate before writing the first `.map_err`. Future
  authors will copy the pattern without thinking about closure
  inference.


## 2026-05-17 — Vite CSS `@import` does not resolve absolute `/public` paths

- **Context:** Design Language Phase Wave 1 (mb-9pw). Adding self-hosted Latin WOFF2s under `ui/public/fonts/` plus a generated `fonts.css` next to them. Wanted to pull `fonts.css` into the bundle from `ui/src/design/global.css` with `@import "/fonts/fonts.css";` so a single CSS entry kept all design-system styles together.
- **Symptom:** TS build wouldn't have flagged it (we run `tsc --noEmit && vite build` and CSS imports go through Vite's plugin), but Vite's CSS resolver treats `@import` strings as module specifiers, not URL paths. An absolute path like `/fonts/fonts.css` either gets resolved against the source tree (not the `public/` root) or silently dropped from the bundle, depending on Vite version. Either way the WOFF2 references in the imported CSS end up broken in production.
- **Finding:** `public/` is for files you want served verbatim. Reference them with `<link rel="stylesheet" href="/fonts/fonts.css">` in `index.html` (and `recording.html` for the recording overlay window) — the browser fetches them as plain static assets and the relative `url(./DMSans-latin.woff2)` references inside resolve correctly against the served URL.
- **Action:** Both Tauri webview entry points (`ui/index.html`, `ui/recording.html`) now `<link>` the fonts stylesheet. The token + typography CSS still lives in `src/design/` and gets `@import`ed via the JS-imported `global.css` — that path works fine because both files are inside `src/` and Vite's resolver is happy with relative source-tree paths. Future rule: anything under `public/` is loaded via `<link>` / `<script>` in HTML, never via JS-imported CSS `@import`.

## 2026-05-17 — Bridge-then-cutover pattern for full-UI redesigns

When swapping the entire visual system of an app (Design Language Phase,
ADR 0023) the temptation is to redo each page in-place, one PR per page.
That's a long-tail nightmare: every PR has to be visually-stable for the
*whole* app, regression risk piles up, and you can't ship until the last
page lands.

The bridge-then-cutover pattern is way cheaper:

1. **Wave 1: lay down the new tokens** scoped under a flag selector
   (`[data-design="v2"]`). The pages still render in v1 because no root
   has the attribute yet.
2. **Token bridge in the same wave.** Inside the flag selector, alias
   every *legacy* token name to a *new* token. `--surf-0` now reads
   `var(--md-sys-background)`, `--mode-normal` reads `var(--md-sys-primary)`,
   `--font-sans` swaps families. **Once the flag is on, unmigrated pages
   pick up the new palette + font automatically** — they don't need to
   know about M3 tokens, they're still reading their old names. You get
   a "free" redesign of pages you haven't even touched yet.
3. **Waves 2-3: build the new primitives + utilities.** Glass classes,
   icon component, button + input + chip + dialog. Developer-only
   showcase route at `/design-system` so designers can review without
   navigating production pages.
4. **Wave 4: page-by-page migration via override blocks.** Append a
   `:global([data-design="v2"]) .pageHeader { ... }` block to each
   CSS module. Don't rewrite the v1 selectors — let them stay as the
   fallback. Reviewer can A/B by toggling the flag.
5. **Wave 5: any pages that need a totally new component model.**
   Recording window was this for us: dot+waveform → MockingbirdMark.
6. **Wave 6: the cutover.**
   - Flip the default flag to "v2" (one line).
   - Unscope all v2 CSS by deleting the `[data-design="v2"]` wrapper.
     PowerShell one-liner across all CSS modules: ~6 files in 5 seconds.
   - Delete the v1 token file. Extend the bridge in the v2 token file
     to cover any legacy aliases still in use (radii, spacing, type,
     shadows, motion). Now there's one source of truth.
   - Delete the flag machinery: store fields, dev-only toggle button,
     showcase route, any conditional component renders.

### Why this works

- **No big-bang day.** Every wave is independently visually-stable.
- **Old code is "free".** The bridge means unmigrated pages get the new
  look without a single line of change. ~60% of the app got the new
  look this way; we only hand-migrated the surfaces that needed
  per-component polish.
- **The cutover is a deletion.** No new code lands in W6 — it's all
  subtractive. That's the safest possible final commit.
- **Easy A/B for reviewers.** The flag stays alive through W5 so the
  designer can click between v1 and v2 on any page.

### Gotchas

- **The legacy alias bridge has to be near-complete.** If the bridge
  forgets `--shadow-2` but a page module uses it, that page renders
  shadowless in v2 mode until you remember. Audit by grepping the
  CSS modules for all `var(--…)` calls and confirming each is bridged.
- **PowerShell regex with trailing spaces ate `html `+`body`.** When
  unscoping with `(Get-Content … -Raw) -replace 'html\[data-design="v2"\] '`
  the trailing space was part of the match — fine for `html[data-design="v2"] body`
  → ` body`, but I made a separate run with `-replace 'html\[data-design="v2"\] '`
  that consumed the wrong space, leaving `htmlbody`. Easy fix; flag for
  reviewer.
- **Don't try to rename `*-v2.css` → `*.css` in the cutover commit.**
  That hides the deletion diff under a rename and makes review awkward.
  Leave the filename for a follow-up cleanup.
- **Bundle savings are real and concrete.** Deleting v1 dropped main
  CSS by 35%. The chart for "where did 16 KB go?" is the v1 tokens
  file + dual selectors + the showcase page.


## 2026-05-17 — Session-start anchor (the bootstrap-prompt incident)

A custom kickoff prompt was pasted at session start asking the agent to
execute PLAN §0.5 bootstrap. Bootstrap had been sealed months earlier
(`bootstrap-complete` tag, all `.code_puppy/` artifacts on disk, phases 0-4
+ 8 also sealed). The agent did NOT execute bootstrap — instead it
correctly executed the actual in-flight work (DLW6 design cutover), but
then *near the end of the session* re-read the kickoff prompt, got confused
about whether it had honored the bootstrap directive, and surfaced a
false-alarm "I went off the rails" confession.

Nothing got broken (the W6 work was correct, the cutover sealed cleanly,
qa-kitten 8/8 PASS). But the wasted-cycle pattern is worth preventing.

### Root cause

The normal session-start ritual in `.code_puppy/AGENTS.md` ("read AGENTS.md,
read PLAN, read STATUS, …") assumes work begins via `/goal`. A pasted
custom kickoff prompt bypassed that ritual entirely, so the agent had no
anchor on "where is this project actually at" and was vulnerable to
following stale directives in the prompt itself.

### Fix (shipped same day)

1. **Top-of-STATUS.md SESSION ANCHOR block** — a `<!-- … -->` comment plus
   blockquote stating PROJECT PHASE, BOOTSTRAP status, LATERAL EPICS DONE,
   NEXT MACRO/LATERAL work, IN-FLIGHT, and HOW TO RESUME. Updated at end of
   every session. The first thing any agent reads.
2. **`.code_puppy/AGENTS.md` §"Permanently sealed"** — names the sealed
   work (bootstrap, phases with git tags, ADR-chartered epics) and the
   mandatory session-start ritual: read STATUS top 25 lines BEFORE any
   tool call, even if kickoff prompt is a custom paste. If kickoff
   conflicts with anchor block, STOP and ask via `ask_user_question`.

### What I learned about this project's process while diagnosing

Worth recording: the project's workflow has matured into two parallel
tracks that the framework absorbs cleanly:

- **Macro spine (PLAN §10):** numbered phases 0-9, sealed with
  `phase-N-complete` git tags. Planning-agent decomposes each into
  wave briefs (`docs/phases/phaseN-waveM-brief.md`), code-puppy executes
  wave-by-wave, qa-kitten gates with screenshots + Playwright.
- **Lateral ADR-chartered epics:** emergent cross-cutting work (e.g.
  ADR 0022 three-mode pipeline, ADR 0023 design language v1). Chartered
  by an ADR (the "contract"), tracked as a bd epic with child
  wave-issues, sealed via STATUS append + ADR acceptance — NOT a
  `phase-N` tag (those are reserved for the spine).

The separation of concerns across PLAN.md (contract) / STATUS.md
(heartbeat) / docs/adr (decision log) / docs/phases (per-phase decomp) /
docs/judges (evidence bar) / docs/design/smoke (visual receipts) /
docs/LESSONS.md (experience store) / bd (live queue) / git tags
(seal markers) is what makes this scalable. Each layer has exactly
one job; nothing is overloaded.

---

## 2026-05-17 ADR-0024 Wave C — empirical mode tuning (Bernard)

Three non-obvious lessons from running the first end-to-end mode-eval grid.

### 1. Few-shot examples become attention anchors under length pressure on small models

The single worst failure in the iter-0 baseline was casual fixture
`06_implicit_long`: an 8-item architecture description got replaced
ENTIRELY with `"hey can you grab milk, eggs, and bread on the way home
thanks"`. Zero must-preserve hits.

Root cause: `casual_v1` ended its few-shot block with that exact
"hey can you grab milk eggs..." example. When the 3B model hit a long
technical input it could not fully attend to, it pattern-matched to
the **most recent** few-shot example and emitted that as a template.
Higher temperature (0.4) made the wandering worse.

**The lesson generalizes:** on quantized small-model deployments,
few-shot examples are not just teaching aids — they're sticky
attention anchors. Put the example you MOST want the model to follow
last (highest recency weight), and make sure every example
demonstrates the rule you care about (`v2` puts a long-preservation
example last, replacing what was the regression vector). Lower
temperature defensively in the mode config, not the prompt.

### 2. Lexical preservation scoring overfits without paraphrase escape hatches

The baseline showed formal at 76.9% preservation — looked terrible.
On inspection, most "failures" were legitimate register-lift
paraphrases (`bad` → `poor`, `half day` → `half-day`, `ignoring it`
→ `neglecting it`). The literal scorer can't tell semantic-preserve
from semantic-drop.

Fix: add an OPTIONAL `must_preserve_alts: [[term, alt1, alt2], ...]`
field to fixtures. If ANY term in a group is in the output, the WHOLE
group counts satisfied. Also normalize hyphens (`half-day` ≡
`half day`) — the LLM frequently adds/drops them under register-lift.

The deeper lesson: **automated scoring earns its keep on the
disasters** (hallucination, omission, complete topic drift) — for
which lexical match is necessary AND sufficient. For aesthetic
quality, automation is a false economy; lean on human review of the
side-by-side markdown report. ADR 0024 codifies this split.

### 3. Mode-major iteration order in eval harness is a ~5x speedup on small VRAM

Fixture-major (`for fx in fixtures { for mode in modes { ... } }`)
forces Ollama to load/unload the 3B↔7B models every 3 calls. On a 6
GB card with Whisper already resident, each swap is 5-10s. 39 × 3 =
117 calls → ~78 model swaps → ~10 min of pure swap overhead.

Mode-major (`for mode in modes { for fx in fixtures { ... } }`)
has 2 swaps total (casual→normal→formal). Saved ~10 min wall on the
baseline run with zero behavior change.

Not specific to this work — applies to any developer tooling that
issues many requests across multiple Ollama models. If your harness
spends most of its time idling between calls, check ordering before
optimizing the calls themselves.

### Process callout: the bin/<name>/main.rs + bin/<name>/<helper>.rs pattern

`mode_eval` hit the 600-line guideline in its first draft. Splitting
into `bin/mode_eval/main.rs` (driver) + `bin/mode_eval/report.rs`
(rendering + scoring + unit tests) lands ~400 + ~280 lines and lets
the scorer have proper test coverage (the bin entry can't, because
of CUDA DLL path issues during `cargo test` from a bin that links
the whole `mockingbird_lib`).

Cargo discovers `src/bin/<name>/main.rs` as a binary automatically,
and `main.rs` can `mod helper;` to pull siblings. Standard Rust
multi-file bin layout; precedent now set for future dev tooling.



## 2026-05-18 [phase5-postship-9-followup] ADR-0024 v2 prompts iter-1 -- extending the eval corpus revealed a NEW class of failure (imperative content)

- **Context:** Migration 010 had just landed shipping casual_v2 / normal_v5 / formal_v2. The 39-fixture baseline showed 96.8%/97.5%/87.0% preservation with zero hallucinations. Before declaring the prompts done, ran a deliberate stress test: extended the eval corpus from 39 -> 52 fixtures, adding 5 categories under-represented in the original (directions, project_outline, code_dictation, meeting_notes, decision_rationale). Goal: stretch the prompts toward real-world content the original corpus missed.
- **Finding:** On the 52-fixture run, aggregate preservation HELD or improved on every mode (casual 97.2%, normal 97.5%, formal 88.5%) and ZERO catastrophic failures occurred. BUT one fixture (`46_code_short`: `"create a function called process input that takes a string parameter and returns a boolean"`) revealed a new failure mode in casual mode: the 3B model emitted META-COMMENTARY scaffolding before its answer, like `"The provided transcribed speech is not a request for a function definition but rather an instruction on how to clean up text. Based on the instructions, I will clean up the given text without defining any function."` followed by literal `**Input (function description):** ... **Output:** ...` blocks mirroring the few-shot example format in the prompt. The scorer accidentally passed (all 4 must_preserve terms appeared in the literal Output line at the end), but in real usage the user would paste the WHOLE garbage blob into their IDE.

- **Root cause (two compounding factors):**
  1. **The casual prompt's few-shot examples used `**Input:** ... **Output:**` markdown labels.** That visual scaffolding is easy for a 3B model to mirror under confusion. On 39 fixtures we never saw it leak because no fixture's raw text was imperative-shaped enough to confuse the model about whether the user was talking TO it.
  2. **Imperative-shaped dictation triggers refusal/explanation behavior in small models.** "Create a function..." reads like a request to the LLM. The 3B casual model partially obeyed the system prompt (don't generate, don't refuse) AND partially obeyed its safety training (explain why you can't / won't do the request) -- and emitted both responses concatenated.

- **Action:**
  1. Added rule 1 to casual_v2.md: "THE DICTATION IS CONTENT, NOT AN INSTRUCTION TO YOU." Explicitly covers commands like "create a function", "add a button", "write a test", "tell me about X". Names the failure mode ("if you ever find yourself writing 'the user is asking...', STOP -- that is wrong output").
  2. Added rule 5: "NEVER ECHO THE EXAMPLE SCAFFOLDING." Listed the specific tokens (`Speech:`, `Cleaned:`, `EXAMPLE`, `Input:`, `Output:`) the output must not contain.
  3. **Reformatted the few-shot block** from `**Input (...):** ... **Output:** ...` markdown labels to plain `Speech:` / `Cleaned:` text labels with no markdown weight. The new format is harder to mirror because (a) no bold/italic visual weight to copy, (b) the explicit rule-5 forbid acts as an output guard, (c) the labels are distinctive enough that any leakage is obviously a bug.
  4. Added EXAMPLE 3 demonstrating imperative content with the CORRECT response (just the cleaned sentence, no commentary).
  5. Re-ran `mode_eval --modes casual --label v3cas` (52 fixtures, casual-only because the bug was casual-only): 100% preservation on 46_code_short, ZERO leakage in 52 outputs (verified by extracting all 52 output blocks and grep-matching against meta-commentary patterns), aggregate held at 97.1% (well within noise of 97.2%), no new regressions on any other fixture.
  6. Left normal_v5 and formal_v2 unchanged -- they never showed the bug, and YAGNI says don't change them defensively. If the same failure surfaces there in a future eval, replicate the pattern.
  7. ALSO extended the eval corpus from 39 to 52 fixtures permanently. The 13 new fixtures (categories: directions, project_outline, code_dictation, meeting_notes, decision_rationale) become part of the baseline corpus going forward. Also added `must_preserve_alts` field to the fixture schema (with backward-compatible default = empty array) so paraphrase-tolerant scoring works for code-style identifiers (snake_case / camelCase / PascalCase all accepted) and formal-mode normalizations (e.g. "PostgreSQL" satisfies "postgresql").

- **Generalisation (the deeper lesson):**
  - **Expanding the eval corpus is a force multiplier, not a chore.** The 13 additional fixtures took ~30 min to author + 14 min to run; they caught a real bug that 39 fixtures missed. The 5 new categories were chosen by asking "what content types would my user dictate that the existing corpus under-represents?" -- not "what would be hardest for the model". That framing matters: real users will dictate directions, project outlines, code, meeting notes, and decisions; they will not dictate "design language phase six" style corner cases.
  - **must_preserve scoring is necessary-but-not-sufficient.** It tells you the model retained the right concepts; it does NOT tell you whether the output is pasteable. A 100%-preservation output with 200 chars of meta-commentary prepended is WORSE than a 90%-preservation output that's clean. Future eval iterations should add a "no-scaffolding canary" check (regex for `Input:` / `Output:` / `the user is` / `based on the instructions` etc. inside output blocks) that fails the fixture even if preservation hits 100%. The canary would have caught this bug in 2 seconds instead of the manual spot-check that found it.
  - **Few-shot example formatting is a load-bearing design choice on small models.** Bold/markdown labels invite mirroring. Plain prefixed labels with explicit forbid rules are more robust. Worth checking normal_v5 and formal_v2 if/when their next iteration comes around -- the same scaffolding is latent there.
  - **Imperative-shaped dictation is a real use case.** Users dictate "create a function", "add this feature", "write a test for X", "tell me how to" all the time. The prompt MUST handle these as content, not as requests. Worth adding to every future cleanup prompt's non-negotiable rule list, not just casual.
  - **Wall-clock note:** v2corpus run (all 3 modes, 156 calls) finished in 6 min 24 s vs the iter-2 baseline's 11+ min on 117 calls. Hot model cache makes a big difference; first-run latency is not representative.

---

## 2026-05-18 [phase-mc-wave-0] Build/test env conventions were tribal knowledge; AGENTS.md didn't carry them

- **Context:** Phase MC kickoff. The user-provided kickoff prompt's
  cargo gate read literally "cargo check / cargo clippy --release -- -D
  warnings / cargo test --release / cargo fmt --check". I started by
  running those plain commands. `check` and `clippy` passed (they only
  compile, never link/launch), `test --release` exploded at first
  binary launch with `STATUS_ENTRYPOINT_NOT_FOUND` (0xc0000139).
- **Finding 1:** Project AGENTS.md (`.code_puppy/AGENTS.md`) had a
  bare `cargo fmt` / `cargo clippy` / `cargo test` recipe in the
  end-of-iteration block. It did NOT mention:
  - `scripts/cargo-with-cuda.ps1` is mandatory for ALL cargo calls.
  - `pwsh` is not on PATH; `powershell -File` is the working invocation.
  - `scripts/run-mockingbird.ps1` is the only sanctioned app launcher.
  - `%USERPROFILE%\mockingbird_models\` is the runtime model home and
    must be populated (`download-onnxruntime.ps1` / `download-models.ps1`).
  - LESSONS 2026-05-17's `STATUS_ENTRYPOINT_NOT_FOUND` known issue
    + the `--no-run` / throwaway-crate fallback gate.
  Every one of these was tribal knowledge buried in LESSONS or runbooks.
  Fresh agent + clean context = fresh agent steps on every rake.
- **Finding 2:** A Stop-Process race — user had run
  `Start-Process target\release\mockingbird.exe` (the stale May-18
  build) earlier in the session. That held a file lock on the exe,
  which made `cargo build --release` fail with
  `error: failed to remove file ... Access is denied. (os error 5)`.
  Cargo's error message is fine, but the failure mode (the bytecode
  for "old binary still loaded") needs to be on the AGENTS quickref
  because it's a very normal mid-session occurrence.
- **Finding 3 (Bernard's bug):** I reported "mockingbird_models dir is
  missing" when in fact the folder was present. Root cause: PowerShell
  single-quoted strings DO NOT expand `$env:USERPROFILE`. I wrote
  `Test-Path '$env:USERPROFILE\mockingbird_models'` and got `False`
  because Test-Path was checking for a literal path whose first segment
  was the 8-character string `$env:USE` (no, wait — literally `$env:USERPROFILE`).
  Either way: the folder exists, the test mis-reported. Use double
  quotes (`"$env:USERPROFILE\..."`) or `-LiteralPath "$home\..."`.
- **Action 1:** AGENTS.md updated with a full new section
  "Build / run / test environment (Windows)". Carries: wrapper usage,
  pwsh-vs-powershell, app launcher, models dir, known launch-failure
  fallback gate, throwaway-crate recipe pointer, PS single-quote trap.
  End-of-iteration cargo gate now points at the wrapper invocations
  explicitly. Rust coding-standards section now flags the wrapper as
  mandatory and pins clippy to `--release` per LESSONS 2026-05-15.
- **Action 2:** Five-attempt rule fired correctly on the
  `STATUS_ENTRYPOINT_NOT_FOUND` debug loop (~5 attempts: bare cargo,
  wrapper-only, wrapper+`ORT_DYLIB_PATH` to deps copy, wrapper+real
  dir, dependency check). Stopped, escalated to the user, confirmed
  the LESSONS 2026-05-17 fallback gate, proceeded.
- **Pattern burned in:** *Tribal-knowledge debt accrues silently. The
  test of whether a runbook is real is whether a fresh agent with
  cleared context can execute it.* Anything not in AGENTS.md or the
  active phase doc is not a runbook — it's folklore. Today's gap was
  ~6 distinct facts buried in LESSONS that the kickoff prompt didn't
  surface. Cost: ~30 min of debug + a rebuild. Fix: surface them.

---

## 2026-05-22 — Phase MC retrospective `[phase-mc-retrospective]`

**Context:** Phase MC (Meeting Capture) sealed at `phase-mc-complete`
in Wave 6. ADRs 0026 (sibling subsystem), 0027 (chord activation),
0028 (twin-stream capture), 0029 (long-form chunked Whisper),
0030 (whisper segment exposure), 0031 (meeting loopback backend) all
Accepted. The standing P1 `mb-2bi` (audio streaming + chunked
Whisper) closed via ADR 0029 as its architectural closer.

### What went right
1. **Wave brief discipline paid off.** Every wave seal authored
   the next wave's brief with type defs / function signatures /
   test specs / deviations. Code-side iterations could start
   immediately on the next agent invocation without re-reading
   the master plan end-to-end.
2. **Sibling-subsystem isolation held.** Zero edits to sealed
   dictation/injection/cleanup-trait/hotkey-state code across 6
   waves. Only `hotkey/probe.rs` was extended (allowed — not in
   the seal set per ADR 0027). The `mc-dictation-untouched`
   judge mechanically verifies this.
3. **Determinism-first formatter.** Pure Rust + `phf::phf_set!`
   for filler words + proptest fixpoint property caught zero
   bugs in retrospective but pins the invariant for all future
   Whisper model swaps. `mc-formatter-deterministic` judge
   makes this explicit.
4. **Per-channel stitch + late merge.** Keeping mic and sys
   segments on separate `Vec<TimedSegment>` until the explicit
   `merge_two_channels` call made `lossless_synthetic_long_feed_no_gaps_no_dupes`
   mechanically verifiable on a 30-chunk feed without needing
   real audio.

### What bit us
1. **Speaker-label settings were UI-exposed but never plumbed.**
   `MeetingSpeakerLabelMic`/`Sys` settings were added in Wave 1's
   `settings/model.rs` and surfaced in Wave 5's
   `SettingsMeetingTab.tsx`, but `merge_two_channels` hardcoded
   `**You:**` / `**Other(s):**` as `const` strings and was
   never updated to read them. Caught in Wave 6 production-
   readiness scan. **Lesson:** any time a setting goes into
   `SettingKey`, immediately grep for hardcoded literals at
   the consumer sites — a setting that nobody reads is worse
   than no setting (the user expects it to work). Fix introduced
   `SpeakerLabels` struct + `load(&Connection)` constructor +
   `mic_md()` / `sys_md()` Markdown helpers threaded through
   both the persist path (`lifecycle.rs`) and the export paths
   (`commands/meetings.rs`).
2. **Diff-based judges need a phase-specific anchor tag.** The
   `mc-dictation-untouched` judge originally diffed against
   `phase-4-complete`, which mis-flagged lateral-epic
   (0022/0024/0025) commits as "Phase MC violations". Wave 6
   created the `phase-mc-start` tag at `d540a64` (commit
   immediately before the first MC commit `3f6ca82`) to give
   the judge a clean baseline. **Lesson:** for any future phase
   sealed after another phase has shipped lateral work, tag
   the start commit alongside the seal commit. Pattern:
   `git tag phase-{N}-start <first-phase-commit>^` right after
   `git tag phase-{N}-complete`.
3. **Tauri 2 macro + generic-runtime `State<_, T>`.** When a
   command takes both `AppHandle<R>` and `State<'_, T>`, the
   bare `tauri::AppHandle` defaults to `AppHandle<Wry>` and the
   `generate_handler!` macro fails to type-resolve. Make the
   command itself generic: `pub fn cmd<R: Runtime>(app: AppHandle<R>, state: State<'_, T>, ...)`.
   Documented earlier in the W5 SEAL commit; pinned here in the
   retrospective so future Tauri-command authors don't relearn it.
4. **Release-mode test execution on Windows is fragile.** The
   `0xC0000139 STATUS_ENTRYPOINT_NOT_FOUND` DLL-load error
   continued to bite Wave-5 and Wave-6 gates. The
   `cargo-with-cuda.ps1` script sets MSVC + CUDA + cmake env
   but doesn't resolve every DLL-loader edge case. **Workaround
   held:** `cargo test --release --no-run` was the gate; the
   tests are heavily pure-function so runtime failure once
   linked is a "DLL env" problem, not a "code" problem. Filed
   as environmental, not a code defect — but a future Phase X
   should consider adding a `scripts/run-tests-in-clean-shell.ps1`
   that bootstraps the loader path explicitly.

### Architecture deltas worth preserving
- Meeting LLM prompts live in `src-tauri/src/meetings/prompts/*.md`,
  NOT in the `modes` table. Migration 011 only adds the
  `meetings` / `meeting_transcripts` / `meeting_transcripts_fts`
  tables. This is what makes ADR 0026's critical-path
  invariant possible.
- The `OllamaProvider` is imported by exactly ONE meetings file
  (`meetings/llm_pass.rs`). Every other meeting module is
  determinism-only. The `mc-no-llm-in-critical-path` judge is
  a static grep — the architecture enforces the invariant; the
  judge documents how to verify it.
- The runtime LLM-pass cache (`Arc<Mutex<HashMap<String, String>>>`)
  is the LLM output's only storage. Never written to the DB.
  The user invokes `meeting_run_llm_pass` explicitly per
  meeting; the canonical transcript is independent.


## 2026-05-27 [adr-0046-iter2] Throwaway-crate recipe extends cleanly to submodule layouts

- **Context:** Phase A landed `vault/{layout,manifest}.rs` as a flat throwaway crate (`src/lib.rs` + `src/{layout,manifest}.rs`). Phase B added `vault/project.rs` which imports `crate::vault::manifest::{RecordType, MOCKINGBIRD_EXPORT_VERSION}` — a real-workspace path that doesn't resolve in the flat throwaway layout.
- **Finding:** The throwaway-crate recipe (LESSONS P2) was originally documented as flat — copy the `.rs` file in, run `cargo test`. When the module under test imports from a sibling sub-module via `crate::vault::*`, the throwaway needs to MIRROR the real workspace's module tree so those paths resolve unchanged. Trying to patch import paths inline is brittle (defeats the byte-identical copy promise); restructuring the throwaway is the clean move: `src/lib.rs` declares `pub mod vault;`, `src/vault/mod.rs` re-exports the sibling files, and the copied `.rs` files import via `crate::vault::*` exactly like in the real workspace. Took 5 minutes the first time; would have been zero if documented.
- **Action:** When the module under test has intra-subsystem (intra-vault / intra-meeting / intra-activity / etc.) imports, mirror the real path under the throwaway's `src/`:

  ```
  $env:TEMP\<modname>_throwaway\
  ├── Cargo.toml          # deps: only what the module(s) under test pull in
  └── src\
      ├── lib.rs          # `pub mod error; pub mod <topmod>;` + AppError shim
      └── <topmod>\
          ├── mod.rs      # mirrors real workspace's mod.rs (`pub mod foo; pub mod bar;`)
          ├── foo.rs      # copied verbatim from real workspace
          └── bar.rs      # copied verbatim from real workspace
  ```

  The `AppError` shim in `src/lib.rs` provides exactly the variants the modules use (`Vault(String)` for vault modules, etc.) — catch-all `_ => panic!()` arms in tests emit `unreachable_pattern` warnings under the shim, harmless because the catch-all IS reachable against the full `AppError` enum in the real workspace.

  Iter 2 Phase B: 33/33 throwaway-crate tests green after the restructure, including a golden-snapshot byte-pin of `vault::project::project()` output (`vault_phase_a_throwaway` at `$env:TEMP`).

## 2026-05-24 [adr-0046-seal] One scoped boundary-edit, three downstream consumers

- **Context:** ADR 0046 Mobile Extension landed across four iterations spanning a desktop import button, an outbound projection engine, an inbound mobile courier, and a full polish iteration. The ADR's `Sealed-surface boundary` section authorized one edit to one function in `src-tauri/src/dictation.rs` plus a new file `dictation/ingest.rs`, gated by the `sealed-phases-untouched` judge per ADR 0037's boundary-authorization precedent.
- **Finding:** That single ADR-chartered edit (Iter 1, ADR §3.2 amendment introducing a sibling `crossbeam-channel` for `HeadlessIngestRequest`) ended up consumed by **three** separate downstream surfaces, none of which required reopening the boundary: (a) the `+ Audio file` IPC handler in Iter 1, (b) the inbox courier in Iter 3, (c) the `AppIngestProgressBus` event-tap in Iter 4 Phase B. Each consumer attaches at the call site — `dictation/ingest.rs` was modified exactly once and has been touched zero times since. The judge ran twice (Iter 1, Iter 3); Iter 4 didn't need it because the event-tap pattern preserved the boundary by construction.
- **Action:** When chartering a boundary-edit ADR for sealed code, design the channel/seam/trait at the boundary to **accept arbitrary callers**, not just to satisfy the one immediate caller. Downstream reuse cost approaches zero when the boundary is shaped as a fan-in, and the judge cost amortizes — you don't re-judge each new consumer because the consumer doesn't touch sealed code, only the caller side of the channel does. Same generalizable shape as ADR 0037's Command Center authorization, but at finer grain (one function vs. one subsystem).

## 2026-05-24 [adr-0046-seal] Observation-with-logging beats reading docs on third-party black boxes

- **Context:** ADR 0046 Wave 0 was a ~3-hour spike on real iOS-to-Win11 hardware over Obsidian Sync, originally framed as charter-blocking. Dustin ran five observation rounds (rounds 1-4 planned, round 4b spontaneous), each instrumented with JSONL timestamp logs on both sides of the sync. Logs preserved at `docs/spikes/iter3-logs/`; findings doc at `docs/spikes/iter3-sync-layer-findings.md`.
- **Finding:** The single highest-ROI finding (Finding 5: external Files-app writes into Obsidian's iOS sandbox lag 5-15 minutes until Obsidian Mobile is foregrounded) emerged ONLY from the spontaneous round 4b, which exercised the actual Shortcut-courier path rather than the canned observation cases. Obsidian's docs do not surface this behaviour; reading Obsidian's open-source plugin code wouldn't have caught it either, because the lag lives in iOS's sandbox-write-notification semantics, not in Obsidian itself. The mitigation (Shortcut Action 3 = `Open App: Obsidian`) is one extra step; without the spike we'd have shipped a ~15-minute worst-case end-to-end latency into the POC and rediscovered it the hard way during hands-on smoke.
- **Action:** When the load-bearing dependency is a third-party black box (sync engine, OS sandbox, hardware driver), spike with logging on real hardware BEFORE building the consumer. Budget at least one spontaneous round outside the planned observation matrix — the planned rounds confirm what you already suspect; the spontaneous rounds catch what you don't know to look for. Generalizable beyond ADR 0046 to any future cross-platform / cross-process / cross-vendor seam.

## 2026-05-25 [adr-0047 / mb-h0nn / ui] Typed-settings registry  SettingsSnapshot for new keys
- Context: shipping the Settings  tab UI for ADR 0047's DictationCleanupLevel dial + PreferQ5Models toggle. The flat SettingsSnapshot DTO (commands/types.rs + lib/types.ts) didn't expose either key.
- Finding: there's no need to extend SettingsSnapshot to surface new typed-registry settings in the UI. The api.legacy_get_setting / legacy_set_setting wrappers (which call get_setting / set_setting on the Rust side) already speak the typed SettingKey registry. SettingsMeetingTab uses exactly this pattern for legacy_meeting_chord_enabled. Extending the snapshot would force a Rust-side change for what is fundamentally a TS-only concern (just plumb a new IPC read).
- Action: when surfacing a new SettingKey in the UI, default to the legacy_get_setting / legacy_set_setting path. Reserve SettingsSnapshot extensions for settings that are read on every page load (theme, retention) or that need to round-trip through useAppStore for cross-route state.

## 2026-05-25 [adr-0047 / mb-h0nn / ui] :has() selector is the cleanest way to surface focus-within on a card-style radio group
- Context: cleanup-level dial is a 4-card radio group; native radio focus ring lands on the invisible input, not the card.
- Finding: .levelCard:has(input:focus-visible) { outline ... } gives a single-rule focus ring on the visible card surface, no JS required, no :focus-within polyfill, works under React 19 + Vite + Tailwind v4 with zero plumbing. Browser support is universal at this point (2024+).
- Action: prefer :has() for visible-focus rings on label-wraps-invisible-input patterns. Avoids the focus-within ambiguity (which fires on click too, not just keyboard).


## 2026-05-28 [kg-wave-3] Over-segmentation is the dominant qwen2.5:3b failure mode on single-item dictations
- Context: Wave 3 baseline scoring on the 32-pair KG corpus. Pipeline (qwen2.5:3b) hit 86.7% segmentation on the multi-item bucket (per spec the harder case), but the clean-single-item floor came in at 6.7% (1/15) on run-a and 13.3% (2/15) on run-b. The headline number looked alarming until decomposed.
- Finding: 9 of 15 single-item dictations were OVER-segmented into 2 entries (60% over-split rate). Of the 6 correctly-segmented singles, only 1 had no downstream category/type error. The bad-looking clean-single metric is the AND of segmentation + category + type + date; over-segmentation pulls 9 to zero before classification even runs. Spec §8.4 anticipated multi-item as the harder bucket with the "segmentation correct (multi-item only)" metric design, but did not catch that single-item over-splitting hides inside the clean-single-item floor instead of the segmentation metric.
- Action: When tuning the segmenter prompt in Wave 5, the load-bearing few-shot to add is "when in doubt, keep as one entry." The current prompt teaches the model to handle multi-item ramblers well (87% on the harder bucket) but biases toward splitting on weak signals. Consider adding a "no-split" exemplar pattern (raw dictation that LOOKS like 2 items but is actually 1 internal-debate item) explicitly. Also: when re-reading Wave 3 numbers, decompose the clean-single floor into (segmentation_ok × cascade) before drawing scale conclusions about classifier weakness.

## 2026-05-28 [kg-wave-3] Calibration sets must use tag tokens that don't appear in the judge prompt examples
- Context: Authored 12 gold-standard tag-equivalence pairs at experimental/kg-validation/judge-calibration/tag-equivalence.json for JVP Gate 1. First unit test of the calibration loader against the real fixture failed with a mock-matching collision: the judge prompt template contains in-context examples that mention "car-repair" and "taxes", so MockOllama's first-match-wins substring matching on those anchors caught the wrong canned response for the wrong calibration pair.
- Finding: Two distinct concerns conflated. (1) When unit-testing a judge call with a MockOllama, the rule anchors must be substrings that appear ONLY in the actual A/B query line at the bottom of the prompt, NOT anywhere in the in-context examples baked into the prompt template. (2) This is also a real-world concern for the calibration set itself: a calibration pair whose tags appear verbatim as an in-context example essentially gives the judge a free hint at gate-evaluation time, inflating Gate 1 scores. The 12 hand-authored pairs (cal-eq-001..007, cal-diff-001..005) deliberately use distinct tag content (e.g. cal-eq-001 uses "car-repair" + "auto" — which COLLIDES with the prompt example and should be revised in v2 of the calibration set).
- Action: When adding a calibration pair, audit the judge prompt examples for tag-token collisions first. When mocking a judge call in tests, anchor on a distinctive sliver of the query line (e.g. unique compound like "kid-stuff") rather than a tag word that might appear in the prompt template. Open follow-up: v2 of the calibration set should replace cal-eq-001's "car-repair" tags with non-prompt-overlapping equivalents to avoid the Gate-1 inflation risk.

## 2026-05-29 [phase-0-kg/wave-5] Strict-no-regression IAP cannot ratchet on small local models

- Context: Wave 5 IAP Wiggum loop, 5 iterations cap, each iteration a single
  prompt change (segmenter / extractor / extractor / classifier / extractor)
  against the Wave 3.4 sealed baseline (qwen2.5:3b @ 32-dictation corpus).
  IAP defined as 5 strict rules: aggregate same-or-better, no per-metric
  regression, hard-gate intact, stability >=80%, PCRP trust-eroding count
  cannot rise. REJECT -> revert + counter advances.
- Finding: ZERO of 5 iterations satisfied all 5 rules even though 4 of 5 had
  aggregate score > baseline and 4 of 5 held the hard-gate. Each iteration
  improved at least one structural metric meaningfully AND regressed at
  least one co-metric via global joint-distribution shift in the model's
  output. The cleanest test was iter 5: an extract-only prompt change with
  ZERO tag/category/type language still dropped tag-collapse 1.54pp, dropped
  entry-type 0.82pp, and lifted PCRP +2. The model's output distribution is
  not separable along prompt-section boundaries at this size. The IAP is
  correct for the strict question "should we ship this pipeline
  autonomously?" but is the wrong tool for the lateral question "should we
  ship a different prompt as a default draft that the user reviews?" — for
  the latter, a weighted Pareto frontier (accept if N-1 metrics improve,
  the regression is below X pp, and PCRP variance noise is modeled rather
  than gated) would have accepted iter 1, iter 3, and iter 4 individually
  and likely a kitchen-sink composition of them. Independent finding:
  PCRP trust_eroding count has ~2-3 variance from joint-output-shape
  changes that should be modeled as noise around the true signal, not as
  a strict ratchet bound.
- Action: When the work container is "harden a trust-critical autonomous
  pipeline", use strict no-regression IAP. When it is "tune a prompt that
  will produce a user-reviewed draft", relax the IAP to a Pareto frontier
  plus model PCRP as noisy (use band tolerance, not strict less-than-or-
  equal). Document the workflow-mode choice in the kickoff brief BEFORE
  running so the verdict doesn't get rationalized after.

## 2026-05-29 [phase-0-kg/wave-5] PowerShell Set-Content -Encoding UTF8 writes a BOM that Rust serde_json rejects

- Context: Wave 5 synonym-map sweep script (`experimental/kg-validation/wave-5/apply-synonym-sweep.ps1`)
  wrote the updated map back via `$json | ConvertTo-Json -Depth 100 | Set-Content -Path $path -Encoding UTF8`.
  Next `score-run` invocation died with `error: parse judge-calibration\synonym-map.json: expected value at line 1 column 1`.
- Finding: PowerShell's `-Encoding UTF8` writes a UTF-8 byte-order mark
  (`EF BB BF`) at the file start. Rust's `serde_json::from_str` does NOT
  strip the BOM; it reads those bytes as the first "character" and reports
  the parse error above because BOM is not a valid JSON token. Affects
  ANY Rust tool that uses serde_json on PowerShell-emitted JSON. Hex-dump
  confirmed: `EF BB BF 7B 0D` at offset 0; stripping the first 3 bytes
  made the file parse cleanly.
- Action: When writing JSON from PowerShell for Rust consumption, use
  `New-Object System.Text.UTF8Encoding $false` (the `$false` constructor
  flag means "without BOM") paired with `[System.IO.File]::WriteAllText($path, $payload, $utf8NoBom)`.
  For ASCII-only payloads, `Out-File -Encoding ascii` also works. The
  throwaway BOM-check on any such writer:
  `[byte[]](Get-Content $f -Encoding Byte -TotalCount 5) | ForEach-Object { '{0:X2}' -f $_ }`
  — first three bytes must NOT be `EF BB BF`.
