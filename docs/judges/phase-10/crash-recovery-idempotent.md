# Judge: crash-recovery-idempotent (Phase 10)

**Target:** `src-tauri/src/activity/crash_recovery.rs::recover_all`,
`mark_interrupted_sessions`, `cleanup_orphan_chunk_dirs`,
`src-tauri/src/lib.rs` setup hook (where `recover_all` is called at
boot), ADR 0036 §Decision item 9 (boot recovery).

**Question:** When `recover_all` runs twice in succession against the
same DB + audio_base_dir, does the second call produce a no-op (no
double-promotion of session rows, no double-deletion of directories,
no DB / FS state drift)? Sub-case: if two `recover_all` calls
run *concurrently* against the same DB (simulating a degenerate
double-boot where the single-instance lock is missing or racing),
does the pair complete without DB corruption?

**Rationale:** Boot is the riskiest moment in the app's lifecycle.
A user closes the laptop lid mid-meeting; the next morning, three
things could go wrong simultaneously: (a) an `activity_sessions` row
left at `status = 'in_progress'` from the prior crash, (b) an audio
chunk_dir on disk with no clean owner, (c) the user double-clicks the
Mockingbird shortcut before the single-instance lock takes hold. The
recovery pass must handle all three cases idempotently — re-running
must never DUPLICATE the recovery (e.g. promote an
already-`crashed_recovered` row to a fictional second state, or
attempt `remove_dir_all` on an already-deleted path and panic). The
concurrent-boot sub-case is the failure mode that's hardest to spot
in single-pass tests but bites in the wild when the user gets
impatient and double-clicks.

**Pass criteria — ALL of:**

1. **Existing idempotency tests pass:**

   ```powershell
   powershell -File scripts\cargo-with-cuda.ps1 test --release --lib `
     -- activity::crash_recovery::tests
   ```

   Expected: all 5 existing tests pass
   (`mark_interrupted_promotes_in_progress_to_crashed_recovered`,
   `mark_interrupted_is_idempotent`,
   `mark_interrupted_leaves_completed_rows_alone`,
   `cleanup_orphan_chunk_dirs_deletes_unknown_and_keeps_known`,
   `cleanup_handles_missing_audio_base_dir`,
   `recover_all_combines_steps`). `mark_interrupted_is_idempotent`
   is the existing single-step idempotency proof.

2. **Full-pass `recover_all` is idempotent (new test):**

   *(New test to author in 6.B as
   `activity::crash_recovery::tests::recover_all_is_idempotent`.)*

   - Fresh in-memory `Database`. Seed one `in_progress` session via
     `seed_in_progress`. Create one known-session chunk dir and one
     orphan chunk dir under a `tempfile::TempDir`.
   - First call: `let r1 = recover_all(&conn, &base);`
     Assert `r1.sessions_recovered == 1`,
     `r1.orphan_dirs_deleted == 1`, `r1.orphan_dirs_kept == 1`.
   - Second call: `let r2 = recover_all(&conn, &base);`
     Assert `r2.sessions_recovered == 0`,
     `r2.orphan_dirs_deleted == 0`, `r2.orphan_dirs_kept == 1`
     (the known-session dir is still there + still associated with a
     valid session row).
   - Assert the `activity_sessions` row's `status` is
     `crashed_recovered` and its `ended_at` is unchanged between
     `r1` and `r2` — no double-promotion timestamp drift.
   - Assert `SELECT COUNT(*) FROM activity_sessions WHERE status =
     'crashed_recovered'` returns 1 (NOT 2).

3. **Concurrent-boot safety (new test — likely RED on Wave 6.A
   dry-run if not yet implemented; that result is information,
   not blame):**

   *(New test to author in 6.B as
   `activity::crash_recovery::tests::recover_all_handles_concurrent_calls`.)*

   - Same fixture as criterion 2.
   - `let conn = Arc::new(Mutex::new(db.conn));`
     `let base = Arc::new(temp_dir.path().to_owned());`
   - Spawn two threads, each calling
     `recover_all(&conn.lock().unwrap(), &base)`. Join both.
   - Assert: across the union of the two `RecoveryReport`s,
     `sessions_recovered` totals exactly 1 (not 2 — the second
     caller saw the row already promoted).
   - Assert across the union, `orphan_dirs_deleted` totals exactly 1
     — `remove_dir_all` should fail gracefully on the
     already-deleted path, and the failure path in
     `cleanup_orphan_chunk_dirs` LOGS + CONTINUES per the module
     doc-comment.
   - Assert the DB has exactly 1 `crashed_recovered` row at the end.

   **Note for the implementer (6.B):** the current `recover_all`
   signature takes `&Connection` (a borrow), which means the
   `Mutex<Connection>` model from the daemon's
   `Arc<Mutex<Connection>>` pattern naturally serializes the two
   calls. The test should still pass — but if it surfaces a race in
   the FS-walk portion (where each thread independently reads the
   directory listing), the fix is either (a) hold the DB lock across
   the FS walk too, or (b) catch+log `ErrorKind::NotFound` from
   `remove_dir_all`. The current code already catches arbitrary
   `io::Error` and logs+continues, so option (b) is the
   already-shipped posture — verify the test reflects that.

4. **The `ended_at` synthesis uses `MAX(updated_at, started_at)`, NOT
   `now_ms` (structural check):**

   ```powershell
   Select-String -Path src-tauri\src\activity\crash_recovery.rs `
     -Pattern 'ended_at|MAX\(updated_at|now_ms' -Context 0,2
   ```

   Expected: `mark_interrupted_sessions` synthesizes `ended_at =
   COALESCE(ended_at, MAX(updated_at, started_at))`. Using `now_ms`
   instead would be misleading on a session left dangling for days
   ("ended today" when the user crashed last Tuesday). The existing
   test `mark_interrupted_promotes_in_progress_to_crashed_recovered`
   asserts this — keep that assertion in any future refactor.

5. **Recovery is wired at boot before runtime spawn:**

   ```powershell
   Select-String -Path src-tauri\src\lib.rs -Pattern 'recover_all|crash_recovery'
   ```

   Expected: `crash_recovery::recover_all(...)` appears in the
   `.setup(...)` callback BEFORE
   `ActivityCaptureRuntime::spawn(...)`. If the order flips, the
   runtime could pick up `in_progress` rows as "live" sessions and
   re-attach to them — at best a UX surprise, at worst a duplicate
   `started_at` event stream.

**On failure:**

- **Block the `phase-10-complete` tag** (or, since this is a Wave 5
  hardening judge against the freshly-shipped subsystem, gate the
  Wave 6 seal on a 6.B fix-loop).
- If criterion 2 surfaces a doubled `sessions_recovered` on the
  second call: the first call's `UPDATE` is missing a `WHERE status =
  'in_progress'` clause, or its synthesized `ended_at` violates the
  precondition for "this row is now terminal".
- If criterion 3 surfaces DB corruption (e.g. duplicate
  `crashed_recovered` rows or a deadlock): the recovery pass needs
  to hold the DB lock through the FS walk, OR be guarded by a
  process-wide single-instance lock at the OS level (the
  `tauri-plugin-single-instance` plugin is the natural fit). The
  RIGHT fix is the OS-level lock, not a Rust-level mutex — but
  the test surfaces the gap either way.

**Last run (Wave 6.A dry-run):** _TBD — see Wave 6.A dispatch report.
Likely-red items: criterion 2 (full-pass idempotency test doesn't
exist yet) + criterion 3 (concurrent-boot test doesn't exist yet).
Both are "fixture mismatch" reds, not real-bug reds._
