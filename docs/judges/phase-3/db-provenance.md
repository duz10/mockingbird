# Judge: db-provenance (Phase 3)

**Target:** `src-tauri/src/dictation.rs` (`insert_session_row`, `persist_complete`), `src-tauri/src/db/sessions.rs`, `src-tauri/src/db/transcripts.rs`, `src-tauri/tests/dictation_orchestrator.rs`

**Question:** After the orchestrator processes a session (regardless of injection outcome), does every provenance field on the `sessions` row hold a non-NULL value, and do the corresponding `transcripts` rows land at the expected stages?

**Rationale:** ADR 0010 is unambiguous: **provenance is total.** Every session row must trace back to its prompt, its dictionary snapshot, its example set, and the hotkey that fired it; every session must record where it started, when it ended, and which app it was meant for. A NULL in any of these columns silently severs the chain that lets future debugging, learning-loop fine-tuning, and the History viewer (Phase 6) work at all. This judge guards the post-Wave-4.9 invariant that `persist_complete` writes BOTH the session metadata AND all three transcript stages (`raw` always, `cleaned` always, `final` only on actual injection).

**Pass criteria — ALL of:**

1. **Provenance non-NULL on every column the orchestrator owns:**

   ```powershell
   pwsh scripts/cargo-with-cuda.ps1 test --release --test dictation_orchestrator `
     -- happy_path_injects_calls_writes_three_transcripts_and_ok_status --exact
   ```

   Asserts (via Rust, not bare SQL):

   ```
   prompt_id              IS NOT NULL
   dictionary_snapshot_id IS NOT NULL
   example_set_id         IS NOT NULL
   hotkey_pressed         == "RightAlt"
   started_at             not empty
   recording_ended_at     not empty
   foreground_app         == "notepad.exe"
   foreground_window_title== "Untitled - Notepad"
   injection_status       == "ok"
   ```

2. **Three transcript stages on the happy path:**

   The same test confirms `transcripts::get_by_session(conn, session_id).len() == 3` AND `stages.contains({"raw","cleaned","final"})`. This is the Wave 4.9 deliverable — earlier waves only persisted the session row, not the transcript stages.

3. **Two transcript stages on the secure-input abort path:**

   ```powershell
   pwsh scripts/cargo-with-cuda.ps1 test --release --test dictation_orchestrator `
     -- secure_input_aborts_injector_unused_two_transcripts_aborted_status --exact
   ```

   Confirms the abort path still writes raw + cleaned (provenance > shortcuts) but does **not** write a `final` row (nothing was injected). The `injection_status` column round-trips `"aborted_secure"`.

4. **Equivalent SQL assertion (manual / sqlite3.exe):**

   For ad-hoc verification on a live DB after one or more dictations:

   ```sql
   SELECT COUNT(*) FROM sessions
   WHERE prompt_id IS NULL OR dictionary_snapshot_id IS NULL
      OR example_set_id IS NULL OR hotkey_pressed IS NULL
      OR started_at IS NULL OR recording_ended_at IS NULL;
   -- expected: 0
   ```

   The `scripts/verify-wave4.9.ps1` runner already includes this probe; the judge wraps both the automated test and the manual probe.

**On failure:**

- **Block the `phase-3-complete` tag.** Provenance is non-negotiable.
- If the happy-path assertion fails on a FK column: `default_normal_config` regressed — check `bootstrap_provenance_rows`.
- If the foreground assertion fails: `insert_session_row` stopped reading from `fg_keyup` — check the `persist_complete` call path.
- If the transcript count is wrong: `persist_complete`'s `transcripts::insert_*` calls were dropped or reordered. The Wave 4.9 contract is `raw` + `cleaned` always; `final` iff `injected_text.is_some()`.

**Last run:** _Wave 5 — **GREEN**. Happy-path test asserts 3 transcript stages + every provenance column; abort-path test asserts 2 stages + `aborted_secure` status. Both pass on a fresh in-memory DB seeded with `default_normal_config(&db.conn)`._
