# ADR-0024: Empirical mode tuning via fixture-driven eval harness

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** Dustin (project lead), code-puppy/Bernard (implementor)

## Context

ADR 0022 (DRAFT) shipped a two-stage cleanup pipeline (deterministic
preprocessor + LLM polish) and three new transcription modes (`casual`,
`normal`, `formal`) with prompts authored defensively after the
2026-05-17 Santa-list smoketest failure. Those prompts (`casual_v1`,
`normal_v4`, `formal_v1`) were never iterated against a broad corpus
— they were minimum-viable patches for one specific bleed.

User feedback after ~2 weeks of real dictation: outputs are "still kind
of random" across modes, casual feels "too liberal with changes",
formal occasionally feels tone-deaf. The empirical claim that ADR 0022
improved quality was never measured. The pipeline-architecture
hypotheses were ripe for either confirmation or refutation.

We need:

1. A **repeatable, reproducible** way to compare mode outputs across
   prompt versions, model choices, and pipeline orderings.
2. **Concrete preservation-vs-presentation acceptance criteria** per
   mode, so "better" stops being a vibe.
3. A **scoring framework** that splits automated (lexical/structural)
   from human-judged (register, tone) axes — and that doesn't
   false-positive on legitimate paraphrase.
4. A clear **release-build wiring path** so prompt improvements
   actually ship to users.

Without these, ADR 0022 stays DRAFT forever; with them, this work
flips it to Accepted and ships migration 010.

## Decision

We adopt a **fixture-driven empirical mode-tuning workflow** as the
permanent path for prompt evolution in Mockingbird. Specifically:

1. **A fixture corpus** at `src-tauri/eval/baseline.json`: 13
   categories × 3 lengths = 39 cases spanning enumeration, narrative,
   technical content, self-correction, numbers, tangents, emphasis,
   and mixed-structure utterances. Each fixture carries:
   - `raw` (simulated Whisper STT output: lowercase, no/light punct)
   - `intent` (human-readable description of speaker meaning)
   - `must_preserve` (terms that MUST appear in cleaned output, for
     automated scoring)
   - `must_preserve_alts` (optional equivalence groups, so legitimate
     register-lift paraphrases — `bad` → `poor`, `half day` →
     `half-day` — don't false-fail)
   - `mode_hints` (per-mode notes for the human reviewer)

2. **A standalone harness** at `src-tauri/src/bin/mode_eval/` that
   runs the FULL production cleanup pipeline (`Preprocessor` →
   DB-resolved prompt + dictionary + few-shot → `OllamaProvider`) for
   every fixture × mode combination and emits a side-by-side
   markdown report at `docs/cleanup/eval-{label}-{timestamp}.md`.

3. **Mode-major iteration order** in the harness (all fixtures for
   casual → all for normal → all for formal). Reduces Ollama VRAM
   thrash from ~78 model swaps per grid to 2; saves ~10 min wall.

4. **Lexical preservation scoring** as the automated bar: case-
   insensitive substring match on `must_preserve`, with
   `must_preserve_alts` equivalence groups for paraphrase-safe terms.
   Hyphens normalised to spaces. **Format-fit and register-fit
   remain human-judged** from the report — we explicitly do NOT try
   to automate aesthetic judgment.

5. **Acceptance thresholds per mode** (the "badass" bar):
   - **Casual:** ≥95% full-preserve, **0 zero-preserve cases**;
     median LLM ≤ 2s, p80 ≤ 3.5s.
   - **Normal:** ≥95% full-preserve, ≥98% with-alts preserve;
     median LLM ≤ 5s, p80 ≤ 8s. (Latency goal acknowledges that
     Wisprflow-parity needs streaming — see Consequences.)
   - **Formal:** ≥80% full-preserve, ≥92% with-alts;
     median LLM ≤ 6s, p80 ≤ 10s.

6. **Iteration loop**: edit prompts (and only prompts unless the
   eval reveals a structural pipeline issue) → re-run harness →
   compare report to previous → decide ship/iterate. Each prompt
   ships as a new `prompts` table version in an append-only
   migration (ADR 0008 compliance).

## Consequences

### Positive

- **Quality is now measured.** Future prompt changes have a numerical
  baseline to beat; no more vibes-driven authoring.
- **The smoking gun for the v1 prompts is now in the literature.**
  Bernard's iter-0 baseline caught the casual hallucination on
  fixture 06_implicit_long: the 3B model regressed to the prompt's
  "milk eggs bread" few-shot example under length pressure and
  emitted `"hey can you grab milk, eggs, and bread on the way home
  thanks"` for an architecture description. Fix shipped in
  `casual_v2` (anti-substitution rule + reordered examples +
  technical-preservation demonstration).
- **The eval rig is reusable.** Future modes (code mode if mb-cjc
  surfaces it; per-app context tuning under mb-xwi) can adopt the
  same fixture format + harness with zero new infrastructure.
- **The split between automated and human scoring is honest.**
  Automated metrics catch the disasters (hallucination, omission);
  the markdown report lets a human eyeball the aesthetics.
  Neither half pretends to do the other's job.

### Negative

- **Latency targets for normal mode are not achievable on a 6 GB
  card without streaming.** Baseline showed median ~7s, p80 ~11s
  for the 7B model on warm calls. The ADR 0024 targets (median
  ≤5s, p80 ≤8s) are reachable for *most* inputs via prompt
  tightening but Wisprflow-parity intuitiveness for long-form
  dictation requires **recording-window text streaming** — that's
  ticketed separately (existing mb-cjc Wave 3 scope + future
  streaming ticket).
- **The fixture corpus is synthesized, not collected.** Bernard
  authored all 39 cases; they reflect Bernard's model of how people
  dictate, not measured distributions from real Mockingbird users.
  As real sessions accumulate in the DB, we should top up
  `baseline.json` with real anonymized raw transcripts.
- **A full grid run costs ~16 min of compute.** Acceptable for the
  Wave C iterate-on-prompts loop (3-5 runs total), uncomfortable as
  a CI gate. We do NOT run mode_eval in CI — it stays a
  developer-invoked workflow.

### Neutral

- **Bin convention extended.** Mockingbird now has four developer
  binaries: `learn`, `stt_test`, `verify_wave49` (example), and
  `mode_eval`. The `bin/<name>/main.rs + bin/<name>/<helper>.rs`
  multi-file pattern is the precedent for future bins that exceed
  the 600-line guideline.

## Alternatives considered

- **Stay vibes-driven.** Edit prompts based on intuition, deploy,
  see how it feels in real use. Rejected: doesn't scale beyond
  one user, no regression detection, ADR 0022 stays DRAFT forever.
- **LLM-as-judge for automated scoring.** Use a stronger model
  (e.g., Claude Haiku) to score each output's preservation + style.
  Rejected for now: adds external dependency (network call,
  paid API), adds latency to the iteration loop (~117 extra
  network calls per eval run), and the "is the format right for
  the mode?" judgment is the kind of thing where Bernard's-judgment
  + Dustin's-veto is sufficient at this scale. Revisit when
  community contributions to fixtures start arriving.
- **Test fewer fixtures (e.g., 10-15) for faster iteration.**
  Rejected: the long-tail cases (fixture 06 hallucination) are
  exactly the ones a small fixture set would miss. 39 is the
  minimum that gives reasonable coverage of the 13 categories.
- **Build the harness as an `#[ignore]` integration test.**
  Rejected: tests want to be silent, harnesses want to be
  noisy + emit artifacts. The bin pattern is the right shape;
  Cargo's `[[example]]` precedent (verify_wave49) confirms the
  project already accepts this layout for verification probes.

## Cross-references

- **PLAN sections:** §4 (cleanup pipeline), §8 (Ollama integration),
  §11 (workflow / iteration cadence).
- **Related ADRs:**
  - **ADR 0022** (three-mode pipeline): this ADR is the empirical
    proof that flips 0022 from DRAFT to Accepted. The pipeline
    architecture survives unchanged; only the prompts change.
  - **ADR 0008** (prompt versioning): every prompt iteration ships
    as a new `prompts` row via append-only migration. Migration
    010 follows the pattern.
  - **ADR 0010** (raw transcript immutability): preserved — the
    raw row in the DB is never touched by the cleanup pipeline.
  - **ADR 0021** (sync cleanup provider): preserved — eval bin
    uses the same sync `provider.cleanup()` path as production.
- **bd issues:**
  - **mb-e7s** (epic): empirical mode tuning.
  - **mb-jh5** (closed): Wave A — rig.
  - **mb-3uv** (closed): Wave B — baseline + Pareto analysis.
  - **mb-e6a** (in flight): Wave C — iterative tuning.
  - **mb-35t** (open): Wave D — migration 010 + ADR seal + UI wiring.
  - **mb-cjc** (still open after this ADR): Wave 3 of ADR 0022 —
    LLM-skip for short casual + streaming. Required for true
    Wisprflow-parity latency on normal/formal.
- **Skill(s):** none new; this work uses existing skills
  (`prompt-versioning`, `migration-author`).
- **Artifacts:**
  - `src-tauri/eval/baseline.json` — fixture corpus.
  - `src-tauri/src/bin/mode_eval/` — harness.
  - `docs/cleanup/eval-*.md` — generated reports. Only the iter-0
    baseline and the final iter-N seal are committed for posterity;
    intermediate iterations are produced locally during the prompt-
    tuning loop and not added to git (they would just clutter
    history). Future epics can revisit this if it becomes useful.
    For ADR-0024 Wave C: `eval-baseline-*.md` (iter-0) and
    `eval-iter2-*.md` (final) are committed.
  - `docs/cleanup/eval-findings-v1.md` — Wave B Pareto analysis.
  - `src-tauri/src/cleanup/prompts/{casual_v2,normal_v5,formal_v2}.md`
    — the v2 prompt bodies.
  - `src-tauri/src/db/migrations/010_adr0024_prompt_v2.sql` — the
    ship vehicle.

---

_This ADR will be marked **Superseded by ADR-XXXX** if the eval
methodology itself changes (e.g., adopting LLM-as-judge). Adding
fixtures, modes, or prompt versions does NOT require superseding
— those are point releases of the existing process._
