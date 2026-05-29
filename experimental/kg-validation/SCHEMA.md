# Mockingbird Knowledge Graph — SCHEMA contract

```yaml
schema_version: 1
schema_revision: phase-0.5-wave-3
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

### Canonical tag vocabulary (closed; Wave 0.5.3 seed)

```yaml
status: closed
closed_in: phase-0.5-wave-3
seed_size: 228
seed_composition:
  corpus_derived: 188   # canonical RHS of synonym-map v1.1
  domain_pad: 40        # broad-coverage tags for clusters the 32-pair corpus undersamples
new_tag_request_channel: extract.proposed_new_tags  # JSON field; collected at runs/<run-id>/new-tag-requests.jsonl
```

Move 3 / `mb-rzpd`. The extract pass selects topic tags ONLY from
this list. Tags the model wants to apply that are NOT in the list
go through the new-tag-request channel (the `proposed_new_tags`
JSON field on the extract output) and are collected per-run at
`runs/<run-id>/new-tag-requests.jsonl` for batch review.

The post-LLM `tag_validator` pass is forgiving:

- A tag in `raw_topic_tags` that **is** in the vocabulary survives
  normalization and lands on the entry.
- A tag in `raw_topic_tags` that **is not** in the vocabulary is
  dropped from the entry AND logged as an *implicit* new-tag-request
  (the model forgot the protocol but proposed a tag).
- A `{tag, rationale}` row in `proposed_new_tags` is dropped from
  the entry AND logged as an *explicit* new-tag-request.

Composition rationale: the 188 corpus-derived canonicals preserve
scoring continuity against existing answer keys (Wave 0.5.1
baseline + scored runs). The 40 domain pads cover clusters a real
user would plausibly hit but the 32-pair corpus undersamples
(communication — `call` / `voicemail` / `follow-up`; work rituals
— `standup` / `one-on-one`; health — `medication` / `prescription`
/ `sleep`; errands — `pickup` / `delivery` / `dmv`; travel —
`flight` / `hotel` / `passport`).

ADR 0048 G7 (person names NEVER collapse to domain tags) is
preserved: corpus persons (`becca`, `brandon`, `chen`, `dad`,
`henderson`, `jamie`, `joe`, `karen`, `lisa`, `madison`, `mom`,
`olivia`, `sarah`, `smith`) stay in the vocabulary as identity
entries.

#### Vocabulary list

<!-- This is THE closed vocabulary. 228 entries, alphabetical, ASCII.
     The schema loader reads this list verbatim; the prompt template
     inlines it (grouped into clusters for readability) at runtime.
     ANY edit here is a schema change — keep the bullet list flat,
     one tag per line, in backticks. -->

- `401k`
- `access-code`
- `accounting`
- `accounts-receivable`
- `ai`
- `apartment`
- `app`
- `appliance`
- `application`
- `appointment`
- `async`
- `auction`
- `bakery`
- `becca`
- `bill`
- `birthday`
- `birthday-cake`
- `book`
- `booth-fee`
- `brake-pad`
- `brandon`
- `brunch`
- `budget`
- `budgeting`
- `budget-revision`
- `business`
- `business-expense`
- `business-tool`
- `call`
- `car`
- `career`
- `career-conversation`
- `career-direction`
- `car-repair`
- `certificate`
- `chat`
- `chen`
- `cleaning`
- `cleat`
- `client`
- `client-visit`
- `code-review`
- `content`
- `conversion`
- `cooking`
- `costco`
- `cover-letter`
- `cpa`
- `craft-tutorial`
- `craft-vinyl`
- `dad`
- `daycare`
- `deadline`
- `debt`
- `delivery`
- `dentist`
- `design`
- `dinner`
- `dishwasher`
- `dmv`
- `docs-migration`
- `doctor-appointment`
- `documentation`
- `dog`
- `dog-food`
- `drill-bit`
- `dry-cleaning`
- `electricity`
- `email`
- `etsy`
- `exercise`
- `expense`
- `family`
- `farmers-market`
- `field-trip`
- `finance`
- `financial-goal`
- `fitness`
- `flight`
- `follow-up`
- `food`
- `freelance`
- `friend`
- `fyi`
- `garage`
- `gate-code`
- `gift`
- `gift-bundle`
- `gift-idea`
- `grocery`
- `growth`
- `habit`
- `henderson`
- `hobby`
- `holiday`
- `home`
- `home-maintenance`
- `home-office`
- `home-office-deduction`
- `hotel`
- `ic-vs-management`
- `insurance`
- `insurance-renewal`
- `internet`
- `interview`
- `inventory`
- `investment`
- `invoice`
- `jamie`
- `job-application`
- `job-search`
- `job-tracking`
- `joe`
- `karen`
- `kid`
- `kids-sport`
- `launch-date`
- `launch-slip`
- `lawn`
- `lead`
- `lisa`
- `listing`
- `madison`
- `maintenance`
- `manager`
- `marketing`
- `meal-planning`
- `mechanic`
- `medication`
- `meeting`
- `mockup`
- `mom`
- `morale`
- `mortgage`
- `notion`
- `nutrition`
- `olivia`
- `one-on-one`
- `parent-teacher`
- `passport`
- `paycheck`
- `payment`
- `payment-plan`
- `pediatrician`
- `performance-review`
- `permission-slip`
- `pet-care`
- `photos`
- `pickup`
- `planning`
- `planning-doc`
- `plumbing-supply`
- `portfolio`
- `post-office`
- `prescription`
- `presentation`
- `pricing`
- `pricing-strategy`
- `product-idea`
- `project-status`
- `q2`
- `q3`
- `q4-holiday`
- `reading`
- `reading-log`
- `receipt`
- `refund`
- `reminder`
- `rent`
- `renters-insurance`
- `repair`
- `resume`
- `resume-update`
- `retirement`
- `return`
- `rollover`
- `roth`
- `rsvp`
- `rust`
- `rust-async`
- `sarah`
- `saving`
- `school`
- `school-auction`
- `self-improvement`
- `shirt`
- `shot`
- `side-business`
- `sleep`
- `slide-deck`
- `small-business`
- `smith`
- `soccer`
- `social`
- `software`
- `sprint-planning`
- `standup`
- `stripe`
- `subscription`
- `summer-reading`
- `tax`
- `tax-prep`
- `team`
- `team-culture`
- `technical-reference`
- `text-message`
- `therapy`
- `thermostat`
- `tokio`
- `tool`
- `tooling`
- `truck`
- `utility`
- `venmo`
- `vet`
- `video-call`
- `voicemail`
- `volunteer`
- `volunteering`
- `water`
- `water-bill`
- `water-heater`
- `website`
- `website-redesign`
- `wedding`
- `wholesale`
- `work`
- `youtube`

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
| `extract` | `mid-confident` | `prompts/extract.closed-vocab.mid-confident.md` |

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
| 0.5.1        | Portable contract; passes load from here at runtime. Model-class calibration profiles + per-profile prompt overrides (`mb-4xtd` hard-gate fix). | `mb-xmgs`, `mb-4xtd` |
| 0.5.2        | (Falsified, no schema change; embeddings infra preserved for Move 4 entity disambiguation — ADR 0049 A1 amendment.) | `mb-yfzy`, `mb-hnb4` |
| 0.5.3 (this) | Closed canonical tag vocabulary (228 seed entries) + new-tag-request validator + closed-vocab extract prompt override. | `mb-rzpd` |
| 0.5.4        | Entity types + entity-disambiguation thresholds. | `mb-o4ni` |
| 0.5.5        | (No schema change — cross-test on 3b.) | `mb-5r1b` |

Each later wave's PR amends this file additively. `schema_version`
bumps when a consumer must change to remain compatible.
