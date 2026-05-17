# Judge: e2e-injection (Phase 3)

**Target:** `src-tauri/src/dictation.rs` (`DictationOrchestrator::run` + `complete`), `src-tauri/tests/dictation_orchestrator.rs`

**Question:** When a full `StartCapture(Normal) → StopCapture` cycle runs through the orchestrator with all dependencies stubbed in-memory, does the injector receive `("hello world", Paste)` and does the session row land with `injection_status = "ok"`?

**Rationale:** Phase 3's promise is that holding the hotkey above an editable Windows app and releasing it pastes the cleaned transcript at the caret. Wave 4 wired the pipeline (`audio → VAD → STT → cleanup → strategy resolve → secure-input + focus-loss → inject → DB persist`) but the only assertions in Waves 1–4 covered the **pure decision layer** (`dictation::pipeline::decide`). This judge wraps the integration test that exercises `complete()` end-to-end with stub trait impls — the same orchestrator code path the production binary takes, minus the OS surfaces. Without it, a refactor that broke the wiring between e.g. `cleaner.clean` and `injector.inject` could ship without a single test failing.

**Pass criteria — ALL of:**

1. **Pure unit test for the decision layer (CI):**

   ```powershell
   pwsh scripts/cargo-with-cuda.ps1 test --release --workspace --lib `
     -- dictation::tests::happy_path_proceeds_with_paste --exact
   ```

   Confirms `pipeline::decide` returns `Decision::Proceed(InjectionStrategy::Paste)` for `notepad.exe → notepad.exe` with `is_secure: false`.

2. **End-to-end orchestrator integration test (CI):**

   ```powershell
   pwsh scripts/cargo-with-cuda.ps1 test --release --test dictation_orchestrator `
     -- happy_path_injects_calls_writes_three_transcripts_and_ok_status --exact
   ```

   Confirms the full `DictationOrchestrator::run` event loop:
   - Calls `Injector::inject` exactly once with `("hello world", InjectionStrategy::Paste)`.
   - Persists a session row with `injection_status = "ok"`.
   - Persists three transcript rows (`raw`, `cleaned`, `final`), all containing `"hello world"`.

3. **Text-fidelity integration test (CI):**

   ```powershell
   pwsh scripts/cargo-with-cuda.ps1 test --release --test dictation_orchestrator `
     -- cleaned_text_round_trips_through_injector_call_verbatim --exact
   ```

   Confirms unicode + punctuation (`"Hello, world! — Mockingbird's clipboard 🎙"`) pass through the `PassthroughCleaner` and into the injector call byte-for-byte. Phase 4's LLM cleaner replaces the passthrough; this test pins the contract that the **orchestrator** does not silently mangle text.

**On failure:**

- **Block the `phase-3-complete` tag.** This is the phase's headline deliverable: hotkey → audio → transcript at caret. Without it, Phase 3 isn't sealed.
- If the unit test fails: regression in `pipeline::decide`. Diff `dictation.rs::pipeline` against the previous tag.
- If the integration test fails on the injector-call assertion: the orchestrator stopped calling the injector. Check `complete()` for an early-return or a swallowed `Decision::Proceed` arm.
- If the integration test fails on the DB assertion: `persist_complete` regressed. Check `db::sessions::insert` + `db::sessions::update_processing_complete`.
- If the text-fidelity test fails: a cleaner or injector somewhere mutated `text` without telling anyone. ADR 0010 says raw is immutable — this extends to "what was injected matches what the cleaner returned."

**Last run:** _Wave 5 — **GREEN**. 3/3 tests pass: pure decision layer + happy-path orchestrator + text-fidelity all green. Run via `pwsh scripts/cargo-with-cuda.ps1 test --release --test dictation_orchestrator` after setting `ORT_DYLIB_PATH=%USERPROFILE%\mockingbird_models\onnxruntime.dll`._
