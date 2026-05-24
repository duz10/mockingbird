# sealed-phases-untouched (Iter 3) — Verdict

**Judge:** [`sealed-phases-untouched-iter3.md`](./sealed-phases-untouched-iter3.md)
**Diff range:** `2b5583d..HEAD` (HEAD = `c1084a8`).
**ADR:** [0046 — Mobile extension via synced Obsidian vault](../../adr/0046-mobile-extension-via-vault.md)
**Bead:** `mb-ksau`
**Graded by:** code-puppy-d7f36e (Bernard), 2026-05-27 (self-grade,
Mode A — no Mockingbird judge runner is wired for ADR-chartered
laterals yet; identical posture to Iter 1's `mb-thmd` and Phase 10
Wave 6.B).
**Verdict:** 🟢 **PASS** (confidence 99%)

---

## Mechanical layer

| # | Check | Result |
|---|---|---|
| M1 | All forbidden subsystems show zero diff (`dictation`/`hotkey`/`meetings`/`activity`/`injection`/`window_context`/`secrets`/`stt`/`cleanup`/`recording_window.rs`/`db/migrations`) | ✅ **empty** |
| M2 | Iter 2 vault subsystem byte-identical (`src-tauri/src/vault`) | ✅ **empty** (stricter than the kickoff's "trivial mod-line addition" allowance — nothing was needed) |
| M3 | Dictation modules byte-identical (`src-tauri/src/dictation`, covers `dictation.rs` + `dictation/{ingest,ingest_channel,events,runtime}.rs`) | ✅ **empty** |
| M4 | No `UPDATE` against `stage='raw'` introductions | ✅ empty |
| M5 | Link surface clean | ⏳ deferred — kickoff explicitly skipped cargo/UI gate for this docs+bd dispatch; the prior Wave 3.3 SEAL commit (`eea2484`) is the standing cargo proof for the diff range under audit |

Raw command outputs (re-runnable verbatim):

```
$ git diff --stat 2b5583d..HEAD -- src-tauri/src/dictation src-tauri/src/hotkey src-tauri/src/meetings src-tauri/src/activity src-tauri/src/injection src-tauri/src/window_context src-tauri/src/secrets src-tauri/src/stt src-tauri/src/cleanup src-tauri/src/recording_window.rs src-tauri/src/db/migrations
(empty)

$ git diff --stat 2b5583d..HEAD -- src-tauri/src/vault
(empty)

$ git diff --stat 2b5583d..HEAD -- src-tauri/src/dictation
(empty)
```

This is the cleanest mechanical layer of any ADR 0046 iteration so far.
Iter 1 had to touch `dictation.rs` (§3 carve-out); Iter 2 had to ship a
whole new `vault/` subsystem; Iter 3's "new top-level subsystem"
pattern means **zero modifications** to any sealed surface — every
existing module is byte-identical relative to the Iter 2 SEAL anchor.

---

## File-by-file audit (25 files in `git diff --stat 2b5583d..HEAD`)

### Authorized — new top-level `src-tauri/src/inbox/` subsystem

| File | Lines | Authorization |
|---|---|---|
| `src-tauri/src/inbox/mod.rs`     | +37   | Iter 3 module declarations + re-exports |
| `src-tauri/src/inbox/watcher.rs` | +896  | Wave 3.1 / `mb-9lgi` — `notify-debouncer-full` wrapper + size-stability state machine grounded in Wave 0 spike Finding 3 (3-4 duplicate FS events within ~12 ms per logical change) |
| `src-tauri/src/inbox/courier.rs` | +886  | Wave 3.2 / `mb-txmy` — decode → `HeadlessIngestRequest` enqueue → atomic archive + fail-route |
| `src-tauri/src/inbox/runtime.rs` | +464  | Wave 3.3 / `mb-3ivf` — `InboxRuntime` settings-gated lifecycle, parallel peer to `VaultRuntime` |

All four are pure adds; nothing under this path existed before commit
`a3c5900`. They form the cohesive new subsystem the kickoff explicitly
authorized.

### Authorized — surgical existing-file edits

| File | Lines | Authorization |
|---|---|---|
| `src-tauri/src/lib.rs` | +24/-2 | Four named edits: (a) `pub mod inbox;` declaration; (b) hoist `runtime.headless_ingest_sender()` out of the prior one-line `app.manage` so it can be cloned for the inbox path; (c) `Arc::new(InboxRuntime::new(headless_ingest_tx))` construction parallel to `VaultRuntime`; (d) `app.manage(Arc<InboxRuntime>)` registration + initial `refresh_config()` so a stock-disabled install does no watcher work. The hoist-and-clone of `headless_ingest_tx` is the only line that changes a prior-iteration semantic — and only to add a `.clone()`, leaving the original `app.manage` call's payload intact. |
| `src-tauri/src/commands/vault.rs` | +7 | `vault_settings_set` IPC handler gains `inbox: State<'_, Arc<InboxRuntime>>` and calls `inbox.refresh_config(&db_arc)?` immediately after `vault.refresh_config(...)`. This is the kickoff-anticipated "vault settings refresh that also triggers InboxRuntime" — both runtimes are gated by the same `MobileSyncEnabled` + `VaultPath` keys, so writes to either key must reconcile both. No new IPC, no new key, no logic change to vault's reconciliation. |

### Authorized — deps / Cargo bookkeeping

| File | Lines | Authorization |
|---|---|---|
| `Cargo.toml`         | +10 | `notify-debouncer-full = "0.3"` workspace dep. The kickoff loosely said "`notify` crate addition"; this is the debouncer-wrapper variant of the same crate family, mandated by ADR §6 + Wave 0 Finding 3. |
| `src-tauri/Cargo.toml` | +8  | Crate-level dep entry pointing at the workspace dep. |
| `Cargo.lock`         | +182 | Mechanical resolution cascade. |

### Authorized — docs / bookkeeping / spike artifacts

| File | Authorization |
|---|---|
| `STATUS.md`, `docs/LESSONS.md`, `docs/PRODUCT-STATE.md` | Per-iteration bookkeeping |
| `docs/historical/2026-05-23-mobile-extension-original-kickoff.md` | Phase 0 cleanup move (commit `d010bdf`) — relocates the original 2026-05-23 feature kickoff under `docs/historical/` with a preamble noting full supersession by ADR 0046 + STATUS |
| `docs/mobile/ios-shortcut.md` | User-facing setup doc amended for the Action 3 `Open App: Obsidian` mitigation (Wave 0 Finding 5) |
| `docs/spikes/iter3-sync-layer-{observation-plan,findings}.md` | Wave 0 spike artifacts |
| `docs/spikes/iter3-logs/round-{1,2,3,4,4b}*.log` | Wave 0 spike JSONL log fixtures (committed evidence backing the findings doc) |
| `scripts/watch-vault.ps1` | Wave 0 spike helper script (under `scripts/`, not `src-tauri/src/` — outside any sealed-subsystem boundary) |
| `.gitignore` | 3-line whitelist for the committed spike logs (`!docs/spikes/**/*.log`) |
| `.beads/issues.jsonl`, `.beads/interactions.jsonl` | Bead-DB updates closing `mb-9lgi`, `mb-txmy`, `mb-3ivf`, `mb-s8s2`, etc. |

### Unauthorized

None observed.

---

## Reasoning summary

The Iteration 3 implementation (`2b5583d..c1084a8`) is the **cleanest
boundary-respect of any ADR 0046 iteration so far**. The mechanical
layer is uniformly empty across every forbidden surface — including
the Iter 2 vault subsystem, which the kickoff allowed to receive
trivial mod-line additions but in practice received zero diff.

The new top-level `src-tauri/src/inbox/` subsystem is the structural
realization of the kickoff's stated discovery: inbound and outbound
vault flows are conceptually independent (only the path is shared), so
the `InboxRuntime` lives as a parallel peer to `VaultRuntime`, both
driven by the same `MobileSyncEnabled` + `VaultPath` settings gate.
That symmetry is reflected in `commands/vault.rs`'s seven-line edit:
the existing `vault_settings_set` IPC simply gains a second
`refresh_config()` call after the first, with no new IPC or new
settings key added. The seven lines are mechanically the smallest
possible diff that wires the runtime into the settings-write reconcile
flow.

`src-tauri/src/lib.rs`'s 24-line edit is bounded by exactly the four
operations the kickoff named — module declaration, sender hoist for
cloning, runtime construction, and `app.manage()` registration with
initial `refresh_config()`. The only semantic change to a prior
iteration's line is the `.clone()` introduced when hoisting
`headless_ingest_tx`; the original Iter 1 `app.manage(headless_ingest_tx)`
call is preserved with the cloned value, so Iter 1's IPC consumers
continue to find the sender in the Tauri state map exactly as before.

The Iter 3 courier never modifies the `HeadlessIngestRequest` channel
or its handler — it constructs requests on the inbox side and pushes
them through the existing Iter 1 channel. This is the strongest form
of consumer-not-modifier respect: every dictation module under
`src-tauri/src/dictation/` is byte-identical to its Iter 1 / Iter 2
state. Migrations 001-018 are all byte-identical (no new migration in
Iter 3; the courier writes `source = 'mobile-inbox'` into the
existing migration-018 column).

The `notify-debouncer-full` dep is a slightly more specific crate than
the kickoff's loose phrasing of "`notify` crate addition" — but it is
the ADR §6 mandated variant per Wave 0 Finding 3 (Windows
`ReadDirectoryChangesW` emits 3-4 duplicate events per logical change
within ~12 ms; the debouncer wrapper coalesces these inside its own
thread on top of which the `inbox::watcher` size-stability state
machine runs). Treating this as authorized matches Iter 1's precedent
of accepting `symphonia` and `crossbeam-channel` under the looser ADR
§3.2 / §6 description.

---

## Final verdict

🟢 **PASS** — ADR 0046 Iteration 3 stayed inside the authorization
boundary. The Iter 1 dictation seal, the Iter 2 vault seal, the Phase
3 / MC / Phase 10 sealed subsystems, and migrations 001-018 are all
byte-identical. The implementation is structurally cleaner than the
kickoff dared to assume — needing exactly **two** existing files to
receive any edits (`lib.rs` for wiring, `commands/vault.rs` for the
settings-reconcile mirror) and zero edits to the Iter 2 vault
subsystem.

**Confidence:** 99%. The 1% gap reflects only the slight crate-naming
variance (`notify-debouncer-full` vs. the kickoff's `notify`); this is
identical in spirit to the ADR §6 mandate and is the same shape of
acceptance Iter 1 applied to its dep additions. If a stricter reading
rejects this, the remedial action is zero code change — only a one-
line ADR §6 amendment to name the exact crate. No Iter 3 work would
be reverted.

**Next action** (per kickoff Deliverable 2): close `mb-1yxp` with a
deferral note per Dustin's 2026-05-27 explicit decision. The technical
pipeline is already proven end-to-end by session 116
(initial-scan picked up the leftover `New Recording 38.m4a` from spike
round 4b and ran it cleanly through STT 1.08s + cleanup 9.87s into
`sessions` row `source='mobile-inbox'` with atomic archive). The
remaining iOS Shortcut install + hands-on latency measurement is
user-side setup Dustin will handle on his own time.
