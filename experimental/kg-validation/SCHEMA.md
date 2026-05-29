# Mockingbird Knowledge Graph — SCHEMA contract

```yaml
schema_version: 1
schema_revision: phase-0.5-wave-1
```

> A portable Markdown contract for the Knowledge Graph pipeline,
> per [ADR 0049](../../docs/adr/0049-knowledge-graph-phase-0-5-and-v1-architectural-pivot.md)
> Move 1 (Clark schema-driven pattern).
>
> **The pipeline reads from this file at runtime.** Rust modules
> under `src/passes/` and `src/harness/` are contract-consumers —
> editing this file (and the prompt files it references) is the
> supported way to change pipeline behaviour.
>
> The loader validates `schema_version` against the value compiled
> into `src/schema_loader.rs::EXPECTED_SCHEMA_VERSION` and refuses
> to start if they disagree — a stronger structural guarantee than
> "the prompt happens to be on disk."

---

## Taxonomy

### Categories (closed enum)

The Layer 1 of the spec §7.2 three-layer tag system. Exactly one
per entry.

- `personal`
- `professional`
- `objective`

### Entry types (closed enum)

The Layer 2 of the spec §7.2 three-layer tag system. Exactly one
per entry. Drives downstream behaviour (a `task` gets a status, a
`note` does not).

- `task`
- `research`
- `idea`
- `note`
- `reference`

### Canonical tag vocabulary

```yaml
status: open
closed_in: phase-0.5-wave-3
```

Phase 0.5 Wave 0.5.1 ships open-vocabulary tags via `extract` →
`normalize`. Wave 0.5.3 (Move 3 / `mb-rzpd`) closes the vocabulary,
seeded from corpus + synonym-map v1.1, with a new-tag-request flow.
The schema slot exists now so consumers can adopt the contract once
the vocabulary lands.

---

## Per-pass model defaults

| Pass | Default model | CLI override | Notes |
|---|---|---|---|
| `segment`  | `qwen2.5:7b-instruct-q4_K_M` | `--model` (global) | Per-pass overrides reserved for v1.1 (Clark Nemotron pattern). |
| `classify` | `qwen2.5:7b-instruct-q4_K_M` | `--model` (global) | Replaced by embeddings in Wave 0.5.2 (`mb-yfzy`); LLM path retained for the head-to-head. |
| `extract`  | `qwen2.5:7b-instruct-q4_K_M` | `--model` (global) | |

The `--model` CLI flag is the global override for all LLM passes in
Phase 0.5. Per-pass overrides are reserved for v1.1+ once empirical
evidence justifies specialist routing.

---

## Pipeline order

1. **`segment`** (LLM) — 1 dictation → 0..N candidate entry strings.
2. **`classify`** (LLM, per segment) — `{category, entry_type}`.
3. **`extract`** (LLM, per segment) — `{title, due_iso, raw_topic_tags}`.
4. **`normalize`** (pure Rust, per segment) — tag canonicalization.

### Failure policy

- `segment` failure → abort that dictation (no segments ⇒ nothing to do).
- `classify` failure → drop only the offending segment, continue.
- `extract` failure → drop only the offending segment, continue.

This is the spec §8.1 contract and is invariant across all four
architectural moves in ADR 0049.

---

## Model-class calibration profiles

Different model families / sizes have different natural priors. A
single prompt body behaves differently across models — instructions
that push a 3b-class model just hard enough to overcome its
cautious-by-default disposition do NOT push a 7b/9b-class model far
enough to overcome its confident-by-default disposition (Wave 0.5.1
7b hard-gate breach; see LESSONS P10).

The portable-contract mission survives this by encoding per-class
calibration in the schema, not by re-tuning the prompt every time we
swap models. Adding a new model = identify its profile (or add a new
profile) and add the mapping; the pipeline picks the right prompt
body automatically.

### Profile: `small-conservative`

- Examples: `qwen2.5:3b-instruct-q4_K_M`, `gemma2:2b-instruct-q4_K_M`.
- Natural prior: cautious; defaults toward `null` / empty when
  uncertain.
- Date-extraction needs: minimal null-bias reinforcement; light
  examples sufficient.
- This is the **default profile** — any prompt the schema does not
  override per-profile lives in the small-conservative variant.

### Profile: `mid-confident`

- Examples: `qwen2.5:7b-instruct-q4_K_M`, `gemma2:9b`,
  `llama3.1:8b-instruct-q4_K_M`.
- Natural prior: assertive; defaults toward committing a value even
  when underspecified.
- Date-extraction needs: forceful null-bias; explicit past-tense and
  vague-future handling; explicit segment-isolation rules;
  duration-vs-deadline disambiguation.

### Profile assignment

| Model | Profile |
|---|---|
| `qwen2.5:3b-instruct-q4_K_M` | `small-conservative` |
| `gemma2:2b-instruct-q4_K_M` | `small-conservative` |
| `qwen2.5:7b-instruct-q4_K_M` | `mid-confident` |
| `gemma2:9b` | `mid-confident` |
| `llama3.1:8b-instruct-q4_K_M` | `mid-confident` |

Default for unknown models: `mid-confident`. Rationale: an unknown
model that turns out to be confident-by-default with a
small-conservative prompt produces invented dates (silent trust
erosion). An unknown model that turns out to be cautious-by-default
with a mid-confident prompt just produces a few extra `null`s (loud,
low-cost). Defaulting to the safer-on-trust-gate side is the call.

---

## Pass prompts

Prompt bodies live in separate files referenced below. The loader
reads them at startup and the passes use them verbatim — the runtime
prompt sent to Ollama is `{prompt_body}{per_pass_context_suffix}`.

### Default prompt body per pass

| Pass | Prompt file |
|---|---|
| `segment`  | `prompts/segment.md` |
| `classify` | `prompts/classify.md` |
| `extract`  | `prompts/extract.md` |

These are the **small-conservative** variants and the implicit
fallback for any `(pass, profile)` not listed in the override table
below.

### Profile-specific prompt overrides

| Pass | Profile | Prompt file |
|---|---|---|
| `extract` | `mid-confident` | `prompts/extract.mid-confident.md` |

Resolution rule: `prompt_body(pass, model)` =
`overrides[(pass, profile_for(model))]` if present, else
`default[pass]`. Profiles that don't override a pass inherit the
default — YAGNI says we add override rows only where empirical
evidence (a hard-gate breach, a per-metric regression) demands them.

### Per-pass context suffix

The per-pass context suffix appended at runtime by the pass module:

- `segment`:  `\n\nCONTEXT: captured at {captured_iso}.\nDICTATION:\n{dictation}\n`
- `classify`: `\n\nSEGMENT:\n{segment}\n`
- `extract`:  `\n\nCONTEXT: {calendar_context}\nSEGMENT:\n{segment}\nCLASSIFICATION: {classification_json}\n`

This split keeps SCHEMA.md a stable contract; only the prompt body
files iterate during prompt tuning, and the calendar / classification
context construction stays in Rust because it depends on runtime
state (captured time, prior pass output) the schema cannot statically
know.

---

## Normalization rules (Pass: `normalize`)

Pure Rust, no LLM. Logic lives in `src/passes/normalize.rs`. Contract:

1. Lowercase.
2. Whitespace and `_` → `-`.
3. Collapse repeated `-`.
4. Trim leading / trailing `-`.
5. Conservative singularization on the last hyphen-segment only:
   - `ies` → `y` (parties → party)
   - `xes`/`zes` → `x`/`z` (taxes → tax)
   - drop trailing `s` only when:
     - word length > 3
     - prior char is NOT in `{s, x, z, u, i, o}`
     - word does NOT end in `{ss, sh, ch, us}`
6. Dedupe (first-seen order preserved).

Phase 0.5 Wave 0.5.3 may extend step 6 with canonical-vocabulary
collapse, but the public signature (`fn normalize_tags(raw: &[String]) -> Vec<String>`)
is invariant.

---

## User preference rules (reserved)

```yaml
status: empty
unlocked_in: v1.1
```

Reserved schema slot for per-user preference overrides — category
routing hints, entry-type heuristics, additional tag synonyms. Phase
0.5 leaves this empty by design; v1.1 (post-ship, empirically driven)
populates it from observed user corrections via the SCHEMA.md learning
loop (ADR 0049 v1.1 deferrals).

---

## Wave provenance

| Wave | Adds to schema | Sub-bead |
|---|---|---|
| 0.5.1 (this) | Portable contract; passes load from here at runtime. Model-class calibration profiles + per-profile prompt overrides (`mb-4xtd` hard-gate fix). | `mb-xmgs`, `mb-4xtd` |
| 0.5.2        | Embeddings-classifier model + exemplar source paths. | `mb-yfzy` |
| 0.5.3        | Closed canonical tag vocabulary + new-tag-request validator. | `mb-rzpd` |
| 0.5.4        | Entity types + entity-disambiguation thresholds. | `mb-o4ni` |
| 0.5.5        | (No schema change — cross-test on 3b.) | `mb-5r1b` |

Each later wave's PR amends this file additively. `schema_version`
bumps when a consumer must change to remain compatible.
