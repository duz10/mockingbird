# Judge: mc-no-llm-in-critical-path (Phase MC)

**Target:** `src-tauri/src/meetings/lifecycle.rs`,
`src-tauri/src/meetings/persist.rs`, `src-tauri/src/meetings/runtime.rs`,
`src-tauri/src/meetings/long_form_stt.rs`,
`src-tauri/src/meetings/formatter.rs`,
`src-tauri/src/meetings/merge.rs`,
`src-tauri/src/meetings/llm_pass.rs`, ADR 0026 §"Critical-path
invariant".

**Question:** Does the recording-to-canonical-transcript critical
path (the `start_meeting` → capture → long-form-stt → format →
merge → persist → emit-state-done sequence) construct an
`OllamaProvider`, call into the `cleanup::` module, or otherwise
reach any LLM? The answer must be **NO**.

**Rationale:** ADR 0026 §"Critical-path invariant" commits the
meeting subsystem to a fully-deterministic pipeline from audio to
the `meetings` table row. The optional LLM pass exists in a
*sibling* module (`llm_pass.rs`) reachable only via the
`meeting_run_llm_pass` IPC command, which writes its output into
an in-memory `HashMap` cache, never into the DB. This separation
is what lets us:

  1. Guarantee transcript reproducibility — the canonical row is
     a function of audio + formatter version + settings, with no
     temperature/model dependency.
  2. Guarantee crash semantics — if Ollama is down or slow, the
     meeting still completes.
  3. Make the `mc-formatter-deterministic` and
     `mc-long-form-stitched-losslessly` judges meaningful — both
     judges are predicated on the absence of an LLM in the path
     they verify.

A violation here doesn't necessarily produce a wrong transcript
on day one — it might just slow things down or add a hidden
failure mode — but it permanently couples Mockingbird's meeting
quality to an external model, breaking the local-first invariant
in spirit if not in letter. The judge is therefore a static
architectural assertion, not a runtime counter; the architecture
itself enforces the invariant, and the judge documents how to
verify the architecture hasn't drifted.

**Pass criteria — ALL of:**

1. **`OllamaProvider` is referenced in exactly one meetings file
   — `llm_pass.rs`:**

   ```powershell
   Select-String -Path src-tauri\src\meetings\*.rs `
     -Pattern 'OllamaProvider'
   ```

   Expected: every match's path is `meetings\llm_pass.rs` OR a
   doc-comment line in `meetings\mod.rs` /
   `meetings\lifecycle.rs` that explicitly NAMES the invariant
   (the `//! NO OllamaProvider` lines). Any match in any other
   file under `meetings\` that is NOT a doc comment is a
   violation.

2. **`cleanup::` is imported in exactly one meetings file —
   `llm_pass.rs`:**

   ```powershell
   Select-String -Path src-tauri\src\meetings\*.rs `
     -Pattern '^use crate::cleanup'
   ```

   Expected: every match's path is `meetings\llm_pass.rs`. No
   other meetings module may import from `crate::cleanup::*`.

3. **The critical-path modules do not transitively reach
   Ollama:**

   ```powershell
   $criticalPath = @(
     'src-tauri\src\meetings\lifecycle.rs',
     'src-tauri\src\meetings\runtime.rs',
     'src-tauri\src\meetings\persist.rs',
     'src-tauri\src\meetings\long_form_stt.rs',
     'src-tauri\src\meetings\formatter.rs',
     'src-tauri\src\meetings\merge.rs',
     'src-tauri\src\meetings\chunker.rs',
     'src-tauri\src\meetings\capture.rs'
   )
   Select-String -Path $criticalPath `
     -Pattern 'OllamaProvider|run_llm_pass|LlmCleaner|reqwest::|ureq::'
   ```

   Expected: every match (if any) is a `//` or `//!` comment
   line that names the invariant. Live `use` statements or
   function calls = violation.

4. **The `meeting:state = done` emit path is not gated on an
   LLM call:**

   Open `src-tauri/src/meetings/lifecycle.rs` and confirm that
   the `stop_meeting` → `persist_meeting` → `emit_state("done",
   …)` call chain has no `?` early-return guarding on an
   `OllamaProvider::*` or `run_llm_pass` call. The persist must
   complete and emit done unconditionally on a successful merge
   + persist, regardless of whether the user later asks for an
   LLM pass.

5. **The `meetings` table schema has no LLM-output column:**

   ```powershell
   Select-String -Path src-tauri\src\db\migrations\*.sql `
     -Pattern 'CREATE TABLE meetings|llm_output|llm_text|llm_summary'
   ```

   Confirm the `meetings` table definition has no column named
   `llm_*`. The LLM output lives in
   `MeetingRuntimeShared::llm_pass_cache` (an in-memory
   `Arc<Mutex<HashMap<String, String>>>`) only, never in any DB
   row. This is the architectural backstop — if a future
   contributor tries to "just add a column to persist LLM
   output", the migration review catches it.

**On failure:**

- **Block the `phase-mc-complete` tag.** This is the binding
  ADR-0026 invariant; a violation makes every other meeting
  invariant unenforceable.
- If criterion 1 surfaces a new file: that's the bug — the LLM
  pass crept into the critical path. Move the call back to
  `llm_pass.rs` and reach it only via the
  `meeting_run_llm_pass` command.
- If criterion 3 surfaces a `reqwest::` or `ureq::` call: a
  contributor added a direct HTTP client to the critical path,
  bypassing the architecture. Rip it out.
- If criterion 5 surfaces a migration adding an LLM column:
  revert the migration. ADR 0026 forbids it.

**Last run:** _Wave 6 — **GREEN**. `OllamaProvider` and
`use crate::cleanup` both appear exclusively in
`meetings/llm_pass.rs` (plus invariant-naming doc comments in
`mod.rs` and `lifecycle.rs`). The critical-path files contain
zero live references to Ollama / LLM constructs. The `meetings`
table schema (migrations 011) has no `llm_*` column._
