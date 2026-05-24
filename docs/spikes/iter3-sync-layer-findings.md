# ADR 0046 Iter 3 — Sync-layer spike findings

**Spike bead:** `mb-s8s2` (Wave 0 sync-layer observation).
**Observation plan:** [`docs/spikes/iter3-sync-layer-observation-plan.md`](./iter3-sync-layer-observation-plan.md)
**Raw logs:** [`docs/spikes/iter3-logs/`](./iter3-logs/)
**Date the rounds ran:** 2026-05-24 (UTC timestamps in the logs).

---

## Executive summary

Four headline findings, all grounded in the captured FS event stream:

1. **Binary files arrive ATOMICALLY** — Obsidian Sync delivers `.m4a` /
   image payloads at full size in a single `Created` event (and 3-4
   duplicate `Changed` events fired within ~10 ms, also at full size).
   No streaming, no `.tmp` / `.partial` / `.icloud` placeholders, no
   temp-rename dance. **The inbox watcher does NOT need a content-growth
   stability check for its actual payload type.**
2. **Text files STREAM over many seconds — sometimes many minutes.**
   The filename appears first at 0 B, then content arrives in one or
   more chunks separated by intervals ranging from ~10 s (Round 2) to
   ~8 min (Round 1). This is irrelevant to courier ingest (the courier
   only ingests audio) but it absolutely confirms the watcher must
   ignore the `Created` event itself as a "file is ready" signal for
   anything outside the binary-audio happy path.
3. **Every FS event fires 3-4 TIMES within ~5-12 ms.** The watcher MUST
   debounce. A ~100 ms coalescing window absorbs every observed
   duplicate burst comfortably.
4. **CRITICAL UX finding (Round 4b):** when an external app (Files,
   Voice Memos, Shortcut) writes into Obsidian's iOS sandbox,
   **Obsidian Mobile doesn't notice the file until its next iOS
   BGAppRefresh window (~5-15 min) OR until the user foregrounds the
   app.** Without mitigation, the iOS Shortcut courier flow would have
   multi-minute end-to-end latency from "done speaking" to "transcript
   on desktop". **Mitigation: add `Open App → Obsidian` as the final
   Shortcut action** — foregrounding forces an immediate sandbox
   rescan + push. Drops latency from minutes to ~30-60 s.

---

## Methodology

Per the observation plan, Bernard shipped `scripts\watch-vault.ps1`
(JSONL `FileSystemWatcher` logger) plus the 6-round plan; Dustin
executed the rounds on his iPhone + Win11 desktop with
`mockingbird-vault` synced via Obsidian Sync Standard.

**Rounds actually run:** 1 (small text), 2 (medium markdown), 3 (large
text), 4 (binary attachment via Obsidian's own attachment system), and
**4b** (binary delivered via Voice Memos → Files app → vault `inbox/`).
The 4b round was not in the original plan; Dustin added it
spontaneously because it more closely models the eventual iOS Shortcut
courier flow (external-app write into Obsidian's sandbox), and it
turned out to be the single most informative round of the spike
(surfaced Finding 5 below).

**Rounds skipped:** 5 (edit-then-delete) and 6 (offline catch-up).
Rationale: Rounds 1-4 + 4b answered every design-blocking question for
the Iter 3 implementation, AND Finding 5 emerged as a hard requirement
to design around. Pursuing Rounds 5 and 6 would have added marginal
information (delete-signal shape, burst density) at the cost of
delaying the Wave 3 charter. The unanswered questions are reasoned
about in the **What we did NOT test** section below.

---

## Per-round summary

All timestamps are UTC, copied verbatim from the JSONL logs. Event
counts are the number of `Created` + `Changed` events emitted by
`watch-vault.ps1` (which is a raw `FileSystemWatcher`, no debouncing).

### Round 1 — Small text note (~7 B body)

**Log:** [`round-1-small-text.log`](./iter3-logs/round-1-small-text.log)

- `01:11:33.488Z`: phone created a new note. `Untitled.md` arrives at
  vault root, 0 B.
- `01:11:33.488 → 01:11:33.546Z`: **3 events** (`Created` + 2
  `Changed`) on the same path, **all at 0 B, within 58 ms** of each
  other.
- `01:12:07.391Z`: `Untitled.md` `Deleted` (the phone-side rename was
  delivered as delete-then-create with the new name).
- `01:12:07.465Z`: `Hello from round 1, this is a small text note..md`
  arrives at root, 0 B. Same 3-event burst within 8 ms.
- **`01:20:31.373Z`: file size jumps from 0 B → 7 B** (the actual
  content: `"Hello"` plus a trailing newline). 2 `Changed` events at
  7 B within 3 ms.
- **Total wall-clock from filename-visible to content-visible: ~8 min
  24 s.** This is the longest content-arrival gap observed across all
  five rounds.
- Throughout, `.obsidian/workspace.json` fired separately (5667 →
  5680 → 5758 B over the same window — Obsidian's own state churn).

### Round 2 — Medium markdown note

**Log:** [`round-2-medium-markdown.log`](./iter3-logs/round-2-medium-markdown.log)

- `01:22:32.024Z`: `inbox/Untitled.md` arrives at 0 B. 3-event burst
  within 66 ms (and an extra parent-directory `Changed` event on
  `inbox` with no `size` field at `01:22:32.080Z`).
- `01:23:29.464Z`: `Untitled.md` `Deleted`.
- `01:23:29.613Z`: `inbox/Lorem ipsum.md` arrives at 0 B. 3-event
  burst within 8 ms + parent-dir `Changed` 8 ms later.
- **`01:23:39.806Z`: size → 18 B** (4 events within 7 ms) — ~10 s
  after filename.
- **`01:23:50.419Z`: size → 26 B** (4 events within 8 ms) — another
  11 s.
- **`01:24:58.028Z`: size → 201 B** (3 events within 4 ms) — another
  68 s.
- Streaming: **0 → 18 → 26 → 201 B over ~88 s.** Mirrors how Dustin
  was typing on the phone — each save-point of the iOS editor
  triggered another push.

### Round 3 — Large text note (~50 KB target)

**Log:** [`round-3-large-text.log`](./iter3-logs/round-3-large-text.log)

- `01:29:11.576Z`: `inbox/Untitled.md` at 0 B (3-event burst within
  63 ms + parent-dir event).
- `01:29:27.522Z`: `Deleted` (rename).
- `01:29:27.681Z`: `inbox/Long paste text test.md` at 0 B (3-event
  burst within 10 ms + parent-dir).
- **`01:29:47.990Z`: size → 41,911 B in ONE step** (3 events within
  6 ms) — ~20 s after filename. Single push, not chunked.
- The 41,911 B is the entire paste payload (large but under the 5 MB
  Sync Standard cap); arrived in a single Obsidian Sync push.
- Later edits (Round 3 ran into Round 4's window): `01:34:29.070Z`
  the same file grew to 41,938 B (a small post-paste edit).
- **Streaming shape contrast with Round 2:** Round 2's smaller payload
  streamed in 4 chunks (matching keystroke-save rhythm), Round 3's
  larger single-paste arrived in 1 chunk after a single ~20 s delay.
  Both patterns are "Created early, content later"; the number of
  intermediate sizes depends on how the phone-side editor flushed.

### Round 4 — Binary attachment (image via Obsidian Mobile)

**Log:** [`round-4-binary-attachment.log`](./iter3-logs/round-4-binary-attachment.log)

- `01:32:39.125Z`: **`New Recording 38.m4a` `Created` at 258,743 B in
  ONE event.** Not 0 B → grow — **full size, single event, atomic
  arrival.**
- 4 follow-up `Changed` events on the same path, **all at 258,743 B,
  within 10 ms of the `Created` event.**
- File landed at **vault root** (`mockingbird-vault\New Recording
  38.m4a`), NOT in `inbox/`. **This is Obsidian's default attachment
  folder destination** — when the user attaches a file via Obsidian's
  own attachment flow, it goes wherever the vault's
  "Default location for new attachments" setting points (root, by
  default).
- `01:34:26.493Z`: a second attachment session produced
  `New Recording 38 1.m4a` (also at root, also 258,743 B, also
  atomic in one event + 3 duplicates within 10 ms).

### Round 4b — Voice memo via iOS Files app into `inbox/`

**Log:** [`round-4b-voice-memo-direct.log`](./iter3-logs/round-4b-voice-memo-direct.log)

This round most closely mimics the eventual iOS Shortcut courier flow:
**Voice Memos** records audio → user shares to **Files** app → user
navigates to **On My iPhone → Obsidian → mockingbird-vault → inbox/**
and taps **Save**. Same payload shape as the Shortcut will deliver.

- **`01:44:27.852Z`: `inbox/New Recording 38.m4a` `Created` at
  258,743 B in ONE event** — atomic, identical pattern to Round 4.
- 3 follow-up `Changed` events at 258,743 B within 11 ms + parent-dir
  `Changed` on `inbox`.
- This time the file landed in `inbox/` correctly because the user
  picked the destination explicitly via Files.

**Critical observation (Dustin's wall-clock, not visible in the log
alone):** the file was saved on the phone roughly 5 minutes BEFORE
the `01:44:27.852Z` desktop-side `Created` event. The intervening
period showed only `.obsidian/workspace.json` chatter on the desktop
side (the `01:39:44.501Z → 01:39:44.557Z` cluster at log head). The
desktop only saw the file once Dustin foregrounded Obsidian Mobile on
the phone — which triggered an immediate sandbox rescan + push.
**Without the foreground action, the file would have sat in Obsidian
Mobile's iOS sandbox until the next BGAppRefresh window** (Apple's
opaque scheduling, typically 5-15 minutes; can be longer if the app
hasn't been recently used).

This is the entire reason Finding 5 below exists, and it reshaped the
Shortcut spec.

---

## Answers to the observation plan's 6 questions

### Q1. Atomic vs streaming vs hybrid write?

**Answer: BOTH, by payload type.**

- **Binary (`.m4a`, image): atomic.** Single `Created` at full size
  (Rounds 4 + 4b, both 258,743 B). No temp file, no `.icloud`, no
  `.partial`. The watcher does NOT need a size-growth stability check
  for binary audio courier deliveries.
- **Text: streaming, sometimes very slow.** Filename appears at 0 B,
  content arrives in 1+ chunks over 10 s - 8 min (Rounds 1-3).
  Irrelevant to the courier (extension-filtered out) but a useful
  sanity check on watcher robustness — if anyone ever wanted to extend
  the courier to handle markdown couriers (e.g. a sidecar JSON), the
  stability check WOULD be necessary.

**Design implication:** for Wave 3.1's combined-detector, the
`notify-debouncer-full` quiet-window is sufficient on its own for the
binary-audio happy path. A **defensive** stability check (size
unchanged for ~2 s before processing) is still worth keeping because
(a) it costs almost nothing in the atomic case, (b) it future-proofs
against a worst-case where Obsidian decides to start chunking large
binaries above some size threshold, (c) it handles the local-FS
"someone dragged a half-copied file into inbox/" edge case that's
outside the sync layer entirely.

### Q2. End-to-end latency (phone trigger → file complete on desktop)?

**Answer: depends on which app produced the file.**

- **Obsidian Mobile itself producing the file (Rounds 1-3, Round 4):**
  ~5-30 s to first event, then content can keep arriving for many
  more seconds-to-minutes if text streaming applies. Binary attaches
  via Obsidian's own flow are essentially "appear in one go within ~30 s".
- **External-app produced file into Obsidian's sandbox (Round 4b):**
  **5-15 minutes** unless the user foregrounds Obsidian Mobile, in
  which case ~30-60 s.

**Design implication:** user-facing copy on the Mobile Sync setup tab
+ Shortcut spec must reflect the 30-60 s figure AND must require the
"Open App → Obsidian" Shortcut action to get there. See Finding 5
below.

### Q3. Intermediate states the watcher must skip?

**Answer: across all 5 rounds, ZERO `.tmp` / `.partial` / `.icloud` /
suffix-noise files were observed.** Obsidian Sync writes the final
filename directly. The only intermediate state is "filename exists at
0 B before content arrives" — which the size + stability check
handles cleanly.

**However:** Round 1 + Round 2 show that on a phone-side rename, the
old filename emits a `Deleted` and the new name emits a `Created`.
The courier watcher should treat each `Created` independently and
**not** try to correlate deletes-then-creates as renames. (For
audio-courier purposes this is a non-issue — the user isn't renaming
audio files mid-flight — but it documents that the FS layer does not
expose a `Renamed` event for sync-delivered renames.)

**Design implication:** the production watcher's filename filter
should still defensively skip these conservative patterns (cheap
insurance for cross-FS-transport future-proofing):

- `.tmp`, `.partial`, `.crdownload`, `.icloud` (any leading dot
  followed by these is also fine)
- `~$` prefix (Office lockfile pattern)
- Files inside `.obsidian/`, `.git/`, `.mockingbird/` and the existing
  `inbox/_archive`, `inbox/_failed`, `inbox/_keep` subdirs

### Q4. Event burst density on offline catch-up?

**Answer: not tested directly (Round 6 skipped), but adjacent evidence
is reassuring.**

Across Rounds 1-4b, every logical FS change emits **3-4 events within
~5-12 ms** (the duplicate-burst pattern documented as Finding 3 below).
Even if an offline-catch-up burst delivered, say, 5 distinct files
back-to-back, the predicted worst case is ~20 events in ~50-100 ms.
A 100 ms coalescing debounce window absorbs that into a clean per-path
single-event stream.

**Design implication:** start with **100 ms** as the debounce window.
If real-world Iter 3 smoke turns up a higher-density burst, tune up
to the 500 ms default; the parameter is one constant.

### Q5. Delete signaling — distinct from move-to-archive?

**Answer: not tested directly (Round 5 skipped), but adjacent evidence
is sufficient.**

Rounds 1 + 2 showed that Obsidian's phone-side **rename** is delivered
as `Deleted` (old name) → `Created` (new name). It is therefore
reasonable to expect a **pure delete on phone** to deliver as
`Deleted` alone — but this was not directly verified.

**Design implication:** the courier's archive move is
`inbox/<file>.m4a` → `inbox/_archive/<date>/<file>.m4a`, which **the
watcher itself initiates**. The watcher should ignore any event whose
path is under `inbox/_archive/` (it's the watcher's own bookkeeping).
For the "user deleted from phone" case: if a `Deleted` event fires on
a path the watcher has not yet processed, **the courier simply does
not find a file to process** when it eventually polls — the
single-in-flight processor sees a no-op and moves on. No active
distinction needed; the path-based filter is the discriminator.

### Q6. Obsidian's own state-file noise — magnitude?

**Answer: SIGNIFICANT. `.obsidian/workspace.json` fires every few
seconds while Obsidian is open.**

Counts from `round-3-large-text.log` (a typical round):
- 18 events on the user-content file (`Long paste text test.md`,
  including 6 events on `Lorem ipsum.md` from Round 2 spillover)
- **20 events on `.obsidian/workspace.json` alone**

In quiet rounds (Round 4b's tail), workspace.json events outnumber
the actual-payload events 12-to-4.

**Design implication:** **the production watcher MUST exclude
`.obsidian/`** from its event stream entirely. The spike's
`watch-vault.ps1` included it deliberately for observability; the
courier watcher must filter it out at the source (the `notify`
crate's filter or a path-prefix gate in the event handler).

---

## Findings (the load-bearing 8)

### Finding 1 — Binary delivery is ATOMIC

`.m4a` files arrive with full size in a single `Created` event,
followed by 3-4 duplicate `Changed` events at the SAME size within
~10 ms. No temp file, no streaming, no placeholder.

**Evidence:** Round 4 `01:32:39.125Z` (Created at 258,743 B);
Round 4b `01:44:27.852Z` (Created at 258,743 B).

**Implication:** courier-relevant content arrives ready-to-process.

### Finding 2 — Text delivery STREAMS

Text files appear as filename-at-0-B first, then content arrives in
1+ later `Changed` events. The lag from filename to first content
event can be **10 s to 8 min** (Round 1: 8 min 24 s; Round 2: 10 s;
Round 3: 20 s).

**Evidence:** Round 1 `01:12:07.465Z → 01:20:31.373Z`; Round 2
`01:23:29.613Z → 01:23:39.806Z`; Round 3 `01:29:27.681Z →
01:29:47.990Z`.

**Implication:** not directly relevant to the Iter 3 courier (audio
only), but explains why a `Created` event alone is never enough to
trigger ingest of any file type — always pair with a stability check.

### Finding 3 — Every FS event fires 3-4 TIMES within ~5-12 ms

The Windows `FileSystemWatcher` (and by extension the `notify` crate
that wraps the same OS API) emits every logical change as a small
burst of duplicates. Across all 5 rounds, the duplicate-burst window
never exceeded ~12 ms.

**Evidence:** Round 1 `01:11:33.488 → .546` (3 events, 58 ms);
Round 4 `01:32:39.125 → .135` (5 events, 10 ms); Round 4b
`01:44:27.852 → .863` (4 events, 11 ms).

**Implication:** the watcher MUST debounce. A **100 ms** coalescing
window comfortably absorbs every observed burst.

### Finding 4 — Filename arrives BEFORE content for text files

For streamed text (Finding 2), the `Created` event fires on the final
filename with `size: 0`. Content arrives later via `Changed` events
that monotonically grow `size`.

**Implication:** size-stability check is the only reliable
"file is ready" signal. A consecutive-stable-reads heuristic (e.g.
"size unchanged at 2 successive 2-second polls" → ingest) is
sufficient.

### Finding 5 — CRITICAL UX: external-app writes into Obsidian's iOS sandbox lag 5-15 min until Obsidian Mobile is foregrounded

**This is the single most consequential finding of the spike** and it
emerged unexpectedly from Round 4b.

When an iOS app outside Obsidian Mobile (Files, Voice Memos, the
forthcoming Shortcut) writes a file into Obsidian Mobile's sandbox,
**Obsidian Mobile does not notice the file until either:**

- the user foregrounds the Obsidian Mobile app, OR
- iOS schedules a BGAppRefresh window for Obsidian Mobile (Apple's
  opaque cadence, typically every 5-15 minutes, can be longer for
  rarely-used apps).

Until Obsidian Mobile notices the file, **it never pushes the file to
Obsidian Sync's server**, which means the desktop never sees it.

**Evidence:** Round 4b — file saved on phone at ~01:39 (Dustin's
wall-clock note); desktop `Created` event at `01:44:27.852Z` — a gap
of approximately 5 minutes. The intervening period on the desktop
shows only `.obsidian/workspace.json` chatter, no payload events.

**Mitigation:** add **`Open App → Obsidian`** as the final action in
the iOS Shortcut. Foregrounding Obsidian Mobile triggers an immediate
sandbox rescan + sync push. Expected end-to-end latency from
"done speaking" → "transcript on desktop" drops from **5-15 minutes**
to **~30-60 seconds**.

**This mitigation is shipped as an amendment to
`docs/mobile/ios-shortcut.md` in the same commit as this findings doc.**

### Finding 6 — Obsidian's Sync log is a gold-mine diagnostic

Settings → Sync → `…` (overflow menu) → "Show sync log" reveals
server-side timestamps for every file event, tagged with the device
that produced it (e.g. `[iPhone]`, `[Dell-XPS]`). Event types include
`Created`, `Updated`, `Server pushed (deleted or renamed)`, etc.

**Implication:** the Mobile Sync user-setup doc + the Iter 3 smoke
checklist should reference this as the FIRST diagnostic step when
"file didn't arrive" is reported. Distinguishes phone-side delay
(no server event yet) from sync-layer delay (server event present,
desktop hasn't picked up) cleanly.

### Finding 7 — `.obsidian/workspace.json` fires CONSTANTLY

Obsidian rewrites this file every few seconds while the app is open
on the desktop. The spike's watcher captured ~20 events on this single
file during Round 3 alone (vs ~18 events on the actual payload file
across two payloads).

**Implication:** the production watcher must exclude `.obsidian/`
(and `.git/`, `.mockingbird/`, `inbox/_archive/`, `inbox/_failed/`,
`inbox/_keep/`) at the watcher's source-level filter, NOT at the
event-handler level — including these in the event stream wastes
debounce slots and CPU.

### Finding 8 — Files-app save destination is critical UX

Round 4 (Obsidian-native attachment flow) landed the binary at the
**vault root**, not in `inbox/`. The watcher only looks at `inbox/`,
so a file at the vault root is invisible to the courier.

Round 4b (Files-app → `inbox/`) landed the binary correctly because
the user explicitly navigated to `inbox/` in the iOS Files picker.

**Implication:** the Shortcut MUST use the **Save File → Ask Where to
Save (ON)** pattern with `inbox/` as the saved-last destination. iOS
remembers the last destination, so this is one tap after the first
use. The user-setup doc already calls this out; the post-spike
amendment reinforces it.

---

## Design implications for Wave 3

Distilled, the eight findings produce this concrete spec for Wave 3.1
(the watcher) and Wave 3.2 (the courier):

### Watcher (Wave 3.1)

- **Crate:** `notify` (or `notify-debouncer-full` if its quiet-window
  semantics fit better than a hand-rolled debounce).
- **Watched root:** `<vault>/inbox/`, recursive.
- **Source-level path exclusions:** `.obsidian/`, `.git/`,
  `.mockingbird/`, `inbox/_archive/`, `inbox/_failed/`, `inbox/_keep/`.
- **Source-level filename exclusions:** `.tmp`, `.partial`,
  `.crdownload`, `.icloud` (suffix match); `~$` prefix.
- **Extension allowlist for processing:** `.m4a` (primary), `.wav`,
  `.mp3`.
- **Debounce window:** **100 ms** per path (Finding 3).
- **Stability check:** when an allowlisted, size > 0 file is observed,
  schedule a re-check 2 s later. If size unchanged → stable.
  If size differs → reset. **Two consecutive stable reads** required
  before emitting (defensive against future chunked-binary delivery).
- **Output:** push `StableInboxFile { path, size, observed_at }` to a
  bounded channel consumed by the courier processor.
- **Logging:** `tracing::info!` for "file detected", "file stable",
  "file processed"; `tracing::debug!` for raw FS events.

### Courier (Wave 3.2)

- Single-in-flight (mutex / single-consumer channel).
- Validates: extension in allowlist, `0 < size < 50 MB` (defensive
  upper bound; Obsidian Sync Standard's 5 MB cap means oversized
  files won't arrive via sync but local-FS drag-drop into `inbox/`
  could).
- Decodes via `audio::decode::decode_to_pcm16_mono_16k` (Iter 1's
  `mb-hxm4`).
- Builds `IngestProvenance { source: SessionSource::MobileInbox,
  original_filename, received_at_iso }`.
- Submits via the `HeadlessIngestRequest` crossbeam channel (Iter 1's
  ADR §3.2 amendment).
- On success → move to `inbox/_archive/<YYYY-MM-DD>/<filename>`.
- On failure → move to `inbox/_failed/<filename>` + verbose
  `tracing::error!`.
- Emit `SessionsEventBus` refresh so the Dictations page picks up
  the new row.

### Runtime wiring (Wave 3.3)

- `InboxRuntime` owns watcher thread + courier thread + their channel.
- Started/stopped by `MobileSyncEnabled` + `VaultPath` settings
  (matches Iter 2's `VaultRuntime` UX: one toggle drives both
  outbound projection and inbound courier).
- **Initial scan** of `inbox/` on Mockingbird startup with
  `MobileSyncEnabled = true` → pick up any files that arrived while
  Mockingbird was off (the "I recorded while my laptop was closed"
  case).

---

## Bonus tools — Obsidian Sync log

When a user reports "saved a memo but nothing appeared on desktop",
the diagnostic order is:

1. **Obsidian Sync log** (Settings → Sync → `…` → Show sync log) on
   the phone: did a `[iPhone] Created` event for the filename appear?
   - **No** → the file never left the phone. Was the Shortcut's
     `Open App` action skipped? Is Obsidian Mobile signed in to Sync?
     Did the user save outside `inbox/`?
   - **Yes** → continue to step 2.
2. **Obsidian Sync log on desktop:** did the corresponding event
   appear here? If no → desktop Sync is stalled (network issue,
   sign-in issue, Obsidian process not running).
3. **Desktop filesystem:** does `<vault>/inbox/<filename>` exist? If
   yes but the courier didn't ingest → Mockingbird inbox watcher bug;
   check tracing logs.

This diagnostic chain should land in the Mobile Sync tab's
troubleshooting copy (Iter 4 polish) and in the Iter 3 smoke checklist
inline.

---

## What we did NOT test (and why it's safe)

### Round 5 — edit-then-delete

**Skipped.** The courier never edits or deletes files based on
phone-side events; its archive move (`inbox/<file>` →
`inbox/_archive/<date>/<file>`) uses paths the watcher's exclusion
filter already covers. Rounds 1-2's incidental observation of
rename-as-delete-then-create gives us 80% of the delete-shape data we
would have collected.

**What we'd learn from running it:** the exact event shape of a pure
phone-side delete (single `Deleted`? `Renamed` to a hidden trash dir?
some hybrid?). Not currently consequential; if the courier ever needs
to react to phone-side deletes (e.g. "user cancelled the courier mid-
flight"), revisit then.

### Round 6 — offline catch-up burst

**Skipped.** Finding 3's per-event duplicate-burst pattern (3-4 events
in 5-12 ms) gives us the per-change density; the 100 ms debounce
window has comfortable headroom even if a 5-file burst delivered
back-to-back. The user-facing impact of "lots of memos arriving at
once" is already correct: each one queues, the single-in-flight
courier processes them serially.

**What we'd learn from running it:** the exact inter-event spacing
during a multi-file catch-up burst, plus whether Obsidian Sync
preserves phone-side event ordering. If a future Iter 3 smoke turns up
an ordering-dependent bug, this is the first follow-up spike to run.

### Other things explicitly out of scope (per the observation plan)

- 5 MB Sync Standard silent-skip behavior — characterized in ADR §9,
  tracked as `mb-0uqb`.
- iCloud Drive sync behavior — ADR 0046 explicitly does NOT use
  iCloud as a transport.
- Conflict-file resolution — `<vault>/history/` is regenerable.
- Real iOS Shortcut courier flow — that's Wave 3.5's live-fire smoke,
  not a spike round.

---

## References

- Observation plan:
  [`docs/spikes/iter3-sync-layer-observation-plan.md`](./iter3-sync-layer-observation-plan.md)
- Raw logs:
  - [`round-1-small-text.log`](./iter3-logs/round-1-small-text.log)
  - [`round-2-medium-markdown.log`](./iter3-logs/round-2-medium-markdown.log)
  - [`round-3-large-text.log`](./iter3-logs/round-3-large-text.log)
  - [`round-4-binary-attachment.log`](./iter3-logs/round-4-binary-attachment.log)
  - [`round-4b-voice-memo-direct.log`](./iter3-logs/round-4b-voice-memo-direct.log)
- Beads consuming this doc:
  - `mb-s8s2` (Wave 0 spike — closed in the same commit as this doc)
  - `mb-9lgi` (Wave 3.1 inbox watcher — description refreshed against
    these findings in the same commit)
  - `mb-txmy` (Wave 3.2 courier processor — description refreshed)
  - **Wave 3.3** (`InboxRuntime` wiring — new bead created)
  - **Wave 3.4** (Iter 3 sealed-phases-untouched judge — new bead)
  - **Wave 3.5** (Iter 3 live-fire smoke — new bead)
- ADR:
  [`docs/adr/0046-mobile-extension-via-vault.md`](../adr/0046-mobile-extension-via-vault.md)
  (§6 combined detector, §8 mobile capture, §9 sidecar descoped)
- Affected spec doc:
  [`docs/mobile/ios-shortcut.md`](../mobile/ios-shortcut.md) (amended
  in the same commit per Finding 5).
