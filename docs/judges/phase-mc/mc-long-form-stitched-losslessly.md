# Judge: mc-long-form-stitched-losslessly (Phase MC)

**Target:** `src-tauri/src/meetings/long_form_stt.rs`
(`run_long_form`, `stitch_chunks`), the integration test
`src-tauri/src/meetings/long_form_stt_tests.rs::
lossless_synthetic_long_feed_no_gaps_no_dupes`, ADR 0029.

**Question:** When the chunked Whisper driver walks a multi-chunk
feed (≥10 chunks with rolling 30 s / 2 s-overlap windowing), does
the stitched per-channel segment vector contain every word of the
ground-truth audio exactly once — no gaps at chunk seams, no
duplicate phrases from the overlap region?

**Rationale:** ADR 0029 commits the meeting subsystem to chunked
inference (vs. one giant Whisper run) so long meetings (1 h +)
don't blow out GPU memory or VRAM mapping. The cost is a
non-trivial stitch algorithm: the 2 s overlap at the head of
chunk N+1 deliberately re-feeds the tail of chunk N to give
Whisper context, but those duplicate words have to be dropped on
the output side or the transcript stutters. The
rolling-initial-prompt mitigates context loss; the overlap-window
dedup mitigates duplicate emission. If either drifts, the
transcript ships with seam-rendered glitches every 30 s — a
"works on a 1-minute test, fails on a 1-hour meeting" trap. The
judge runs against a deliberately-synthetic many-chunk feed where
correctness is mechanically verifiable.

**Pass criteria — ALL of:**

1. **23 unit tests across `long_form_stt_tests.rs` + `long_form_stt_pure_tests.rs` pass:**

   ```powershell
   pwsh scripts\cargo-with-cuda.ps1 test --release --lib `
     -- meetings::long_form_stt_tests `
        meetings::long_form_stt_pure_tests
   ```

   Covers: empty feed, single-chunk feed, two-chunk-no-overlap,
   two-chunk-with-overlap, multi-chunk-with-rolling-prompt,
   chunk-order preservation, CRC32 mismatch → AppError, missing
   chunk seq → AppError, global-timeline shift correctness,
   per-channel independence (mic and sys advance independently),
   `take_chunk_rx` Option::take handoff, dropped-Receiver clean
   shutdown.

2. **Lossless-feed integration test passes:**

   ```powershell
   pwsh scripts\cargo-with-cuda.ps1 test --release --lib `
     -- meetings::long_form_stt_tests::lossless_synthetic_long_feed_no_gaps_no_dupes
   ```

   Test body (sealed in W3): scripts a `StubStt` with a known
   per-chunk segment sequence (1 s segments, 0.1 s gaps, ~30
   chunks). After `run_long_form` finishes, the assertion walks
   the stitched output and confirms:

   - Every (start, text) pair from the ground-truth script
     appears in the stitched vector EXACTLY once. The judge
     surfaces "duplicate segment in stitched output: <key>" on
     dupe, "missing segment: <key>" on gap.
   - Per-channel global timeline is monotonic and non-negative.
   - The mic and sys channels remain on separate vectors (no
     accidental interleave at the stitch layer — interleave is
     the formatter's / merge's job downstream).

3. **Two-channel independence assertion:**

   ```powershell
   pwsh scripts\cargo-with-cuda.ps1 test --release --lib `
     -- meetings::long_form_stt_tests::two_channels_yield_independent_segment_streams
   ```

   Interleaved mic + sys chunks fed through one
   `run_long_form` call must come back as two independent
   stitched vectors with no cross-pollination. This pins the
   "the stitch layer is per-channel; the merge layer interleaves
   later" architectural seam from ADR 0029 §3.

**On failure:**

- **Block the `phase-mc-complete` tag.** A losing-words
  meeting transcript is a feature-killing defect.
- If `lossless_synthetic_long_feed_no_gaps_no_dupes` flags a
  duplicate: the overlap-dedup window is wrong (likely too
  narrow — re-check `OVERLAP_WINDOW_MS` against the
  `CHUNK_OVERLAP_MS` constant).
- If it flags a missing segment: the rolling-prompt advance is
  wrong (likely lost the tail-of-chunk-N text from chunk-N+1's
  context window).
- If `two_channels_yield_independent_segment_streams` shows
  cross-pollination: a shared `Vec` / `HashMap` is leaking
  between channels — the stitch state must be per-channel-keyed.

**Last run:** _Wave 6 — **GREEN**. All 23 long-form tests link
clean under `--release --no-run`; lossless-feed assertion is
mechanically verified by the test itself, not by external script._
