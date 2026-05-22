# Judge: retention-preserves-abstracts (Phase 10)

**Target:** `src-tauri/src/activity/retention.rs::sweep_once`,
`src-tauri/src/db/migrations/015_activity_wave5_hardening.sql`
(`activity_blocks.raw_events_purged_at` column), ADR 0042
(cascade-option-(a)), AGENTS.md Principle 1 ("raw data is immutable").

**Question:** When the retention sweep runs against a fixture DB
holding events older than `events_days` AND those events have an
associated `activity_blocks` row, does the sweep
(a) DELETE the aged raw events, (b) UPDATE the Block's
`raw_events_purged_at` column to a non-NULL timestamp, and (c)
**leave the Block's `generated_abstract` text intact**?

**Rationale:** ADR 0042 specifies option (a) — when raw events age
out under a TTL, the derived Block's *abstract* survives as the
"human-readable archaeology" of the session. The retention sweep
must preserve that property: a user setting `events_days = 30` is
deliberately asking "delete the firehose, keep the summaries", and
silently nuking the abstracts alongside the events would amount to
data loss under a privacy knob. The `raw_events_purged_at`
breadcrumb on `activity_blocks` is the durable signal that "this
Block's underlying events are gone — re-running the abstractor on
it is no longer possible", and the provenance judge
(`provenance-is-total`) leans on it for the "missing event-id
references are OK when raw_events_purged_at is set" carve-out.

**Pass criteria — ALL of:**

1. **`retention.rs` pure-Rust unit suite is green:**

   ```powershell
   powershell -File scripts\cargo-with-cuda.ps1 test --release --lib `
     -- activity::retention::tests
   ```

   Existing tests cover `compute_cutoff_ms` (the pure helper) +
   `any_ttl_set` logic. Per LESSONS P2, fall back to the
   throwaway-crate recipe if live exec is blocked — `retention.rs`
   has only `rusqlite` + `serde` + `tracing` + the workspace
   `settings::model` dep.

2. **New cascade-correctness test passes:**

   *(New test to author in 6.B as
   `activity::retention::tests::sweep_marks_blocks_and_deletes_events`.)*

   - Fresh `Database::open_in_memory()`. All migrations applied
     through 015.
   - Seed one `activity_sessions` row, status = `completed`.
   - Seed three `activity_events` rows: `ts = now - 5_days`,
     `now - 3_days`, `now - 0_days` (all attached to the session).
   - Seed one `activity_blocks` row referencing all three events
     via its `source_event_ids` JSON, `generated_abstract = "wrote
     the Wave 6.A judge slate"`, `raw_events_purged_at IS NULL`.
   - Set `activity_retention_events_days = 1` via `Settings::set`.
   - Call `retention::sweep_once(&mut conn, now_ms)`.
   - Assert returned `SweepResult.events_deleted == 2` (the
     `now - 5_days` and `now - 3_days` rows).
   - Assert returned `SweepResult.blocks_marked_purged == 1`.
   - Assert the Block row still exists and its `generated_abstract`
     equals `"wrote the Wave 6.A judge slate"` byte-for-byte.
   - Assert the Block row's `raw_events_purged_at` is non-NULL and
     equal to `now_ms` (the value passed to `sweep_once`).
   - Assert `SELECT COUNT(*) FROM activity_events WHERE session_id =
     ?1` returns `1` (the `now - 0_days` row survived).

3. **Sweep is wrapped in a single transaction (structural check):**

   ```powershell
   Select-String -Path src-tauri\src\activity\retention.rs `
     -Pattern 'fn sweep_once|conn.transaction|tx.commit'
   ```

   Expected: the function body contains `conn.transaction()` and a
   matching `tx.commit()`. If the transaction is missing, a sweep
   failure mid-DELETE would leave the DB in a state where some
   events are purged and the corresponding Block's
   `raw_events_purged_at` is still NULL — a provenance lie.

4. **Settings-default privacy posture:**

   *(Reuse existing test
   `activity::retention::tests::policy_any_ttl_set_logic` +
   author one more.)*

   - Open a fresh DB, do NOT touch any retention settings.
   - Assert `retention::load(&conn).events_days == 0`,
     `segments_days == 0`, `blocks_days == 0`. Privacy-by-default:
     out of the box, nothing is auto-deleted.

5. **Block-deletion path (blocks_days TTL) is independent of the
   events path (structural eyeball):**

   ```powershell
   Select-String -Path src-tauri\src\activity\retention.rs `
     -Pattern 'blocks_cutoff|DELETE FROM activity_blocks' -Context 0,3
   ```

   Expected: the `if let Some(cutoff) = blocks_cutoff` branch
   appears AFTER the events/segments branches and AFTER the
   `raw_events_purged_at` UPDATE, and uses its own cutoff (the
   `blocks_days` TTL, not `events_days`). This is the ordering
   in ADR 0042 §Sweep order — a user with `events_days = 1` +
   `blocks_days = 0` keeps the Blocks forever; a user with
   `events_days = 0` + `blocks_days = 30` keeps the events forever
   and only ages the Blocks. The two knobs are orthogonal.

**On failure:**

- **Block the `phase-10-complete` tag.**
- If criterion 2 surfaces an abstract that got mutated or deleted:
  the sweep is over-cascading. The `DELETE FROM activity_blocks`
  must be gated on `blocks_cutoff` (the blocks_days TTL), NOT on
  `events_cutoff`. Re-check the `if let Some(cutoff) = blocks_cutoff`
  branch.
- If criterion 2 surfaces `raw_events_purged_at IS NULL` after the
  sweep: the UPDATE step (1) ran AFTER the DELETE step (2) and so
  the subquery `SELECT DISTINCT session_id FROM activity_events
  WHERE ts < ?2` returned empty. Flip the order — the UPDATE must
  precede the DELETE.
- If criterion 3 surfaces no transaction wrapper: wrap the whole
  thing in `conn.transaction()` / `tx.commit()`. Atomicity is
  non-negotiable for retention work.

**Last run (Wave 6.A dry-run):** _TBD — see Wave 6.A dispatch report._
