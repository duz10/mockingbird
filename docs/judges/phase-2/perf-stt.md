# Judge: perf-stt (Phase 2)

**Target:** `src-tauri/benches/whisper_latency.rs`, `src-tauri/src/stt/whisper.rs`

**Question:** On a real speech WAV with GPU acceleration, is end-to-end transcribe latency < 1000 ms for a 10-second audio fixture?

**Rationale:** PLAN line 1376 sets the latency budget at **< 1 s for 10 s audio on RTX 2060**. This is the user-perceptible budget: hold hotkey, speak, release, see text appear within roughly one second of release. Anything slower breaks the dictation rhythm and the product feels broken. The bench catches latency regressions even when correctness tests pass.

**Pass criteria:**

```powershell
# Requires WHISPER_MODEL_PATH set + cuda feature compiled in
cargo bench --bench whisper_latency
```

The bench produces criterion's HTML report at `target/criterion/whisper_latency_*/report/index.html`. **Pass** = mean latency < 1000 ms on a 10-second speech WAV (NOT `sine_440.wav` — that's 1 s, won't exercise the budget, and triggers Whisper's non-speech iteration loop).

Wave-5 follow-up: commit a 10-second speech fixture as `tests/fixtures/audio/speech_10s.wav` (Helios delegation candidate — `System.Speech.Synthesis` Windows TTS) and rename the bench to `whisper_latency_10s_speech_gpu`.

**Tolerance:**

- CPU baseline (informational, not the judge target): typically 5–20× the GPU budget on a desktop CPU. The `whisper_latency_1s_sine_cpu` bench from Wave 4 is informational only.
- GPU pass band: mean ≤ 1000 ms, p95 ≤ 1500 ms.

**On failure:**

- **Block the `phase-2-complete` tag.**
- Check whether `gpu_used = true` in the bench's per-iter Transcript — if it's false, the cuda feature regressed (see `cuda-verified` judge).
- Check `whisper-rs` version — bumps may change default params (n_threads, beam size). Pin in `Cargo.lock`.
- Check the model variant — only `large-v3-turbo` meets the budget; `large-v3` (full) is ~5× slower.

**Last run:** _Wave 5 — **NOT RUN** (gated on `cuda-verified` going green; bench harness is wired and graceful-skips today). Wave-4 CPU smoke on `sine_440.wav` ran > 19 CPU-minutes (non-speech iteration loop) — irrelevant for this judge by design._
