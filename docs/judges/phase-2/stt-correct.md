# Judge: stt-correct (Phase 2)

**Target:** `src-tauri/src/stt/whisper.rs`, `src-tauri/src/bin/stt_test.rs`, `src-tauri/tests/whisper.rs`

**Question:** Does the STT pipeline produce transcripts that match expected output within tolerance on a known speech fixture?

**Rationale:** Whisper is a probabilistic model — exact-string assertion would be brittle. But "did it produce *anything* speech-shaped, and was that shape close to the reference" is a strong regression signal. A silent regression (wrong model loaded, mel filter broken, language flag stripped) would let the binary "succeed" while emitting garbage. The fixture-vs-edit-distance check catches that.

**Pass criteria:**

```powershell
# Requires WHISPER_MODEL_PATH set to whisper-large-v3-turbo-q5_0.bin
cargo test --workspace -- whisper::transcribe_silent_fixture_yields_short_output
cargo test --workspace -- whisper::transcribe_sine_does_not_panic
cargo test --workspace -- whisper::transcribe_accepts_initial_prompt_without_error
```

Plus a real-speech smoke check (Wave-5 follow-up — needs a speech fixture, not just sine/silent):

```powershell
$env:WHISPER_MODEL_PATH = "$env:USERPROFILE\mockingbird_models\whisper-large-v3-turbo-q5_0.bin"
& target\release\stt_test.exe src-tauri\tests\fixtures\audio\hello.wav --json
```

Output JSON must contain a `"text"` field whose value, after lowercase + whitespace normalisation, has an edit distance ≤ 25% of the expected transcript length against the fixture's `.expected.txt` sidecar.

**Additional sanity check:**

- `tx.model_id == "whisper-large-v3-turbo-q5_0"` — confirms the intended model loaded.
- `tx.latency_ms > 0` — confirms the timer is wired.
- `tx.text.len() < 100` for `silent.wav` — confirms no fabrication (Whisper's silence-hallucination risk is non-negotiable per PLAN line 1752).

**On failure:**

- **Block the `phase-2-complete` tag.**
- Check `WHISPER_MODEL_PATH` resolves to an actual 547 MB+ file with the pinned SHA-256 (`394221709c…`).
- Verify `set_language(Some("en"))` is still passed in `WhisperStt::transcribe` — if removed, multilingual mode kicks in and English fixtures get mis-detected.
- Confirm `audio_f32` normalisation is `s as f32 / 32768.0` (NOT `i16::MAX`) — Whisper expects `[-1.0, 1.0]`.

**Last run:** _Wave 5 — passes for `silent.wav` + `sine_440.wav` integration tests (model loaded, compute buffers allocated, transcribe path verified end-to-end). Real-speech `hello.wav` fixture pending — needs Helios TTS fixture generation OR a recorded sample committed to `tests/fixtures/audio/`._

**Known limitation Wave 5 onward:** The Wave-4 stt_test smoke on `sine_440.wav` triggers Whisper's non-speech-iteration loop (CPU runs ~10+ min on 1s of pure tone). This is expected — sine tones aren't speech. Real-speech latency on GPU is ≤ 1 s per PLAN line 1376; see `perf-stt` judge.
