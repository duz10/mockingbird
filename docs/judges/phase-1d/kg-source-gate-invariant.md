# Judge: kg-source-gate-invariant (Phase 1D Wave 1D.6)

**Target:**
- `src-tauri/src/dictation.rs::try_enqueue_for_kg_filing` (the
  3-gate cascade authored at Wave 1D.1 per ADR 0052 §D1)
- `src-tauri/src/kg/ingest_text.rs::ingest_text_note` (the
  text-note ingest path authored at Wave 1D.3)
- `src-tauri/src/kg/source_gate_invariant.rs` + the binary
  `src-tauri/src/bin/kg_source_gate_invariant.rs` (this judge's
  deterministic probe)
- Migration 025 (`SettingKey::KgGraphEnabled` default-`false` seed +
  `sessions.capture_kind` column)

**Question:** Does the KG filing queue receive **exactly** the rows
that (a) originate from a KG capture surface (`capture_kind IN
('kg-note', 'kg-note-text')`) AND (b) are filed while the global
`KgGraphEnabled` toggle is on — for every combination of capture
kind × toggle state, across both entry points (dictation tail + text
note ingest)?

The answer must be **YES** with row counts matching the expected
matrix:

| Cell | Capture kind | Toggle | Expected `kg_filing_queue` rows |
|---|---|---|---|
| 1 | `Dictation` | off | 0 |
| 2 | `Dictation` | on | 0 |
| 3 | `KgNote` (audio) | off | 0 |
| 4 | `KgNote` (audio) | on | 1 |
| 5 | `KgNoteText` | off | 0 |
| 6 | `KgNoteText` | on | 1 |

Total: 2 rows across the 6-cell corpus.

**Rationale:** This is the principal ADR 0052 invariant — the
"trigger-direction" drift Phase 1D corrects. Pre-1D, every
successful dictation enqueued for KG filing the moment the toggle
was on; the user had no per-capture opt-in. ADR 0052 §"Context"
Drift 1 documents the reversal: only entries from a KG capture
surface participate, and the toggle is the kill switch (not the
trigger).

The sibling [`kg-graph-off-untouched`](../phase-0-kg/README.md)
probe (ADR 0050 §D8 gate 2; binary
`src-tauri/src/bin/kg_graph_off_invariant.rs`) already covers the
audio path's source-gate at all eight `InjectionOutcome` variants
under toggle-off. This judge extends coverage to the **text-note
ingest path** — a distinct entry point with no
`InjectionOutcome`, no dictation pipeline, and its own
toggle-only gate. The two probes together prove the cross-path
invariant.

The probe is **deterministic Rust** rather than LLM-graded
because the property is binary (queue row counts per corpus cell);
a language model cannot grade `SELECT COUNT(*) WHERE capture_kind
= ?` more reliably than `rusqlite` can. AGENTS.md §"Judges — when"
authorizes a single one-off judge for a narrow invariant; the
sibling `kg_graph_off_invariant` (Phase 1B) and `mc-formatter-
deterministic` (Phase MC) are the precedents.

**Pass criteria — ALL of:**

1. **Probe exits clean.**

   ```powershell
   powershell -File scripts\cargo-with-cuda.ps1 run --release `
     --bin kg_source_gate_invariant
   ```

   Expected output ends with:

   ```text
   ✅ SOURCE-GATE INVARIANT GREEN: all 6 corpus cells (3 capture_kinds × 2 toggle states) match expected enqueues; only kg-note + on AND kg-note-text + on produced queue rows
   ```

   Exit code: `0`. Any cell mismatch produces a `❌` line on
   stderr identifying the cell + observed-vs-expected counts.

2. **The 3-gate cascade in `try_enqueue_for_kg_filing` is
   unmodified since Wave 1D.1 except via a successor ADR.**

   ```powershell
   git log --oneline -- src-tauri\src\dictation.rs |
     Select-String 'mb-pxzk|mb-q2p1|ADR 005[2-9]'
   ```

   Expected: only Wave 1D.1's chartering commit (`mb-pxzk`,
   ADR 0052) and the Wave 1D.6 seal pass (`mb-q2p1`) reference
   the cascade. A new commit modifying the gate ordering or
   adding a fourth gate without a successor ADR is a violation.

3. **`ingest_text_note` still checks `KgGraphEnabled` before
   enqueueing.** The function-level unit tests in
   `src-tauri/src/kg/ingest_text.rs::tests` enumerate four cells
   (toggle on/off × text empty/non-empty). All four still pass
   under the throwaway-crate recipe (see LESSONS PINNED P2).

4. **`CaptureKind` enum unchanged or only additively extended.**

   ```powershell
   git diff phase-1c-... -- src-tauri\src\db\sessions.rs |
     Select-String 'CaptureKind'
   ```

   New variants added since 1D.0 are fine; renamed or removed
   variants are a violation. The match arm in
   `try_enqueue_for_kg_filing` is exhaustive — a new variant
   forces an explicit decision about whether it participates in
   filing.

5. **Migration 025 seed for `kg_graph_enabled` is still
   `'false'`.**

   ```powershell
   Select-String -Path src-tauri\src\db\migrations\025_*.sql `
     -Pattern "kg_graph_enabled.*false"
   ```

   Expected: at least one match. The probe relies on this seed
   to drive the "toggle off" cells without an explicit `Settings.set()`.

**On failure:**

- **Block the Wave 1D.6 / Phase 1D seal.**
- If criterion 1 surfaces a per-cell mismatch:
  - Cells 1/2 mismatch ⇒ source-gate bypassed; check the order of
    gates in `try_enqueue_for_kg_filing`. The source gate must
    appear before the toggle check (ADR 0052 §D1 rationale: a
    flipped toggle alone must never re-activate stale drift).
  - Cells 3/5 mismatch ⇒ toggle gate bypassed; same gate-order check.
  - Cells 4/6 mismatch (expected 1, observed 0) ⇒ the KG capture
    path itself regressed. Check `runtime.rs::start_kg_note` and
    `commands::kg::kg_ingest_text_note` haven't lost their
    `capture_kind` assignment.
- If criterion 2 surfaces an unauthorized cascade modification:
  revert to the Wave 1D.1 form and re-author behind a successor ADR.
- If criterion 5 surfaces a default-on seed: migration 025 is
  broken; the principal invariant for the entire KG subsystem
  (default-off per ADR 0049 §6) is violated. STOP and surface.

**Last run:** _Wave 1D.6 — **GREEN**. 6/6 corpus cells matched
expected enqueue counts (0, 0, 0, 1, 0, 1); corpus total = 2 as
expected. Probe binary built clean under `cargo-with-cuda.ps1 build
--release --bin kg_source_gate_invariant` (3m 45s cold). Companion
`kg_graph_off_invariant` regression: 8/8 outcomes + source-gate
negative + positive control GREEN. `kg_parity` default + `--persist`:
32/32 GREEN._
