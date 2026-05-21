# Judge: mc-two-channel-merged (Phase MC)

**Target:** `src-tauri/src/meetings/merge.rs` (`merge_two_channels`),
`src-tauri/src/meetings/export.rs` (`render_body`,
`both_renders_merged_when_present`,
`both_falls_back_to_interleave_when_merge_missing`),
`src-tauri/src/meetings/lifecycle.rs` (the `merge_two_channels` call
site), ADR 0028.

**Question:** When a meeting captures both the mic and the system-
loopback channels and produces two timestamped segment vectors,
does the merge step produce a single chronologically-interleaved
transcript with the correct speaker labels (`MeetingSpeakerLabelMic`
on mic-origin runs, `MeetingSpeakerLabelSys` on sys-origin runs)?

**Rationale:** ADR 0028 commits the meeting subsystem to twin
cpal streams + clock-aligned merge. The two channels are captured
independently (different `cpal::Stream` instances on different
threads with different sample clocks) but their timestamps are
sample-counted from a shared `Instant::now()` baseline on capture
start. The merge step is the only place where the two timelines
re-converge. If the merge drops a segment, the user loses words
from one speaker. If it mis-labels a segment, the user reads
something Bob said and attributes it to themselves. If it
duplicates a segment, the transcript reads as if someone repeated
themselves. None of these are individually catastrophic but they
erode trust in the canonical transcript.

**Pass criteria — ALL of:**

1. **9 `merge::tests` pass:**

   ```powershell
   pwsh scripts\cargo-with-cuda.ps1 test --release --lib `
     -- meetings::merge::tests
   ```

   Covers: empty/empty → empty, mic-only → mic-only output (no
   `Other(s):` prefix appears), sys-only → sys-only output (with
   `Other(s):` prefix), interleaved-by-timestamp order, two-
   segments-same-start tie-break (mic wins per ADR 0028 §4 to
   keep speaker turns stable across re-runs), late-arriving-mic-
   segment after a sys-segment block, single-channel-multi-
   segment monotonic preservation.

2. **`export::tests::both_renders_merged_when_present` passes:**

   ```powershell
   pwsh scripts\cargo-with-cuda.ps1 test --release --lib `
     -- meetings::export::tests::both_renders_merged_when_present
   ```

   Confirms the export markdown's body section consumes the
   pre-merged string from `merge_two_channels`, NOT a sequential
   "all mic first, then all sys" interleave. This pins the
   contract that the merge runs at `lifecycle.rs` and the export
   layer is the *consumer*, not a second merge implementation.

3. **`export::tests::both_falls_back_to_interleave_when_merge_missing` passes:**

   ```powershell
   pwsh scripts\cargo-with-cuda.ps1 test --release --lib `
     -- meetings::export::tests::both_falls_back_to_interleave_when_merge_missing
   ```

   Asserts the defensive fallback: if a `MeetingDetail` row
   arrives from the DB with a `source = both` but a missing
   merged text (corruption / migration gap), the export still
   produces a best-effort interleaved transcript rather than
   silently truncating. This is belt-and-suspenders — Wave 4's
   `persist_meeting` always writes the merged text — but the
   defensive path is tested.

4. **Speaker labels round-trip from `SettingKey` to merged output:**

   The labels `MeetingSpeakerLabelMic` (default `"You"`) and
   `MeetingSpeakerLabelSys` (default `"Other(s)"`) are read in
   `lifecycle.rs` and threaded into `merge_two_channels`. Static
   check that the merge function takes them as parameters (not
   hardcoded):

   ```powershell
   Select-String -Path src-tauri\src\meetings\merge.rs `
     -Pattern 'pub fn merge_two_channels'
   ```

   The signature must accept the labels (or a `&FormatOpts`)
   from the caller, not bake `"You"`/`"Other(s)"` literally.

**On failure:**

- **Block the `phase-mc-complete` tag.**
- If criterion 1 flags ordering: the segment-comparator (likely
  `<` vs `<=` on `start_ms`) regressed.
- If criterion 2/3 fails: the export layer started double-merging
  or stopped merging — re-walk `render_body`'s match-arm on
  `MeetingSource`.
- If criterion 4 surfaces hardcoded labels: rip them out and
  thread the typed setting in.

**Last run:** _Wave 6 — **GREEN**. 9 `merge::tests` link clean
under `--release --no-run`; both `export::tests::both_*` cases
verified; `merge_two_channels` signature accepts speaker labels
from the caller (no hardcoded strings)._
