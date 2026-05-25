# Mockingbird LLM Cleanup — Deep Dive & Strategic Review

**Author:** External review pass (Claude)
**Date:** 2026-05-24
**Scope:** End-to-end audit of the dictation + meeting LLM cleanup pipeline, cross-checked against the user's submitted research doc and against 2025–2026 best practices for local-LLM transcript cleanup.
**Goal:** Identify what is actually causing over-consolidation of dictations, what is already correct, and what changes give the best expected return per unit of effort — without pushing the user to compensate for individual model whims.

---

## 0. TL;DR for the impatient

The bones of this pipeline are already well above the median for local-LLM dictation tools. ADR 0010 (immutable raw transcripts), ADR 0022 (deterministic pre-pass + scoped LLM passes), the dictation prompt iteration (`normal_v3 → v5`, `casual_v2`, `formal_v2` with non-negotiable "preserve every sentence" rules), and the three-stage transcript storage (`raw → cleaned → final`) all match — and in places exceed — what Wispr Flow ships.

The user's intuition about over-consolidation is correct, but the residual cause is **not** a single misbehaved prompt. It is a small set of structural cracks the existing architecture didn't quite close:

1. **The meetings pipeline has a hardcoded `"Be concise"` system header that contradicts its own `cleaner_punctuation` prompt body.** This is the textbook "competing-objectives collapse" failure for small models, in the codebase, today. *(meetings/llm_pass.rs:36–37)*
2. **There is no length-ratio sanity check anywhere in the pipeline.** When the LLM silently drops a preamble, nothing notices. The raw transcript is preserved, but no automatic fallback fires.
3. **The cleanup level is bundled with the tone mode.** `casual / normal / formal` is a register dial, not a verbosity dial. Users who want "Light cleanup, casual register" or "verbatim, casual register" cannot get there without changing modes.
4. **Whisper's `initial_prompt` is wired all the way through to `set_initial_prompt`, but the dictation orchestrator passes `None`.** Dictionary terms never reach Whisper, so every named-entity error is pushed downstream onto a 3B/7B local model — which is exactly the wrong place to spend instruction-following budget.
5. **`casual` mode still runs on `qwen2.5:3b-instruct-q4_K_M`.** Per ADR 0022's own findings, 3B is below the headroom threshold for nuanced instruction-following; the "preserve every sentence" rule is the first thing the model drops under pressure.
6. **All models are pulled at Q4_K_M.** Current research (2025–2026) shows Q5_K_M is the more reliable choice for restraint-heavy tasks at the cost of ~10 % VRAM/disk and negligible latency.

Strategic answer to the user's framing — *"do we even need this LLM pass?"*: **For short dictations, mostly no.** The deterministic preprocessor + Whisper-large-v3-turbo's native punctuation already produces output that meets the bar for the majority of casual sends. The LLM should be re-positioned as a *pull* (user-invoked) rather than a *push* (always-on) for most utterances, and reserved for the cases where it actually adds value: list rendering, paragraph structure on multi-thought passages, and register transformation when the user explicitly chose formal.

The rest of this document explains each finding, with file/line citations, and proposes a prioritized change set.

---

## 1. What the codebase actually does today

### 1.1 Dictation pipeline (PTT and headless ingest)

```
audio (cpal)
  → VAD trim (Silero, ORT)
  → Whisper-rs (CUDA when available)        ← initial_prompt: None
  → Cleaner trait                           ← single Box<dyn Cleaner>
      ├─ PassthroughCleaner (Wave 4 default)
      └─ LlmCleaner ──→ Preprocessor (deterministic, ~5 ms)
                        → prompt_builder (system + dict + few-shot + fg + raw)
                        → Ollama provider (HTTP, 60 s cap, retry-free)
                        → strip outer code fence (defensive)
  → transcripts table: insert_raw / insert_cleaned / insert_final
  → Tier-0 clipboard paste injection (paste_payload)
```

Key files:

| Concern | File | Lines |
|---|---|---|
| Orchestrator (PTT) | `src-tauri/src/dictation.rs` | 693 (cleanup begin), 889–898 (raw/cleaned/final persist) |
| Orchestrator (headless / file import) | `src-tauri/src/dictation/ingest.rs` | 305 (raw_text), 332 (cleaner.clean), 415–419 (persist) |
| Cleaner glue | `src-tauri/src/cleanup/llm_cleaner.rs` | 85–169 (run_cleanup) |
| Preprocessor (deterministic) | `src-tauri/src/cleanup/preprocessor.rs` | 182–219 (pipeline order) |
| Prompt builder | `src-tauri/src/cleanup/prompt_builder.rs` | 145–243 (build) |
| Ollama HTTP provider | `src-tauri/src/cleanup/ollama.rs` | 107–147 (cleanup) |
| Prompts on disk | `src-tauri/src/cleanup/prompts/*.md` | `normal_v5`, `casual_v2`, `formal_v2` are the live set |
| Prompts in DB | migration `008_wave2_three_modes.sql` | mode→prompt assignments, model_id, temperature |
| Raw immutability hook | `src-tauri/src/db/transcripts.rs` | 6–9 (hook contract), 66–73 (insert_raw) |
| UI raw/cleaned/final exposure | `ui/src/pages/Dictations.tsx` | 587–599 (three `<Stage>` views) |

**What's done well, by direct comparison to the user's research doc:**

- **Immutable raw transcript** with a hook gate (`block-raw-transcript-edit`) that statically scans non-test code. This is the safety net the doc demands; it is in place and load-bearing.
- **Three-stage UI surfacing of raw / cleaned / final.** The user already has the "see what the model dropped" affordance per session, even if it isn't labelled as an "Undo AI edit" button (see §5).
- **Deterministic preprocessor with disciplined scope.** Module-level docs explicitly enumerate what is in/out of scope (`preprocessor.rs:21–38`); idempotence is unit-tested (`is_idempotent_on_second_pass`); a list-shaped input is explicitly verified not to be mangled (`list_pattern_passes_through_for_llm`).
- **Strong front-loaded "PRESERVE EVERY SENTENCE" non-negotiable rules** in `normal_v5`, `casual_v2`, `formal_v2`. Each prompt also pins anti-substitution and anti-summarization. These are exactly the disciplines the research doc identifies as best-in-class.
- **Versioned prompts (`normal_v1..v5`) preserved in the DB.** ADR 0008 prompt-versioning means every old session row resolves to the exact prompt that produced it. Provenance is total.
- **Few-shot examples are budgeted, not stuffed.** `prompt_builder::fit_dictionary` and `few_shot::fit_to_budget` drop the tail rather than truncate the raw. Raw transcript is always last in the prompt so the model's "continue this" instinct is aligned with cleanup.

### 1.2 Meeting pipeline

```
audio (loopback + mic, twin-stream)
  → chunker (30 s windows, 2 s overlap, CRC-verified)
  → long-form Whisper
  → deterministic formatter (filler set, repeat collapse, paragraph gaps)
  → merge (two-channel deduplication)
  → persist (canonical transcript, MeetingStatus::Complete)
  → [off the critical path] optional run_llm_pass(...)
       ← prompt_body: summary | action_items | cleaner_punctuation
       ← system header: SYSTEM_HEADER ("Be concise...")
       ← stored in in-memory HashMap<Uuid, String>, NEVER persisted
```

The architectural commitment — *LLM is never in the meetings critical path; meeting transcripts are committed to the DB by the deterministic formatter; the LLM pass is opt-in and ephemeral* — is enforced by the `mc-no-llm-in-critical-path` judge both statically and dynamically. This is correct and impressive.

The dictation feature **reuses** the meeting LLM-pass engine (`run_llm_pass_with_provider`) via `dictation::llm_prompts::resolve_dictation_prompt`, but with dictation-tuned prompt bodies (one speaker, first-person decisions count as actions). This reuse is good.

---

## 2. Where the over-consolidation is actually coming from

The user's research doc correctly identified the structural shape of the failure. The cracks in the *Mockingbird* implementation specifically are these:

### 2.1 The meetings `SYSTEM_HEADER` injects "Be concise" into every LLM pass — including cleaner_punctuation

`meetings/llm_pass.rs:36–37`:

```rust
pub const SYSTEM_HEADER: &str = "You are a meeting-transcript assistant. \
Be concise. Do not invent facts not in the transcript.";
```

`meetings/llm_pass.rs:120–122`:

```rust
pub fn assemble_prompt(prompt_body: &str, transcript_text: &str) -> String {
    format!("{SYSTEM_HEADER}\n\n{prompt_body}\n\n---\n\n{transcript_text}")
}
```

This header is **prepended unconditionally to every LLM pass**, including `cleaner_punctuation`. The cleaner_punctuation body says:

> *Keep the speaker's wording byte-for-byte identical except for whitespace and punctuation. Drop, add, or reorder words → DO NOT.*

The system header above it says: *"Be concise."*

This is the **exact pattern the user's research doc identified as the worst-case failure mode for small models**: two competing objectives in one prompt, where one is general ("be concise") and one is specific ("preserve byte-for-byte"). A 3B–7B local model resolves this ambiguity by collapsing toward the salient attractor — and `concise` + summarization is heavily over-represented in instruction-tuning data. The specific rule wins on a frontier model and loses on a local one.

The `summary` prompt legitimately wants concision. The `action_items` prompt is happy with concision. **The `cleaner_punctuation` prompt is actively damaged by concision instruction.** This single shared header is the most plausible mechanical cause of the over-consolidation behaviour on meetings, and (because dictation reuses the same engine via `resolve_dictation_prompt`) on dictation's `cleaner_punctuation` pull as well.

**Fix shape:** make the system header per-pass, not global. Specifically, `cleaner_punctuation` should ship with a header like *"You are a transcript punctuation assistant. Preserve every word; modify only whitespace and punctuation."* — and the word "concise" should not appear anywhere in its assembled prompt.

### 2.2 No length-ratio sanity check / no graceful degradation

The research doc proposes: *if the LLM output is meaningfully shorter than the input (e.g., < 85 % of input token count) AND no self-correction was flagged, discard the processed version and emit the deterministic-preprocessor output instead.*

I grepped the entire `src-tauri/src` tree for `length_ratio`, `shrink`, `sanity_check`, `too_short` — **no matches.** The pipeline trusts whatever the LLM returns. If the LLM ate the user's preamble, the cleaned row is missing the preamble; the raw row has it, but the user has to manually open the Dictations detail page and copy-paste the raw text to recover. That's exactly the "the model dropped my intro and I don't know" failure the user is trying to eliminate.

This is the second-highest leverage missing piece. The fix is cheap: in `LlmCleaner::run_cleanup`, after `provider.cleanup(req)?`, compute `cleaned_len / pre_text_len` and if it falls under a configurable threshold (suggest 0.6–0.75 depending on how aggressive you want to be — start at 0.65 and tune), log a `tracing::warn!` and return the preprocessor output (`pre_text`) instead. The cleaned row in the DB then carries the preprocessor output (still cleaner than raw) and an annotation that the LLM output was rejected by the sanity check. The deterministic preprocessor's punctuation + capitalization output is *always* a safe fallback — better than raw, but not at risk of dropping content.

Wispr Flow's own post-hoc explanation for their 30-second-truncation bug is exactly this: *"the dictation appears either fully formatted or as plain transcribed text, but never missing."* You can ship that guarantee in a 20-line change.

### 2.3 Cleanup level and tone mode are bundled

Mockingbird's mode dial is `casual / normal / formal`, which controls **register**:

- `casual`: 3B model, contractions, lists rendered inline as prose
- `normal`: 7B model, bullet lists allowed, no headers
- `formal`: 7B model, numbered lists, headers permitted

Wispr Flow's recent UI change (documented post-2025) is explicitly to **separate** these two axes. Their Style tab now exposes:

- **Auto Cleanup** (the consolidation dial): **None / Light / Medium / High** — orthogonal to tone.
- **Styles** (the register dial): Personal / Work / Email / Other, each with Formal / Casual / Excited tonal variants.

The two-axis design is what lets a user who wants "verbatim but in formal register" actually get there. With Mockingbird's current shape, the only way to ask for "preserve everything I said, just punctuate it" is to either (a) use Fragment mode (which is disabled — `enabled=0` per migration 008) or (b) accept the PassthroughCleaner (which is the Wave-4 stub, but bypasses the deterministic preprocessor that the user actually wants).

**Recommendation:** add a second setting, `dictation.cleanup_level: None | Light | Medium | High`, where:

- **None** = raw STT, no preprocessor, no LLM. (Mirrors Superwhisper's default.)
- **Light** = deterministic preprocessor only. No LLM call. Lowest latency. *Should be the default after this change*; the deterministic preprocessor + Whisper's native punctuation handle ~80 % of the polish work, per ADR 0022's own analysis.
- **Medium** = deterministic preprocessor + LLM with **additive-only** prompt (insert punctuation/structure, never delete). This is the current `normal_v5` behaviour after the system-header fix from §2.1.
- **High** = deterministic preprocessor + LLM with full register transformation (current `casual` / `normal` / `formal` semantics, depending on the selected tone).

Tone remains the existing `casual / normal / formal` selection, orthogonal to cleanup level.

This is the single highest-leverage UX change. It directly answers the user's stated frustration: *"I don't want to compensate for every little thing I don't want it to do."* The user wants to dial the LLM's authority *down*; the architecture has to expose that dial as a first-class control.

### 2.4 Whisper's initial_prompt is wired but always passed `None`

`stt/whisper.rs:137–138`:

```rust
if let Some(p) = req.initial_prompt {
    params.set_initial_prompt(p);
}
```

The plumbing exists. But in `dictation.rs:679` and `dictation/ingest.rs:300`:

```rust
initial_prompt: None, // prompt-builder wiring is Phase 4
```

The comment was a TODO from Phase 4 that never landed. Meanwhile, Whisper's `initial_prompt` is the canonical place to feed:

1. **The user's dictionary** (proper nouns, project names, library names) — Whisper's last-224-token window is enough room for the top-N most-frequent dictionary entries.
2. **Light style priming** ("This is a formal document about software architecture.") — Whisper mimics the prompt's punctuation and tone.

Every named-entity miss that Whisper makes today becomes one more thing the local LLM has to clean up downstream. That is precisely the wrong place to spend the local model's instruction-following budget — *because* the local model will then summarize while it's also trying to fix the spelling of `rusqlite`.

This change has compounding benefits: better Whisper output reduces the LLM's workload, which reduces the LLM's opportunity to over-consolidate. It also moves dictionary substitution **upstream** of the LLM (today it's an item in the prompt, which the LLM may or may not honour).

### 2.5 `casual` runs at 3B; per the project's own findings, 3B can't hold the rules

ADR 0022's Context section is unusually direct: *"Qwen-3B-q4 doesn't have the headroom for this... The model's attention mechanism cannot reliably honour [the rules] under the few-shot pressure."* The fix in Wave 2 was to move `normal` and `formal` to 7B, and the eval scores went from "preamble dropped" to "96.8 % preservation on the baseline."

`casual` was left at 3B in migration 008 because "casual one-liners (≤ 5 words) should land in ≤ 500 ms total." That's a defensible latency argument. But casual mode is also the mode users use for **the long Slack message about what they had for lunch**, not just one-liners — and on those, the same instruction-following collapse happens.

External research backs ADR 0022's finding. Qwen 2.5 14B-instruct outperforms 7B-instruct on 7 of 8 standard benchmarks, with the largest deltas on instruction-following and structured-output. A 7B-q4 at 3–4 GB VRAM is the floor for restraint-heavy cleanup; 3B-q4 is below it.

**Recommendation:** move `casual` to `qwen2.5:7b-instruct-q4_K_M` (or, if VRAM permits with Whisper resident, `qwen2.5:7b-instruct-q5_K_M` — see §3 below). Accept the latency hit. Pair it with §3.4's "skip LLM entirely for short utterances" rule so the latency budget on actual one-liners is still recovered.

### 2.6 Q4_K_M everywhere — Q5_K_M is the better default for this task

Every mode in `008_wave2_three_modes.sql` uses Q4_K_M. The 2025–2026 quantization research consensus:

- **Q4_K_M:** ~3.5 % quality loss vs FP16. Fine for routine chat, extraction, and other "tolerant" tasks.
- **Q5_K_M:** ~1.5 % quality loss vs FP16. The "sweet spot" for nuanced reasoning, structured output, and instruction-following — *all* of which describe transcript cleanup.

A `qwen2.5:7b-instruct-q5_K_M` is ~5.2 GB; vs Q4_K_M's ~4.7 GB, that's a ~500 MB delta. On the RTX 2060 / 6 GB target hardware with Whisper-large-v3-turbo (~2 GB) resident, this is tight but workable. On any newer GPU, it's a free win.

The user's research doc's exact framing — *"A 14B at Q4 will out-restrain a 7B at Q8 for this task"* — is correct in general, but **between the two realistic options here (7B-Q4 vs 7B-Q5)**, 7B-Q5 is the better trade. 14B is gated by VRAM on the target hardware. Q5 is gated by ~500 MB extra.

### 2.7 Temperature 0.1 is borderline; 0.2 is more robust

- Dictation `normal` + `formal`: temperature 0.1 (`008_wave2_three_modes.sql:90`)
- Dictation `casual`: temperature 0.2 after `010_adr0024_prompt_v2.sql:79` bumped it from 0.4
- Meetings LLM pass: temperature 0.2 (`meetings/llm_pass.rs:48`)

2025–2026 research consensus: **temperature 0 creates an increased risk of repetition/loop degeneration on small models**, and Ollama's own quantization paths have been observed to produce repetitive output at strict 0 on some quantizations (this is exactly the rationale already cited in `meetings/llm_pass.rs:47`). 0.1 is closer to 0 than to 0.2.

On a Q4 quantized model, the recommendation is to *prefer 0.2 over 0.1* for the same reason the meetings code already does: avoid the repetition cliff. Quality differences between 0.1 and 0.2 for cleanup tasks are within the noise of run-to-run variance; the failure-mode difference is not.

**Recommendation:** standardize on `temperature = 0.2` across all modes. Cheap, single-column UPDATE migration. The migration message can document this as "align with meetings/llm_pass.rs which already shipped this."

---

## 3. What's already correct (don't break it)

A reviewer's worst sin is recommending changes that overwrite work that's already right. To be explicit:

1. **The deterministic preprocessor is the right architecture.** The pipeline order (`self-correction → tier-1 fillers → tier-2 fillers → stutters → cues → whitespace → capitalize → terminal punct`) is correct and unit-tested. Don't touch it.
2. **Few-shot example budgeting (`fit_to_budget`).** Stuffing examples until the prompt overflows would be worse than truncating raw; the current implementation gets the tradeoff right.
3. **Raw transcript immutability + the hook gate.** This is the load-bearing safety net. The hook scans non-test code for `update_raw` / `upsert_raw` / `UPDATE transcripts WHERE stage='raw'`. Keep this *strictly*.
4. **LLM is off the meetings critical path.** ADR 0026 + the `mc-no-llm-in-critical-path` judge are the right binding for meetings. The deterministic formatter is the canonical pass; the LLM is opt-in and ephemeral. This is in the top decile of dictation/meeting tool architectures, full stop.
5. **The on-demand LLM pass card on the dictations detail view (`LlmPassCard`).** This is exactly the "Transforms / Command Mode" pattern from the user's research doc — aggressive rewriting is *pulled* by the user, not pushed by default. Keep this pattern; extend it (see §4).
6. **The empirical iteration discipline visible in the prompt history** (`normal_v3 → v4` after the "list of keyboard supplies" smoketest; `casual_v2` iter-0 → iter-1 after the imperative-content failure). That this exists in the markdown footers and ADR 0024 is *the* sign that this team takes the cleanup problem seriously. Don't lose this discipline.

---

## 4. Strategic recommendation: when should the LLM run at all?

The user's framing question — *"do we even need it, or when should we use it strategically?"* — deserves a direct answer.

The deterministic preprocessor + Whisper-large-v3-turbo's native punctuation can produce a usable output for **at least 70 %** of typical dictation utterances. Specifically:

| Utterance shape | LLM adds value? | Notes |
|---|---|---|
| One-liner ≤ 10 words ("yes please send me the link") | **No** | Preprocessor + Whisper punctuation is enough. LLM is a latency tax. |
| Single-sentence chat reply | **No** | Same. |
| Multi-sentence chat reply, no structure | **Marginal** | Preprocessor handles fillers + caps + terminal punct. LLM adds minor flow fixes. Mostly a tone thing. |
| Imperative content for IDE/code (`"create a function that..."`) | **No, and risky** | Casual_v2's iter-1 fix exists because the LLM kept *interpreting* these. Better to skip. |
| Multi-paragraph thought | **Yes** | LLM detects paragraph breaks from semantic shift; preprocessor only handles verbal cues. |
| List with verbal enumeration ("first, second, third") | **Yes** | Preprocessor preserves cues; LLM renders as bullets. |
| List without verbal enumeration | **Yes** | Only LLM can detect implicit lists. |
| Register-shifted dictation (user picked `formal`) | **Yes** | Whole point of register transformation. |
| Email body | **Yes** | Multi-paragraph + register. |

The strategic positioning that follows:

### Default: Light cleanup, LLM-skip on short utterances

Make the new `cleanup_level` default to `Light` (deterministic preprocessor only) for short utterances. Add a heuristic in `LlmCleaner::run_cleanup`:

```rust
// Short utterance after deterministic preprocessing → skip LLM.
// Threshold to start: 12 words. Tunable via a SettingKey.
let word_count = pre_text.split_whitespace().count();
if word_count <= self.config.llm_skip_word_threshold
    && !pre.notes.looks_listy()
{
    return Ok(pre_text);  // preprocessor output, no LLM hop
}
```

`looks_listy()` already exists in `preprocessor.rs:152–158` as a stub — wiring it up (true when ≥ 2 ordinal cues were rendered or ≥ 3 enumeration markers detected) is the right shape.

Result: the casual one-liner "yes please send me the link" goes preprocessor → done, ~5 ms total cleanup latency vs ~2–3 s. The latency budget for actual long-form dictations is then unbounded — you can afford a 14B model for the cases that actually need an LLM, because most cases don't reach the LLM.

### When the LLM does run: split it into scoped passes (eventually)

The user's research doc proposes splitting cleanup into three single-job passes (disfluency removal → self-correction → additive-only formatting). For Mockingbird specifically:

- **Disfluency removal + self-correction** is already done by the deterministic preprocessor. ✓
- **Additive-only formatting** is a single re-scoping of the existing prompts. Don't add new LLM passes — re-scope the existing one so the only objective is *"add punctuation, paragraph breaks, and list structure; never remove content."*

The "make it concise" objective that's implicit in the current `casual_v2` (which renders lists *inline as prose*, i.e. *consolidates*) is the exact wrong default for the always-on path. Move the concision/consolidation behaviour out of always-on cleanup and into the on-demand `LlmPassCard` flow — a separate "Compress" or "Tighten" Transform the user clicks when they want it.

This is the architectural answer to the over-consolidation. Don't try to teach the small model to compress *only when appropriate*. Remove compression from its job description entirely. The user can always pull it.

### When to use cloud Claude vs local Ollama

The Claude provider exists (`cleanup/claude.rs`) but is currently behind an opt-in. Reasonable use cases for cloud Claude in this product:

- The user explicitly selects `formal` mode for a high-stakes email and accepts the privacy tradeoff per-session.
- An on-demand Transform that needs nuanced register transformation (the "Compress" / "Tighten" pull above).
- Action items on a meeting with named participants and implicit owners — a frontier model gets owner-inference right far more often than 7B-Q4.

In all three cases, the gate is **explicit per-call opt-in**, not "Claude on, Ollama off as a global setting." That's what preserves the "no telemetry / no cloud by default" position. The current provider abstraction supports this; the UX wiring just needs to surface the cloud option as a one-shot toggle on the LlmPassCard, not as a Settings flip.

---

## 5. Prioritized change set

Ordered by leverage (impact × ease). Each item maps to specific files. None of these require schema migrations except where noted.

### P0 — single-day changes, highest leverage

1. **Make the meetings system header per-pass.** Replace the global `SYSTEM_HEADER` in `meetings/llm_pass.rs:36` with a per-pass header table. `cleaner_punctuation` gets *"You are a transcript punctuation assistant. Preserve every word; modify only whitespace and punctuation."* `summary` and `action_items` keep their concision instruction. This is the single most likely fix for the user's observed over-consolidation on meetings.

2. **Add a length-ratio sanity check in `LlmCleaner::run_cleanup`.** After `provider.cleanup(req)?`, if `result.text.split_whitespace().count() < 0.65 * pre_text.split_whitespace().count()` and `pre.notes.self_corrections == 0`, log + return `pre_text` instead. Suffix `last_model_used` with `-shrink-fallback` so the provenance column tells the truth. ~20 lines + 2 unit tests.

3. **Wire Whisper's `initial_prompt` from the dictionary.** Replace the `None` at `dictation.rs:679` and `dictation/ingest.rs:300` with a `make_whisper_prompt(dictionary)` call that emits a short style-priming sentence containing the top-N most-frequent dictionary terms. Cap at 200 tokens to stay under Whisper's 224 limit. Most-frequent ordering is already implied by `dictionary.use_count`.

4. **Bump every mode's temperature to 0.2.** Single column UPDATE migration (call it 018). The justification text already exists in `meetings/llm_pass.rs:47`.

### P1 — week-of changes

5. **Move `casual` to `qwen2.5:7b-instruct-q4_K_M`.** Migration 019. Update `casual_v2`'s few-shots are already 7B-friendly (you raised the bar for `normal` and the same examples work). Reassess `casual`'s latency budget after wiring the LLM-skip rule in P1 #6.

6. **Implement LLM-skip-on-short-utterance.** Add `SettingKey::LlmSkipWordThreshold` (default 12, tunable), wire `looks_listy()` to be non-stub (return true on ≥ 2 ordinal cues or ≥ 3 enumeration markers), and short-circuit in `LlmCleaner::run_cleanup` when the word count is below threshold and no list structure was detected. Run latency benches before/after — expect ~70 % of dictations to land in < 100 ms.

7. **Switch all model_ids from `q4_K_M` to `q5_K_M`.** Pre-flight check: confirm Whisper-large-v3-turbo + qwen2.5:7b-q5_K_M fit in 6 GB VRAM on the dev box; if borderline, gate this behind a setting and let the first-run wizard probe VRAM and pick. Migration 020.

8. **Add an explicit cleanup-level dial.** Introduce `SettingKey::DictationCleanupLevel: None | Light | Medium | High` (`Light` is the new default). Map:
    - `None` → return raw STT directly (skip preprocessor + LLM)
    - `Light` → preprocessor only, no LLM
    - `Medium` → preprocessor + LLM with an **additive-only** prompt (new prompt body, `normal_v6_additive.md`)
    - `High` → preprocessor + current mode-specific LLM (existing `casual_v2` / `normal_v5` / `formal_v2`)

    Tone (`casual / normal / formal`) becomes orthogonal — it only matters at level `High`. Schema migration 021 adds the column + seeds `Light` as the default.

### P2 — patient improvements

9. **Reframe the dictations UI "Raw / Cleaned / Final" Card as an explicit "Undo AI edit" affordance.** The three stages are already there (`Dictations.tsx:587–599`); add a button on the cleaned/final stage that pastes the raw transcript into the user's clipboard as a one-click recovery, and surface this prominently when the length-ratio fallback (P0 #2) fired. Mirror Wispr's "Undo AI edit" — the affordance the research doc identifies as the trust baseline.

10. **Add an `edit-free-send` instrumentation column.** When a session's `cleaned_text` is injected without subsequent user edits in the target app, mark the session as edit-free. (Detecting downstream edits is hard cross-app; a soft heuristic is whether the user invoked the LlmPassCard or copied raw within 5 minutes of injection.) Surface the rate in Insights. This is your replacement for WER and the metric that will tell you whether changes are working.

11. **Author a "Compress" / "Tighten" Transform on the LlmPassCard.** Move the *consolidation* behaviour out of always-on cleanup and into an opt-in card action, alongside the existing summary / action_items / cleaner_punctuation. This is the Wispr "Transforms" pattern and the cleanest way to retain the value of compression while removing its push-default risk.

12. **Optionally: add a `cleaner_punctuation` quality regression eval.** The codebase already has `src-tauri/eval/`; a fixture-driven eval that feeds 20 preamble-bearing transcripts through `cleaner_punctuation` and asserts the preamble survives (token-set comparison) would catch the P0 #1 regression class permanently. Pair with `phase-mc` judges.

### P3 — keep for later

- **Consider Calm-Whisper or large-v3-turbo + non-speech-suppression for meetings.** Whisper-large-v3 hallucinates on silence (`"thank you for watching"`-style artifacts). Calm-Whisper claims ~80 % reduction in non-speech hallucinations at ≤ 0.1 % WER cost. Worth a Phase-MC-v2 ADR if those artifacts have been showing up in meeting captures.
- **Evaluate Canary Qwen 2.5B and Parakeet TDT 2.0** as alternative STT engines once their English WER (5.63 % per Open ASR Leaderboard) holds up on the user's accent / domain. Whisper-large-v3-turbo is currently solid; no need to move until there's user-visible quality pressure.

---

## 6. Honest answers to the user's exact questions

**"Do we even need it?"**
For short-to-medium dictations, mostly not. The deterministic preprocessor + Whisper's native punctuation handles ~70 % of utterances at acceptable quality. The LLM is necessary for: (a) implicit list detection, (b) multi-paragraph semantic structure, (c) register transformation. Default to skipping it; reach for it when the utterance actually needs it. The P1 #6 change implements this.

**"When should we use it strategically?"**
Three cases, in order of confidence:
1. **The user explicitly chose `formal` register and the input is multi-sentence.** Register transformation is the LLM's strongest play.
2. **The deterministic preprocessor flagged the input as list-shaped (`looks_listy() == true`).** The LLM is good at rendering bullets and lead-ins.
3. **The user pulled an on-demand Transform (`LlmPassCard`).** They asked for it; the model gets to operate.

Everywhere else, prefer the preprocessor's output.

**"Don't just take my whims or suggestions — what does the research say objectively?"**
The user's research doc is well-sourced and the prescriptions are sound. Three places where the codebase already exceeds the doc's recommendations (raw immutability, deterministic preprocessor with explicit scope, ADR 0010-style provenance). Three places where the doc's prescriptions are *exactly* the missing pieces here (per-pass system headers, length-ratio fallback, additive-only formatting prompts). One place where the doc's recommendation is theoretically right but the realistic choice is different: the *"14B at Q4 beats 7B at Q8"* line — true in general, but the practical choice on this hardware is *7B at Q5 beats 7B at Q4*. 14B is gated by VRAM.

**"How do I stop compensating for every little thing?"**
Change the architecture so the LLM is *invited in* on the cases that need it rather than *always-on suppressing* its worst behaviour. The combination of P0 #1 (system header per-pass), P0 #2 (length-ratio fallback), P1 #6 (skip on short utterances), P1 #8 (Light as the new default cleanup level), and P2 #11 (move compression to a pull-only Transform) collectively means: the LLM no longer has the standing authority to consolidate, the user no longer has to tell it not to.

---

## 7. Risks of these changes

- **Latency regressions if you move `casual` to 7B without the LLM-skip rule.** Mitigation: ship P1 #5 and P1 #6 together, or ship #6 first and #5 second.
- **Eval regressions from a temperature bump (P0 #4).** The current eval suite was tuned at 0.1/0.2/0.4. Re-run the full eval at 0.2 before merging; if scores drop, treat as a per-mode decision rather than a global one.
- **Q5_K_M VRAM pressure on the 6 GB dev box.** Mitigation: gate behind the first-run wizard probe (P1 #7); ship `q4_K_M` as the fallback.
- **Cleanup-level dial confusion.** Adding a new orthogonal dial introduces UX surface area. Mitigation: ship P1 #8 with a one-time first-run nudge explaining the two-axis model; offer a "Use the old behaviour" preset that maps to `High` cleanup + the user's previous tone mode.

---

## 8. What I'd do first if I had one afternoon

In strict order:

1. **P0 #1** (per-pass system headers in meetings/llm_pass.rs) — 15 minutes, single file, no migration.
2. **P0 #2** (length-ratio fallback in llm_cleaner.rs) — 30 minutes including tests, single file, no migration.
3. **P0 #3** (Whisper initial_prompt from dictionary) — 1–2 hours including dictation.rs + ingest.rs + a small helper module + tests. No migration.
4. **P0 #4** (temperature 0.1 → 0.2) — 5 minutes, one migration file.

Total: a half-day for changes that, together, attack the over-consolidation symptom from four independent angles. After that lands, observe the next week of dictations — most likely the symptom is gone or substantially reduced, and the P1 changes become refinements rather than rescues.

---

## Sources

Research synthesis drew from:

- [Wispr Flow — How do I use Smart Formatting & Backtrack](https://docs.wisprflow.ai/articles/5373093536-how-do-i-use-smart-formatting-and-backtrack)
- [Wispr Flow Smart Formatting (Auto Cleanup levels) — spokenly.app](https://spokenly.app/blog/wispr-flow-review)
- [Northflank — Best open source STT in 2026 (benchmarks)](https://northflank.com/blog/best-open-source-speech-to-text-stt-model-in-2026-benchmarks)
- [Deepgram — Whisper-v3 Hallucinations on Real World Data](https://deepgram.com/learn/whisper-v3-results)
- [Calm-Whisper — non-speech hallucination reduction (arXiv 2505.12969)](https://arxiv.org/html/2505.12969v1)
- [LLM-Stats — Qwen2.5 14B vs 7B instruct comparison](https://llm-stats.com/models/compare/qwen-2.5-14b-instruct-vs-qwen-2.5-7b-instruct)
- [Quantization Q4_K_M vs Q5_K_M vs Q8 (2026)](https://bmdpat.com/blog/gguf-quantization-q4-q5-q8-explained-2026)
- [Speechmatics — The Problem with Word Error Rate](https://www.speechmatics.com/company/articles-and-news/the-problem-with-word-error-rate)
- [GDELT — LLM Infinite Loops at temperature 0](https://blog.gdeltproject.org/llm-infinite-loops-in-llm-entity-extraction-when-temperature-basic-prompt-engineering-cant-fix-things/)
- [Whisper initial_prompt guidance — Sotto](https://sotto.to/blog/improve-whisper-accuracy-prompts)
- [Whisper-Large-v3-Turbo — Hugging Face](https://huggingface.co/openai/whisper-large-v3-turbo)
- [Superwhisper vs Wispr Flow — Voibe](https://www.getvoibe.com/resources/wispr-flow-vs-superwhisper/)

Codebase findings drew from direct reads of:
- `src-tauri/src/cleanup/{mod,llm_cleaner,preprocessor,prompt_builder,ollama}.rs`
- `src-tauri/src/cleanup/prompts/{normal_v3..v5,casual_v2,formal_v2,fragment,verbose}.md`
- `src-tauri/src/meetings/{llm_pass,formatter}.rs`
- `src-tauri/src/meetings/prompts/{summary,action_items,cleaner_punctuation}.md`
- `src-tauri/src/dictation/{llm_prompts,ingest}.rs`, `src-tauri/src/dictation.rs`
- `src-tauri/src/stt/whisper.rs`
- `src-tauri/src/db/transcripts.rs`
- `src-tauri/src/db/migrations/{003,005,008,010}*.sql`
- `docs/adr/0008`, `0010`, `0021`, `0022`, `0024`, `0026`
- `ui/src/pages/Dictations.tsx`
