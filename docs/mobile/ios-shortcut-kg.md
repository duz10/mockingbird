# Quick Knowledge-Graph Capture from your iPhone - iOS Shortcut setup

> **Status:** spec + setup walkthrough. The desktop side of this flow (the
> KG-Inbox courier that ingests `.m4a` files from
> `<vault>/Knowledge Graph/Inbox/`) ships in ADR 0053 Wave 1E.6 (sealed
> 2026-06-08). The end-to-end Personal Knowledge Engine round-trip
> (chat-LLM Ingest crystallizing raw entries into wiki-linked pages) is
> chartered under ADR 0054 §H and lands in Phase 2. Install the
> Shortcut now if you want to be ready; files saved today land in
> `Knowledge Graph/Entries/` as Layer 1 raw entries, ready for the
> chat-LLM to crystallize when Phase 2 lights up.

> **This is a SECOND Shortcut.** If you have not yet set up the standard
> dictation Shortcut from
> [`ios-shortcut.md`](./ios-shortcut.md), do that one first and
> get comfortable with it. The KG variant has the same three-action
> shape and the same trigger options - only the save destination and
> the recommended name change. Both Shortcuts can coexist on the same
> phone; pick which one to fire per capture based on whether the
> recording is dictation (one-shot text you want pasted somewhere) or a
> knowledge-graph note (a thought you want filed into your vault).

---

## What this is for

This Shortcut lets you record a voice memo on your iPhone with one tap
and have it filed into your Personal Knowledge Engine on your desktop
automatically - distinct from the regular dictation Shortcut, which
treats captures as one-shot text-to-paste.

The flow is:

1. You trigger the Shortcut on the phone (back-tap, home-screen icon,
   or Action Button).
2. The phone records audio and prompts you to save the file.
3. You tap to confirm the save destination (your Obsidian vault's
   `Knowledge Graph/Inbox/` folder - note the space, and the capital
   letters).
4. Obsidian Sync ferries the file to your desktop within ~30 seconds.
5. Mockingbird's **KG-Inbox courier** (sibling of the standard inbox
   courier; ADR 0053 §D6) picks the file up, runs it through the same
   transcription pipeline as your desktop dictations, and tags the
   resulting session with `capture_kind = 'kg-note'`.
6. The KG filing pipeline projects the entry into
   `Knowledge Graph/Entries/<YYYY-MM-DD>-<slug>__<hash>.md` and updates
   `INDEX.md` + `LOG.md`. Entity / project / tag stub pages get
   auto-created for anything the entry mentions.
7. The audio file is moved out of `Inbox/` into
   `Knowledge Graph/History/<YYYY-MM>/<session-uuid>.m4a` for archive
   (this is correct behavior - it's not lost, just filed).

Mockingbird's processing stays 100% local. The only network hop is
Obsidian Sync moving the audio file from your phone to your desktop
through Obsidian's servers - encrypted end-to-end if you've set an
encryption password on the vault (recommended; see the POC config in
ADR 0046).

The chat-LLM Ingest step (the half of the Personal Knowledge Engine
that crystallizes Layer 1 raw entries into Layer 2 wiki pages) is
chartered under ADR 0054 §H and runs in your chat-LLM (Claude Code /
Cursor / OpenCode / etc.), not in Mockingbird. Until Phase 2 ships,
your raw entries pile up cleanly in `Entries/` and `INDEX.md` -
nothing is lost, the chat-LLM just isn't yet wired to read
`SCHEMA.md` and synthesize across them on a cron.

---

## Prerequisites

Before building the Shortcut, confirm all four of these:

- **Obsidian Mobile is installed on your iPhone**, signed in to the
  same Obsidian account as your desktop, and the `mockingbird-vault`
  appears in the Obsidian Mobile vault list and is opened at least
  once (initial sync needs to complete).
- **The `Knowledge Graph/Inbox/` folder exists in the vault.** Open
  Obsidian Mobile, open `mockingbird-vault`, expand the file tree on
  the left - you should see a `Knowledge Graph/` top-level folder with
  `Inbox/`, `Entries/`, `History/`, `Entities/`, `Projects/`, and
  `Tags/` beneath it. If the `Knowledge Graph/` subtree is missing,
  the desktop side hasn't bootstrapped it yet - open Mockingbird on
  the desktop, go to Settings → Knowledge Graph, and toggle
  **Knowledge Graph** on so the subtree gets created (this is
  idempotent; flipping it on twice has no effect).
- **Mockingbird's Knowledge Graph toggle is ON on your desktop**, so
  the KG-Inbox courier is actually running. This is the toggle in
  Settings → Knowledge Graph. The courier is gated by both this
  toggle AND a valid `VaultPath`; if either is missing, the watcher
  is not spawned.
- **The standard mobile sync prerequisite (Obsidian Sync running on
  the phone, vault opened at least once)** from
  [`ios-shortcut.md`](./ios-shortcut.md) also applies here - the
  transport is identical, only the watched folder differs.

---

## Build the Shortcut

The Shortcut is intentionally tiny: **three built-in actions, nothing
more**. No iOS Scripting JS, no custom URL handlers, no third-party
apps. The shape is identical to the dictation Shortcut, locked at
this pattern per ADR 0046 §8 + ADR 0046 Iter 3 Wave 0 spike findings
(see "Why this design" at the bottom of this doc).

> **Important - three actions, not two.** Same caveat as the
> dictation Shortcut. Without the third **Open App** action, files
> saved by the Shortcut sit in Obsidian Mobile's iOS sandbox for
> 5-15 minutes before being pushed to Sync. The third action drops
> end-to-end latency to ~30-60 s. Full background:
> [`docs/spikes/iter3-sync-layer-findings.md`](../spikes/iter3-sync-layer-findings.md)
> § Finding 5.

### Step-by-step in the Shortcuts app

1. Open the **Shortcuts** app on your iPhone.
2. Tap the **+** in the top right to create a new Shortcut.
3. Tap **Add Action**. Search for **Record Audio**. Tap it to add it.
4. Configure the Record Audio action:
   - **Audio Quality:** Low (32 kbps AAC mono). Same reasoning as the
     dictation Shortcut - the 5 MB Obsidian Sync Standard cap is the
     binding constraint, and Low gets you ~20 min per recording.
   - **Start Recording:** **On Tap**. Tap red to start, tap stop when
     done.
5. Tap **Add Action** again. Search for **Save File**. Tap it to add
   it. Its input auto-feeds from Record Audio's output (you'll see a
   magic-variable pill referencing the recorded audio).
6. Configure the Save File action:
   - **Service:** Files.
   - **Ask Where to Save:** **ON**. (One confirmation tap per
     capture - same trade-off as the dictation Shortcut, same
     reasoning under "Why this design".)
   - **Destination Path:** leave blank. The iOS Files picker will
     pop up at run time; you'll navigate to the vault's
     `Knowledge Graph/Inbox/` folder (NOT the standard `inbox/`!)
     and tap Save. iOS remembers the last destination, so after
     the first use it's a single confirmation tap.
   - **Overwrite If File Exists:** OFF (default - the auto-generated
     filename includes a timestamp so collisions don't happen).
7. Tap **Add Action** a third time. Search for **Open App**. Tap it
   to add it. Configure:
   - **App:** Obsidian.

   **This third action is critical, same as in the dictation
   Shortcut.** Without it, Obsidian Mobile stays backgrounded after
   the save and doesn't notice the new file in its sandbox until
   iOS schedules a BGAppRefresh window - typically 5-15 minutes,
   sometimes longer. Foregrounding Obsidian forces an immediate
   sandbox rescan and Sync push, reducing end-to-end latency from
   "done speaking" to "entry filed on desktop" to about 30-60
   seconds.
8. Tap the Shortcut name at the very top (it defaults to "New
   Shortcut") and rename to **Mockingbird Quick Capture (Knowledge
   Graph)**. This is the **recommended naming convention** to keep
   the two Shortcuts cleanly distinguishable; pair it with
   **Mockingbird Quick Capture (Dictation)** on the dictation
   Shortcut.
9. Tap **Done** in the top right.

The Shortcut is now installed and ready to bind to a trigger.

---

## Pick a trigger

Same three options as the dictation Shortcut. **Pick one** - they all
work the same way, just different ergonomics. If you already have the
dictation Shortcut bound to one trigger, pick a different one for
the KG variant so you can fire either at will.

### Option A: Home Screen icon (recommended for the KG variant)

Most visible, no special hardware needed. Recommended because the
visual icon makes it obvious which Shortcut you're firing - a meaningful
difference when you have two Shortcuts that look identical at trigger
time.

1. In the Shortcuts app, **long-press** your Mockingbird Quick
   Capture (Knowledge Graph) Shortcut.
2. Tap **Add to Home Screen**.
3. Pick an icon + name + position. Suggestion: use a distinct icon
   color from the dictation Shortcut (e.g. green for KG, blue for
   dictation) so they're visually unambiguous on the Home Screen.

Tapping the icon launches the recorder. Works from the Home Screen
or Spotlight search.

### Option B: Back-tap (Triple Tap, if Double Tap is taken)

Lowest friction, no visible hardware change, works on every recent
iPhone. If you have the dictation Shortcut on Double Tap, use Triple
Tap for the KG variant.

1. Open **Settings → Accessibility → Touch → Back Tap**.
2. Tap **Triple Tap** (or Double Tap if it's free).
3. Scroll the action list and select **Mockingbird Quick Capture
   (Knowledge Graph)**.

Now triple-tapping the back of the phone launches the KG recorder.

### Option C: Action Button (iPhone 15 Pro / 16 / 17)

Same setup as the dictation Shortcut. The Action Button only binds
to a single Shortcut - pick whichever variant you fire most often,
and use a Home Screen icon for the other.

---

## First use walkthrough

1. Trigger the Shortcut (Home Screen icon, back-tap, or Action
   Button).
2. The Record Audio sheet appears. Tap the **red record button** to
   start, speak, tap **Stop** when done.
3. The iOS Save File picker appears. Navigate:
   **On My iPhone → Obsidian → mockingbird-vault → Knowledge Graph →
   Inbox**. Tap **Save** (or **Open** depending on iOS version) at
   the top right.
4. iOS remembers the destination - **second and subsequent uses are
   single-tap confirmations**, no navigation needed.
5. **Obsidian Mobile opens automatically** (the third Shortcut
   action). You can leave it open or swipe back to whatever you were
   doing; the key thing is that Obsidian was foregrounded long
   enough to notice the new file and push it to Sync.
6. Within ~30-60 seconds the file syncs to your desktop. The KG-Inbox
   courier picks it up, transcribes it, files an entry into
   `Knowledge Graph/Entries/<YYYY-MM-DD>-<slug>__<hash>.md`, and
   moves the original audio into
   `Knowledge Graph/History/<YYYY-MM>/<session-uuid>.m4a`.
7. Open Mockingbird's Knowledge Graph dashboard (or the
   `Knowledge Graph/INDEX.md` file in Obsidian) to verify the entry
   appeared and the catalog updated.

> **The audio leaving `Inbox/` is correct.** Unlike the standard
> dictation inbox courier (which discards the audio to a local temp
> after ingest), the KG-Inbox courier preserves it in `History/`
> alongside a JSON sidecar. This is so the chat-LLM Ingest pass can
> reach back to the original audio if it ever needs to (per ADR 0054
> §H). If you check `Inbox/` after a successful capture and the file
> is gone - that's the success path, not a bug.

---

## Known limitations (read these - they save real headaches)

### The 5 MB Obsidian Sync Standard cap

Identical to the dictation Shortcut's behavior; see
[`ios-shortcut.md`](./ios-shortcut.md) § "The 5 MB Obsidian Sync
Standard cap" for the full math and workarounds. Short version: at
Low quality you get ~20 min per recording before the cap silently
drops the file.

### Save destination must be the `Knowledge Graph/Inbox/` folder

If you accidentally save to a different folder in the vault (most
commonly the standard `inbox/` folder, which is one level higher
in the tree), the file syncs but lands in the wrong courier:

- File in `<vault>/inbox/` → standard dictation courier ingests it as
  plain dictation (`capture_kind = 'dictation'`); no KG filing.
- File in `<vault>/Knowledge Graph/Inbox/` → KG-Inbox courier ingests
  it as a knowledge-graph note (`capture_kind = 'kg-note'`) and
  files it into `Entries/`.

This is **positional routing** per ADR 0048 Q2 and ADR 0053 §D6 -
the destination folder *is* the intent signal. iOS Files remembers
your last destination, so after the first save to the right place
it's sticky; but if you ever save to the wrong place, the remembered
destination changes to that wrong place and stays wrong until you
manually re-navigate. If KG captures stop appearing in `Entries/`,
this is the first thing to check.

### No silent zero-tap variant

Same as the dictation Shortcut. The "Ask Where to Save" toggle is
intentionally ON; the silent variant would require an iCloud-Drive
vault, which we explicitly do not do.

### Knowledge Graph toggle must be ON

If the desktop Settings → Knowledge Graph toggle is OFF, the courier
is not running and your captures pile up in `Knowledge Graph/Inbox/`
unprocessed. They are not lost - flip the toggle on and the
startup catch-up scan picks them all up in dedup-ledger order.

### Obsidian Mobile must be installed, signed in, and have opened
the vault at least once after install

Same as the dictation Shortcut.

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| Saved a file but it never appeared in `Entries/` on desktop | (a) Obsidian Sync on phone is paused or unreachable; (b) Mockingbird Knowledge Graph toggle on desktop is OFF; (c) file landed in `<vault>/inbox/` instead of `<vault>/Knowledge Graph/Inbox/`; (d) file is >5 MB | Check Obsidian Mobile → Settings → Sync (look for green check); check Settings → Knowledge Graph on desktop; verify the file is actually under `Knowledge Graph/Inbox/` via Obsidian Mobile file tree; check the file size on disk |
| File showed up in the standard Dictations page, not in `Entries/` | Saved to the wrong folder - went into `<vault>/inbox/` (standard dictation courier) instead of `<vault>/Knowledge Graph/Inbox/` (KG courier) | Trigger the Shortcut again, re-navigate the Save File picker carefully: the path is **On My iPhone → Obsidian → mockingbird-vault → Knowledge Graph → Inbox**, not just "inbox" |
| File appears in `Knowledge Graph/Inbox/_failed/` instead of being filed | Audio decode failed, transcription pipeline errored, or content failed validation. The courier quarantines the file to `_failed/` so it doesn't loop-retry forever | Open the file in `_failed/`; check Mockingbird's desktop log (Settings → About → Open log folder) for the specific error. Common causes: zero-byte recording (mic permission revoked), garbage-encoded m4a, recording shorter than the minimum-audio threshold. Move the file back to `Knowledge Graph/Inbox/` to retry after fixing the root cause |
| Shortcut crashes when launched | iOS Shortcut runtime bug | Force-close the Shortcuts app and reopen. If it keeps happening, delete and rebuild the Shortcut (only takes a minute) |
| Record Audio doesn't capture any sound | First-run mic permission denied | Settings → Privacy & Security → Microphone → Shortcuts → enable. Trigger the Shortcut again |
| Save File picker can't find the `Knowledge Graph/` folder | Obsidian Mobile hasn't synced the subtree yet, or the desktop hasn't bootstrapped it | On desktop, toggle Settings → Knowledge Graph off then on (idempotent bootstrap re-runs); wait for Obsidian Sync to ferry the new folders to the phone (~30 s). Then try the Shortcut again |
| Recording saved but takes 5-15 minutes to file | The Shortcut is missing the **Open App** action (Action 3). Without it, Obsidian Mobile stays backgrounded and doesn't notice the saved file in its sandbox until iOS schedules a BGAppRefresh window | Edit the Shortcut: tap **Add Action** → search **Open App** → add it as the final step → configure App = Obsidian. Tap Done. Next capture should land within ~30-60 s |
| Audio file disappeared from `Knowledge Graph/Inbox/` shortly after appearing there | **This is the success path.** The KG worker moves the audio to `Knowledge Graph/History/<YYYY-MM>/<session-uuid>.m4a` after the entry is filed | Check `Entries/` for the new `.md` and `History/<YYYY-MM>/` for the moved audio. If neither exists, fall through to the "never appeared in Entries/" row above |

The three-step diagnostic chain from the dictation Shortcut doc
(phone Sync log → desktop Sync log → Mockingbird desktop log)
applies here unchanged; see
[`ios-shortcut.md`](./ios-shortcut.md) § "Troubleshooting" for the
full walkthrough.

---

## Why this design

A short note on the choices that look weird at first, identical in
spirit to the dictation Shortcut's "Why this design" section
([`ios-shortcut.md`](./ios-shortcut.md)) but with the KG-specific
deltas called out:

- **Same three-action shape as the dictation Shortcut**, not a fancier
  KG-specific variant. The choice was deliberate: any divergence
  between the two Shortcut shapes is a place users get confused and
  Mockingbird gets a tail of "why doesn't the KG one work the same
  way" bugs. Parity is a feature.
- **Save destination is `Knowledge Graph/Inbox/` (with the space and
  the capitals), not a flatter `kg-inbox/` or `knowledge-graph/`.**
  The literal folder name is fixed by the KG layout module
  (`src-tauri/src/vault/kg_layout.rs::KG_SUBTREE_ROOT_NAME`) and is
  Obsidian-tooling-friendly (the Tasks plugin, Bases plugin, etc.
  all happily index a folder with a space in the name). Don't rename
  it on disk; the courier won't find it.
- **No importable `.shortcut` file shipped.** Same reason as the
  dictation Shortcut (ADR 0046 §8): Apple's `.shortcut` format
  embeds the user's specific iCloud / Files paths, which makes it
  non-portable across vault locations. The 90 seconds it takes to
  build the Shortcut by hand is cheaper than the support tail of a
  shipped file that silently points at the wrong folder.
- **Two Shortcuts, not one parameterized Shortcut with a "kind"
  picker.** Adding a kind-picker step would (a) add a tap, defeating
  the back-tap / Action Button low-friction case; (b) introduce a
  user-decision-at-capture-time when the whole point of positional
  routing (ADR 0048 Q2) is that the destination folder IS the
  decision. Two Shortcuts, two triggers, zero decisions at capture
  time.
- **The KG courier preserves the audio in `History/`, the standard
  inbox courier discards it.** Per ADR 0054 §H, the chat-LLM Ingest
  step may need to reach back to the raw audio for re-transcription
  or context expansion when crystallizing Layer 1 entries into Layer
  2 wiki pages; the standard dictation flow has no such requirement.
  This is the load-bearing reason the two couriers exist as siblings
  rather than one parameterized courier.

---

## Cross-references

- **Dictation Shortcut precedent:** [`ios-shortcut.md`](./ios-shortcut.md)
  (ADR 0046 §8). Identical three-action shape; differs only in
  destination folder, recommended name, and post-ingest disposition.
- **KG-Inbox courier charter:** ADR 0053 §D6 (sibling courier rationale,
  positional routing per ADR 0048 Q2). Implementation sealed at Wave
  1E.6.
- **iOS Shortcut docs charter:** ADR 0053 §D10. This doc satisfies
  Wave 1E.8.
- **Personal Knowledge Engine framing:** ADR 0054 §H (Ingest contract
  - what the chat-LLM does with the raw entries this Shortcut
  produces). Lands in Phase 2.
- **Sync-layer findings (Open App necessity):**
  [`docs/spikes/iter3-sync-layer-findings.md`](../spikes/iter3-sync-layer-findings.md)
  § Finding 5.
