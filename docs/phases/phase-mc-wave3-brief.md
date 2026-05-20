# Phase MC Wave 3 brief — Twin-stream capture + loopback + long-form STT driver

> Authored end of Wave 2 (commit `bec0423`). Wave 3 author: read this
> before opening `docs/phases/phase-meeting-capture.md`. The plan is
> still binding; this brief narrows the design choices Wave 2 left
> open and pins the per-deliverable signatures + test specs.

---

## What Wave 2 shipped (so you know what to build on)

| module | net status after Wave 2 |
|---|---|
| `meetings/activation.rs` | `on_event` state machine **complete** (ADR 0027). 23 tests. **Wave 3 wires it to the OS hook** — see §4. |
| `meetings/formatter.rs` | Deterministic formatter **complete** (Section MC.3). 30 tests incl. 2 proptests. Pure, idempotent, no clocks. |
| `meetings/chunker.rs` | Rolling 30s/2s-overlap chunker + CRC32 + WAV writer **complete** (ADR 0029). 15 tests. Filenames are `<uuid>_<channel>_<seq>.wav`. |
| `meetings/long_form_stt.rs` | **Skeleton only.** `TimedSegment` is now `pub use stt::SttSegment as TimedSegment` (alias). The driver loop is Wave 3's deliverable. |
| `meetings/loopback_windows.rs` | Skeleton only. |
| `meetings/capture.rs` | Skeleton only. |
| `stt::SpeechToText::transcribe_segments` | **Complete** (ADR 0030). Default trait impl single-segment-wraps; `WhisperStt` overrides with the per-segment timestamp walker. 4 `#[ignore]`-gated tests. |

Project test count after Wave 2: ~481 (was 413; +68 net).

---

## Wave 3 deliverables (5 tasks)

### 3.1 `meetings/loopback_windows.rs` — `LoopbackCapture` impl of `AudioCapture` (P0)

**Backend decision (binding for Wave 3): `cpal` 0.15's loopback support.**

Justification (from Wave 2 exploration; record this in the ADR you'll
author for it):

- `cpal` is **already in Cargo.toml** and powers `audio::CpalCapture`.
  Re-using its event loop, sample format negotiation, default-device-
  changed handling, and `Stream` Drop semantics keeps the cross-module
  surface area minimal and the test scaffolding identical to mic.
- The `wasapi` crate (alternative considered) would buy a few hundred
  µs of latency and direct access to `AUDCLNT_STREAMFLAGS_LOOPBACK`,
  but at the cost of a new unsafe footprint, a new error taxonomy,
  and a separate Drop story. Phase MC's targets (whole-meeting
  capture, paragraph-grained output) do not need sub-ms latency.
- `cpal` 0.15 on Windows exposes loopback via
  `Host::default_output_device()` opened with an input config — the
  WASAPI backend internally sets the loopback flag. Confirm in your
  ADR by linking the `cpal` source line you used; fall back to
  `wasapi` ONLY if loopback returns silence on this test box and
  document the empirical evidence.

**ADR to author**: `docs/adrs/0031-meetings-loopback-backend.md`.
Status: accepted. Charter the choice + record the fall-back trigger.

#### Types & signatures

```rust
// src-tauri/src/meetings/loopback_windows.rs (replace skeleton)

#[cfg(target_os = "windows")]
pub struct LoopbackCapture {
    // cpal::Stream is !Send — owned by the construction thread.
    stream: Option<cpal::Stream>,
    // Resampled/downmixed mono i16 @ 16 kHz lives here; drained
    // by the caller. ringbuf::HeapRb<i16>, same shape as CpalCapture.
    consumer: SampleConsumer,
    // For the cargo gate; populated on start().
    sample_rate: u32,
    channels: u16,
}

#[cfg(target_os = "windows")]
impl LoopbackCapture {
    /// Open the default RENDER endpoint as a loopback INPUT stream.
    /// Returns Err(AppError::Audio) if no render endpoint exists
    /// (e.g. headless CI box with no audio device).
    pub fn new() -> AppResult<Self>;
}

#[cfg(target_os = "windows")]
impl super::super::audio::AudioCapture for LoopbackCapture {
    fn start(&mut self) -> AppResult<()>;
    fn stop(&mut self) -> AppResult<()>;
    fn drain(&mut self, buf: &mut Vec<i16>) -> AppResult<usize>;
    fn sample_rate(&self) -> u32 { 16_000 }
    fn channels(&self) -> u16 { 1 }
}

#[cfg(not(target_os = "windows"))]
pub struct LoopbackCapture; // todo!() stub; Phase 9.
```

#### Test specs (5–7 tests)

1. `new_succeeds_when_default_render_endpoint_exists` — `LoopbackCapture::new().is_ok()` on a box with audio. Skip-pattern (early-return) when no device, mirroring `audio::capture::tests`.
2. `start_then_stop_is_idempotent` — `start(); start(); stop(); stop();` returns Ok every call.
3. `drain_before_start_returns_zero` — fresh instance, no samples available.
4. `drain_after_brief_run_writes_mono_16khz` — `start()`, sleep 200 ms, `drain(&mut buf)`. Assert `buf.len()` is in the range `[100, 5000]` (some samples captured; not blocked). NOTE: this is hardware-dependent; gate behind `#[ignore]` if it flakes on a silent box.
5. `drop_stops_stream_without_panic` — construct, start, drop; no panic, no leak (use `tracing-test` to confirm the stop log fires).
6. `sample_rate_is_16khz_after_resample` — assert `sample_rate() == 16_000` and `channels() == 1` regardless of the render endpoint's native format (the resampler in `audio::resampler` handles this; re-use the existing `AudioPipeline`).
7. (optional) `two_loopback_instances_do_not_share_stream` — two `LoopbackCapture::new()` calls produce independent `Stream`s. Useful catch for a future singleton-by-accident refactor.

Target file size: ~250 lines (modeled on `audio::capture::CpalCapture` ≈ 350; loopback path is simpler).

---

### 3.2 `meetings/capture.rs` — `TwinStreamCapture` coordinator (P0)

Owns one mic stream + (optional) one loopback stream + one
`MeetingChunker` per active channel. Streams are `!Send` on Windows
(WASAPI handles are thread-bound), so the coordinator runs an
**owner thread** per stream and communicates via crossbeam channels.

#### Types & signatures

```rust
// src-tauri/src/meetings/capture.rs (replace skeleton)

use crate::meetings::activation::ActivationSource;
use crate::meetings::chunker::{ChunkWritten, ChunkerConfig, MeetingChunker};

/// One drained-and-chunked frame on its way to disk.
#[derive(Debug)]
pub struct ChannelChunk {
    pub channel: Channel,
    pub chunk: ChunkWritten,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Mic,
    Sys,
}

pub struct TwinStreamCapture {
    // Spawned threads + their stop signals + the channel that
    // surfaces chunks back to the runtime.
    mic_thread: Option<JoinHandle<AppResult<()>>>,
    sys_thread: Option<JoinHandle<AppResult<()>>>,
    chunk_rx: Receiver<ChannelChunk>,
    stop_tx: Sender<()>,           // broadcast: clone for both threads
    meeting_uuid: String,
    chunk_dir: PathBuf,
}

impl TwinStreamCapture {
    pub fn start(
        meeting_uuid: String,
        source: ActivationSource,
        chunk_dir: PathBuf,
        config: ChunkerConfig,
    ) -> AppResult<Self>;

    /// Stop the streams, flush trailing chunks via finalize(), join
    /// threads. Returns the trailing ChunkWritten records (one per
    /// active channel; up to 2). Idempotent on second call.
    pub fn stop(&mut self) -> AppResult<Vec<ChannelChunk>>;

    /// Non-blocking pull of any chunks that rolled since the last
    /// call. The runtime polls this at ~5 Hz.
    pub fn try_recv_chunks(&mut self) -> Vec<ChannelChunk>;
}

impl Drop for TwinStreamCapture {
    fn drop(&mut self);  // calls stop() if not already stopped
}
```

Each owner thread runs the same loop:
```
loop {
    select! {
        recv(stop_rx) -> _ => break,
        default(Duration::from_millis(50)) => {
            let n = capture.drain(&mut buf)?;
            if n > 0 {
                for cw in chunker.feed(&buf)? {
                    chunk_tx.send(ChannelChunk { channel, chunk: cw })?;
                }
                buf.clear();
            }
        }
    }
}
// On stop, call chunker.finalize() and emit a final ChannelChunk if any.
```

#### Test specs (6–8 tests)

Use a **synthetic AudioCapture** (test-only impl) that emits a known
i16 ramp at a known rate; ringbuf-backed. Construct
`TwinStreamCapture` with the synthetic mic + synthetic sys.

1. `single_channel_mic_only_emits_chunks_in_order` — `source = MicOnly`. Drive 1.5 s of audio; assert ≥1 mic chunk, no sys chunks, seqs 0,1,… contiguous.
2. `both_sources_produce_disjoint_filenames` — `source = Both`. Assert mic chunks have `_mic_` in path, sys chunks have `_sys_`, no overlap.
3. `stop_returns_trailing_chunks_from_both` — `source = Both`; drive 1 s; stop; expect 2 `ChannelChunk`s (one per channel) from `stop()`.
4. `drop_without_explicit_stop_finalizes_cleanly` — construct, drive 0.5 s, drop. No panic; trailing chunk WAV exists on disk.
5. `try_recv_chunks_is_nonblocking` — call before any audio fed; returns empty vec instantly.
6. `clock_alignment_first_sample_is_zero_for_both` — `source = Both`; assert the FIRST chunk from each channel has `first_sample = 0` (twin streams start at the same wall-clock instant; the chunker's global timeline is per-channel-zero-indexed).
7. (optional) `system_only_mode_does_not_spawn_mic_thread` — `source = SystemOnly`; assert `mic_thread.is_none()`.

Target file size: ~400 lines. If you push past 500, pre-split the
owner-thread loop into a `mod thread;` sibling.

---

### 3.3 `meetings/long_form_stt.rs` — chunked driver (P0)

Walks chunk WAVs in seq order, calls `transcribe_segments` per chunk
with a rolling 224-token `initial_prompt` (the tail of the previous
chunk's text), drops segments that fall inside the LEADING overlap
window, emits `meeting:progress` events, returns the stitched
`Vec<TimedSegment>` in **global-timeline ms**.

#### Types & signatures

```rust
// src-tauri/src/meetings/long_form_stt.rs (replace types-only stub)

pub use crate::stt::SttSegment as TimedSegment; // already present after W2

pub struct LongFormStt<'a> {
    stt: &'a mut dyn crate::stt::SpeechToText,
    chunk_rx: Receiver<ChannelChunk>,  // from TwinStreamCapture
    on_progress: Box<dyn FnMut(LongFormProgress) + Send + 'a>,
    config: LongFormConfig,
}

#[derive(Debug, Clone)]
pub struct LongFormConfig {
    /// 32_000 (2 s @ 16 kHz). Segments with t0_ms < overlap_ms inside
    /// chunks N>=1 are dropped (they're duplicates of chunk N-1's
    /// trailing region).
    pub overlap_samples: u32,
    /// 16_000.
    pub sample_rate: u32,
    /// 224. Whisper's max initial_prompt token cap.
    pub max_prompt_tokens: usize,
}

#[derive(Debug, Clone)]
pub struct LongFormProgress {
    pub channel: Channel,
    pub chunk_seq: u32,
    pub chunks_done: u32,
    pub chunks_total: Option<u32>, // None until stop() is observed
}

#[derive(Debug)]
pub struct LongFormOutput {
    pub mic_segments: Vec<TimedSegment>,
    pub sys_segments: Vec<TimedSegment>,
    pub mic_gpu_used: bool,
    pub sys_gpu_used: bool,
}

impl<'a> LongFormStt<'a> {
    pub fn new(
        stt: &'a mut dyn crate::stt::SpeechToText,
        chunk_rx: Receiver<ChannelChunk>,
        on_progress: impl FnMut(LongFormProgress) + Send + 'a,
        config: LongFormConfig,
    ) -> Self;

    /// Consume the chunk receiver until it's closed (the
    /// TwinStreamCapture was stopped). Returns the stitched per-channel
    /// segment vectors with t0_ms/t1_ms in the GLOBAL timeline.
    pub fn run(self) -> AppResult<LongFormOutput>;
}
```

#### Stitch algorithm (binding)

For each incoming `ChannelChunk`:

1. Read the WAV from disk (mono i16 16 kHz; `hound::WavReader`).
2. Recompute CRC32 over the payload and compare with
   `chunk.crc32`. Mismatch → `AppError::LongFormStt`.
3. Build `initial_prompt`: the trailing N tokens of the prior chunk's
   joined text (up to `max_prompt_tokens`). For chunk 0, prompt is
   `None`.
4. Call `stt.transcribe_segments(req)` with the chunk's i16 buffer.
5. For each returned `SttSegment`:
   - **For chunks N ≥ 1**: drop segments with `t1_ms <= overlap_ms`
     (`overlap_ms = overlap_samples / 16`). These are duplicates of
     the prior chunk's tail.
   - Translate `t0_ms` and `t1_ms` to the global timeline:
     `global_ms = (chunk.first_sample / 16) + segment_ms_within_chunk`.
   - For chunks N ≥ 1, the overlap shift applies — the segment's
     local timeline starts at the START of the overlap prefix in
     chunk N, which corresponds to global sample `chunk.first_sample`.
     So the conversion is just `chunk.first_sample / 16 + t_local_ms`.
6. Append to the per-channel vector.

After the receiver closes:
7. Sort per-channel vector by `t0_ms` ascending (defensive; should
   already be sorted by construction).
8. Return `LongFormOutput`.

#### Test specs (6–10 tests)

A **mock STT** is mandatory: define a `StubStt` test impl that
returns a pre-canned `TranscriptWithSegments` per input audio
length. Don't need real Whisper to test the stitch logic.

1. `single_chunk_passes_through_unchanged` — one chunk, one segment from the stub; output segments == input segments translated to global ms (chunk.first_sample/16 offset).
2. `two_chunks_overlap_dedup_drops_leading_2s` — chunk 0 emits segments at [0–2000ms, 2000–4000ms]; chunk 1 emits at [0–2500ms, 2500–6000ms]. Expect chunk 1's [0–2500ms] to be DROPPED (covered by overlap) and chunk 1's [2500–6000ms] to be admitted as [global 30000–33500ms] (if chunk 0 spans 0–30s and chunk 1 starts at 28s global).
3. `crc32_mismatch_returns_long_form_stt_error` — corrupt the WAV after the chunker writes it; assert `run()` returns `AppError::LongFormStt`.
4. `initial_prompt_is_tail_of_prior_chunk_text` — record `StubStt`'s last-seen `initial_prompt`; after 3 chunks assert it equals the last ~224 tokens of the prior chunk's joined output.
5. `progress_events_emitted_per_chunk` — track an `Arc<Mutex<Vec<LongFormProgress>>>` from the callback; assert N progress events for N chunks.
6. `two_channels_yield_independent_segment_streams` — push interleaved mic + sys chunks; assert `mic_segments` and `sys_segments` are each correctly stitched, with no cross-channel mixing.
7. `chunks_arrive_out_of_order_within_channel_is_rejected` — defensive: if seq goes `0, 2, 1`, return an `AppError::LongFormStt` with a "non-monotonic seq" message. (Wave 3 author: this is a safety net; the chunker emits seqs in order by construction, but the channel could reorder under load.)
8. (optional) `lossless_90s_synthetic` — synthesize a 90 s monotone-ramp WAV, push through chunker → long_form_stt with stub STT that returns "segment for sample range [start, end]"; assert the concatenated output covers `[0, 90 000 ms]` with no gaps and no duplicates. This is the integration test the plan calls out (Wave 3 task #3).

#### A note on prompt token count

Token-counting is hard without a tokenizer. Phase 4 made the dictation
prompt builder do an approximation (`text.split_whitespace().count() *
1.3` ≈ tokens). Re-use `stt::prompt_builder` if its public API exposes
the truncation helper; otherwise do the same word-based approximation
inline with a clear comment. If you need a new helper, put it in
`stt::prompt_builder` (it's the canonical home) — but keep the
existing dictation call paths untouched.

Target file size: ~450 lines. Pre-split if you push past 500.

---

### 3.4 `meetings/activation.rs` — wire to OS hook (P0)

The `on_event` state machine is done. Wave 3 wires it to a real
**second** `WH_KEYBOARD_LL` install on a **dedicated meetings
message-pump thread**.

#### Critical constraints (do not violate)

1. **Do NOT touch `hotkey/state.rs`, `hotkey/windows.rs`, or
   `hotkey/driver.rs`**. The dictation hook is sealed. Phase MC
   installs a SECOND, INDEPENDENT hook on its OWN thread. The two
   hooks share nothing.
2. The Phase MC hook calls `CallNextHookEx` so the dictation hook
   (installed earlier) still sees every keystroke. Order of
   installation matters: dictation installs first at app boot;
   meetings installs second at app boot (after dictation). Test
   this in the integration test.
3. **No global mutable state at file scope.** Use a `OnceCell` or
   pass the channel through `thread_local!` keyed by thread id, the
   same pattern dictation uses internally.

#### Types & signatures

```rust
// extend src-tauri/src/meetings/activation.rs

pub struct MeetingHotkeyInstaller {
    chord_config: ChordConfig,   // VKs from settings
    sender: Sender<ActivationEvent>,
    thread: Option<JoinHandle<()>>,
    stop_signal: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
pub struct ChordConfig {
    pub modifier_vk: u32,    // default VK_RCONTROL = 0xA3
    pub main_vk: u32,        // default VK_M = 0x4D
}

impl MeetingHotkeyInstaller {
    pub fn install(
        chord_config: ChordConfig,
        sender: Sender<ActivationEvent>,
    ) -> AppResult<Self>;

    pub fn stop(self) -> AppResult<()>;
}
```

The installer spawns a thread that:
1. Calls `SetWindowsHookExW(WH_KEYBOARD_LL, …)`.
2. Runs a `GetMessageW` pump until `stop_signal` is set.
3. The hook procedure pushes raw key events into a thread-local
   channel, which a small adapter translates into
   `ActivationEvent::ModifierKeyDown/Up` + `MainKeyDown/Up`.
4. Calls `CallNextHookEx(ptr::null_mut(), …)` for every event so
   dictation's hook downstream still sees them.

#### Test specs (3–5 tests; integration-style)

Lives in `src-tauri/tests/meeting_activation_integration.rs`
(new file). Gated `#[cfg(target_os = "windows")]`.

1. `install_and_stop_cleanly` — install, sleep 100 ms, stop. No panic, no leaked hook handle.
2. `synthetic_chord_emits_main_key_down` — install with a stub
   sender; inject a synthetic chord via `keybd_event` (Win32):
   modifier down → main down → main up → modifier up. Assert the
   sender saw `ModifierKeyDown, MainKeyDown, MainKeyUp, ModifierKeyUp`
   in order.
3. `dictation_hook_still_fires_after_meeting_install` — install
   meeting hook AFTER the dictation hook (use the existing dictation
   driver from `hotkey::driver`); inject a dictation chord; assert
   the dictation hook fired (peek at its event channel).
4. (optional) `re_install_after_stop_works` — install, stop, install
   again; second install succeeds.
5. (optional) `bad_vk_returns_error_at_install_time` — `chord_config
   .modifier_vk = 0xFFFF`; assert install returns `AppError::Hotkey`
   (or just silently no-ops if Win32 accepts the install but the VK
   never fires — that's also acceptable; document which).

**Synthetic key injection**: use `windows::Win32::UI::Input::
KeyboardAndMouse::keybd_event` (already in the `windows` crate
dependency surface). If you can't get synthetic injection to fire
the global hook reliably on a headless CI box, mark the test
`#[ignore]` and document why in the brief's deviations.

---

### 3.5 Conflict probe extension (P1)

Extend `hotkey::probe` (don't fork it). Add a helper that **rejects**
the meeting chord at startup if its VK collides with the dictation
binding. Reuse `candidate_chain` to walk fallbacks.

```rust
// extend src-tauri/src/hotkey/probe.rs

/// Wave 3 (Phase MC). Probe a candidate meeting chord against the
/// configured dictation chord. Returns Ok(vk) for the first survivor
/// in [main_vk, VK_M, VK_F13, VK_F14], or Err if all collide.
pub fn probe_meeting_main_vk(
    configured_main_vk: u32,
    dictation_main_vk: u32,
) -> AppResult<u32>;
```

Two tests:
1. `probe_meeting_returns_configured_vk_when_no_collision` — `configured=VK_M, dictation=VK_F23` → `Ok(VK_M)`.
2. `probe_meeting_walks_chain_on_collision` — `configured=VK_M, dictation=VK_M` → returns the next non-colliding VK in the chain.

---

## Deviations from `phase-meeting-capture.md` (justified)

1. **Loopback backend = `cpal` (Wave 1 deferred this decision).** Pinned in §3.1 above; ADR 0031 charters it.
2. **`LongFormStt` takes a `&mut dyn SpeechToText` reference, not ownership.** Reason: the runtime constructs the `WhisperStt` once at app boot (loading the model takes ~10 s); the long-form driver borrows it for the duration of one meeting. Passing a `Box` would force a clone of the model context, which whisper-rs doesn't support cheaply.
3. **`LongFormStt::run` is blocking (not async).** Reason: the Tauri runtime already drives the meeting lifecycle on a dedicated worker (mirrors dictation); the chunk receiver is a blocking `crossbeam::Receiver`. No tokio in this path.
4. **`TwinStreamCapture::start` is a constructor that returns a `Self`, not a `fn start(&mut self)`.** Reason: the `Send`-bound on `cpal::Stream` makes a struct that's "constructable in the IDLE state and then started" awkward — each `start` would have to re-spawn the owner threads. A fresh `Self` per meeting matches the lifecycle the runtime already wants. The `AudioCapture` trait method `start()` on `LoopbackCapture` is unchanged — that's the trait obligation; the COORDINATOR's interface is different.
5. **`probe_meeting_main_vk` lives in `hotkey::probe`, not `meetings::activation`.** Reason: it's a hotkey concern; lives with the existing probe machinery. `meetings::activation` calls it from its `install` path.

---

## Cargo gate (must be green at Wave 3 seal)

```pwsh
cd src-tauri
cargo fmt --check
cargo check
cargo clippy --release -- -D warnings
cargo test --release --no-run   # LESSONS 2026-05-17 fallback
```

Live test execution is still blocked on this box. Live-run gate on
a clean machine, when wired:
```pwsh
cargo test --release
```

---

## Test-density targets

| module | LoC budget | tests |
|---|---|---|
| `loopback_windows.rs` | ~250 | 5–7 |
| `capture.rs` | ~400 | 6–8 |
| `long_form_stt.rs` | ~450 | 6–10 |
| `activation.rs` (additions) | +~150 | +0 (integration tests live in `tests/`) |
| `meeting_activation_integration.rs` | ~200 | 3–5 |
| `probe.rs` (additions) | +~30 | +2 |

Wave 3 net test delta target: **+22 to +32**. Project total
exits Wave 3 at ~**503–513** tests.

---

## bd issues to open at the start of Wave 3

```
bd create "Phase MC W3: LoopbackCapture impl + ADR 0031" --type task --priority 0
bd create "Phase MC W3: TwinStreamCapture coordinator" --type task --priority 0
bd create "Phase MC W3: LongFormStt driver + stitch algorithm" --type task --priority 0
bd create "Phase MC W3: wire meetings hotkey installer (second WH_KEYBOARD_LL)" --type task --priority 0
bd create "Phase MC W3: probe_meeting_main_vk + collision tests" --type task --priority 1
bd create "Phase MC W3: Wave 4 brief author at seal" --type task --priority 0
```

---

## Five-attempt rule reminders

The two failure-prone deliverables in this wave are:
- **3.1 loopback**: if cpal loopback returns silence on the test box,
  STOP and pivot to the `wasapi` crate. Document the empirical evidence
  in ADR 0031.
- **3.4 second hook**: if the second `WH_KEYBOARD_LL` install doesn't
  fire (Windows can be picky about which thread owns which hook), STOP
  and escalate via STATUS. Don't burn 5 attempts hand-rolling Win32
  threading — surface it and the human will pair on it.

---

End of brief. Wave 4 brief will be authored at end-of-Wave-3 against
the persist + UI + LLM-pass + export deliverables.
