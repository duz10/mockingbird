# ADR-0031: Meeting loopback capture rides cpal's WASAPI backend, not a separate `wasapi` crate

- **Status:** Accepted
- **Date:** 2026-05-20
- **Deciders:** Dustin (project lead), code-puppy (implementor)
- **Phase MC companion to:** ADR 0026 (sibling-subsystem charter), ADR 0028 (twin-stream design)

## Context

ADR 0028 established that meeting capture's "System" and "Both" modes
need to record the system audio output via Windows WASAPI loopback.
That ADR explicitly left the **backend choice** open:

> WASAPI loopback support varies by Windows build. Documented in the
> plan's Risk #1. Wave 1 brief confirms the cpal version we ship
> supports loopback on Windows 10 1809+; older builds get the demote-
> to-mic path. If cpal turns out to lack loopback (it does in some
> older releases), the fallback is the lightweight `wasapi` crate
> (`wasapi = "0.15"`, Windows-only feature-gated) — that decision lands
> in Wave 2 with its own LESSONS entry.

Wave 2 didn't materially touch loopback (it focused on the pure
modules: activation state machine, formatter, chunker, and the ADR
0030 `transcribe_segments` trait extension). The decision is therefore
deferred to **Wave 3** and pinned here.

The candidates remain:

1. **`cpal` 0.15.3's WASAPI backend** — already in `Cargo.toml`
   (powers `audio::CpalCapture`). The WASAPI backend (`cpal::host::
   wasapi`) **transparently** sets `AUDCLNT_STREAMFLAGS_LOOPBACK`
   whenever `build_input_stream(...)` is invoked on a device whose
   `data_flow() == eRender`. Source evidence (cpal 0.15.3,
   `src/host/wasapi/device.rs` lines 568–572):
   ```rust
   let mut stream_flags = Audio::AUDCLNT_STREAMFLAGS_EVENTCALLBACK;
   if self.data_flow() == Audio::eRender {
       stream_flags |= Audio::AUDCLNT_STREAMFLAGS_LOOPBACK;
   }
   ```
   And the crate's own top-level docstring on the WASAPI `Host`:
   ```rust
   /// Note: If you use a WASAPI output device as an input device it
   /// will transparently enable loopback mode (see
   /// https://docs.microsoft.com/en-us/windows/win32/coreaudio/
   /// loopback-recording).
   ```
   In other words: `host.default_output_device()` + `build_input_stream`
   on that device == loopback recording, automatically, no flag
   plumbing required.

2. **The standalone `wasapi` crate (`wasapi = "0.15"`)** — a thinner
   binding to `IAudioClient`/`IAudioCaptureClient`. Gives direct
   control over the loopback flag, share-mode, buffer duration.
   Higher unsafe footprint, separate error taxonomy, separate Drop
   story, but bypasses any cpal-specific bug we might trip on.

## Decision

**Phase MC meeting capture rides cpal's WASAPI backend for loopback.**
No new audio crate is added. The `audio::CpalCapture` type gains a
`new_loopback()` constructor that targets the default render endpoint
instead of the default input endpoint; everything downstream (the
resampler pipeline, the ringbuf, the device-change watcher, the
`AudioCapture` trait impl) is reused unchanged.

Concretely:

1. **`audio::capture::DeviceSource`** — a new private enum
   (`Input | Loopback`) is added to `audio/capture.rs`. `CpalCapture`
   stores one and consults it in `start()` and in the device
   watcher; otherwise the impl is identical for both modes.
2. **`CpalCapture::new()` continues to mean "default input device"**
   (`source = DeviceSource::Input`). No existing call site changes.
   All 11 existing tests in `audio::capture::tests` and
   `tests/audio_capture.rs` stay green.
3. **`CpalCapture::new_loopback()` is added** as a sibling constructor
   (`source = DeviceSource::Loopback`). Its body is a one-line
   wrapper that delegates to a shared private constructor with the
   source argument.
4. **`meetings::loopback_windows::LoopbackCapture`** is the
   meetings-side public type. It's a thin newtype wrapper around a
   private `CpalCapture` instance constructed via `new_loopback()`.
   It implements `AudioCapture` by delegating every method to the
   inner instance. Same module boundary the phase plan called for; no
   logic duplication.
5. **The device-change watcher polls the appropriate endpoint** based
   on `source`: `default_input_device()` for `Input`,
   `default_output_device()` for `Loopback`. Otherwise the watcher
   thread, the `Arc<AtomicBool>` flag, and the `device_changed()` /
   `take_device_changed()` API are identical.
6. **Fallback to the `wasapi` crate is NOT pre-wired.** If real-world
   testing on Dustin's box (or a future user's box) produces silent
   loopback streams or driver-specific failures, the LESSONS entry
   from that incident will trigger a successor ADR (0031-bis or 0032)
   that adds `wasapi` as a feature-gated alternative. Pre-wiring it
   now would be YAGNI: two backends with no empirical reason to need
   them.

## Consequences

### Positive

- **Zero new dependencies.** No `Cargo.toml` change for loopback.
  The `cargo audit` / supply-chain surface area stays where it is.
  ADR 0026's "reuse only existing primitives" guidance is honored.
- **Code reuse maximized.** ~6 lines change in `audio/capture.rs`
  (the `DeviceSource` field, the dispatch in `start()`, the dispatch
  in `spawn_watcher`). `LoopbackCapture` in `meetings/` is ~80 lines
  of pure delegation. No copy-pasted stream-build logic, no
  duplicated resampler wiring, no parallel watcher thread to keep in
  sync with the dictation one.
- **Same resampler pipeline.** Whatever sample format / rate / channel
  count the render endpoint exposes (typically 48 kHz stereo f32),
  `audio::resampler::AudioPipeline` handles the conversion to 16 kHz
  mono i16. Dictation already exercises every code path in that
  pipeline; loopback gets it for free.
- **Same restart-after-stop contract.** ADR 0013 / Phase 2 Wave 4.8
  established that `start()` rebuilds the ringbuf fresh each time.
  Loopback inherits this. No second design for "what happens if the
  user stops + restarts capture mid-meeting" — there's one design.
- **`!Send` constraint already understood.** `cpal::Stream` is `!Send`
  on Windows (WASAPI handles are thread-bound; see LESSONS 2026-05-20
  W2 Finding 3). Wave 3's `TwinStreamCapture` (ADR 0028) already
  factors in owner-thread-per-stream; both mic and loopback inherit
  the same constraint, same mitigation.

### Negative

- **A future cpal regression silently breaks loopback.** If a future
  cpal release changes how `eRender` devices route through
  `build_input_stream` (unlikely — the auto-loopback comment is on
  the public `Host` doc), we'd discover it during Phase MC's hands-on
  QA matrix (Wave 4). Mitigation: the test
  `loopback_start_actually_captures_audio` runs in Wave 3 and asserts
  that ≥ 1 second of capture produces a non-trivial sample count.
- **No knob to force exclusive-mode loopback.** cpal's loopback path
  is shared-mode only. For meeting capture this is the right default
  (shared-mode is what every other app on the box uses); if anyone
  ever wants exclusive-mode loopback (a niche use case — exclusive
  mode blocks all other audio on the endpoint), the `wasapi` crate
  fallback is where that lands.
- **Loopback availability still varies by Windows build.** Win10 1809+
  is the floor (per ADR 0028); older builds get the
  demote-to-mic-only path that ADR 0028 already specifies. This ADR
  inherits that mitigation; it doesn't add new ones.

### Neutral

- **The `wasapi = "0.15"` candidate stays in the toolbox.** If the
  empirical evidence ever flips, the successor ADR is straightforward
  to write: add the dep behind a `loopback-wasapi-backend` feature
  flag, add a `wasapi_loopback.rs` sibling to `loopback_windows.rs`,
  swap which `Box<dyn AudioCapture>` `TwinStreamCapture` constructs.
  The trait boundary already exists.

## Alternatives considered

- **Adopt `wasapi = "0.15"` pre-emptively.** Rejected. YAGNI. We have
  no empirical evidence cpal's path fails on Dustin's hardware. Adding
  the dep now means carrying two backends through the rest of Phase
  MC; if cpal works, the `wasapi` backend becomes dead code. The
  fallback ADR pattern (charter on demand) is the cheaper road.

- **Fork `CpalCapture` into a separate `LoopbackCapture` type with
  duplicated stream-build / resampler / watcher logic.** Rejected.
  Two ~350-line modules differing in one device-resolver line would be
  a textbook DRY violation. The `DeviceSource` enum + 6-line dispatch
  is the minimum-touch refactor that keeps the public API of
  `CpalCapture` stable while adding the new variant.

- **Make `audio::AudioCapture` itself parameterized over a device
  resolver / source.** Rejected. The trait is consumed by
  `dictation::*` which is sealed for Phase MC. Changing the trait
  shape would force changes in sealed code. Keeping `audio::AudioCapture`
  unchanged and shipping the variation inside the impl is the correct
  layer to extend.

- **Run the loopback stream on a different thread than the mic
  stream.** Rejected. ADR 0028 explicitly puts both streams on the
  same meetings thread (cpal's WASAPI worker pool services the
  audio callbacks asynchronously regardless). Spinning a second
  thread for the loopback stream alone would add a cross-thread
  channel for ringbuf draining and force the chunker per-channel
  state to be `Sync`. Net: more code, no win.

## Cross-references

- **PLAN:** `docs/phases/phase-meeting-capture.md` §MC.2 (capture
  pipeline), Risk #1 (loopback availability).
- **ADR 0013:** cpal ringbuf design — the abstraction reused
  wholesale here.
- **ADR 0026:** Phase MC charter — establishes "reuse only existing
  primitives" guidance; honored here (no new dep).
- **ADR 0028:** twin-stream capture — calls out the loopback choice as
  a Wave-2-or-3 decision; this ADR closes it.
- **Wave 3 brief:** `docs/phases/phase-mc-wave3-brief.md` §3.1 —
  specifies the impl this ADR charters.
- **bd issues:** this ADR is the closer for `mb-pdv.6` (LoopbackCapture
  impl + ADR 0031).

---

_The `adr-format` judge validates this structure exists in every numbered
ADR. Keep section headings stable._
