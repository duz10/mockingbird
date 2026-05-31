# Judge: kg-dictation-untouched (Phase 1D Wave 1D.6)

**Target:**
- `src-tauri/src/dictation.rs::try_enqueue_for_kg_filing` (the
  3-gate cascade authored Wave 1D.1)
- `src-tauri/src/dictation/runtime.rs::start` vs.
  `runtime.rs::start_kg_note` (the two distinct PTT/KG entry
  points; the discriminator is the `capture_kind` they bind into
  the orchestrator's `NewSession`)
- `src-tauri/src/commands/dictation.rs` (PTT IPC handlers)
- `src-tauri/src/kg/source_gate_invariant.rs` (the Phase 1D
  deterministic probe that operationalizes this assertion)

**Question:** A standard `Dictation`-class session — produced by
the legacy PTT path or the in-app dictation hotkey — must produce
**ZERO** writes to any `kg_*` table, regardless of the
`KgGraphEnabled` toggle state. Does the current code preserve
this property?

The answer must be **YES**. This is structurally true post-Wave
1D.1 because of the source gate in
`try_enqueue_for_kg_filing` (rejects any `capture_kind !=
KgNote`); this judge formalizes the assertion so a future
"helpful" patch removing or reordering the gates cannot land
without tripping a deterministic probe.

**Rationale:** ADR 0052 §"Acceptance gates" J2 lifts the Phase MC
[`mc-dictation-untouched`](../phase-mc/mc-dictation-untouched.md)
invariant ("sealed dictation files unchanged") into a Phase 1D
twin: the runtime behavior of those files for standard dictations
must remain untouched by the KG subsystem. Three reasons:

1. **The KG subsystem is a sibling to dictation, not a fork of
   it.** ADR 0049 §"Sandbox isolation" mandates that any future
   surgery on the KG pipeline cannot reach back into the
   dictation pipeline's user-visible behavior.

2. **The Phase 1D drift correction is the reason the gates
   exist.** Pre-1D, every successful dictation enqueued for KG
   filing the moment the toggle was on — exactly the behavior
   this judge now forbids. The judge is the regression net for
   the drift correction itself.

3. **The Phase MC invariant `mc-dictation-untouched` is a *diff*
   judge** (it inspects file paths in the commit range). This
   judge is its **runtime** complement: the cascade can be
   modified across the seal boundary (and is — Wave 1D.1 added
   the source gate to it), but the *behavior for standard
   dictations* must not change. The Phase MC judge alone would
   not catch a behavioral regression that left the file list
   unchanged.

**Pass criteria — ALL of:**

1. **Source-gate probe cells 1 & 2 are GREEN.** This is a
   re-assertion of [`kg-source-gate-invariant`](
   kg-source-gate-invariant.md) criterion 1 narrowed to the two
   `capture_kind = Dictation` cells:

   ```powershell
   powershell -File scripts\cargo-with-cuda.ps1 run --release `
     --bin kg_source_gate_invariant
   ```

   Expected: probe output contains

   ```text
   ✓ dictation + toggle off → 0 queue row(s) (expected 0)
   ✓ dictation + toggle on → 0 queue row(s) (expected 0)
   ```

   These two lines being absent or showing non-zero observed
   counts is a violation regardless of the overall probe verdict.

2. **The audio path's 3-gate cascade has the source gate before
   the toggle gate.**

   ```powershell
   Select-String -Path src-tauri\src\dictation.rs `
     -Pattern 'CaptureKind::KgNote|SettingKey::KgGraphEnabled' `
     -SimpleMatch:$false
   ```

   The first match must be the `CaptureKind::KgNote` line; the
   `SettingKey::KgGraphEnabled` line must follow it (within the
   `try_enqueue_for_kg_filing` body). ADR 0052 §D1 rationale:
   the source gate is what reverses the trigger direction; the
   toggle gate is the kill switch sitting after it. Reordering
   them is a violation.

3. **`kg_graph_off_invariant` probe still sweeps both
   `(Dictation, KgNote)` capture kinds under toggle-off across
   all 8 `InjectionOutcome` variants.**

   ```powershell
   powershell -File scripts\cargo-with-cuda.ps1 run --release `
     --bin kg_graph_off_invariant
   ```

   Expected: output contains `× (Dictation, KgNote)` and `5 kg_*
   tables empty across both capture_kinds` for every variant. The
   probe was extended at Wave 1D.1 (`mb-pxzk`) to sweep both
   capture kinds; a regression that dropped the `Dictation`
   sweep would mask criterion 1's verdict.

4. **No `kg::enqueue_for_filing` call sites outside the gated
   helper.**

   ```powershell
   Select-String -Path src-tauri\src `
     -Pattern 'kg::enqueue_for_filing|kg::store::enqueue_for_filing' `
     -Recurse
   ```

   Expected: exactly TWO matches — one inside
   `dictation.rs::try_enqueue_for_kg_filing` (the audio path)
   and one inside `kg/ingest_text.rs::ingest_text_note` (the
   text-note path). A third call site anywhere else (e.g. a new
   IPC handler, a backfill harness landed prematurely) is a
   potential bypass of the source gate and must be reviewed.

5. **The Phase MC judge
   [`mc-dictation-untouched`](../phase-mc/mc-dictation-untouched.md)
   is still GREEN** for the Wave 1D commit range.

   ```powershell
   git diff --name-only phase-mc-complete..HEAD -- `
     src-tauri\src\hotkey\state.rs `
     src-tauri\src\hotkey\windows.rs `
     src-tauri\src\hotkey\driver.rs `
     src-tauri\src\injection\ `
     src-tauri\src\recording_window.rs `
     src-tauri\src\cleanup\provider.rs `
     src-tauri\src\cleanup\llm_cleaner.rs
   ```

   Expected: empty. `src-tauri\src\dictation*` is **intentionally
   omitted** from this narrowed sweep — Wave 1D.1 legitimately
   added the source gate to `dictation.rs` per ADR 0052, which
   supersedes the original Phase MC seal of that file. The other
   sealed dictation-adjacent paths remain untouched.

**On failure:**

- **Block the Wave 1D.6 / Phase 1D seal.**
- Criterion 1 mismatch ⇒ source-gate is bypassed for standard
  dictations. Highest-severity failure: a default-on KG toggle in
  the field would silently file every PTT capture. Fix
  immediately and re-run criterion 3 to confirm.
- Criterion 2 reordering ⇒ the toggle gate is now in front of
  the source gate; a flipped toggle could re-activate stale
  drift. Restore Wave 1D.1's gate order.
- Criterion 4 surfaces a third call site ⇒ document the new
  call site's gate story in an ADR (either it goes through
  `try_enqueue_for_kg_filing` and inherits the cascade, or it
  has its own gate set documented and probed).

**Last run:** _Wave 1D.6 — **GREEN**. Source-gate probe cells 1+2:
both `0 queue row(s) (expected 0)`. `kg_graph_off_invariant`:
sweeps `(Dictation, KgNote)` across all 8 outcomes; source-gate
negative control + positive control flip both green. Two call
sites to `kg::store::enqueue_for_filing` only:
`dictation.rs::try_enqueue_for_kg_filing` and
`kg/ingest_text.rs::ingest_text_note`. The Phase MC
do-not-touch sweep is empty over the narrowed file set (i.e.
ignoring `dictation*` per the ADR 0052 supersession)._
