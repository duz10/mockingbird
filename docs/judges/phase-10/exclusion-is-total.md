# Judge: exclusion-is-total (Phase 10)

**Target:** `src-tauri/src/activity/exclusion.rs`,
`src-tauri/src/activity/runtime.rs::ActivityCaptureRuntime::record_event`,
`src-tauri/src/activity/runtime.rs::ActivityCaptureRuntime::reload_exclusion_rules`,
`src-tauri/src/db/migrations/015_activity_wave5_hardening.sql`
(built-in seeded rules), ADR 0043, AGENTS.md Principle 8.

**Question:** For any fixture activity session run with the exclusion
matcher enabled, do **zero** rows reach `activity_events` for windows
matched by any enabled rule (`app_glob` / `title_regex` / `system`)?
And on mid-session rule reload, do events from the new-rules-active
window never accidentally bleed into the old-rules-active window (or
vice versa)?

**Rationale:** Principle 8 ("secure-input fields abort injection")
extends naturally to activity capture: a password manager's foreground
window, a UAC consent dialog, or a UIA-flagged password field must
never produce a persisted row. The matcher is consulted in
`runtime.rs::record_event` BEFORE the `INSERT`, so the contract is
"honored at capture, not display" (phase10.md Cross-wave invariant
#4). If the gate ever leaks — even one row — the affected window's
title or app name lives in raw immutable storage forever (Principle
1), defeating the privacy-by-default posture. The mid-session reload
sub-case exists because ADR 0043 §Hot reload mandates that adding
or disabling a rule via the Settings UI takes effect on the *next*
sampler tick, not on next process boot.

**Pass criteria — ALL of:**

1. **Pure-`exclusion.rs` test suite is green:**

   ```powershell
   powershell -File scripts\cargo-with-cuda.ps1 test --release --lib `
     -- activity::exclusion::tests
   ```

   Per LESSONS P2, if live exec is blocked on this box, fall back to
   the throwaway-crate recipe — `exclusion.rs` has no whisper-rs /
   cpal / ort / cuda deps (just `rusqlite`, `regex`, `serde`, `tracing`).
   Expected: all 13 unit tests pass (`empty_matcher_never_excludes`,
   `glob_matches_are_case_insensitive`, `glob_question_mark_*`,
   `glob_star_handles_internal_and_trailing`,
   `app_glob_matches_app_name`, `title_regex_matches_window_title`,
   `system_password_active_drops_regardless_of_app`,
   `unknown_system_sentinel_is_not_a_match`,
   `first_matching_rule_wins_and_returns_its_id`,
   `idle_event_shaped_args_never_match`,
   `validate_rejects_*`, `rule_kind_round_trips_via_db_str`).

2. **Built-in rules are present + enabled by default after migration 015:**

   ```powershell
   powershell -File scripts\cargo-with-cuda.ps1 test --release --lib `
     -- activity::exclusion::tests::builtin_rules_load_via_load
   ```

   *(New test to author in 6.B if not already present.)* Open an
   in-memory `Database`, run all migrations through 015, call
   `ExclusionMatcher::load(&conn)`, assert `len() == 8`
   (one rule per row seeded in migration 015 — 1Password*, Bitwarden*,
   KeePass*, LastPass*, consent.exe, LogonUI.exe, the bank/login
   `title_regex`, and the `password_field_active` system rule).

3. **End-to-end integration assertion against the runtime:**

   *(New integration test to author in 6.B as
   `activity::runtime::tests::record_event_drops_matched_rows`.)*

   - Fresh in-memory DB, all migrations applied through 015.
   - Spawn `ActivityCaptureRuntime` with `StubSampler` feeding
     three events: (a) `AppSwitch{app="Notepad.exe", title="x"}`,
     (b) `AppSwitch{app="1Password 7", title="Vault"}`, (c)
     `ContextSnapshot{password_field_active=true, app="chrome.exe",
     title="ok"}`.
   - Call `record_event` for each.
   - Assert `SELECT COUNT(*) FROM activity_events WHERE app_name LIKE
     '1Password%' OR app_name = 'consent.exe' OR app_name = 'LogonUI.exe'`
     returns **0**.
   - Assert `SELECT COUNT(*) FROM activity_events WHERE snapshot_json
     LIKE '%"password_field_active":true%'` returns **0**.
   - Assert event (a) was persisted (positive control — the matcher
     isn't a sledgehammer dropping everything).

4. **Mid-session reload doesn't leak across the rule-window boundary:**

   *(New test to author in 6.B as
   `activity::runtime::tests::reload_exclusion_rules_no_leak_across_window`.)*

   - Start a fresh runtime with **only** `app_glob` rule for
     `"Foo*"` enabled.
   - Record one `AppSwitch{app="Bar", ...}` event — assert it
     persists (positive control).
   - Record one `AppSwitch{app="Foo 1", ...}` event — assert
     it does NOT persist.
   - DB-level INSERT into `activity_exclusion_rules` of a new
     `app_glob` rule for `"Bar*"`.
   - Call `runtime.reload_exclusion_rules()`.
   - Record one more `AppSwitch{app="Bar", ...}` event — assert
     it does NOT persist (new rule active).
   - DB-level UPDATE setting the `"Bar*"` rule to `enabled = 0`.
   - Call `runtime.reload_exclusion_rules()`.
   - Record one more `AppSwitch{app="Bar", ...}` event — assert
     it persists again (back to the original window).
   - Assert exactly **2** rows in `activity_events` after all four
     samples — the two `"Bar"` rows when "Bar*" wasn't matching,
     zero "Foo 1" rows ever, zero "Bar" rows during the
     enabled window.

5. **The runtime calls the matcher BEFORE the INSERT (static
   structural check):**

   ```powershell
   Select-String -Path src-tauri\src\activity\runtime.rs `
     -Pattern 'exclusion_matcher|matches\(|INSERT INTO activity_events'
   ```

   Expected: the `exclusion_matcher.matches(...)` call site appears
   *textually* before the `INSERT INTO activity_events` in
   `record_event`. Eyeball-verify. If the structure flips, the
   exclusion gate becomes a display-time filter — a violation of
   phase10.md Cross-wave invariant #4 (capture-time enforcement).

**On failure:**

- **Block the `phase-10-complete` tag.**
- If criterion 3 surfaces a leaked row: the bug is in `record_event` —
  the matcher must be called and its `Some(hit)` return path must
  `return Ok(())` (or log + return) without touching the DB.
- If criterion 4 surfaces a cross-window leak: `reload_exclusion_rules`
  is racing the sampler tick. Fix is to swap the matcher under a
  `Mutex` / `ArcSwap` rather than rebuild-in-place. The new test
  becomes the regression for that fix.
- If criterion 2 surfaces fewer than 8 built-in rules: migration 015
  was misapplied. Inspect the `INSERT` block in
  `015_activity_wave5_hardening.sql` and the `Database::open`
  migration runner.

**Last run (Wave 6.A dry-run):** _TBD — see Wave 6.A dispatch report
in STATUS.md / commit history. Wave 6.B fills this with the green
verdict + commit hash._
