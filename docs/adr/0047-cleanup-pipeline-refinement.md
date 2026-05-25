# ADR-0047: Cleanup pipeline refinement — over-consolidation hotfixes, scoped passes, and pull-only compression

- **Status:** Proposed
- **Date:** 2026-05-24
- **Deciders:** Dustin (project lead), code-puppy / Bernard (implementor)
- **Supersedes:** none
- **Amends / extends:** ADR 0022 (three-mode cleanup pipeline), ADR 0024
  (empirical mode-prompt tuning). Touches sealed surfaces in
  `meetings/llm_pass.rs` (Phase MC) under the explicit-boundary
  pattern established by ADR 0035; the `mc-no-llm-in-critical-path`
  and `mc-dictation-untouched` judges remain binding.

## Context

A post-`stable-alpha-v0.1` audit of the LLM cleanup pipeline
(`docs/reviews/2026-05-24-llm-cleanup-deep-dive.md`) identified the
mechanical cause of the over-consolidation behaviour Dustin has been
observing on dictations and meeting LLM passes: the meetings
`SYSTEM_HEADER` ("Be concise...") is prepended to **every** LLM pass,
including `cleaner_punctuation` — whose prompt body specifies
byte-for-byte preservation. Small (3B-7B) local models resolve this
competing-objectives ambiguity by collapsing toward the salient
concision attractor, so the user's preambles, parentheticals, and
secondary clauses get silently dropped. The review surfaces six
additional structural cracks (no length-ratio fallback, cleanup-level
bundled with tone, Whisper `initial_prompt` always `None`, casual still
on 3B, Q4 everywhere, borderline temperature 0.1) that together amount
to "the LLM has standing authority to consolidate, and the user
constantly has to tell it not to."

The review's prescription is to **invert the default**: the LLM should
be invited in on the cases that need it (multi-paragraph thought,
implicit list rendering, explicit register transformation) rather than
running always-on and being suppressed via prompt engineering. The
deterministic preprocessor + Whisper-large-v3-turbo's native
punctuation already produce acceptable output for ~70 % of utterances;
the LLM's job description should shrink accordingly.

All file/line citations in the review doc have been re-verified
against current `main`; they remain accurate as of this charter.

This ADR is the charter for that work. It bundles the review's P0 and
P1 items (excluding the "flip Light to default" migration, which is
deferred to post-release observation — see § Out of charter) into a
three-wave lateral epic.

## Decision

We adopt a three-wave ADR-chartered lateral epic. Per LESSONS PINNED
P5 and ADR 0032/0035 precedent: **no new `phase-*-complete` tag**;
seal is via this ADR flipping Proposed → Accepted, STATUS.md update,
and bd epic closure.

### Wave 1 — P0 hotfixes (single session, no schema migrations except #4)

Four tightly-scoped changes that attack the over-consolidation
symptom from four independent angles. No architectural commitments;
each is reversible and tunable from settings.

1. **Per-pass system headers in `meetings/llm_pass.rs`.**
   Replace the global `SYSTEM_HEADER` constant with a
   `header_for_prompt(&LlmPassPrompt) -> &'static str` function.
   - `LlmPassPrompt::CleanerPunctuation` returns
     *"You are a transcript punctuation assistant. Preserve every
     word; modify only whitespace and punctuation."*
   - `LlmPassPrompt::Summary` and `LlmPassPrompt::ActionItems`
     retain the existing concision instruction.

   This is the single most likely fix for the observed
   over-consolidation. Sealed-surface edit; falls under the
   ADR 0035 "surgical edits to sealed surfaces with explicit
   boundary authorization" pattern. The `mc-no-llm-in-critical-path`
   judge remains green by construction (no provider/cleaner
   construction added; only the header string differs by enum
   variant). The `mc-dictation-untouched` judge sees a diff in
   this file; the review's audit confirms `meetings/llm_pass.rs`
   is the **shared** LLM-pass engine (dictation reuses it via
   `dictation::llm_prompts::resolve_dictation_prompt`), so the
   change is in-charter for cleanup work and the judge's binding
   list does not need to grow.

2. **Length-ratio sanity check in `cleanup/llm_cleaner.rs::run_cleanup`.**
   After `provider.cleanup(req)?`, compute
   `cleaned_words / pre_words`. If the ratio falls below a
   threshold AND `pre.notes.self_corrections == 0` (the
   preprocessor didn't legitimately consume content already),
   log a `tracing::warn!`, return `pre_text` (the
   preprocessor-only output), and suffix `last_model_used` with
   `-shrink-fallback` so provenance tells the truth.

   Threshold ships as a new `SettingKey::LlmShrinkFallbackThreshold`
   (default `0.65`). Tunable from day one because 0.65 is a
   reasoned guess, not a measured optimum (see § Risks).

   Minimum test coverage: 2 unit tests — one where the LLM
   legitimately compressed self-corrected text (must NOT fall
   back), one where the LLM dropped content with no
   self-corrections (MUST fall back). The `mc-no-llm-in-critical-path`
   judge is unaffected (this change is in dictation cleanup, not
   meetings).

3. **Wire `stt::prompt_builder::build_prompt` into Whisper.**
   The builder is already authored and unit-tested (top-N
   dictionary terms by `use_count`, capped at 200 tokens to stay
   under Whisper's 224-token initial-prompt window). The two
   call sites — `dictation.rs:679` and `dictation/ingest.rs:300`
   — currently pass `initial_prompt: None` with a stale Phase 4
   TODO comment. Replace with `build_prompt(...)`.

   Compounding effect: better Whisper output reduces the
   downstream LLM's workload, which reduces its opportunity to
   over-consolidate. Dictionary substitution moves **upstream**
   of the LLM where it belongs.

4. **Migration 019 — temperature 0.1 → 0.2 for normal and formal.**
   The casual mode is already at 0.2 (per migration 010 / ADR 0024).
   The meetings LLM pass is already at 0.2 (`meetings/llm_pass.rs:47`,
   with the rationale comment cited in this ADR's header).
   Standardize.

   **Gating step:** re-run `mode_eval` rig before merging this
   task. If any mode regresses by more than 2 points on the
   baseline fixture set, abort the bump for that mode and document
   the per-mode decision in the migration header. This is the
   review's explicit risk-mitigation recommendation; the migration
   is cheap to defer per-mode if needed.

**Wave 1 exits when:** all four sub-tasks land in one commit (or
one commit per task, at the implementor's discretion), cargo +
UI gates green per AGENTS.md end-of-iteration checklist, and the
five Phase MC judges (`mc-formatter-deterministic`,
`mc-long-form-stitched-losslessly`, `mc-two-channel-merged`,
`mc-no-llm-in-critical-path`, `mc-dictation-untouched`) remain
green when dry-run via
`scripts\dry-run-phase-mc-judges.ps1` (or the equivalent in
`docs/judges/phase-mc/`).

### Wave 2 — P1 architectural changes (~1 week, schema migrations)

Six changes that re-shape the cleanup pipeline so the LLM is
pulled, not pushed, for the majority of utterances.

1. **Migration 020 + cleanup-level dial.**
   New `SettingKey::DictationCleanupLevel` with four variants:

   | Variant | Behaviour |
   |---|---|
   | `None`   | Raw STT directly. Skip preprocessor + LLM. |
   | `Light`  | Deterministic preprocessor only. No LLM call. |
   | `Medium` | Preprocessor + LLM with an *additive-only* prompt (new `cleanup/prompts/normal_v6_additive.md`; insert punctuation, paragraph breaks, list structure; never delete content). |
   | `High`   | Preprocessor + current mode-specific LLM (existing `casual_v2` / `normal_v5` / `formal_v2`). |

   **Default ships as `High`** for this epic. Flipping the default
   to `Light` is explicitly out of charter (see § Out of charter)
   so we can observe `edit_free_send` rates (Wave 2 task #5) on
   the current behaviour before committing to the change.

   Tone (`casual / normal / formal`) becomes orthogonal — it only
   matters at level `High`.

2. **LLM-skip on short utterances** — consumes existing bead `mb-cjc`
   (ADR 0022 Wave 3). Un-stub `cleanup/preprocessor.rs::looks_listy()`
   so it returns `true` when **either** the preprocessor's
   `ProcessedNotes` recorded ≥ 2 ordinal cues **or** ≥ 3 enumeration
   markers (both signals are already tracked). Then in
   `cleanup/llm_cleaner.rs::run_cleanup`, short-circuit to return
   `pre_text` when `word_count <= SettingKey::LlmSkipWordThreshold`
   (new key, default `12`) AND `!pre.notes.looks_listy()`.

   This is the load-bearing change for re-positioning the LLM as
   pull-not-push: ~70 % of casual one-liners short-circuit to the
   preprocessor in ~5 ms, freeing the latency budget for the cases
   that actually need an LLM. **Close bead `mb-cjc` at merge.**

3. **Migration 021 — `casual` to `qwen2.5:7b-instruct-q4_K_M`.**
   ADR 0022's own Context section identified 3B as below the
   headroom threshold for restraint-heavy cleanup; `casual` was
   left at 3B in migration 008 only for the ≤ 500 ms latency on
   one-liners. With Wave 2 #2 above, one-liners take the skip path
   and never pay the 7B latency tax. Existing `casual_v2`
   few-shots are already 7B-compatible.

   **Ordering constraint:** this migration MUST land **after**
   Wave 2 #2 in commit order. If shipped first, casual one-liners
   pay 7B latency they don't need.

4. **Migration 022 — Q5_K_M model swap, gated by VRAM probe.**
   New default for fresh installs that pass the VRAM probe;
   existing installs stay on Q4_K_M unless the user opts in via
   Settings or via a first-run-wizard flag re-runs the probe.
   Probe threshold: if total VRAM < 6 GB, stay on Q4. The
   ~500 MB delta vs Q4 is significant on a 6 GB RTX 2060 with
   Whisper-large-v3-turbo (~2 GB) resident.

   Default-off for existing installs is the correct conservative
   posture: a user who upgrades shouldn't have their working
   pipeline silently transition to a heavier model.

5. **`edit-free-send` instrumentation.**
   New column on `sessions`: `edit_free_within_5min: Option<bool>`
   (NULL while the window is still open, then committed). Heuristic:
   flips `false` if within 5 minutes of inject the user opens the
   `LlmPassCard` for that session **or** copies the raw transcript
   from the Dictations detail page. Otherwise flips `true` at the
   5-minute mark.

   Surface as a tile in the Insights "Your usage" tab. **This is
   the metric that tells us whether the rest of the epic worked.**
   Non-negotiable for the charter: without this signal we have no
   empirical basis to later flip the default to `Light` (which is
   the post-observation step parked out of charter).

6. **"Compress" / "Tighten" Transform on `LlmPassCard`.**
   New prompt body at `dictation/prompts/compress.md`; new variant
   on the dictation LLM-prompt enum; new option on the
   `LlmPassCard` UI. This is the **pull-only** affordance for the
   consolidation behaviour we are removing from the always-on push
   path. Users who want the old "tighten this up" behaviour click
   the button instead of getting it whether they wanted it or not.

   The mode-specific dictation prompts (`casual_v2`, `normal_v5`,
   `formal_v2`) are NOT rewritten in this wave — they remain the
   level-`High` behaviour. Re-scoping them to additive-only across
   the board is a candidate for a successor ADR after we observe
   the level-`Medium` (`normal_v6_additive`) field performance.

**Wave 2 exits when:** all six sub-tasks land, all four migrations
(020/021/022 plus the implicit migration for the sessions column in
#5) apply cleanly on a fresh DB and on a DB at the
`phase-10-complete` baseline, cargo + UI gates green, and the Phase
MC judges remain green.

### Wave 3 — Verification & seal

1. **`cleaner_punctuation` regression eval.**
   Author 20 preamble-bearing fixtures under
   `eval/fixtures/cleaner_punctuation/` (one input file + one
   expected-survives-tokens file per case). Eval rig pipes each
   input through the `cleaner_punctuation` LLM pass and asserts
   the preamble tokens survive via a token-set comparison
   (existing eval infrastructure under `src-tauri/eval/` is the
   prior art). This permanently catches the Wave 1 #1 regression
   class.

2. **Optional one-off LLM judge** for the per-pass-header
   invariant: "the system header for `cleaner_punctuation` does
   not contain the word 'concise' (case-insensitive) and the
   prompt body does not request compression or summarization."
   Single-purpose grader prompt; NOT a 5-judge bundle. Authored
   only if the regression eval (#1) is insufficient to catch
   the failure mode in practice — author-it-on-demand by the
   implementor.

3. **Flip ADR 0047 Proposed → Accepted.** Update STATUS.md
   "Lateral epics accepted via ADR" table with the closing
   commit hash and a one-line summary.

4. **Update `docs/PRODUCT-STATE.md`** cleanup-pipeline section
   to reflect: cleanup-level dial, LLM-skip-on-short heuristic,
   the new `compress` Transform on the LlmPassCard, the
   `edit_free_send` metric in Insights, the model/quantization
   updates from migrations 021 + 022.

**Wave 3 exits when:** ADR is Accepted, STATUS + PRODUCT-STATE
updated, regression eval green, and the bd epic closed.

## Consequences

### Positive

- The over-consolidation symptom is attacked from four independent
  angles in Wave 1, three of which (per-pass headers, length-ratio
  fallback, Whisper `initial_prompt`) are reversible and tunable.
  Expected outcome: the symptom is largely gone after Wave 1,
  Wave 2 is refinement rather than rescue.
- The LLM is re-positioned as pull-not-push. The user no longer
  has to teach the small model what NOT to do; the architecture
  removes the LLM's standing authority to consolidate.
- The cleanup-level dial separates verbosity from tone, matching
  the two-axis design the field has converged on (Wispr Flow's
  "Auto Cleanup" levels x "Style" tones). Users who want
  "verbatim but in formal register" can now express that.
- `edit_free_send` gives us the first proper empirical signal
  about whether changes are actually working. WER is the wrong
  metric for cleanup; this one isn't.
- The "Compress" Transform on `LlmPassCard` retains the value
  of compression without the push-default risk. Users who
  genuinely wanted the old behaviour have a one-click path.

### Negative / Risks (with mitigations)

1. **Phase MC's `mc-dictation-untouched` judge must still pass
   after Wave 1 Task 1 edits a sealed-phase file
   (`meetings/llm_pass.rs`).**
   Mitigation: the precedent is ADR 0035's pattern of "surgical
   edits to sealed surfaces with explicit boundary authorization."
   The shared LLM-pass engine is the **shared** surface between
   dictation cleanup and meeting LLM passes; the binding-list
   judge sees this file but the edit is in-charter for cleanup
   work. Dry-run the judge before committing; if it red-flags,
   refactor so the per-pass header lives in a new file
   (`meetings/llm_pass_headers.rs`) and `llm_pass.rs` only adds
   a one-line call.

2. **The 0.65 shrink-fallback threshold is a reasoned guess.**
   The review picked the range 0.6–0.75; we pick 0.65 as the
   middle-of-aggressive. Mitigation: ship as
   `SettingKey::LlmShrinkFallbackThreshold` from day one so
   observation can tune without code changes. Surface in
   Settings only if observation shows the default needs
   user-visible adjustment; otherwise leave as a power-user
   IPC.

3. **Q5_K_M VRAM blow-up on lower-end GPUs.**
   Mitigation: VRAM probe gate in migration 022; default-off
   for existing installs; first-run-wizard re-probe. If
   VRAM < 6 GB the user stays on Q4 with no degradation
   from their pre-epic experience.

4. **Existing prompts may be over-tuned to 0.1 temperature.**
   Mitigation: Wave 1 #4 explicitly gates the 0.1 → 0.2 bump
   on a `mode_eval` re-run with a > 2-point regression abort.
   This is cheaper than picking a per-mode temperature later
   from observed regressions.

5. **Phase MC's `mc-no-llm-in-critical-path` judge must still
   pass after Wave 1 Task 1.**
   Mitigation: the per-pass-header change adds zero new
   provider construction, zero new cleanup-engine calls, zero
   new awaits in the meetings hot path. The change is a
   string-table lookup. The judge's static scan should remain
   green by construction; dry-run before committing.

### Neutral

- Three new migrations (019, 020, 021) plus 022 (Q5 swap) plus
  the sessions-column migration for #5 = four-to-five migrations
  in this epic. None are destructive; all are additive or
  in-place column updates on settings/modes rows.
- STATUS.md "Lateral epics accepted via ADR" grows by one
  entry at seal. Same shape as ADR 0022/0023/0032/0035.
- New prompt files on disk: `cleanup/prompts/normal_v6_additive.md`
  and `dictation/prompts/compress.md`. Both follow the existing
  prompt-file convention (front-loaded non-negotiables, few-shot
  examples, scope statement footer).

## Out of charter (parked as separate beads)

The review surfaced four additional items that are intentionally
NOT in this epic. They become separate `bd` issues, each
referencing this ADR for context:

1. **"Undo AI edit" raw-copy button** on Dictations cards
   (review P2 #9). The three-stage Card already exposes raw;
   the explicit one-click affordance is a UX polish that's
   better authored after Wave 2 #5's `edit_free_send` data
   tells us how often users actually need it.

2. **Light-as-new-default migration after ≥ 2 weeks of
   `edit_free_send` observation** (review P2 #8 final step).
   The data has to exist before this migration can be
   justified. Parked as a follow-up that's chartered by a
   one-paragraph ADR amendment to 0047, not a new epic.

3. **Calm-Whisper / non-speech-suppression eval** (review P3).
   Phase-MC-v2-class work; gated by user-visible meeting
   hallucination pressure, which we currently don't have
   evidence of.

4. **Cloud Claude one-shot opt-in on `LlmPassCard`** (review
   § 4). The provider abstraction supports it; the UX wiring
   for a per-call cloud opt-in is its own design problem and
   touches the privacy posture in ways worth their own ADR.

## Alternatives considered

- **Ship only Wave 1 and call it done.** Tempting; Wave 1
  attacks the symptom from four angles. Rejected because the
  structural cracks (cleanup-level bundled with tone, the
  always-on LLM with no skip path, no metric for whether changes
  worked) outlive the symptom — patching the symptom without
  closing the cracks just defers the next over-consolidation
  incident to whenever the user discovers a new shape that
  bypasses the Wave 1 fallbacks.
- **Re-scope the always-on prompts (`casual_v2` / `normal_v5` /
  `formal_v2`) to additive-only across the board.** Rejected as
  premature. The level-`Medium` (`normal_v6_additive`) prompt
  gives us a clean additive-only baseline to observe; if it
  outperforms the existing prompts in field use, a successor
  ADR re-scopes the always-on path. Doing it pre-emptively
  burns the empirical signal.
- **Flip the cleanup-level default to `Light` in this charter.**
  Rejected. The review explicitly recommends doing this only
  after observing `edit_free_send` rates on the current
  behaviour, and the LESSONS PINNED P4 "no surprise defaults"
  posture says new defaults need an observation window.
  Parked out of charter (#2 above) as a follow-up amendment.
- **One ADR per wave instead of one ADR / three waves.**
  Rejected. The three waves share a single audit origin, a
  single binding constraint (the Phase MC judges), and a
  single seal moment. ADR 0032/0035 precedent is one ADR
  covering a multi-piece lateral polish epic; same shape
  applies here.
- **Mint a `phase-cleanup-refinement-complete` git tag at seal.**
  Rejected per LESSONS PINNED P5: phase tags are PLAN §10
  boundaries only. Lateral epics seal via ADR Acceptance +
  STATUS update.

## Cross-references

- **Source review:** `docs/reviews/2026-05-24-llm-cleanup-deep-dive.md`
  (file/line citations re-verified against current `main` at
  charter time).
- **Related ADRs:**
  - **ADR 0008** (prompt versioning) — preserved; new prompt
    files (`normal_v6_additive.md`, `compress.md`) follow the
    convention.
  - **ADR 0010** (raw transcript immutability) — preserved
    strictly; nothing in this epic mutates raw rows.
  - **ADR 0021** (sync cleanup provider) — preserved; the
    LLM-skip path returns `pre_text` synchronously without
    touching the provider trait.
  - **ADR 0022** (three-mode pipeline) — extended. Wave 2 #2
    consumes its Wave 3 bead `mb-cjc`. The cleanup-level dial
    is orthogonal to the mode dial 0022 introduced.
  - **ADR 0024** (empirical mode tuning) — extended. The
    Wave 1 #4 temperature bump uses the same mode_eval rig
    0024 introduced.
  - **ADR 0026** (meeting sibling subsystem) — preserved.
    Wave 1 #1 edits the shared LLM-pass engine; the
    meetings critical path is not affected.
  - **ADR 0035** (MC v1.2 stable alpha — surgical edits to
    sealed surfaces) — pattern source for the Wave 1 #1
    sealed-surface edit.
- **bd issues:**
  - **mb-{epic-w1}** (P1, to be created): Wave 1 P0 hotfixes.
  - **mb-cjc** (P2, open): consumed by Wave 2 #2; closes at
    Wave 2 merge.
  - **mb-{w2}**, **mb-{w3}** to be created when Wave 1 lands.
  - Out-of-charter beads (4 above) to be created at charter time.
- **LESSONS:**
  - **PINNED P5** — lateral epics seal via ADR, not by a new
    `phase-*-complete` tag.
  - **PINNED P8** — fresh subagent dispatches per wave; no
    `session_id` carryover from this charter.
  - **2026-05-17** `0xC0000139` STATUS_ENTRYPOINT_NOT_FOUND —
    the `cargo test --release --no-run` fallback gate applies
    for all three waves; pure-Rust modules (preprocessor,
    llm_cleaner, prompt_builder) can use the throwaway-crate
    recipe for live testing.
- **Artifacts (Wave 2):**
  - `src-tauri/src/cleanup/prompts/normal_v6_additive.md` — new
    prompt body (level `Medium`).
  - `src-tauri/src/dictation/prompts/compress.md` — new prompt
    body (pull-only Transform).
- **Judges to preserve:** the five Phase MC judges
  (`mc-formatter-deterministic`, `mc-long-form-stitched-losslessly`,
  `mc-two-channel-merged`, `mc-no-llm-in-critical-path`,
  `mc-dictation-untouched`). Dry-run via the existing rig
  before sealing each wave.

---

_This ADR supersedes nothing. It is **superseded** if a future
epic (a) revisits the cleanup-level dial's default after
`edit_free_send` observation OR (b) re-scopes the
mode-specific dictation prompts to additive-only across the
board — either becomes a new ADR with its own bd epic._
