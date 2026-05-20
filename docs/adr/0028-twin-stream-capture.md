# ADR-0028: Two-channel meeting capture via twin cpal streams + clock-aligned merge

- **Status:** Accepted
- **Date:** 2026-05-20
- **Deciders:** Dustin (project lead), code-puppy (implementor)
- **Phase MC companion to:** ADR 0026 (sibling-subsystem charter)

## Context

Dictation captures from one device: the system default input (mic),
through the cpal `Stream` abstraction documented by ADR 0013. The ring
buffer is sized for 300 s of 16 kHz mono PCM; one stream, one buffer,
one Whisper pass.

Meeting capture's "Both" mode needs to record **the user** (mic) and
**the room / call** (system audio out) simultaneously and produce two
labeled transcripts (`You` / `Other(s)`). On Windows this means
capturing the **loopback** of the default output endpoint via WASAPI,
which cpal supports on its WASAPI backend via
`Host::default_output_device()` opened in loopback mode.

Two cpal streams running concurrently presents a few real concerns:

1. **`!Send` streams.** cpal streams are not `Send` on Windows (their
   internal `IAudioClient` is COM-thread-affined). Both streams must be
   created and destroyed on the same thread that runs their callbacks.
2. **Two clock domains.** The mic's `IAudioClient` and the loopback
   `IAudioClient` are different endpoints; their sample clocks drift
   relative to each other over hours.
3. **Loopback availability is not guaranteed.** Exclusive-mode audio,
   unusual drivers, or a headless audio stack can deny loopback access.
   The UI must demote `Both` → `Mic` gracefully.
4. **RAM cost upper bound.** A 6 h ceiling at 16 kHz × 2 bytes × 2
   channels is ≈ 690 MB per channel if the entire meeting sat in RAM.
   That number is the **paranoid worst case**, not the actual working
   set, because the chunker (ADR 0029) drains each ringbuf to disk
   every 30 s and the ringbuf never holds more than ~32 s of PCM live.
5. **Time-alignment for merge.** Producing a chronological "merged"
   view (mic line, then system line, then mic line again) needs a
   shared timeline. Two independent `Instant::now()` snapshots taken at
   stream-start give a reasonable anchor; sample-counted offsets within
   each stream give precise relative timestamps.

## Decision

**The meeting runtime opens two cpal streams in parallel — one against
the default input device (mic), one against the default output device's
loopback endpoint (system audio) — both on the dedicated meetings
thread (per ADR 0027). Each stream feeds its own ringbuf and its own
chunker. After stop, each channel's chunk pile is transcribed
independently; the two transcript sets are merged chronologically by
`(channel, segment_start_ms)` for presentation.**

Concretely:

1. **Stream ownership lives on the meetings thread.** Both `Stream`
   handles are constructed inside the meetings runtime thread (the
   same thread that owns the activation hook per ADR 0027); they never
   cross thread boundaries, sidestepping the `!Send` constraint.
2. **Per-channel ringbufs, sized for 32 s** (one chunk window + 2 s
   overlap buffer + slack). The 4 h / 6 h meeting ceiling is **not**
   the ringbuf size; it's the disk-side write budget. The 690 MB upper
   bound from the plan is preserved as documentation only.
3. **Lazy stream init.** The loopback stream is only opened when the
   user selects `System` or `Both`. `Mic`-only meetings open one stream
   only.
4. **Loopback probe at source-pick time.** A new
   `meeting_probe_sources` Tauri command (Section MC.6) attempts a
   brief loopback `IAudioClient::Initialize` round-trip; if it fails,
   the UI hides `System` and `Both` and surfaces a "system audio not
   available" hint. If it fails *mid-meeting* (rare; can happen if the
   default output device hot-unplugs), the runtime demotes to
   `Mic`-only, marks the session `status='demoted'`, and surfaces a
   toast.
5. **Clock anchoring.** Each stream takes an `Instant::now()` snapshot
   immediately after `Stream::play()` returns; the snapshots are stored
   on the `meeting_sessions` row. Within each stream, per-sample
   timestamps are computed as `samples_so_far / sample_rate_hz` — purely
   sample-counted, no wall-clock drift. The merge step pairs
   `(channel, t_anchor + sample_offset_ms)` for chronological ordering.
6. **Two-channel merge is presentation-only.** Each channel keeps its
   own canonical `meeting_transcripts` row (`channel='mic'`,
   `channel='system'`); the merged chronological view is computed at
   read time and optionally cached as a third row (`channel='merged'`)
   when the user exports.

## Consequences

### Positive

- **Two transcripts, one meeting.** The user gets `You: …` and
  `Other(s): …` lines, deterministic source attribution, no
  diarization-within-channel required.
- **Existing cpal/Audio abstractions are reused unchanged.** No new
  trait method; the `AudioCapture` interface accommodates both input and
  loopback by varying the device passed at construction time.
- **Mic-only meetings pay zero loopback cost.** No stream opened, no
  ringbuf allocated, no probe.
- **Failure modes are surfaced, not silent.** Loopback unavailable →
  UI demotes pre-meeting. Loopback fails mid-meeting → session marked
  `demoted`, user toast. Either way the mic transcript still ships.

### Negative

- **Cross-channel drift over multi-hour meetings.** Mic clock vs.
  loopback clock skew on typical Windows hardware is < 100 ms over 4 h
  (empirical, USB Class-1 mic + integrated DAC). Long-tail hardware
  could be worse. v1 documents the limit and accepts up to ~500 ms
  drift; if a real user reports > 500 ms, the fix is per-segment
  wall-clock anchoring (LESSONS extension, future). The 4 h target is
  meeting-scale, not concert-recording-scale; this is acceptable for
  the use case.
- **More COM surface area on the meetings thread.** Two
  `IAudioClient`s, two `IAudioCaptureClient`s, two render endpoints to
  release on shutdown. The cpal `Drop` impls handle this, but a panic
  on the meetings thread mid-recording leaks the COM handles until the
  process exits. Mitigation: the runtime wraps both streams in a
  `Drop`-aware wrapper that logs on Drop, and the Phase MC integration
  test asserts no leak after a panic-induced abort.
- **WASAPI loopback support varies by Windows build.** Documented in
  the plan's Risk #1. Wave 1 brief confirms the cpal version we ship
  supports loopback on Windows 10 1809+; older builds get the demote-
  to-mic path. If cpal turns out to lack loopback (it does in some
  older releases), the fallback is the lightweight `wasapi` crate
  (`wasapi = "0.15"`, Windows-only feature-gated) — that decision lands
  in Wave 2 with its own LESSONS entry.

### Neutral

- **The 690 MB paranoid ceiling never materializes** in normal use
  (chunker drains every 30 s). Documenting the upper bound prevents
  future "should we worry about RAM for a 4 h meeting" rediscovery.

## Alternatives considered

- **Single mixed stream (mic + system pre-mixed by Windows).** Rejected.
  Windows mixes input + system into one capture stream only when you
  enable the legacy "Stereo Mix" device, which is driver-dependent and
  generally absent on modern laptops. Also defeats two-channel labeling
  — you get one transcript with no way to attribute lines.

- **Capture system audio via the legacy Stereo Mix endpoint.**
  Rejected. Driver-dependent, frequently disabled, no reliable way to
  programmatically enable it. Modern WASAPI loopback is the supported
  path.

- **One thread per stream (two meetings threads).** Rejected. Adds a
  cross-thread channel for ringbuf consumption and complicates the
  chunker's per-channel state. The single-thread two-stream design works
  because both cpal streams take their callbacks asynchronously off
  WASAPI's worker pool — the meetings thread only drains the ringbufs,
  it doesn't service the audio callbacks itself.

- **Time-align via wall-clock per-sample timestamps.** Rejected for v1.
  Adds `std::time::Instant::now()` calls in the hot path (one per
  ringbuf drain at 50 ms tick = 20/s — fine, but unnecessary). The
  sample-counted approach is deterministic and avoids any wall-clock
  jitter. If long-tail hardware needs per-segment wall-clock anchoring
  later, the fix is local to the merge step.

## Cross-references

- **PLAN:** `docs/phases/phase-meeting-capture.md` §MC.2 (capture
  pipeline), Risks #1, #3, #10.
- **ADR 0013:** cpal ringbuf design (the abstraction this reuses).
- **ADR 0026:** charter — establishes shared-primitives-only reuse rule.
- **ADR 0027:** activation thread — same thread also owns the cpal
  streams.
- **ADR 0029:** chunked Whisper inference — the consumer of the
  ringbufs this ADR sets up.
- **bd issues:** this ADR is `mb-1xh`. Twin-stream capture impl is
  scheduled for Wave 3 (separate bd tasks to be created in Wave 2's
  brief).

---

_The `adr-format` judge validates this structure exists in every numbered
ADR. Keep section headings stable._
