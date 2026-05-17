# ADR 0022 (DRAFT) — Three-mode cleanup: Normal / Casual / Formal with a deterministic pre-pass

**Status:** DRAFT — awaiting decisions in §"Open questions"
**Date:** 2026-05-17
**Authors:** Bernard (with Dustin in the loop)
**Supersedes:** parts of ADR 0008 (prompt versioning still binding; mode set changes here)

## Context

Live evidence from the 2026-05-17 user smoketest, session id 51:

- **Raw STT:** `"Here I have a list of keyboard supplies. First thing is air duster. Second is alcohol wipes. Third is an extra cable."`
- **What the LLM (qwen2.5:3b-instruct-q4_K_M, prompt normal@v3) produced:**
  ```
  ```ery keyboard supplies:

  - air duster
  - alcohol wipes
  - extra cable
  ```
  ```
- **Latency:** STT 1250 ms + Clean **3198 ms** + Inject 63 ms = 4511 ms.

Failure modes visible:
1. **Hallucinated/glitched intro** (`` ```ery ``) — model emitted a stray code-fence + a truncated word, instead of the `Keyboard supplies:` heading the v3 prompt few-shots demonstrated.
2. **Wrapped whole output in fences** — explicitly forbidden by the v3 prompt, ignored anyway.
3. **Lost the speaker's actual framing** ("Here I have a list of keyboard supplies") — collapsed to a glitched stub.
4. **70 % of end-to-end latency is the LLM cleanup step.** STT is GPU-accelerated and finishes in 1.25 s; cleanup takes 2.5× as long for what is mostly a punctuation + light-formatting job.

Root cause is structural, not promptcraft: we are asking a small quantized local model to perform every cleanup decision (filler removal, capitalization, sentence boundaries, dictionary substitution, format detection, AND style adaptation) inside one ~5 KB system prompt + few-shot block. Qwen-3B-q4 doesn't have the headroom for this. The prompt-rule "do NOT wrap output in fences" is literally one line buried in 4800 characters of other instructions; the model's attention mechanism cannot reliably honour it under the few-shot pressure.

The user wants a three-mode design — **Normal / Casual / Formal** — modeled on Wisprflow's "it just knows how to format my voice ramblings" behaviour, without giving up the no-cloud guarantee or the snappy <2 s end-to-end target.

## Forces

- **Latency budget.** Hold-and-release dictation has to land in the user's app in roughly the same time it takes them to switch focus. Anything over ~2 s feels broken. Current 4.5 s is the ceiling of "I'd rather just type it." Casual one-liners (≤ 5 words) should land in ≤ 500 ms total.
- **Determinism > LLM cleverness** for the boring 80 % of cleanup. Filler removal, capitalization, sentence-final punctuation, dictionary substitution, and explicit verbal cues ("new paragraph", "new line", "bullet point") are all rule-shaped problems with well-known regex / state-machine solutions. They run in single-digit ms. We currently delegate all of them to a 3 B-parameter LLM.
- **Model headroom is the binding constraint** for everything the LLM still has to do. Qwen-3B-q4 cannot reliably follow a 5 KB prompt with 7 rules + 4 few-shot examples + dictionary + style examples + raw transcript. The "`\`\`\`ery`" hallucination is the model exhausted, not the prompt wrong.
- **Provenance is total** (ADR 0010). Raw STT row is sacred. Cleaned row is the LLM output. Final row is what was injected. The deterministic pre-pass introduces a NEW stage between raw and cleaned; provenance requires we either store it (new schema column) or treat the LLM's input as `cleaned` and the deterministic pass output as the cleaner's input. Decision in §"Open questions" #2.
- **Cross-platform from day one.** No regexes that assume CRLF, no Windows-only Unicode tricks.
- **No telemetry.** All language stats / filler counts live in the user's DB or nowhere.

## Proposed architecture

### Two-stage cleanup pipeline

```
Raw STT
  ↓
Stage 1: deterministic preprocessor (Rust, ~5 ms)
  • Filler removal             (um, uh, like, you know, I mean, sort of, kind of)
  • Stutter collapse           (the the the → the)
  • Self-correction stitching  ("X, wait, Y" / "X, no I mean Y" → "Y")
  • Dictionary substitution    (existing — moves earlier in pipeline)
  • Explicit-cue rendering     ("new paragraph" → \n\n, "new line" → \n,
                                "bullet point X" → "- X", "open quote" → ")
  • Sentence capitalization    (first letter + after . ! ?)
  • Trailing punctuation       (heuristic: utterance with no terminal . ! ? gets one)
  ↓
Pre-cleaned transcript (deterministic output)
  ↓
Stage 2: LLM polish (mode-dependent, 200ms – 3s)
  • Casual:  SHORT prompt (≤ 500 chars), maybe skip LLM entirely for short utterances
  • Normal:  MEDIUM prompt (≤ 1.5 KB), focused on paragraph breaks + list formatting
  • Formal:  LONGER prompt (≤ 2.5 KB), section headers + structured formatting
  ↓
Cleaned text (DB: transcripts.stage='cleaned')
  ↓
Inject
```

**Why this wins on latency:** the LLM no longer has to remove "um"s and capitalize sentence-starts (deterministic does that). It only has to do the *judgment* work: "is this a list?", "should I break this paragraph?", "should this be a heading?". Smaller prompts → fewer input tokens → less work for the model → lower latency. Empirically this is roughly proportional: halving the prompt size cuts cleanup latency by ~40 %.

**Why this wins on quality:** the LLM no longer has to recite 7 rules + 4 few-shots — it just has to do ONE thing well per mode. Single-task prompts on small models work dramatically better than multi-task ones.

### The three modes

#### Casual — "text to a friend"
- **Default formatting:** prose, no markdown. Lists rendered inline (`"I made a list of keyboard supplies: air duster, alcohol wipes, and extra cable."`).
- **Voice:** preserve speaker's casualisms ("gonna", "kinda", contractions). Remove fillers but NOT informality.
- **LLM step:** **OPTIONAL.** For utterances ≤ 20 words with no explicit format cues, skip the LLM entirely — deterministic output is good enough. Target ≤ 300 ms total for short casual dictations. Longer / more complex utterances still hit the LLM with a tiny prompt (~400 chars) to handle paragraphing.
- **Latency target:** 80 % of dictations ≤ 500 ms total; tail ≤ 1.5 s.

#### Normal — "well-edited written English" (DEFAULT)
- **Default formatting:** sentence + paragraph structure. Lists detected from speech cues → bulleted or numbered with a one-line intro (the v3 prompt's current intent, but reliably). No section headers unless the speaker explicitly says "heading".
- **Voice:** light grammar fixes (subject-verb agreement, run-on sentences), keep speaker register, no slang-stripping.
- **LLM step:** REQUIRED. Medium prompt (~1.5 KB) focused on three judgments: (a) is this a list? (b) where do paragraphs break? (c) what is the natural sentence structure of these utterances?
- **Latency target:** 80 % ≤ 1.5 s; tail ≤ 2.5 s.

#### Formal — "professional document / presentation prose"
- **Default formatting:** rich markdown. Section headers when topic shift detected. Bulleted/numbered lists with introductions. Bold for key terms (when explicitly cued). Paragraph breaks more aggressive (one idea = one paragraph).
- **Voice:** slang stripped ("gonna" → "going to"), contractions expanded, register lifted. Speaker's casual asides toned down or dropped if they don't carry meaning.
- **LLM step:** REQUIRED, more expensive. Longer prompt (~2.5 KB) with structural-cue detection rules. Optionally a larger model (qwen2.5-7B if installed, else fall back to 3B with a warning).
- **Latency target:** 80 % ≤ 3 s; tail ≤ 5 s. Formal users expect to wait a beat for a polished result.

### Mode-specific examples (target outputs)

Raw STT (same for all three): `"Here I have a list of keyboard supplies. First thing is air duster. Second is alcohol wipes. Third is an extra cable."`

- **Casual:**
  `"Here's my list of keyboard supplies: air duster, alcohol wipes, and an extra cable."`

- **Normal:**
  ```
  Here's my list of keyboard supplies:

  - air duster
  - alcohol wipes
  - extra cable
  ```

- **Formal:**
  ```
  ## Keyboard Supplies

  The following items are required:

  1. Air duster
  2. Alcohol wipes
  3. Extra cable
  ```

### Deterministic pre-pass: scope discipline

The pre-pass is intentionally restricted to "the rules everyone agrees on". It does NOT make stylistic decisions; it removes obvious noise and renders explicit cues.

In scope (Wave 1):
- **Filler list:** `um, uh, er, ah, hmm, like, you know, I mean, sort of, kind of, basically, literally, actually` (configurable per user)
- **Stutter collapse:** consecutive identical tokens (≤ 3 chars) → one
- **Self-correction:** `(.*?), (wait|no wait|sorry|scratch that|i mean|actually), (.*)` → keep group 3, drop groups 1-2 (conservative regex — only the most obvious cases)
- **Verbal-cue rendering:** small fixed vocabulary, applied as token-stream rewrites:
  - `"new paragraph"` → `\n\n`
  - `"new line"` → `\n`
  - `"bullet point X"` → `\n- X`
  - `"period"` / `"full stop"` → `.`
  - `"comma"` → `,`
  - `"question mark"` → `?`
  - `"exclamation point"` / `"exclamation mark"` → `!`
  - `"open quote"` / `"close quote"` → `"`
- **Sentence capitalization:** first letter; first letter after `.` `?` `!` + whitespace
- **Terminal punctuation:** if utterance has no `.` `?` `!` at end, append `.`

Out of scope (LLM's job, not pre-pass's):
- Detecting *implicit* lists (no verbal cue, but the speaker enumerated)
- Paragraph breaks from semantic topic shift
- Tone/register transformation
- Sentence restructuring for grammar
- Slang → formal substitutions

### Provenance impact

Three options for the deterministic pre-pass output:

A. **New `transcripts.stage = 'preprocessed'` row.** Cleanest provenance, full reversibility. One extra row per session.
B. **No persistence; pre-pass output is the LLM's input + becomes `cleaned` if LLM is skipped.** Smallest schema change. Pre-pass output is reproducible from raw + the pre-pass version, so we don't *lose* anything.
C. **Pre-pass output is appended as a JSON sidecar to the raw row.** Awkward, rejected.

Recommendation: **B**. Pre-pass version goes in a new `transcripts.preprocessor_version` column (nullable; null = legacy pre-Wave-2). Migration is append-only per the post-Phase-1 invariant.

## Open questions

1. **Mode set.** Replace `verbose` + `fragment` with `casual` + `formal`? Or keep them and ADD the new ones? The current `verbose` ≈ proposed `formal`-lite; `fragment` is a different beast (no sentence assembly). Recommendation: replace. `verbose`/`fragment` were placeholder names anyway.

2. **Casual mode latency commitment.** Are we comfortable saying "for short casual utterances, no LLM runs at all"? Pros: 300 ms total. Cons: the `cleaned` stage in the DB equals the pre-pass output for those rows — provenance is honest but it changes the meaning of `cleaned`.

3. **Larger model for formal mode.** Do we want formal to default to a 7 B model (if user has one installed) for quality? Or stay 3 B for predictability? My instinct: detect at boot, prefer 7 B for formal if available, log it.

4. **Filler list user-customisation.** Ship a fixed list in Wave 1, expose UI for editing in a later wave? Or expose immediately? Pros of fixed: nothing to misconfigure. Cons: some users say "literally" non-ironically a lot.

5. **Verbal-cue vocabulary expansion.** Should we support `"new section"`, `"sub heading"`, `"bold the next word"`, `"code block"`? Each adds ambiguity (false positives in casual speech). Recommendation: ship the small set above in Wave 1; expand on user request.

6. **Bigger philosophical one.** Wisprflow's secret is probably a fine-tuned small model + heavy preprocessing. We have the preprocessing now. Do we want a Phase 8 task to fine-tune our own 3 B on dictation-cleanup data the user has voluntarily marked as good (the existing "Mark as style example" flow)? Out of scope for THIS ADR, but worth noting the path.

## Implementation phasing

Implementable as three sequential commits, each independently shippable:

- **Wave 1: deterministic pre-pass.** New `cleanup/preprocessor.rs` module. Wired in BEFORE the LLM step. Migration 008 adds `transcripts.preprocessor_version`. Unit-tested with ~30 cases. No mode/prompt changes yet. Expected latency win: ~30 % off cleanup time even with current prompts (LLM gets cleaner input → shorter generation).
- **Wave 2: three modes + per-mode prompts.** New prompts `casual_v1.md`, `normal_v4.md`, `formal_v1.md`. Migration 009 inserts the new prompt rows, marks `verbose`/`fragment` modes as disabled (rows preserved for provenance), inserts `casual` / `formal` modes. Modes page already handles three transcription modes — only the slug list expands.
- **Wave 3 (optional, deferrable): LLM-skip for short casual.** Heuristic in `LlmCleaner::clean`: if `mode_slug == "casual"` AND `pre_cleaned.split_whitespace().count() <= 20` AND no list cues in input → return pre-cleaned directly. Tag `model_used = "preprocessor_only"`. Latency win: 300 ms total for one-liners.

## Risks

- **Pre-pass over-correction.** A user who actually means "um" as an interjection in a quoted dialog gets it stripped. Mitigation: pre-pass leaves anything inside detected quotes alone. Filler-list user-editable (Wave 2+).
- **Verbal-cue false positives.** "I'll meet you at the new line of trees" → renders `\n`. Mitigation: cues only fire when isolated (preceded + followed by punctuation or sentence boundary). Tested.
- **Smaller prompts may regress some edge cases the current v3 handles.** Mitigation: keep `normal_v3` row in DB; per ADR 0008 every historical session keeps pointing at its original prompt. New sessions use `normal_v4`.
- **Three modes ≠ users' actual mental model.** Casual / Normal / Formal is a register axis but some users think "code dictation" or "email" or "Slack message" — orthogonal dimensions. Mitigation: ship the three, watch real usage, add a fourth (e.g. `code`) only if demanded.
