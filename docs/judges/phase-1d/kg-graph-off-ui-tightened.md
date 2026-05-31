# Judge: kg-graph-off-ui-tightened (Phase 1D Wave 1D.6)

**Target:**
- `ui/tests/kg-graph-off-invariant.spec.ts` (the Playwright spec
  authored Wave 1C.5 and progressively tightened across Waves
  1D.2, 1D.4, 1D.5)
- `ui/src/lib/tauri.ts::invoke` (the `window.__KG_IPC_SPY__` hook
  it relies on)
- `ui/src/pages/KnowledgeGraph*.tsx` (dashboard + capture surface
  + retrieval surface)
- `ui/src/pages/Settings*.tsx` (Settings → Knowledge Graph tab)
- `ui/src/pages/Dictations*.tsx` (Dictations page — must remain
  KG-free after Wave 1D.4's chip/modal relocation)

**Question:** With `KgGraphEnabled = false`, does the running UI
fire **zero** `kg_*` IPCs beyond the read-only
`kg_settings_get_all`, across every page the user might land on
during normal app usage — including the new KG screen, the
expanded Settings panel, and the post-1D.4-relocation Dictations
page?

The answer must be **YES**.

**Rationale:** This is the UI-side complement to the Rust-side
[`kg-graph-off-untouched`](../phase-0-kg/README.md) probe
(ADR 0050 §D8 gate 2). Together they enforce ADR 0049 §6's
default-off binding: a user who never opts into the KG subsystem
must observe zero KG-related work, on both the storage side
(no `kg_*` table rows) and the IPC side (no `kg_*` invocations
beyond the single read that determines whether the toggle is on).

**No new code is shipped at Wave 1D.6 for this judge.** Three
prior waves already tightened the Playwright spec:

- **Wave 1D.2** (`mb-j00j`, KG screen scaffold): extended the
  walk to step 3b (sidebar navigation to KG screen) + step 3c
  (5-band dashboard mount), asserting the dashboard's two
  `kg_*` IPCs (`kg_dashboard_snapshot`, `kg_queue_status`) do
  NOT fire when toggle is off (the screen mounts an empty-state
  banner instead).

- **Wave 1D.4** (`mb-6hm2` / `mb-f4gn`, chip/modal relocation):
  extended step 4b to walk the Dictations page after the
  KG-specific chip/modal UI was moved to the KG screen.
  Asserts the Dictations page is now KG-free (no
  `kg_entries_summary`, no `kg_concept_*` calls) regardless of
  toggle state. This is the regression net for the relocation
  itself.

- **Wave 1D.5** (`mb-navi`, Settings expansion): added the
  vocabularies-allowlist assertion — the expanded Settings KG
  tab still calls only `kg_settings_get_all` when off, even
  though it now exposes vault path + Obsidian launch + future
  vocab editor stubs.

Wave 1D.6 (this judge) **documents the consolidated invariant
set** as fully satisfied; the spec itself is the executable
contract.

**Pass criteria — ALL of:**

1. **The Playwright spec passes on `chromium`.**

   ```powershell
   cd ui ; npx playwright test kg-graph-off-invariant
   ```

   Expected: `1 passed`, suite header line
   `KG graph-off-UI invariant -- 1D.4 tightened (mb-f4gn / mb-6hm2)`
   (the title was last updated at Wave 1D.4; Wave 1D.5's
   tightening landed inside the same spec's body without a
   title bump).

2. **Toggle-off allowlist is exactly `{ "kg_settings_get_all" }`.**

   ```powershell
   Select-String -Path ui\tests\kg-graph-off-invariant.spec.ts `
     -Pattern 'kg_settings_get_all'
   ```

   Expected: at least one assertion line of the shape
   `expect(...).toEqual(new Set(["kg_settings_get_all"]))` or a
   sorted-array equivalent. Any other `kg_*` command name
   appearing inside the toggle-off allowlist (not the positive-
   control on-toggle block) is a violation.

3. **The IPC spy hook is still installed unconditionally in
   `tauri.ts::invoke`.** Zero cost when no test has opted in
   (one `if (window.__KG_IPC_SPY__)` per call):

   ```powershell
   Select-String -Path ui\src\lib\tauri.ts -Pattern '__KG_IPC_SPY__'
   ```

   Expected: at least two matches — the type declaration + the
   call-site `if` guard. A regression that removes the hook
   blinds the Playwright spec.

4. **Step coverage hasn't regressed.** The spec walks ALL of:
   - Settings → Knowledge Graph tab
   - Sidebar navigation to the KG screen (1D.2 addition)
   - KG screen dashboard mount (1D.2 addition)
   - Dictations page mount + a dictation-row click
     (1D.4 addition — must be KG-free)

   ```powershell
   Select-String -Path ui\tests\kg-graph-off-invariant.spec.ts `
     -Pattern 'knowledge-graph|dictations|settings'
   ```

   Expected: matches for all three route fragments. Missing
   `knowledge-graph` walk = 1D.2 regression. Missing
   `dictations` walk = 1D.4 regression.

5. **Positive-control block still flips toggle on and observes
   the expected `kg_*` IPCs fire.** This catches the
   vacuous-pass mode where the spy was somehow disabled and the
   off-block trivially observed an empty set:

   ```powershell
   Select-String -Path ui\tests\kg-graph-off-invariant.spec.ts `
     -Pattern 'kg_list_failed_filings|kg_queue_status|kg_dashboard_snapshot'
   ```

   Expected: at least one match (the positive-control block's
   expectation that on-toggle invocations DO fire from the
   Settings KG tab mount).

**On failure:**

- **Block the Wave 1D.6 / Phase 1D seal.**
- Criterion 1 failure: re-run with `--debug` and inspect which
  IPC slipped into the off-mode spy log. The most common
  regression mode is a new IPC added without checking it against
  the spec's allowlist (e.g. a new dashboard probe added to KG
  screen mount that doesn't gate on `kgGraphEnabled`).
- Criterion 3 removal: any "cleanup" PR that strips the
  `__KG_IPC_SPY__` hook from `tauri.ts` blinds the entire
  judge. Restore it; the hook is opt-in (cost-free when no
  test installs the spy callback).
- Criterion 4 missing walk: the spec's step coverage shrank.
  Re-add the missing route walk; do NOT delete walks just
  because the page is "obviously KG-free now" — that
  obviousness was the assertion.

**Last run:** _Wave 1D.6 — **GREEN**. `npx playwright test
kg-graph-off-invariant` reports `1 passed (12.6s)` on the
single spec
`KG graph-off-UI invariant -- 1D.4 tightened (mb-f4gn /
mb-6hm2)`. Toggle-off allowlist asserted as
`{ "kg_settings_get_all" }`; spy hook intact in `tauri.ts`;
positive-control block flips on and observes
`kg_list_failed_filings` + `kg_queue_status` from the
SettingsKgFailedFilings mount. Step coverage: Settings KG tab
+ KG screen dashboard + Dictations row click — all three walks
present._
