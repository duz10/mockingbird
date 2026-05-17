# Judge: secure-input-respected (Phase 3)

**Target:** `src-tauri/src/injection/secure_guard.rs` (`SecureInputGuard` trait, `WinSecureInputGuard`, `NeverSecureGuard`), `src-tauri/src/dictation.rs` (`pipeline::decide` + `complete`), `src-tauri/tests/dictation_orchestrator.rs`, ADR 0017

**Question:** When `SecureInputGuard::is_secure(&fg_keyup)` returns `true`, does the orchestrator abort injection — meaning the injector is never called, no `final` transcript row is written, and the session row records `injection_status = "aborted_secure"`?

**Rationale:** ADR 0017 amended: Mockingbird uses a process/window-class allowlist (`SECURE_CLASSES` + `ES_PASSWORD` style check) rather than the kernel-level `BlockInput`/`GUI_SECUREINPUT` mechanisms that didn't exist or didn't behave as documented. The contract is binary: the guard answers yes/no, and if yes the orchestrator MUST abort. Without this judge, a refactor that accidentally routed the abort decision through a path that still called `injector.inject` would silently start typing dictations into password fields — the worst class of bug Mockingbird could ever ship. This judge proves the abort path is wired at BOTH the pure decision layer (the orchestrator decides not to inject) AND the side-effect layer (the orchestrator actually does not call the injector + persists the canonical status string).

**Pass criteria — ALL of:**

1. **Pure decision-layer test (CI):**

   ```powershell
   pwsh scripts/cargo-with-cuda.ps1 test --release --workspace --lib `
     -- dictation::tests::secure_input_aborts_even_if_focus_matches --exact
   ```

   Confirms `pipeline::decide(.., is_secure: true, ..)` returns `Decision::Abort(InjectionOutcome::AbortedSecure)` regardless of `fg_keydown`/`fg_keyup` matching.

2. **Belt-and-suspenders test (CI):**

   ```powershell
   pwsh scripts/cargo-with-cuda.ps1 test --release --workspace --lib `
     -- dictation::tests::focus_change_into_secure_input_still_aborts --exact
   ```

   Even with the permissive focus-change policy from ADR 0020 (user dictates in app A, releases over app B), the secure-input guard on `fg_keyup` still wins. This is the test that protects against "user alt-tabs from notepad into 1Password right before releasing the hotkey."

3. **End-to-end orchestrator integration test (CI):**

   ```powershell
   pwsh scripts/cargo-with-cuda.ps1 test --release --test dictation_orchestrator `
     -- secure_input_aborts_injector_unused_two_transcripts_aborted_status --exact
   ```

   Drives the full `DictationOrchestrator::run` loop with a `ConstSecureGuard(true)` and a `RecordingInjector` whose `inject` calls are captured in an `Arc<Mutex<Vec<_>>>`. Asserts:
   - `injector.calls().is_empty()` — the recording injector saw zero calls.
   - `sessions.injection_status == "aborted_secure"` — the canonical string from `InjectionOutcome::AbortedSecure.as_db_str()`.
   - `transcripts::get_by_session(...).len() == 2` AND stages contain `{"raw","cleaned"}` AND NOT `"final"`. The "no final" assertion is the heart of the judge — `final` rows mean "this got typed somewhere," which is exactly what must NOT happen.

4. **DB column round-trip test (CI):**

   ```powershell
   pwsh scripts/cargo-with-cuda.ps1 test --release --workspace --lib `
     -- db::sessions::tests::injection_status_persists_aborted_secure --exact
   ```

   Pins the canonical string against an accidental schema break (e.g. someone renames `as_db_str()`'s output to `"aborted-secure"` and breaks the History viewer's filter).

**On failure:**

- **Block the `phase-3-complete` tag.** This is the most safety-critical judge in the phase.
- If only the unit test fails: `pipeline::decide`'s match arm for `is_secure` regressed. Diff `dictation.rs::pipeline`.
- If only the integration test fails on `injector.calls().is_empty()`: the orchestrator's `complete()` method's `Decision::Abort` arm is calling `inject` anyway. This is the production bug. Trace the `decision` match in `complete()`.
- If the integration test fails on the transcript count: `persist_complete` started writing a `final` row even on abort. Check the `injected_text` selection logic — only `Ok` / `OkClipboardNotRestored` outcomes produce `Some(text)`.

**Last run:** _Wave 5 — **GREEN**. Pure decision tests pass; orchestrator integration test passes with `injector.calls() = []`, `injection_status = "aborted_secure"`, 2 transcript rows (raw + cleaned, no final)._
