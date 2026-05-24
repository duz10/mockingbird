# ADR 0046 Iter 3 — Sync-layer observation plan

**Bead:** `mb-s8s2` (Wave 0 sync-layer spike).
**Tool:** `scripts\watch-vault.ps1` (JSONL FileSystemWatcher log of every
event under `<vault>/`).
**Goal:** characterize how Obsidian Sync writes synced files to disk on
Windows 11, so Wave 3's combined-detector + debounce parameters
(`notify-debouncer-full` quiet-window, minimum absolute file age,
exclusive-open probe gating) can be tuned against real evidence instead
of guesses.

This plan is **the input to Dustin's hands-on observation work**. Bernard
landed the script + this plan; Dustin (coordinated by planning-agent in a
follow-up dispatch) actually runs the rounds. A subsequent dispatch
analyzes the logs and charters the implementation beads.

---

## Why this spike exists

ADR 0046 §6 calls out the sync-layer write pattern as the
*charter-wide blocking unknown* — the choice between
`notify-debouncer-full`'s default 500 ms quiet window vs. a more aggressive
or more conservative value depends on whether Obsidian Sync writes files:

- **Atomically** (temp + rename, partner is invisible until complete) →
  the watcher can fire on the rename event immediately.
- **Streaming** (one `.m4a` file that grows in place over several seconds)
  → the watcher MUST wait for size+mtime stability before opening it for
  ingest.
- **Hybrid** (some hidden state files appear, then the final file
  materializes via rename) → the watcher needs to know which extensions /
  name patterns to ignore.

Web research (paraphrased in ADR 0046's "Web research" section) is
inconclusive — Obsidian Sync's local-write behavior is undocumented.
Real-hardware observation is the only path to a defensible parameter
choice. That is this spike.

---

## Pre-flight checklist

Before starting any round:

1. **Desktop:** Obsidian Desktop running with the `mockingbird-vault`
   open. Confirm the vault is at `C:\Users\dboyd\mockingbird-vault\` (per
   ADR 0046 "Realized POC configuration"). Obsidian's status bar in the
   bottom right should show the cloud icon idle (no in-flight sync arrow).
2. **iPhone:** Obsidian Mobile open on the `mockingbird-vault`. Confirm
   the vault file tree shows the desktop content (i.e. sync is healthy in
   both directions; if not, fix that before running rounds).
3. **Mockingbird:** the desktop app should be **closed** for these
   rounds — we want the vault's `<vault>/.mockingbird/` zone quiet (no
   reconciliation writes competing with Obsidian Sync's writes in the
   log).
4. **A scratch terminal** ready to launch the monitoring script. The
   script writes the log to whatever path you pass via `-LogPath`;
   suggested naming below.
5. **A note app on the desktop** open to scribble timing observations
   that the script can't capture (e.g. "tapped record at 10:42:13",
   "first cloud icon spin on phone at 10:42:18").

### Launching the monitor

For each round:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\watch-vault.ps1 `
    -LogPath "spike-round-N-<short-name>.log"
```

(`-VaultPath` defaults to `%USERPROFILE%\mockingbird-vault`, which is
correct for the POC config. Override only if the vault has moved.)

Stop the script with Ctrl+C **after** the sync has visibly settled (no
new events for ~10 s). The script prints `logged N events to <path>` on
exit so you have a sanity check that something was captured.

---

## Observation rounds

Six rounds, smallest to largest, ending with a stretch round on offline
catch-up. Each round produces one log file.

For every round, after the trigger:

- **Wait until events have visibly stopped** in the script's stdout for
  ~10 s before Ctrl+C'ing the monitor. If you stop too early, you'll
  miss the second half of streaming writes or late cloud-state file
  updates.
- **Note the wall-clock time you triggered the action on the phone.**
  Compare against the first event timestamp in the log to compute
  end-to-end latency (phone tap → desktop file lands).
- **Glance at the log** after the round for surprises before moving on.
  Anything weird is worth a note for the analysis dispatch.

### Round 1 — Small text note (~100 B)

- **Goal.** Baseline write pattern for a trivially-small markdown file.
  Establishes the "minimum-cost sync envelope" — does Obsidian Sync still
  use a temp+rename dance for a 1-line file, or does it write in place?
- **Trigger steps.**
  1. On iPhone Obsidian Mobile, tap the New Note button.
  2. Type 1-2 sentences.
  3. Tap back / close the note.
- **Expected timing.** First desktop event within 5-30 s. Settle within
  ~60 s.
- **What to look for.**
  - One `Created` event vs. `Created` followed by `Renamed` (the latter
    indicates a temp-file pattern even for tiny payloads).
  - Whether `.obsidian/workspace-mobile.json` or similar Obsidian state
    files update around the same time (informative noise but not what
    we're characterizing).
- **Suggested log name.** `spike-round-1-small-text.log`

### Round 2 — Medium markdown note (~5 KB)

- **Goal.** See whether mid-sized files trigger materially different
  write patterns than tiny ones — specifically, does the temp-file
  dance kick in at a size threshold?
- **Trigger steps.**
  1. On iPhone, create a new note.
  2. Paste a ~5 KB chunk of formatted markdown (a long article, a
     dense set of bullets, a few hundred lines of headings + lists).
     Tip: keep a known-good 5 KB snippet on your phone clipboard for
     consistency across rounds.
  3. Close the note.
- **Expected timing.** Similar to Round 1.
- **What to look for.**
  - Does the event sequence differ from Round 1? Same shape, just a
    bigger file? Or does a temp-file pattern appear here that wasn't
    in Round 1?
  - Compare the final `size` field in the log to the typed content
    length (markdown bytes ≈ rendered character count for plain
    ASCII).
- **Suggested log name.** `spike-round-2-medium-text.log`

### Round 3 — Large note (~50 KB)

- **Goal.** Characterize whether large files arrive atomically or
  stream in. **This is the most important round for inbox courier
  design**, because the courier's actual payload is multi-MB `.m4a`
  audio, and 50 KB is the smallest size at which streaming-vs-atomic
  behavior reliably distinguishes itself.
- **Trigger steps.**
  1. On iPhone, create a new note.
  2. Paste a ~50 KB wall of text (e.g. the first chapter of a public-
     domain book — Project Gutenberg's "Pride and Prejudice" Ch. 1 is
     conveniently in the right ballpark).
  3. Close the note.
- **Expected timing.** First event within 5-30 s. Settle within
  ~60-90 s.
- **What to look for.**
  - **Streaming pattern:** one `Created` followed by multiple
    `Changed` events over several seconds, with `size` monotonically
    increasing in the log. If you see this, the inbox watcher MUST
    use size+mtime stability before opening files.
  - **Atomic pattern:** a `Created` (and maybe an immediate
    `Changed`) and that's it — the file appears at full size in one
    visible step. If you see this, the watcher can fire on `Created`
    much more aggressively.
  - **Hybrid pattern:** a `.tmp` / `.partial` / random-suffix file
    `Created`, several `Changed` events on the temp file, then a
    `Renamed` event that materializes the final filename. If you see
    this, the watcher should match on filename extension and ignore
    the temp.
- **Suggested log name.** `spike-round-3-large-text.log`

### Round 4 — Binary attachment

- **Goal.** See how Obsidian Sync ferries non-text content. The inbox
  courier's actual payload is binary (`.m4a` from the iOS Shortcut), so
  it matters whether binary syncs identically to text or hits a
  different code path on Obsidian's side.
- **Trigger steps.** Choose whichever is easiest:
  - **Option A (preferred):** in Obsidian Mobile, attach an image to a
    note (camera roll → image). Obsidian will copy the image into the
    vault under a default attachments path.
  - **Option B:** use the iOS Files app, navigate to "On My iPhone →
    Obsidian → mockingbird-vault", and drop a small binary (a
    ~100 KB image saved from Safari, or a short voice memo you've
    already recorded). This is the closest analog to the eventual
    iOS Shortcut courier flow.
- **Expected timing.** Similar to Round 3.
- **What to look for.**
  - Same questions as Round 3 (atomic vs streaming vs hybrid).
  - Are binary files placed under a different vault subdirectory by
    default? (Obsidian's attachment-folder setting controls this; note
    where they land.)
- **Suggested log name.** `spike-round-4-binary-attachment.log`

### Round 5 — Edit-then-delete

- **Goal.** Characterize how deletes appear in the event stream.
  Specifically: is a delete a single `Deleted` event, a two-step
  `Renamed → Deleted` pattern (some sync engines move-to-trash), or
  something else? This matters because the inbox courier's "I'm
  archiving this courier I just ingested" move MUST be
  distinguishable from "user deleted this on phone" in the watcher's
  event handling.
- **Trigger steps.**
  1. On iPhone, open an existing note (you can reuse Round 1's note).
  2. Edit it (add a sentence).
  3. Save.
  4. Delete the note.
- **Expected timing.** Edit event arrives within ~30 s; delete event
  follows after a separate sync round.
- **What to look for.**
  - Does the delete appear as a clean `Deleted` event, or as
    `Renamed` (e.g. into a `.trash/` subdir, or with a
    deletion-timestamp suffix), or both?
  - Does Obsidian Sync ever rename to `.obsidian/.trash/` or similar
    before deleting?
- **Suggested log name.** `spike-round-5-edit-then-delete.log`

### Round 6 (stretch) — Offline-then-online catch-up

- **Goal.** Characterize the event burst pattern when sync delivers
  multiple queued changes in a row. This directly informs the
  `notify-debouncer-full` quiet window: if catch-up bursts compress
  many events into <500 ms, the default window is wrong; if events
  spread out over several seconds anyway, the default is fine.
- **Trigger steps.**
  1. Put iPhone in airplane mode (Control Center → airplane icon).
  2. Make three distinct changes on the phone: e.g. create a new
     note, edit an existing note, delete a note (in that order).
  3. Wait ~15 s to ensure the changes are queued, not in-flight.
  4. Turn airplane mode off.
  5. Watch the desktop event stream and capture the burst.
- **Expected timing.** Burst starts within ~10 s of network restore;
  could complete in 1-5 s once it starts.
- **What to look for.**
  - **Burst density:** how close together (in milliseconds) do
    successive events arrive within the burst?
  - **Order preservation:** do events for the three changes arrive
    in the same order you made them on the phone, or does Obsidian
    Sync reorder?
  - **Are there interleaved Obsidian state file updates that mix in
    with the user-action events?**
- **Suggested log name.** `spike-round-6-offline-catchup.log`

---

## Analysis section (for the follow-up dispatch)

When all six logs are in hand, the analysis dispatch should answer the
following questions. Each answer feeds directly into one or more Wave 3
implementation parameters.

1. **Atomic vs streaming vs hybrid write?**
   - Inspect Rounds 3 and 4 specifically. Look for `Created` followed
     by `Changed` events with monotonically-growing `size`.
   - **Feeds:** decision on whether the inbox watcher needs the
     `notify-debouncer-full` quiet-window debounce *plus* a
     size+mtime stability check before opening (combined detector,
     ADR §6), or just the debounce.

2. **End-to-end latency (phone trigger → file complete on desktop)?**
   - Inspect each round: time from "manual trigger note" to last
     event for that file in the log.
   - **Feeds:** the user-facing "you should see your transcription
     within ~X seconds" UI copy in the Mobile Sync setup flow.

3. **Intermediate states the watcher must skip?**
   - Across all rounds, identify any file extensions or name patterns
     that appear in `Created` events but are never the final settled
     file. Examples to watch for: `.tmp`, `.partial`, `.icloud`,
     random hex suffixes, `~$lockfile` style.
   - **Feeds:** the watcher's filename-filter regex.

4. **Event burst density on offline catch-up?**
   - Round 6: maximum events per 100 ms during the burst peak.
   - **Feeds:** the `notify-debouncer-full` quiet-window value —
     specifically, whether 500 ms (default) absorbs the burst into
     one logical event per file or splits it. Bursts faster than
     500 ms inter-event gap need the larger window; bursts naturally
     spaced wider are fine with the default.

5. **Delete signaling — distinct from move-to-archive?**
   - Round 5: shape of the delete event sequence.
   - **Feeds:** confidence that the watcher can distinguish
     "user deleted this courier from phone" (do nothing — they
     changed their mind) from "courier moved itself to
     `inbox/_archive/`" (the watcher's own action — also do nothing,
     but for a different reason). The watcher uses its own
     `inbox/_archive/` path as the discriminator either way; the
     question is whether the `Deleted` event alone is unambiguous or
     needs cross-checking against the path.

6. **Obsidian's own state file noise — magnitude?**
   - Across all rounds, count events under `.obsidian/` vs events
     for the actual user-content files.
   - **Feeds:** decision on whether the watcher's default exclusion
     list needs `.obsidian/` filtered (current `watch-vault.ps1`
     keeps it ON deliberately for this spike; the production
     watcher will almost certainly exclude it).

---

## What this plan does NOT cover

These are deliberately scoped out of Wave 0 — flagged here so the
analysis dispatch doesn't reach for them or charter them prematurely:

- **The 5 MB Sync Standard cap silent-skip behavior.** Already
  characterized in ADR §9 and addressed via the reconciliation pass +
  user-facing copy in the Mobile Sync tab. The descoped sidecar
  mechanism is tracked as `mb-0uqb`.
- **iCloud Drive sync behavior.** ADR 0046 explicitly does NOT use
  iCloud as the vault transport (§8: "vault location flexibility...
  the silent zero-tap variant requires the vault to live inside
  iCloud Drive, which we explicitly do NOT do"). No iCloud rounds.
- **Conflict resolution.** Obsidian Sync's conflict-file naming
  convention is documented elsewhere; the desktop side treats the
  vault as transport (§ "Vault as transport, not source of truth"),
  so conflict files in `<vault>/history/` are regenerable and don't
  need spike-time characterization.
- **Real iOS Shortcut courier flow.** That's Round 1+ of Iter 3's
  actual implementation smoke test, not this spike. The spike uses
  Obsidian Mobile as the trigger because it's the simplest path to
  "a file appears on the desktop via Obsidian Sync" without needing
  the Shortcut chain to be installed first.

---

## Handoff

When the rounds are complete:

1. Drop the 6 log files into a folder Dustin or planning-agent can
   reference (suggest `docs/spikes/iter3-logs/` — gitignored or
   committed depending on size; 6 small JSONL files should be
   comfortable to commit).
2. Comment on `mb-s8s2` with the log file paths + any surprises
   noted during the rounds.
3. Schedule the analysis dispatch — its inputs are the logs + this
   plan's Analysis section, its outputs are the answers to questions
   1-6 above and the resulting Wave 3 parameter choices (charter as
   `mb-9lgi` follow-up work).
