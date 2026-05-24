# Quick Voice Capture from your iPhone — iOS Shortcut setup

> **Status:** spec + setup walkthrough. The desktop side of this flow (the
> inbox watcher that ingests `.m4a` files from `<vault>/inbox/`) ships in
> ADR 0046 Iteration 3's implementation waves. Install the Shortcut now if
> you want to be ready; until the watcher ships, files you save will sync to
> the vault but won't be transcribed.

---

## What this is for

This Shortcut lets you record a voice memo on your iPhone with one tap
and have it transcribed by Mockingbird on your desktop automatically.

The flow is:

1. You trigger the Shortcut on the phone (back-tap, home-screen icon,
   or Action Button).
2. The phone records audio and prompts you to save the file.
3. You tap to confirm the save destination (your Obsidian vault's
   `inbox/` folder).
4. Obsidian Sync ferries the file to your desktop within ~30 seconds.
5. Mockingbird's inbox watcher picks the file up and runs it through
   the same transcription pipeline as your desktop dictations.
6. A new transcription appears in the Dictations page, marked as
   coming from the mobile inbox.

Mockingbird's processing stays 100% local. The only network hop is
Obsidian Sync moving the audio file from your phone to your desktop
through Obsidian's servers — encrypted end-to-end if you've set an
encryption password on the vault (recommended; see the POC config in
ADR 0046).

---

## Prerequisites

Before building the Shortcut, confirm all three of these:

- **Obsidian Mobile is installed on your iPhone**, signed in to the
  same Obsidian account as your desktop, and the `mockingbird-vault`
  appears in the Obsidian Mobile vault list and is opened at least
  once (initial sync needs to complete).
- **The `inbox/` folder exists in the vault.** Open Obsidian Mobile,
  open `mockingbird-vault`, expand the file tree on the left — you
  should see at least `history/`, `inbox/`, and a `Welcome.md` file.
  If `inbox/` is missing, the desktop side hasn't initialized the
  vault layout yet — open Mockingbird on the desktop, go to
  Settings → Mobile (or Advanced → Mobile Sync preview while Iter 3
  is in flight), and toggle mobile sync on so the vault zones get
  created.
- **Mockingbird mobile sync is enabled on your desktop**, so the
  inbox watcher is actually running. This is the toggle in Settings →
  Mobile (or Advanced → Mobile Sync preview in pre-Iter-4 builds).

---

## Build the Shortcut

The Shortcut is intentionally tiny: **three built-in actions, nothing
more**. No iOS Scripting JS, no custom URL handlers, no third-party
apps. The spec is locked at this shape per ADR 0046 §8 + the
ADR 0046 Iter 3 Wave 0 spike findings — see the "Why this design"
section at the bottom of this doc for the reasoning.

> **Important — three actions, not two.** Earlier drafts of this doc
> described a two-action Shortcut. The ADR 0046 Iter 3 Wave 0 spike
> (May 2026) found that without a third **Open App** action, files
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
   - **Audio Quality:** Low (32 kbps AAC mono). This is the single
     biggest knob for "how many minutes of recording fit under the
     5 MB Obsidian Sync cap" — Low gets you ~20 min per recording,
     Normal gets you ~5 min, High gets you ~2.5 min.
   - **Start Recording:** **On Tap**. You'll see a recording UI when
     you trigger the Shortcut; tap the red button to start, tap stop
     when done. This is the "press-talk-release" rhythm and matches
     your desktop PTT mental model.
5. Tap **Add Action** again. Search for **Save File**. Tap it to add
   it. Its input auto-feeds from Record Audio's output (you'll see
   a magic-variable pill referencing the recorded audio).
6. Configure the Save File action:
   - **Service:** Files.
   - **Ask Where to Save:** **ON**. (This is the explicit
     confirmation step — see "Why this design" below.)
   - **Destination Path:** leave blank. The iOS Files picker will
     pop up at run time; you'll navigate to the vault's `inbox/`
     folder and tap Save. iOS remembers the last destination, so
     after the first use it's a single confirmation tap.
   - **Overwrite If File Exists:** OFF (default — the
     auto-generated filename includes a timestamp so collisions
     don't happen).
7. Tap **Add Action** a third time. Search for **Open App**. Tap it
   to add it. Configure:
   - **App:** Obsidian.

   **This third action is critical.** Without it, the Shortcut leaves
   Obsidian Mobile in the background, and Obsidian Mobile doesn't
   notice the saved file in its sandbox until its next iOS
   BGAppRefresh window — typically **5-15 minutes**, sometimes longer.
   Foregrounding Obsidian via this action forces an immediate sandbox
   rescan and Sync push, reducing end-to-end latency from
   "done speaking" to "transcript on desktop" to about **30-60
   seconds**. The ADR 0046 Iter 3 Wave 0 spike observed exactly this
   pattern empirically (Round 4b, ~5 minute lag without
   foregrounding); the third action is the mitigation. Full detail:
   [`docs/spikes/iter3-sync-layer-findings.md`](../spikes/iter3-sync-layer-findings.md)
   § Finding 5.
8. Tap the Shortcut name at the very top (it defaults to "New
   Shortcut") and rename to something memorable — suggestion:
   **Quick voice capture** or **Capture for Mockingbird**.
9. Tap **Done** in the top right.

The Shortcut is now installed and ready to bind to a trigger.

---

## Pick a trigger

Three options. **Pick one** — they all work the same way, just
different ergonomics.

### Option A: Back-tap (recommended)

Lowest friction, no visible hardware change, works on every recent
iPhone.

1. Open **Settings → Accessibility → Touch → Back Tap**.
2. Tap **Double Tap** (or Triple Tap if you want to keep Double Tap
   for something else).
3. Scroll the action list and select your **Quick voice capture**
   Shortcut.

Now double-tapping the back of the phone launches the recorder. Works
in any app, screen on or off.

### Option B: Home Screen icon

Most visible, no special hardware needed.

1. In the Shortcuts app, **long-press** your Quick voice capture
   Shortcut.
2. Tap **Add to Home Screen**.
3. Pick an icon + name + position.

Tapping the icon launches the recorder. Works from the Home Screen
or Spotlight search.

### Option C: Action Button (iPhone 15 Pro / 16 / 17)

Physical side button, no looking at the screen, works pocket-blind.

1. Open **Settings → Action Button**.
2. Swipe to the **Shortcut** card.
3. Tap **Choose a Shortcut** and select your Quick voice capture
   Shortcut.

Pressing and holding the Action Button launches the recorder. You'll
still need to tap once on screen to confirm the save destination
(the "Ask Where to Save" prompt); there is no fully-silent zero-tap
variant in v1.

---

## First use walkthrough

1. Trigger the Shortcut (back-tap, icon, or Action Button).
2. The Record Audio sheet appears. Tap the **red record button** to
   start, speak, tap **Stop** when done.
3. The iOS Save File picker appears. Navigate:
   **On My iPhone → Obsidian → mockingbird-vault → inbox**.
   Tap **Save** (or **Open** depending on iOS version) at the top
   right.
4. iOS remembers the destination — **second and subsequent uses are
   single-tap confirmations**, no navigation needed.
5. **Obsidian Mobile opens automatically** (the third Shortcut
   action). You'll see Obsidian's vault view briefly — you can leave
   it open or swipe back to whatever you were doing; the key thing
   is that Obsidian was foregrounded long enough for it to notice
   the new file and push it to Sync.
6. Within ~30-60 seconds the file syncs to your desktop. Mockingbird's
   inbox watcher picks it up and queues it for transcription.
7. A new row appears in the Dictations page on your desktop.

---

## Known limitations (read these — they save real headaches)

### The 5 MB Obsidian Sync Standard cap

Obsidian Sync Standard silently skips files larger than 5 MB —
they appear to save on the phone but **never sync**, and Obsidian
Sync gives no notification that they were dropped.

At the Shortcut's Low quality preset (32 kbps AAC mono), the math
works out to **~20 minutes of audio per recording** under the cap.
That covers ~95% of practical use; keep an eye on it for
genuinely long recordings.

If you hit this limit regularly:

- Upgrade to **Obsidian Sync Plus** (200 MB cap, ~13 hours of audio
  at the same quality).
- Or switch to **Syncthing** (no cap; documented as a future
  backend in ADR 0046 §10, not wired in the v1 POC).

A future Mockingbird polish iteration may detect missing files via
the reconciliation scan and warn you (`mb-0uqb`); v1 does not.

### Save destination must be the `inbox/` folder

If you accidentally save to a different folder in the vault (or
worse, outside the vault entirely), the file syncs but the
Mockingbird watcher only looks under `inbox/`. Nothing will happen.

iOS Files **remembers your last destination**, which is a UX win
for repeat use but means: **if you ever save somewhere wrong, the
"remembered" destination changes to that wrong place and stays
wrong until you manually re-navigate to `inbox/`**. If captures
stop appearing on the desktop, this is the first thing to check.

### No silent zero-tap variant

The "Ask Where to Save" toggle is intentionally **ON** in v1, which
adds one confirmation tap per capture. The silent variant (toggle
OFF) would require the vault to live inside iCloud Drive, which
ADR 0046 explicitly does not do (Obsidian Sync owns the vault
transport).

If you really need zero-tap capture, the workaround is to bind a
**second** Shortcut to a different trigger that does e.g. "Save to
Notes" instead of "Save File" — but that file won't reach
Mockingbird. Better to live with the one tap and keep the vault
clean.

### Obsidian Mobile must be installed, signed in, and have opened the vault at least once after install

Even with the "Open App" action, the Shortcut needs an Obsidian Mobile
instance that knows about the `mockingbird-vault` and is signed in to
Obsidian Sync. If Obsidian Mobile has never opened the vault (e.g.
fresh install on a new phone), foregrounding it via the Shortcut just
drops you on the vault-picker screen — the file save still landed,
but Sync isn't going to push it until the vault is actually opened.
Open the vault manually once after install, then the Shortcut works
as described from then on.

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| Saved a file but it never appeared on desktop | (a) Obsidian Sync on phone is paused or unreachable; (b) Mockingbird mobile sync toggle on desktop is OFF; (c) file landed somewhere other than `inbox/`; (d) file is >5 MB | Check Obsidian Mobile → Settings → Sync (look for green check); check Settings → Mobile on desktop; verify the file is actually under `inbox/` via Obsidian Mobile file tree; check the file size on disk |
| Shortcut crashes when launched | iOS Shortcut runtime bug | Force-close the Shortcuts app and reopen. If it keeps happening, delete and rebuild the Shortcut (only takes a minute) |
| Record Audio doesn't capture any sound | First-run mic permission denied | Settings → Privacy & Security → Microphone → Shortcuts → enable. Trigger the Shortcut again |
| Save File picker is empty / can't find vault | Obsidian Mobile hasn't initialized the vault on disk yet | Open Obsidian Mobile, open `mockingbird-vault`, wait for sync to settle. Then try the Shortcut again |
| Audio quality sounds different after editing the Shortcut | Old iOS 14-era bug where toggling quality back and forth could lose the binding | Toggle Audio Quality off and back to Low. Test with a 5-second recording before relying on it |
| Recording saved but takes 5-15 minutes to appear on desktop | The Shortcut is missing the **Open App** action (Action 3). Without it, Obsidian Mobile stays backgrounded and doesn't notice the saved file in its sandbox until iOS schedules a BGAppRefresh window for it. | Edit the Shortcut: tap **Add Action** → search **Open App** → add it as the final step → configure App = Obsidian. Tap Done. Next capture should land on the desktop within ~30-60 s. |

If something else goes wrong, the **Obsidian Sync log** is the
fastest way to separate phone-side from desktop-side problems:

- **In Obsidian (on phone OR desktop):** Settings → Sync → `…`
  overflow menu → **Show sync log**. This shows server-side
  timestamps for every file event with a device tag (e.g. `[iPhone]`,
  `[Dell-XPS]`) and event type (Created, Updated, Server pushed,
  etc.).
- Look for a `[iPhone] Created` (or `Updated`) line matching your
  recording's filename around the time you triggered the Shortcut.
  - **No such line** → the file never left the phone. Obsidian Mobile
    didn't see it (missing Open App action? saved outside `inbox/`?)
    or Sync is paused / unreachable on the phone.
  - **Line present** → the file made it through Obsidian Sync's
    server. Check the desktop's Sync log for the corresponding
    receive event; if it's there but the file didn't get transcribed,
    check Mockingbird's desktop log (Settings → About → Open log
    folder) for inbox watcher activity.

This three-step chain (phone Sync log → desktop Sync log →
Mockingbird desktop log) covers the vast majority of "my recording
never appeared" reports.

---

## Why this design

A short note on the choices that look weird at first, in case you're
ever tempted to "improve" the Shortcut and want to know why each knob
landed where it did. (Full reasoning is in ADR 0046 §8 and §9.)

- **"Ask Where to Save" is ON, not OFF.** The silent variant requires
  iCloud-Drive-hosted vaults, which we do not do (Obsidian Sync owns
  the vault transport). The one-tap confirmation cost is genuinely
  small once iOS remembers the destination, and it removes a whole
  category of "I thought it saved but it didn't" surprises that the
  iCloud + sandboxing model creates. Variant 1 from the ADR §8 POC
  iteration log.
- **Audio quality is Low, not Normal/High.** The Obsidian Sync
  Standard tier caps individual files at 5 MB. Low gives you ~20 min
  of recording per file; Normal gives you ~5 min; High gives you
  ~2.5 min. For voice memo / dictation use, Low is more than enough
  quality — Whisper transcribes Low-quality AAC essentially as well
  as High.
- **No `.json` sidecar.** An earlier ADR §9 design had the Shortcut
  write a tiny sidecar JSON next to the audio so the desktop could
  detect "sidecar arrived but audio never did → silent-skip
  warning". The sidecar was **descoped at ADR 0046 Accept** because
  it required a third Shortcut action and the strict
  two-built-in-actions lock-in was judged more valuable than the
  early-warning signal. The fallback is the reconciliation scan
  (which detects missing files eventually) plus the up-front user
  copy in this doc. If POC use shows the silent-skip gap matters in
  practice, the sidecar mechanism can be re-added as a Shortcut v2;
  this is tracked as `mb-0uqb`.
- **The third action IS "Open App: Obsidian", not a fourth action
  for "show me the file URL after save".** The latter would add
  user-visible setup surface (one more place to misconfigure) for
  marginal benefit. The Open App action, by contrast, is a hard
  requirement — not a polish step. The ADR 0046 Iter 3 Wave 0 spike
  empirically observed a ~5 minute lag without it (Round 4b); with
  it, latency is ~30-60 s. Findings doc:
  [`docs/spikes/iter3-sync-layer-findings.md`](../spikes/iter3-sync-layer-findings.md).
