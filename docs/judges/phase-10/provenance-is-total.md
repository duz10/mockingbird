# Judge: provenance-is-total (Phase 10)

**Target:** `activity_blocks.prompt_version_sha` column,
`activity_blocks.source_event_ids` JSON,
`activity_blocks.raw_events_purged_at` column (migration 015),
`activity_events` primary key referenced by the JSON array,
`src-tauri/src/activity/abstractor.rs::TEMPLATE_NO_PAYLOAD_SHA`,
`activity/abstractor.rs::current_prompt_set_sha()`,
`activity/abstractor.rs::current_prompt_set_sha_audio()`,
`activity/blocks_persist.rs::insert_block`, AGENTS.md Principle 2
("provenance is total").

**Question:** For every `activity_blocks` row in the DB, does the
provenance chain hold?

- (a) `prompt_version_sha` is non-NULL and non-empty.
- (b) The value resolves to a **known** fingerprint family:
  - the sentinel `"template_no_payload_v1"` (the deterministic
    template path), OR
  - the family `abstract_v1-<8 hex>` (Wave 3 LLM path), OR
  - the family `abstract_v2_audio-<8 hex>` (Wave 4 audio-aware LLM
    path), OR
  - a "looks-like-an-LLM-prompt-sha" string (64-hex SHA-256 or
    similar — escape hatch for future LLM-grader prompts that
    haven't been re-fingerprinted into the families above; logged
    + accepted with a `provenance-judge-unknown-prefix` warning).
- (c) The JSON in `source_event_ids` parses as a list of strings.
- (d) Every id in `source_event_ids` either:
  - References an existing `activity_events.id`, OR
  - The Block's `raw_events_purged_at` is non-NULL (the retention
    sweep deleted the events but the breadcrumb is set — that's
    the ADR 0042 §option-(a) carve-out).

**Rationale:** Principle 2 says "every session row references the
exact prompt version, dictionary snapshot, and example set used".
For the Activity Capture subsystem, "the prompt version used" is
the `prompt_version_sha` (one of the three fingerprint families
above, depending on whether the deterministic template short-
circuited, the v1 prompt ran, or the v2 audio-aware prompt ran).
"The example set used" is implicit in the source event corpus, so
`source_event_ids` is the row-level provenance pointer. If either
column is NULL or dangling, a future user cannot answer the
question "why does this Block say what it says?" — which is the
exact failure mode `STATUS.md` calls out as the *thing that
makes Mockingbird different from telemetry-shipping competitors*.

The `raw_events_purged_at` carve-out exists because ADR 0042
deliberately allows raw events to age out under retention while
preserving the Block's abstract. A Block in that state has dangling
`source_event_ids` BY DESIGN — and the judge has to know not to
flag it.

**Pass criteria — ALL of:**

1. **`abstractor.rs` fingerprint constants are present and named as
   expected (structural check):**

   ```powershell
   Select-String -Path src-tauri\src\activity\abstractor.rs `
     -Pattern 'TEMPLATE_NO_PAYLOAD_SHA|abstract_v1-|abstract_v2_audio-'
   ```

   Expected: at least one `pub const TEMPLATE_NO_PAYLOAD_SHA: &str
   = "template_no_payload_v1";`, plus the two CRC32-formatted
   string literals `"abstract_v1-{:08x}"` and
   `"abstract_v2_audio-{:08x}"`. If any name drifts (e.g.
   `TEMPLATE_V1_SHA`, `abstract_v3_*`), update both this judge
   spec and the consumers — but DO NOT silently let the
   fingerprint vocabulary expand without the judge knowing about
   it.

2. **Every `activity_blocks` row has non-NULL, non-empty
   `prompt_version_sha`:**

   *(SQL probe, runnable against a real DB or a fixture DB. New
   test to author in 6.B as
   `activity::blocks_persist::tests::all_blocks_have_prompt_version_sha`.)*

   ```sql
   SELECT COUNT(*) FROM activity_blocks
   WHERE prompt_version_sha IS NULL OR prompt_version_sha = '';
   ```

   Expected: `0`. Run against a fixture DB seeded by walking the
   abstractor over a representative event stream (use the existing
   abstractor test fixtures as input).

   Live-DB version (Dustin runs this against his real DB before
   the seal — informational, not blocking):

   ```powershell
   $db = Join-Path $env:USERPROFILE "AppData\Roaming\com.mockingbird.app\mockingbird.db"
   sqlite3 $db "SELECT COUNT(*) FROM activity_blocks WHERE prompt_version_sha IS NULL OR prompt_version_sha = '';"
   ```

   Expected: `0`. A non-zero answer here is a real Wave-5-era bug
   and warrants a Wave 6.B fix.

3. **Every `prompt_version_sha` value parses as a known fingerprint
   family (SQL + regex, runnable against a real DB or fixture):**

   *(New test to author in 6.B as
   `activity::blocks_persist::tests::prompt_version_sha_is_known_family`.)*

   For each row's `prompt_version_sha` value `v`, assert one of:

   - `v == "template_no_payload_v1"`, OR
   - `v` matches `^abstract_v1-[0-9a-f]{8}$`, OR
   - `v` matches `^abstract_v2_audio-[0-9a-f]{8}$`, OR
   - `v` matches `^[0-9a-f]{40,64}$` (LLM prompt SHA fallback;
     log a `provenance-judge-unknown-prefix` warning but don't
     fail — this is the escape hatch).

   Anything else (e.g. an empty string, a base64 blob, a UUID, a
   "WARNING:..." debug string accidentally written) is a FAIL.

4. **JSON validity of `source_event_ids`:**

   *(New test as
   `activity::blocks_persist::tests::source_event_ids_is_valid_json_array_of_strings`.)*

   For each row, `serde_json::from_str::<Vec<String>>` on the
   `source_event_ids` column must succeed. If a Block is the result
   of a `merge_blocks` operation, the array may contain duplicates
   — that's fine; the test only enforces *parseability*, not
   uniqueness.

5. **Event-id FK walk holds (or the Block is post-purge):**

   *(New test as
   `activity::blocks_persist::tests::source_event_ids_reference_existing_rows_or_block_is_purged`.)*

   For each `activity_blocks` row:

   - Parse `source_event_ids` into `Vec<String>`.
   - If empty, skip (a Block with no source events is a
     `template_no_payload_v1` row — fine, but should be rare; log
     a count of these for diagnostic purposes).
   - Else: assert every id either
     - exists in `activity_events.id`, OR
     - the Block's `raw_events_purged_at` IS NOT NULL.

   In other words, a dangling reference is ONLY legal when the
   retention sweep has explicitly marked the Block as
   "raw-events-cleaned-up".

6. **The `insert_block` and `upsert_block` paths cannot write a
   NULL `prompt_version_sha` (structural check):**

   ```powershell
   Select-String -Path src-tauri\src\activity\blocks_persist.rs `
     -Pattern 'prompt_version_sha'
   ```

   Expected: the `INSERT` / `UPDATE` SQL strings include
   `prompt_version_sha` as a non-optional parameter (the `params!`
   tuple binds a `&str`, not an `Option<&str>`). If the binding is
   `Option<...>`, FAIL — that allows a future caller to pass `None`
   and silently break the provenance invariant.

**On failure:**

- **Block the `phase-10-complete` tag.**
- If criterion 2 fails on a real DB: a Wave 3 / Wave 4 path is
  inserting a Block without computing the prompt SHA. Trace the
  `insert_block` call site for that Block — usually the abstractor
  ran in a code path that bypassed `current_prompt_set_sha()`.
- If criterion 3 surfaces an unknown prefix: figure out which
  module is generating the new family and either (a) rename it to
  `abstract_v3_*` and update this judge, or (b) add a new family
  arm to the list. Don't paper over by widening the SHA escape
  hatch.
- If criterion 5 surfaces a dangling FK in a non-purged Block: the
  abstractor or `merge_blocks` is referencing event ids it didn't
  actually consume. Re-check the `source_event_ids_json`
  construction at the call site.
- If criterion 6 surfaces an optional binding: tighten the
  signature to `&str` (no `Option`) and surface the now-broken
  callers in the compile error.

**Last run (Wave 6.A dry-run):** _TBD — see Wave 6.A dispatch
report. Criteria 1, 6 (structural eyeball + grep) are runnable
this dispatch; criteria 2-5 (test suite) likely red on
"fixture mismatch" — new tests don't exist yet — but a live-DB
probe of criteria 2 + 3 against Dustin's real DB IS runnable
inside 6.A and reported below._
