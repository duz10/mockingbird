# Judge: mc-formatter-deterministic (Phase MC)

**Target:** `src-tauri/src/meetings/formatter.rs` (`format`,
`strip_phrases_and_fillers`, `strip_repeats`, `capitalize`),
ADR 0026, master plan §"Cross-wave invariants" #6.

**Question:** Is the meeting transcript formatter pure and
deterministic — same input ⇒ byte-identical output, with `format` a
fixpoint (`format(format(x)) == format(x)`) under arbitrary
Unicode input?

**Rationale:** The formatter is the **canonical** post-stitch pass.
ADR 0026 forbids an LLM in the critical recording-to-canonical-
transcript path, which means everything between the per-channel
stitched segments and the row written to `transcripts(stage='raw')`
has to be deterministic Rust — no RNG, no clock reads, no global
state, no environment-dependent branches. If `format` ever drifts
from pure, the `transcripts` row stops being a stable function of
the audio + config, which breaks every downstream invariant
(provenance, re-formattable history, judge reproducibility). The
fixpoint property is the strongest sanity check available: it
catches off-by-one capitalization, double-applied filler strips,
and accidental capture of mutable state in one shot.

**Pass criteria — ALL of:**

1. **31 unit tests for `formatter::*` pass:**

   ```powershell
   pwsh scripts\cargo-with-cuda.ps1 test --release --lib `
     -- meetings::formatter::tests
   ```

   Covers: empty input, single token, filler strip (`um`/`uh`/
   `you know`/`i mean`/`sort of`/`you see` — greedy-longest
   match), repeat collapse (`the the` → `the`), paragraph-gap
   boundary at the exact threshold and ±1 ms, three-segment mixed
   gaps, punctuation preservation, UTF-8 (CJK + emoji) pass-through
   without panic, leading/trailing whitespace strip, filler-only
   input → empty, and the `strip_fillers = false` / `strip_repeats
   = false` opt-outs.

2. **Two proptests in `formatter.rs` pass:**

   ```powershell
   pwsh scripts\cargo-with-cuda.ps1 test --release --lib `
     -- meetings::formatter::tests::format_is_idempotent_fixpoint `
        meetings::formatter::tests::format_never_panics_on_arbitrary_unicode
   ```

   - `format_is_idempotent_fixpoint(segments)` — for any
     reasonably-shaped segment vector, `format(format(x))` equals
     `format(x)` bytewise. **This is the fixpoint property.**
   - `format_never_panics_on_arbitrary_unicode(segments)` —
     proptest pushes arbitrary Unicode tokens through `format` to
     catch any UTF-8 boundary mishap in capitalization or
     repeat-collapse.

3. **Static purity check (eyeball the diff):**

   ```powershell
   git diff phase-mc-start..HEAD -- `
     src-tauri\src\meetings\formatter.rs `
     src-tauri\src\meetings\filler_words.rs |
     Select-String -Pattern 'std::time|Instant|SystemTime|thread_rng|rand::|Once|OnceCell|OnceLock|env::|getenv'
   ```

   Expected output: empty. The formatter must not read the system
   clock, must not call into an RNG, must not consult environment
   variables, and must not hold lazy-init shared state. If any of
   these appear in `formatter.rs` or `filler_words.rs`, FAIL.

4. **`FILLER_WORDS` is a const-init `phf` set, not a `Lazy<HashSet>`:**

   ```powershell
   Select-String -Path src-tauri\src\meetings\filler_words.rs `
     -Pattern 'Lazy<|OnceCell|OnceLock|lazy_static'
   ```

   Expected output: empty. The set is built at compile time
   (`phf::phf_set!`) so its initialization is provably
   deterministic. Falling back to a `Lazy`-initialized set
   re-introduces global-state ordering, which proptest probably
   wouldn't catch but the judge will.

**On failure:**

- **Block the `phase-mc-complete` tag.** Determinism is the
  cornerstone invariant of the meeting subsystem.
- If criterion 2's fixpoint property fails: examine the proptest
  shrink — it'll print a minimal segment vector that breaks the
  property. Common culprits: a regex with the `g` flag where the
  per-call state carries across, or a `to_uppercase()` call that
  hits a Unicode special-casing rule which expands then expands
  again (Greek sigma is the classic offender).
- If criterion 3 surfaces a clock/RNG/env read: that's the bug;
  rip it out. There is **never** a justification for non-pure
  formatter code in this module.

**Last run:** _Wave 6 — **GREEN**. 31 unit + 2 proptest cases
pass on `--release --no-run` link; static purity grep returns
empty across `formatter.rs` and `filler_words.rs`._
