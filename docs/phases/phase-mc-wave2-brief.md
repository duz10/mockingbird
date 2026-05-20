# Phase MC — Wave 2 Brief

**Brief author:** code-puppy (active agent for Wave 1; same hat for Wave 2).
**Date authored:** 2026-05-20.
**Entry tag:** post-Wave-1 commit `79414db` (Wave 1 code) on top of `3f6ca82` (ADR 0026–0030) on top of `phase-4-complete`.
**Wave 2 deliverables (from plan §"Task waves"):**

1. `meetings/activation.rs` — pure chord state machine (Section MC.1), ≥20 tests.
2. `meetings/formatter.rs` + `meetings/filler_words.rs` — deterministic formatter (Section MC.3), ≥25 tests + proptest fixpoint.
3. `meetings/chunker.rs` — pure-state 30s/2s-overlap chunker, ≥12 tests.
4. `stt::SpeechToText::transcribe_segments` — additive trait method + `WhisperStt` impl, 4 tests (ADR 0030).

**Test density target:** +60 to +80 tests across the four files. Cumulative project test count target after Wave 2: **~470–500 path** progresses by +30 (Wave 1) + ~70 (Wave 2) ≈ ~480 vs the 383 baseline → on track.

**Cargo gate (must be green at wave seal):**
```
powershell -File scripts/cargo-with-cuda.ps1 check
powershell -File scripts/cargo-with-cuda.ps1 clippy --release -- -D warnings
powershell -File scripts/cargo-with-cuda.ps1 test --release   # see LESSONS 2026-05-17 fallback
powershell -File scripts/cargo-with-cuda.ps1 fmt --check
```
Per LESSONS 2026-05-17, if `cargo test --release` exits with
`STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)` at first test-exe launch,
the documented fallback is `cargo test --release --no-run`. Wave 2's
new tests are pure-Rust (no ORT / Whisper DLL touch except the 4
`transcribe_segments` tests; see §4 below), so the fallback is
type-equivalent for ~70 of the ~70+ tests this wave adds. The 4
whisper tests require a live test binary — gate them behind
`#[cfg(feature = "live-stt-tests")]` or `#[ignore]` so the wave seals
cleanly on this box; document in the wave brief that they need a
known-good WhisperStt sandbox to run (any developer machine where
`cargo test --release stt::tests::whisper_one_shot` passes today
already satisfies this).

---

## 1. `meetings/activation.rs` — pure chord state machine

### Status entering Wave 2

Wave 1 shipped types, struct skeleton, `new(LastChosenSource)`, and observable accessors (`state()`, `is_paused()`, `last_source()`, `set_last_source()`). The body of `on_event` is `todo!()`. Two smoke tests exist; Wave 2 replaces them with the ≥20-test suite.

### Types already on disk (do not redefine)

```rust
pub enum ModifierKey { RCtrl, LCtrl, RAlt, LAlt, RShift, LShift, RWin, LWin }
pub enum LastChosenSource { Mic, System, Both }

pub enum ActivationEvent {
    ModifierDown { ts_ms: u64 },
    ModifierUp   { ts_ms: u64 },
    MainKeyDown  { ts_ms: u64 },
    MainKeyUp    { ts_ms: u64 },
    Tick         { ts_ms: u64 },
    PauseToggle  { paused: bool },
}

pub enum ActivationAction {
    MeetingToggle { source: LastChosenSource },
    Noop,
}

pub enum ActivationState { Idle, ModHeld, MainPressed }

pub struct Activation { /* state, last_source, paused — private */ }
impl Activation {
    pub fn new(last_source: LastChosenSource) -> Self;
    pub fn state(&self) -> ActivationState;
    pub fn is_paused(&self) -> bool;
    pub fn last_source(&self) -> LastChosenSource;
    pub fn set_last_source(&mut self, src: LastChosenSource);
    pub fn on_event(&mut self, event: ActivationEvent) -> AppResult<ActivationAction>;
}
```

### Wave 2 implementation of `on_event`

Follow Section MC.1 verbatim. The plan's ASCII state diagram is the spec; this brief just normalises it into pseudocode:

```text
match (self.paused, self.state, event) {
    // Pause toggle wins everywhere: emit Noop, force IDLE.
    (_, _, PauseToggle { paused })       => self.paused = paused;
                                            self.state = Idle; return Noop.
    // While paused, suppress all key events.
    (true, _, ModifierDown|ModifierUp|MainKeyDown|MainKeyUp|Tick) => Noop.

    // Tick: never advances state (kept for input-enum symmetry).
    (_, _, Tick { .. }) => Noop.

    // IDLE
    (false, Idle, ModifierDown)          => self.state = ModHeld;       Noop.
    (false, Idle, ModifierUp|MainKeyDown|MainKeyUp) => Noop.  // chord broken / stale edge

    // MOD_HELD
    (false, ModHeld, MainKeyDown)        => self.state = MainPressed;
                                            MeetingToggle { source: self.last_source }.
    (false, ModHeld, ModifierUp)         => self.state = Idle;          Noop.
    (false, ModHeld, ModifierDown|MainKeyUp) => Noop.  // stale / redundant edge

    // MAIN_PRESSED (chord fully down; suppressing Windows key-repeat)
    (false, MainPressed, MainKeyUp)      => self.state = ModHeld;       Noop.
    (false, MainPressed, ModifierUp)     => self.state = Idle;          Noop.
    (false, MainPressed, MainKeyDown)    => Noop.  // key-repeat — already fired this hold.
    (false, MainPressed, ModifierDown)   => Noop.  // redundant
}
```

`on_event` never returns `Err` in Wave 2's spec — the function is total
on the input enum. Keep the `AppResult` return type for forward-compat
(Wave 3's hook installer surfaces install errors; future timing-based
gestures may want to surface clock-skew warnings).

### Test specs — ≥20 (target 22)

All tests construct via `Activation::new(LastChosenSource::Mic)` unless
noted; use `Mic` as the carried last-source so the test-side assertions
on `MeetingToggle.source` are unambiguous. Timestamps are arbitrary
monotonic-increasing u64s (the chord state machine doesn't read them).

| # | Test name                                       | Setup                                  | Input(s)                                            | Expected action(s)                                | Expected end-state |
|---|-------------------------------------------------|----------------------------------------|-----------------------------------------------------|---------------------------------------------------|--------------------|
| 1 | idle_modifier_down_goes_to_mod_held             | new                                    | ModifierDown                                        | Noop                                              | ModHeld            |
| 2 | idle_lone_main_keydown_is_noop                  | new                                    | MainKeyDown                                         | Noop                                              | Idle               |
| 3 | idle_lone_main_keyup_is_noop                    | new                                    | MainKeyUp                                           | Noop                                              | Idle               |
| 4 | idle_lone_modifier_up_is_noop                   | new                                    | ModifierUp                                          | Noop                                              | Idle               |
| 5 | mod_held_main_down_fires_meeting_toggle         | new                                    | ModifierDown, MainKeyDown                           | [Noop, MeetingToggle{Mic}]                        | MainPressed        |
| 6 | mod_held_modifier_up_returns_to_idle            | new                                    | ModifierDown, ModifierUp                            | [Noop, Noop]                                      | Idle               |
| 7 | mod_held_main_up_without_main_down_is_noop      | new                                    | ModifierDown, MainKeyUp                             | [Noop, Noop]                                      | ModHeld            |
| 8 | mod_held_redundant_modifier_down_is_noop        | new                                    | ModifierDown, ModifierDown                          | [Noop, Noop]                                      | ModHeld            |
| 9 | main_pressed_key_repeat_does_not_re_fire        | new                                    | ModifierDown, MainKeyDown, MainKeyDown, MainKeyDown | [Noop, Toggle{Mic}, Noop, Noop]                   | MainPressed        |
|10 | main_pressed_main_up_returns_to_mod_held        | new                                    | ModifierDown, MainKeyDown, MainKeyUp                | [Noop, Toggle{Mic}, Noop]                         | ModHeld            |
|11 | main_pressed_modifier_up_returns_to_idle        | new                                    | ModifierDown, MainKeyDown, ModifierUp               | [Noop, Toggle{Mic}, Noop]                         | Idle               |
|12 | release_and_re_press_fires_again                | new                                    | ModifierDown, MainKeyDown, MainKeyUp, MainKeyDown   | [Noop, Toggle{Mic}, Noop, Toggle{Mic}]            | MainPressed        |
|13 | release_modifier_then_re_chord_fires_again      | new                                    | ModifierDown, MainKeyDown, MainKeyUp, ModifierUp, ModifierDown, MainKeyDown | [Noop, Toggle{Mic}, Noop, Noop, Noop, Toggle{Mic}] | MainPressed |
|14 | tick_in_idle_is_noop                            | new                                    | Tick                                                | Noop                                              | Idle               |
|15 | tick_in_mod_held_is_noop                        | new                                    | ModifierDown, Tick                                  | [Noop, Noop]                                      | ModHeld            |
|16 | tick_in_main_pressed_is_noop                    | new                                    | ModifierDown, MainKeyDown, Tick                     | [Noop, Toggle{Mic}, Noop]                         | MainPressed        |
|17 | pause_toggle_in_idle_resets_to_idle             | new                                    | PauseToggle{true}                                   | Noop                                              | Idle (paused)      |
|18 | pause_toggle_in_main_pressed_resets_to_idle     | new                                    | ModifierDown, MainKeyDown, PauseToggle{true}        | [Noop, Toggle{Mic}, Noop]                         | Idle (paused)      |
|19 | paused_suppresses_chord                         | new + PauseToggle{true} first          | ModifierDown, MainKeyDown                           | [Noop, Noop]                                      | Idle (paused)      |
|20 | unpaused_chord_resumes                          | new + PauseToggle{true} + PauseToggle{false} | ModifierDown, MainKeyDown                       | [Noop, Noop, Noop, Toggle{Mic}]                   | MainPressed        |
|21 | set_last_source_affects_next_toggle             | new + set_last_source(Both)            | ModifierDown, MainKeyDown                           | [Noop, Toggle{Both}]                              | MainPressed        |
|22 | toggles_during_main_pressed_carry_current_src   | new + set_last_source(System) AFTER Toggle{Mic} fired | ModifierDown, MainKeyDown, MainKeyUp, MainKeyDown (post-set) | last action: Toggle{System}             | MainPressed        |

All tests use a helper:

```rust
fn drive(a: &mut Activation, events: &[ActivationEvent]) -> Vec<ActivationAction> {
    events.iter().map(|e| a.on_event(*e).expect("never errs in W2")).collect()
}
```

`ts_ms` values can be `0` for every event — the chord machine ignores
them.

### Out of scope for Wave 2

- The `WH_KEYBOARD_LL` install + dedicated meetings message-pump thread
  (Wave 3, task in §"Wave 3" of the plan).
- The conflict-probe extension to `hotkey::probe` (Wave 3).
- The synthetic-event integration test that drives the real listener
  (Wave 3).

---

## 2. `meetings/formatter.rs` + `meetings/filler_words.rs` — deterministic formatter

### Status entering Wave 2

Wave 1 shipped `FormatOpts` (with Section MC.3 defaults), the `format`
signature, the static `FILLERS` + `FILLER_PHRASES` `phf::Set`s, and
`MAX_PHRASE_TOKENS = 3`. Five filler-set tests already pass. `format`'s
body is `todo!()`.

### Algorithm (Section MC.3 verbatim — re-stated for the implementor)

```text
Inputs:
  segments:   &[TimedSegment]                     (sorted by t0_ms)
  filler_set: &phf::Set<&'static str>              (caller passes &FILLERS)
  opts:       &FormatOpts

Output:
  String   (the formatted transcript)

1. For each segment, tokenize on whitespace (preserve internal
   punctuation by tokenizing on \s+ rather than non-word boundaries).
2. PHRASE pass — slide a window of length min(MAX_PHRASE_TOKENS,
   remaining_tokens) over the token stream. For each window, lowercase-
   normalise the tokens (drop trailing/leading punctuation when
   matching), join with single space, look up in FILLER_PHRASES.
   If a hit: drop the entire window (advance by window length) and
   restart the slide from the next token. If no hit at any length:
   - If opts.strip_fillers AND single-token-lowercase ∈ FILLERS,
     drop the token and advance by 1.
   - Else keep the token verbatim and advance by 1.
   Greedy-longest: always try MAX_PHRASE_TOKENS first, then
   MAX_PHRASE_TOKENS-1, etc., before falling back to single-token.
3. REPEAT pass (if opts.strip_repeats): walk the surviving token
   stream and collapse exact-match consecutive tokens after lowercase
   normalisation. Preserve the FIRST occurrence's original case +
   punctuation.
4. Walk segments in order, joining tokens with single spaces.
   Between segment[i] and segment[i+1]:
     - If (segment[i+1].t0_ms - segment[i].t1_ms) >= opts.paragraph_gap_ms,
       insert "\n\n".
     - Else insert a single space.
5. CAPITALIZATION (single forward walk):
   - First non-whitespace char → uppercase (if alphabetic).
   - First non-whitespace char after "\n\n" → uppercase.
   - First non-whitespace char after any [.!?] followed by [\s]+ → uppercase.
   - Mid-word case is preserved.
6. If opts.strip_leading_trailing_ws, trim() the final string.
```

### Implementation notes for the author

- `TimedSegment` is `use super::long_form_stt::TimedSegment` — already
  imported in the scaffold.
- Tokenize via `s.split_whitespace().collect::<Vec<_>>()` per segment
  before the phrase pass; segment-internal joining happens AT step 4,
  not before.
- For the phrase-pass lookup, normalisation = `to_ascii_lowercase()`
  + strip leading/trailing `.,!?;:"'()[]{}` only. Internal punctuation
  is preserved on the original token so step 4 emits Whisper's own
  punctuation correctly.
- Use a single `String` buffer with `push_str` + `push('\n')` /
  `push(' ')` for step 4 — avoid `Vec<String>` intermediates.
- Step 5 walks `result.chars()` once; use a `bool sentence_break = true`
  carrying state so the first char is always uppercased.
- For UTF-8 safety: never split on byte boundaries. `chars()` /
  `char_indices()` everywhere. Section MC.3's invariant "multi-byte
  UTF-8 (CJK, emoji) → no panic, no character splitting" is a hard
  contract.

### Test specs — ≥25 (target 27) + 2 proptests

Place in `meetings/formatter.rs::tests`. Naming: `{behavior}_{condition}`.

| # | Test                                          | Input segments + opts                                                              | Expected output                       |
|---|-----------------------------------------------|------------------------------------------------------------------------------------|----------------------------------------|
| 1 | empty_input_is_empty                          | `[]`                                                                                | `""`                                   |
| 2 | single_token_no_fillers                       | `[("hello world", 0, 1000)]`                                                       | `"Hello world"`                        |
| 3 | first_char_is_uppercased                      | `[("the cat", 0, 1000)]`                                                           | `"The cat"`                            |
| 4 | strip_fillers_removes_um                      | `[("um the cat", 0, 1000)]`                                                        | `"The cat"`                            |
| 5 | strip_fillers_removes_multiple_um             | `[("um uh um the cat", 0, 1000)]`                                                  | `"The cat"`                            |
| 6 | strip_repeats_collapses_the_the               | `[("the the cat", 0, 1000)]`                                                       | `"The cat"`                            |
| 7 | combined_um_uh_um_the_the_cat                 | `[("um uh um um the the cat", 0, 1000)]`                                           | `"The cat"`                            |
| 8 | phrase_you_know_at_start_dropped              | `[("you know the cat", 0, 1000)]`                                                  | `"The cat"`                            |
| 9 | phrase_i_mean_mid_sentence_dropped            | `[("the i mean cat", 0, 1000)]`                                                    | `"The cat"`                            |
|10 | phrase_sort_of_dropped                        | `[("it was sort of weird", 0, 1000)]`                                              | `"It was weird"`                       |
|11 | greedy_longest_match_for_you_see              | `[("you see this", 0, 1000)]`                                                      | `"This"`                               |
|12 | filler_at_end_of_segment_no_double_space      | `[("the cat um", 0, 1000)]`                                                        | `"The cat"`                            |
|13 | two_segments_short_gap_single_space           | `[("the cat", 0, 1000), ("ran fast", 1500, 2500)]`                                 | `"The cat ran fast"`                   |
|14 | two_segments_long_gap_paragraph               | `[("the cat", 0, 1000), ("dog barked", 3500, 4500)]`                               | `"The cat\n\nDog barked"`              |
|15 | gap_exactly_paragraph_gap_ms_is_paragraph     | `[("a", 0, 1000), ("b", 3000, 4000)]` (gap = 2000)                                 | `"A\n\nB"`                             |
|16 | gap_one_less_than_paragraph_gap_is_space      | `[("a", 0, 1000), ("b", 2999, 4000)]` (gap = 1999)                                 | `"A b"`                                |
|17 | whisper_punctuation_preserved                 | `[("hello, world. how are you?", 0, 1000)]`                                        | `"Hello, world. How are you?"`         |
|18 | sentence_start_after_period_capitalized       | `[("hello. world", 0, 1000)]`                                                      | `"Hello. World"`                       |
|19 | sentence_start_after_question_capitalized     | `[("ok? then go", 0, 1000)]`                                                       | `"Ok? Then go"`                        |
|20 | utf8_cjk_passes_through_without_panic         | `[("hello 你好 world", 0, 1000)]`                                                  | `"Hello 你好 world"`                    |
|21 | utf8_emoji_passes_through                     | `[("hi 🎉 there", 0, 1000)]`                                                       | `"Hi 🎉 there"`                         |
|22 | leading_trailing_ws_stripped                  | `[("  hello  ", 0, 1000)]`                                                         | `"Hello"`                              |
|23 | strip_fillers_false_keeps_um                  | opts.strip_fillers=false, `[("um the cat", 0, 1000)]`                              | `"Um the cat"`                         |
|24 | strip_repeats_false_keeps_the_the             | opts.strip_repeats=false, `[("the the cat", 0, 1000)]`                             | `"The the cat"`                        |
|25 | mid_word_case_preserved                       | `[("the iPhone is great", 0, 1000)]`                                               | `"The iPhone is great"`                |
|26 | three_segments_mixed_gaps                     | `[("a", 0, 1000), ("b", 1500, 2500), ("c", 5000, 6000)]`                           | `"A b\n\nC"`                           |
|27 | filler_only_input_emits_empty                 | `[("um uh um", 0, 1000)]`                                                          | `""` (note: trim wins over capitalize-empty) |
|28 (proptest) | format_is_idempotent_fixpoint               | arbitrary segments via `proptest` strategy                                         | `format(segments) == format([(format(segments), 0, gap)])` for any single-segment re-feed |
|29 (proptest) | format_never_panics_on_arbitrary_unicode    | arbitrary strings via `prop::collection::vec(any::<String>(), 0..10)`              | no panic                               |

For the proptests:
- Strategy for segments: `prop::collection::vec((any::<String>(), 0u32..100_000, 0u32..100_000), 0..20)` then `.prop_map` to (a) sort by t0, (b) clamp t1 ≥ t0.
- The idempotence test re-wraps the output as a single segment of (text, 0, 1000) and re-runs the formatter. The fixpoint assertion is that the second pass equals the first. This catches: double-stripping of fillers (none should remain), double-capitalization (no all-caps creep), double-trimming (trim of trim is identity).
- The no-panic test asserts only that `format(...)` returns `Ok(_)` (or `Err(AppError::Formatter(...))` but never panics) for any input.

### Out of scope for Wave 2

- Custom filler-list overrides at runtime (current contract: `FILLERS`
  is static + lookup is by `&phf::Set<&'static str>`; runtime override
  via a Vec<&str> facade is a Wave 5 polish item if any user asks for
  it).
- The `mc-formatter-deterministic` judge (Wave 6 lands the
  judges-template entry; this wave just produces the test corpus the
  judge will rerun).

---

## 3. `meetings/chunker.rs` — pure-state rolling chunker

### Status entering Wave 2

Wave 1 shipped `ChunkWritten`, `ChunkerConfig` (defaults match ADR
0029: 30 s * 16 kHz = 480 000 samples per chunk; 2 s * 16 kHz = 32 000
samples leading overlap; sample_rate = 16 000), `MeetingChunker` with
`new` + `feed` (todo) + `finalize` (todo). Two smoke tests exist.

### Implementation

```rust
struct MeetingChunker {
    config: ChunkerConfig,
    meeting_uuid: String,
    channel_tag: &'static str,    // "mic" | "sys"
    chunk_dir: PathBuf,

    // Wave 2 adds:
    pending: Vec<i16>,            // samples buffered toward the next chunk
    next_seq: u32,                // chunk index for filenames
    global_sample: u64,           // total samples consumed; used for first/last_sample
    last_chunk_tail: Vec<i16>,    // last overlap_samples of the previous chunk;
                                  // prepended to the next chunk on roll.
}

fn feed(&mut self, samples: &[i16]) -> AppResult<Vec<ChunkWritten>> {
    // 1. Push samples into `pending`.
    // 2. While pending.len() >= chunk_samples (minus overlap on chunks > 0):
    //      - Compose the next chunk: last_chunk_tail || pending[..take_n]
    //        where take_n = chunk_samples - last_chunk_tail.len().
    //      - Write WAV to chunk_dir/<uuid>_<channel_tag>_<seq>.wav.
    //      - CRC32 over the i16-as-LE-bytes of the chunk's full sample
    //        payload (including overlap prefix). Stored on ChunkWritten.
    //      - first_sample = global_sample - last_chunk_tail.len() (the
    //        overlap prefix). last_sample = global_sample + take_n.
    //      - Set last_chunk_tail = chunk_payload[chunk_payload.len() - overlap_samples..].
    //      - Drain take_n samples from `pending`; bump global_sample by take_n;
    //        bump next_seq.
    // 3. Return Vec<ChunkWritten> (one per chunk rolled; usually 0 or 1).
}

fn finalize(&mut self) -> AppResult<Option<ChunkWritten>> {
    // Flush whatever is in `pending`, including the overlap prefix.
    // Don't roll a chunk if pending.is_empty() AND last_chunk_tail
    // contains only already-emitted samples (i.e. the previous feed
    // ended on a clean boundary).
    // Write the trailing chunk if pending.len() > 0; same path/crc
    // bookkeeping as feed(); set last_sample = global_sample + pending.len().
}
```

WAV writing: use the `hound` crate (already on the workspace — confirm
with `cargo tree -p hound` in src-tauri; it's transitively pulled in by
the existing audio path). Spec: 16 kHz, 16-bit PCM, 1 channel.

CRC32: `let mut h = crc32fast::Hasher::new(); for s in chunk_payload { h.update(&s.to_le_bytes()); } h.finalize()`. The crc is over the sample payload, NOT the WAV header — `long_form_stt` (Wave 3) will reconstruct the byte sequence the same way to verify.

### Test specs — ≥12 (target 14)

All tests use `tempfile::tempdir()` for `chunk_dir`. Helpers:

```rust
fn make_chunker(dir: PathBuf) -> MeetingChunker {
    MeetingChunker::new("test-uuid".into(), "mic", dir, ChunkerConfig::default())
}

fn zeros(n: usize) -> Vec<i16> { vec![0i16; n] }
fn ramp(n: usize) -> Vec<i16> { (0..n).map(|i| (i % 32768) as i16).collect() }
```

| # | Test                                              | Input                                          | Expected                                                                                    |
|---|---------------------------------------------------|------------------------------------------------|---------------------------------------------------------------------------------------------|
| 1 | feed_under_chunk_returns_empty                    | `zeros(479_999)`                               | `vec![]`                                                                                    |
| 2 | feed_exactly_one_chunk_rolls_one                  | `zeros(480_000)`                               | 1 ChunkWritten; first_sample=0; last_sample=480_000; file exists                            |
| 3 | feed_two_chunks_in_one_call                       | `zeros(2 * 480_000 - 32_000)`                  | 2 ChunkWritten; seq 0 and 1; file names include `_0.wav` + `_1.wav`                         |
| 4 | second_chunk_includes_overlap_prefix              | feed chunk 0, feed (480_000 - 32_000) ramp     | seq 1's first_sample = 480_000 - 32_000 (overlap), last_sample = 2*480_000 - 32_000          |
| 5 | finalize_with_empty_pending_returns_none          | new + finalize                                 | `Ok(None)`                                                                                  |
| 6 | finalize_with_residual_writes_trailing_chunk      | feed(zeros(100_000)) + finalize                | Some(ChunkWritten); chunk payload length = 100_000; file readable                           |
| 7 | finalize_after_full_chunk_returns_none            | feed(zeros(480_000)) + finalize (no extra feed)| `Ok(None)`                                                                                   |
| 8 | crc32_matches_hand_computation                    | feed(ramp(480_000))                            | crc32 == crc32fast::hash(ramp.to_le_bytes-concat)                                            |
| 9 | filenames_use_uuid_and_channel_tag                | feed(zeros(480_000))                           | `chunk_dir.join("test-uuid_mic_0.wav")` exists                                              |
|10 | sequential_seqs_zero_indexed                      | feed(zeros(3 * 480_000))                       | seq 0, 1, 2 — but with overlap so total samples = 3*480_000 - 2*32_000 → 3 rolls + 1 residual? actually 3*480k - 32k stays in buffer; assert at least 2 ChunkWritten with seq 0,1 |
|11 | mic_and_sys_chunkers_separate_files               | two chunkers in same dir, "mic" + "sys" tags   | filenames disjoint                                                                          |
|12 | overlap_zero_works                                | ChunkerConfig { overlap_samples: 0, ..default }| no overlap prefix; first_sample == previous last_sample                                     |
|13 | very_small_chunk_size                             | ChunkerConfig { chunk_samples: 100, overlap_samples: 10, .. } + feed(ramp(250)) | 2 ChunkWritten + residual in pending      |
|14 | wav_round_trip_via_hound                          | feed(ramp(480_000))                            | `hound::WavReader::open(path)` returns 480_000 samples matching the input modulo the LE-i16 round-trip |

### Out of scope for Wave 2

- The actual writing of `meeting.wav` (the concatenated canonical
  blob); that's Wave 4's persist step.
- CRC verification on read; that's `long_form_stt` (Wave 3).
- A streaming-WAV writer that writes chunks incrementally — use
  buffered+close-on-roll. Each chunk is small enough (~960 KB) that
  open/write/close per chunk is fine.

---

## 4. `stt::SpeechToText::transcribe_segments` — ADR 0030 additive method

### Status entering Wave 2

`SpeechToText` currently has one method:
```rust
fn transcribe(&mut self, req: TranscribeRequest<'_>) -> AppResult<Transcript>;
```
The existing `Transcript` struct has `text`, `gpu_used`, `latency_ms`,
`model_id` — no segments. Wave 1 did NOT modify this trait (ADR 0030
authored, not implemented).

### Wave 2 implementation

Add to `src-tauri/src/stt/mod.rs`:

```rust
/// Per-Whisper-segment timing. Matches meetings::long_form_stt::TimedSegment
/// shape; meetings re-exports / wraps this if Wave 2 lands the type in
/// stt:: (preferred — keeps meetings independent of long_form_stt::TimedSegment
/// for the trait surface). Decide at impl time: if it's natural to put
/// the canonical TimedSegment in stt::, do so and have long_form_stt
/// `pub use` it.
#[derive(Debug, Clone, PartialEq)]
pub struct SttSegment {
    pub text: String,
    pub t0_ms: u32,
    pub t1_ms: u32,
}

#[derive(Debug, Clone)]
pub struct TranscriptWithSegments {
    pub text: String,
    pub segments: Vec<SttSegment>,
    pub gpu_used: bool,
    pub latency_ms: u64,
    pub model_id: String,
}

pub trait SpeechToText: Send {
    fn transcribe(&mut self, req: TranscribeRequest<'_>) -> AppResult<Transcript>;

    /// ADR 0030. Returns segments alongside the top-line text.
    /// Default impl falls back to single-segment-equal-to-text for
    /// impls that don't have segment access (none today; the default
    /// keeps the trait extension non-breaking for any future external
    /// implementor).
    fn transcribe_segments(
        &mut self,
        req: TranscribeRequest<'_>,
    ) -> AppResult<TranscriptWithSegments> {
        let t = self.transcribe(req)?;
        Ok(TranscriptWithSegments {
            text: t.text.clone(),
            segments: vec![SttSegment { text: t.text, t0_ms: 0, t1_ms: t.latency_ms as u32 }],
            gpu_used: t.gpu_used,
            latency_ms: t.latency_ms,
            model_id: t.model_id,
        })
    }
}
```

For `WhisperStt`, override `transcribe_segments` by walking
`state.full_n_segments()` + `full_get_segment_text(i)` +
`full_get_segment_t0(i)` + `full_get_segment_t1(i)`. The whisper-rs
crate exposes these on `WhisperState`. T0/T1 are in centiseconds in
whisper.cpp — multiply by 10 to get ms.

Resolve the SttSegment / long_form_stt::TimedSegment duplication: in
Wave 2, the cleanest move is to make `meetings::long_form_stt::TimedSegment`
a `pub use crate::stt::SttSegment as TimedSegment` alias. Document
this in the wave-2 commit message.

### Test specs — 4 (per plan)

All four tests live in `src-tauri/src/stt/whisper.rs::tests` (cfg-gated
behind `#[ignore]` or `#[cfg(feature = "live-stt-tests")]` because the
test exe hits the STATUS_ENTRYPOINT_NOT_FOUND known issue on this box;
on a clean machine they run with `cargo test --release stt::`).

Use the existing dictation fixtures under
`src-tauri/tests/fixtures/audio/` — the same WAVs that drive the
dictation `whisper.rs` tests today.

| # | Test                                               | Setup                                  | Assertion                                                                                              |
|---|----------------------------------------------------|----------------------------------------|--------------------------------------------------------------------------------------------------------|
| 1 | transcribe_segments_returns_at_least_one_segment   | load short fixture (e.g. `hello.wav`)  | result.segments.len() >= 1                                                                             |
| 2 | segment_t1_strictly_after_t0                       | load short fixture                     | for s in segments: assert!(s.t1_ms >= s.t0_ms) (whisper.cpp can emit t0==t1 for zero-length segments)  |
| 3 | segments_monotonic_in_t0                           | load 5+ second fixture                 | windows(2).all(|w| w[0].t0_ms <= w[1].t0_ms)                                                           |
| 4 | joined_segment_text_matches_top_line               | load short fixture                     | segments.iter().map(\|s\| s.text.trim()).join(" ").to_lowercase() == result.text.trim().to_lowercase() (modulo whitespace + case) |

If the live test-exe issue persists across the bench used for
Wave 2 development, mark all 4 tests `#[ignore]` and write a one-line
README at `src-tauri/src/stt/README-meeting-tests.md` documenting that
`cargo test --release stt:: -- --ignored` is the wave-seal command.

### Out of scope for Wave 2

- A dictation-side refactor to re-implement `transcribe` in terms of
  `transcribe_segments` (would touch 383 dictation tests; the ADR
  defers this explicitly).
- A `proptest` for segment monotonicity (Whisper isn't a Rust function —
  property-testing a model is the eval rig's job).

---

## Deviations from the plan — Wave 2 carries forward from Wave 1

1. **Hook layout & name.** Plan called for `.code_puppy/hooks/block-cross-module-coupling-meeting-dictation.toml`. The codebase's actual hook mechanism is Python scripts under `scripts/hooks/` wired via `.code_puppy/settings.json`. Wave 1 shipped the hook in the real layout; Wave 2 inherits and the plan's task table reflects the real path. **No Wave 2 deviation per se — just carry-forward.**

2. **Migration path.** Plan's earlier reconciliation incorrectly named `src-tauri/migrations/`; the real path is `src-tauri/src/db/migrations/`. Wave 1's commit `79414db` lands migration 011 at the real path. **No Wave 2 deviation.**

3. **`cargo test --release` live-run.** Per LESSONS 2026-05-17, the test runner on this box exits with `STATUS_ENTRYPOINT_NOT_FOUND` for ALL test crates (including pure-Rust ones; it's a load-time DLL-export resolution failure not specific to ORT or whisper). Wave 2 declares the cargo gate green via the documented fallback `cargo test --release --no-run`. The 4 `transcribe_segments` tests in §4 are gated behind `#[ignore]` so they don't block the seal; they run on a clean machine via `cargo test --release stt:: -- --ignored`. The Wave 6 judge `mc-no-llm-in-critical-path` and the new `mc-formatter-deterministic` judge will pin formatter outputs against a fixture set rather than relying on live STT in CI.

4. **`stt::SttSegment` vs `meetings::long_form_stt::TimedSegment`.** ADR 0030 left the type's home open. Wave 2 places the canonical type in `stt::` (next to `Transcript`) and has `meetings::long_form_stt` re-export it as `TimedSegment`. Rationale: the segment type belongs to the STT layer's vocabulary; meetings consumes it. Alternative (both types coexisting with a `From` impl) is more boilerplate for no benefit.

5. **Default `transcribe_segments` impl on the trait.** ADR 0030 didn't specify whether the new method must have a default impl or be required. Wave 2 ships it WITH a single-segment fallback default. Rationale: keeps the trait extension non-breaking for any future external implementor (cloud-STT layer, mocked test STT) without forcing them to author a real segment walker.

---

## Cargo-gate checklist (Wave 2 seal)

```pwsh
# Run from repo root (NOT src-tauri/) to pick up the workspace.
powershell -File scripts/cargo-with-cuda.ps1 fmt --check
powershell -File scripts/cargo-with-cuda.ps1 check
powershell -File scripts/cargo-with-cuda.ps1 clippy --release -- -D warnings
powershell -File scripts/cargo-with-cuda.ps1 test --release          # if blocked by 0xc0000139, fall back:
powershell -File scripts/cargo-with-cuda.ps1 test --release --no-run # documented gate per LESSONS 2026-05-17

# Also run the meeting-coupling hook dry-run harness to confirm Wave 2's
# new files (activation impl, formatter impl, chunker impl,
# stt::transcribe_segments) don't accidentally cross the binding line:
python scripts/hooks/_test_meeting_coupling.py
```

Expected new test count delta: **+62 to +73** (22 activation + 27 formatter + 2 proptests + 14 chunker + 4 stt_segments − the 2 activation smokes the Wave 2 set replaces − the 1 formatter smoke the Wave 2 set replaces − the 1 chunker smoke the Wave 2 set replaces). Project total entering Wave 3: ~445–460.

## Bd-tasks to open for Wave 2

```
bd create "Phase MC Wave 2: activation.rs on_event + 22-test suite" --type task --priority 2 --parent mb-pdv
bd create "Phase MC Wave 2: formatter.rs + filler-set sliding-window + 27 tests + 2 proptests" --type task --priority 2 --parent mb-pdv
bd create "Phase MC Wave 2: chunker.rs rolling 30s/2s-overlap + crc32 + 14 tests" --type task --priority 2 --parent mb-pdv
bd create "Phase MC Wave 2: stt::SpeechToText::transcribe_segments + WhisperStt impl + 4 tests" --type task --priority 2 --parent mb-pdv
bd create "Phase MC Wave 2: Wave 3 brief (phase-mc-wave3-brief.md)" --type task --priority 2 --parent mb-pdv
```

## Pointer for Wave 3

Wave 3 picks up: twin-stream capture (mic + WASAPI loopback), the long-form STT driver that walks the chunker's output, and the actual `WH_KEYBOARD_LL` install on a dedicated meetings message-pump thread that feeds the Wave 2 activation state machine. The Wave 2 brief will reference its own outputs as the contract surface Wave 3 builds on.

— Bernard, 2026-05-20
