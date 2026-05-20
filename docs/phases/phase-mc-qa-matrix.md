# Phase MC — Hands-on QA Matrix (HUMAN-IN-LOOP)

**Status:** template — fill in the **Actual** and **Pass/Fail** columns
during a live QA pass. The template itself is sealed by Wave 4
(bd `mb-pdv.18`); the data captured by running it lives in the
git-tracked file plus screenshots / log excerpts under
`docs/qa-runs/phase-mc-<YYYY-MM-DD>/` (create the folder as needed).

**Run order matters.** Scenarios 1–3 establish baseline behaviour
across the three sources. Scenario 4 stresses the chunker and merge
path. Scenario 5 is the overnight endurance test — start it last so
nothing else competes for the laptop. Do NOT shuffle.

> 🐶 Bernard note: this template is the only documented end-to-end
> verification path for the meeting subsystem until Wave 6 lands the
> five `mc-*` judges. Until then, **the QA pass IS the test**. Take it
> seriously; if a scenario fails, file a `bd create … --type bug
> --priority 0` issue and STOP the QA pass — don't push through.

---

## Pre-flight checklist (run once before scenario 1)

Stop and re-check if any of these fail.

- [ ] `git tag --list "phase-mc-*"` shows Wave 4 sealed (or you're
      running this from a Wave 4 working branch). If Wave 5/6 has
      shipped, see the "Re-running post-judges" note at the bottom.
- [ ] `git status` is clean (or has only intentional WIP — no stale
      migrations).
- [ ] `cargo run --release` boots the Tauri app cleanly. Watch the
      console for `meeting_runtime: probe ok` and
      `dictation_runtime: hotkey installed` log lines — both
      subsystems must come up.
- [ ] Right-Alt dictation still works (smoke test: hold Right-Alt
      while focused on Notepad, speak one sentence, release). This is
      the `mc-dictation-untouched` invariant. If dictation is broken,
      STOP — the meeting subsystem regressed the hotkey driver and
      that's a P0.
- [ ] Audio I/O sanity: open Windows Sound settings → Recording →
      confirm your mic is the default capture device, and a
      loopback-capable device exists (it always does on Win10/11;
      `MMDevice` enumeration finds it). Make a note of which one.
- [ ] Disk: at least 2 GB free under
      `%APPDATA%/Mockingbird/audio_blobs/`. The 10-min stress at
      32 kHz mono PCM is ~38 MB; the overnight idle test writes
      nothing if no recording is active, but be paranoid.
- [ ] Close Slack, Discord, Zoom, Teams. They all grab the mic on
      startup and confuse the source-probe. Re-open them only if a
      scenario calls for it.
- [ ] Open Task Manager → Performance → Memory pane. You'll diff the
      working-set delta at the start and end of scenario 5.

**Pre-flight ran on:** `____-__-__ __:__` by `__________`
**Build hash:** `git rev-parse --short HEAD` → `_______`
**Working-set baseline at app boot (idle, no recording):** `____ MB`

---

## Scenario 1 — 60 s mic-only meeting

**Hypothesis:** mic-only path writes `formatted_mic`, leaves
`formatted_sys` NULL, persists a single audio blob.

### Steps

1. Open the Meetings page in the main window (sidebar → Meetings).
2. Source selector: **Microphone only**.
3. Click **Start meeting**.
4. Speak naturally for 60 s. Suggested script (read aloud, no need
   to memorize): _"This is a phase MC QA test, scenario one,
   microphone only. I am verifying that the deterministic formatter
   produces speaker-labelled lines and that no LLM call happens on
   the critical recording path. The current time is roughly noon."_
   Then count slowly: "one, two, three, … thirty." Then pause 5 s
   (deliberate silence — exercises the VAD-or-equivalent boundary).
5. Click **Stop meeting**.
6. Wait for the persist toast: **"Meeting saved"** (or the equivalent
   i18n string in `ui/src/i18n/en.json` → `meetings.saved`).

### Expected — fill in **Actual**

| Check | Expected | Actual | ✓ / ✗ |
|---|---|---|---|
| Toast on start | "Recording…" indicator visible | | |
| Toast on stop | "Meeting saved" within 5 s | | |
| Row in DB (see SQL §A) | `status = 'complete'` | | |
| `formatted_mic` | non-empty, ≥3 lines | | |
| `formatted_sys` | NULL | | |
| `formatted_merged` | NULL or `== formatted_mic` (single source) | | |
| Audio blob | file at `%APPDATA%/Mockingbird/audio_blobs/<uuid>.wav` exists, > 0 bytes | | |
| Critical-path invariant | `llm_pass_runs` table has **zero** rows for this `meeting_uuid` | | |
| Detail page render | Open meeting in UI → transcript visible, scrollable | | |
| Export markdown | Click **Export** → file written; opens cleanly in any markdown viewer | | |
| Copy to clipboard | Click **Copy** → paste in Notepad → matches the rendered markdown | | |

**Verdict:** ☐ Pass ☐ Fail
**Notes / screenshots / log excerpts:**

---

## Scenario 2 — 60 s system-only meeting (YouTube tab)

**Hypothesis:** loopback-only path writes `formatted_sys`, leaves
`formatted_mic` NULL.

### Steps

1. Open a YouTube tab. Pick a video with **clear single-speaker
   English** (a recorded talk works better than music; e.g. a
   conference keynote). Have it queued but NOT yet playing.
2. **Mute the mic** at the Windows level (Sound settings → Input →
   Microphone → Mute). This is the cleanest test of system-only;
   if the mic is hot, ambient room noise contaminates the test.
3. Source selector: **System audio only**.
4. Click **Start meeting**.
5. Hit Play on the YouTube tab. Let it run 60 s.
6. Click **Stop meeting**. Unmute your mic.

### Expected — fill in **Actual**

| Check | Expected | Actual | ✓ / ✗ |
|---|---|---|---|
| Toast on stop | "Meeting saved" | | |
| Row in DB (§A) | `status = 'complete'` | | |
| `formatted_mic` | NULL | | |
| `formatted_sys` | non-empty | | |
| Speaker label | lines prefixed `[System]` (or whatever the formatter renders) | | |
| Detail page | renders system transcript only — no empty mic pane | | |

**Verdict:** ☐ Pass ☐ Fail
**Notes:**

---

## Scenario 3 — 60 s Both, you-and-a-podcast talking over each other

**Hypothesis:** dual-channel merge interleaves by `t0_ms`, both
columns populated.

### Steps

1. Queue a podcast or talk clip on YouTube — same single-speaker
   English. Unmute your mic.
2. Source selector: **Both (mic + system)**.
3. Click **Start meeting**.
4. Hit Play on the YouTube tab. **Immediately** start talking over
   it. Say: _"Scenario three, both sources, I am deliberately
   talking over the podcast to verify the merge interleaves by
   timestamp. Three, two, one."_ Then pause and let the podcast
   speak for ~10 s. Then talk over it again. Repeat for 60 s.
5. Click **Stop meeting**.

### Expected — fill in **Actual**

| Check | Expected | Actual | ✓ / ✗ |
|---|---|---|---|
| Row in DB (§A) | `status = 'complete'` | | |
| `formatted_mic` | non-empty | | |
| `formatted_sys` | non-empty | | |
| `formatted_merged` | lines from BOTH channels, **strictly non-decreasing** by `t0_ms` | | |
| Speaker labels | `[Mic]` and `[System]` (or equivalent) BOTH present | | |
| Eyeball | mic lines appear roughly where you spoke; system lines roughly where the podcast spoke (no obvious cross-attribution) | | |
| `mc-two-channel-merged` judge | (Wave 6 only — leave blank if pre-judges) | | |

**Verdict:** ☐ Pass ☐ Fail
**Notes:**

> 🐶 If `formatted_merged` is just mic-then-system (block-concatenated),
> that's a merge-strategy bug, not a transcript bug. File it as P0 —
> the `mc-two-channel-merged` judge will fail on this in Wave 6.

---

## Scenario 4 — 10 min mic-only stress

**Hypothesis:** chunker emits ~20 chunks, no OOM, persisted size sane,
stitching loss-less across chunk boundaries.

### Steps

1. Open Task Manager → Details → find the `mockingbird.exe` PID.
   Note **working-set start:** `____ MB`.
2. Source selector: **Microphone only**.
3. Click **Start meeting**.
4. Talk for **10 minutes straight**. Hard. Tips for sustaining
   monologue without going hoarse:
   - Read aloud from any technical doc (this very file works).
   - Pause occasionally — silence is fine, the chunker should
     handle it. Don't artificially fill.
   - Vary cadence — this stresses the chunker boundaries differently
     than a metronome would.
5. Hit Stop.
6. Watch the **persist toast** time-to-fire from stop-click. If it
   takes > 30 s, that's a flush-path bug — file it.
7. Note **working-set at finish:** `____ MB`.

### Expected — fill in **Actual**

| Check | Expected | Actual | ✓ / ✗ |
|---|---|---|---|
| No crash, no error toast during recording | quiet UI | | |
| Persist toast within 30 s of Stop | | | |
| Row in DB (§A) | `status = 'complete'` | | |
| `chunk_count_mic` | **20 ± 2** (depends on silence; 1 chunk per ~30 s nominal) | | |
| `chunk_count_sys` | 0 (mic-only) | | |
| `formatted_mic` line count | reasonable (≥40 lines) | | |
| Stitching audit (§B) | NO duplicated phrase or dropped word at chunk boundaries — eyeball the lines around `t0_ms = 30000, 60000, 90000, …` | | |
| Audio blob size | ~38 MB for 10 min @ 32 kHz mono PCM (±10%) | | |
| Working-set delta | < 200 MB growth over the 10 min | | |

**Verdict:** ☐ Pass ☐ Fail
**Notes:**

> 🐶 The "no duplicated phrase / dropped word at chunk boundaries"
> check is the `mc-long-form-stitched-losslessly` judge in disguise.
> If you find a duplication or drop, paste the offending lines into a
> bug + LESSONS entry. The fix is usually an overlap-window tweak,
> NOT a rewrite.

---

## Scenario 5 — 4-hour idle endurance (overnight)

**Hypothesis:** no memory leak in the meeting runtime when idle (i.e.
no recording in flight). Working-set growth < 200 MB over 4 h.

### Steps

1. Make sure no scenario 1–4 recording is in flight. Stop any
   active meeting.
2. Note **working-set at t=0 (idle baseline after scenarios 1–4):**
   `____ MB` and timestamp `____:__`.
3. Leave the app running, **window visible** (so the webview tick
   isn't suspended by Windows). Close all unrelated apps. Plug in
   the laptop.
4. **Disable** Windows sleep + display sleep for the duration
   (Settings → System → Power → "Never" for both). Re-enable
   afterwards.
5. Walk away. Sleep. Touch nothing.
6. Return ≥ 4 hours later. Note **working-set at t=4h:** `____ MB`
   and timestamp `____:__`.
7. Bonus: start a 60 s mic meeting NOW. Does it still work? If the
   meeting subsystem has decayed (e.g. probe returns no devices,
   start fails), file P0.

### Expected — fill in **Actual**

| Check | Expected | Actual | ✓ / ✗ |
|---|---|---|---|
| Working-set delta | **< 200 MB** over the 4 h | | |
| App still responsive | sidebar nav clicks, no UI hang | | |
| Post-idle meeting (step 7) | starts + stops + persists like scenario 1 | | |
| Log file (`%APPDATA%/Mockingbird/logs/`) | no `ERROR` lines from `meeting_*` modules during the idle period | | |

**Verdict:** ☐ Pass ☐ Fail
**Notes:**

> 🐶 If the working-set creeps but you can't pin which subsystem,
> drop a `tracing-tracy` or `heaptrack` session into a follow-up
> bd issue. Don't fix it inside this QA run.

---

## Appendix A — SQL probes (DB at `%APPDATA%/Mockingbird/mockingbird.sqlite`)

```sql
-- Most recent meeting (per scenario, after Stop):
SELECT uuid, status, source, started_at, ended_at,
       length(formatted_mic)    AS mic_len,
       length(formatted_sys)    AS sys_len,
       length(formatted_merged) AS merged_len,
       chunk_count_mic, chunk_count_sys
FROM meetings
ORDER BY started_at DESC
LIMIT 1;

-- Confirm critical-path invariant (zero LLM rows on the recording path):
SELECT COUNT(*) AS llm_rows_on_critical_path
FROM llm_pass_runs
WHERE meeting_uuid = (SELECT uuid FROM meetings ORDER BY started_at DESC LIMIT 1)
  AND started_at <= (SELECT ended_at FROM meetings WHERE uuid =
                     (SELECT uuid FROM meetings ORDER BY started_at DESC LIMIT 1));
-- Expected: 0. Any LLM rows MUST be after `ended_at`.

-- For scenario 3, inspect the merge ordering:
SELECT segment_index, channel, t0_ms, t1_ms, substr(text, 1, 60) AS preview
FROM meeting_segments
WHERE meeting_uuid = ?
ORDER BY t0_ms, segment_index;
```

Open a SQLite shell (`sqlite3 path/to/mockingbird.sqlite`) or use
DBeaver / TablePlus / whatever you have. Don't UPDATE — these tables
follow the **raw-data-is-immutable** principle (binding rule #1 in
AGENTS.md).

---

## Appendix B — Stitching audit (scenario 4)

For the 10-min stress, eyeball the formatted-mic transcript around
known chunk boundaries. Default chunker emits a chunk every ~30 s
with 2 s overlap, so look at the lines whose `t0_ms` is near
`30000, 60000, 90000, 120000, 150000, 180000, 210000, 240000, 270000,
300000, 330000, 360000, 390000, 420000, 450000, 480000, 510000,
540000, 570000`.

**Pass criteria:**
- No phrase appears twice in a row across the boundary.
- No mid-sentence drops (a sentence that ends abruptly in chunk N
  and resumes incomplete in chunk N+1).
- Punctuation flows naturally across the boundary (the deterministic
  formatter handles this).

If a duplication or drop is found, copy the surrounding ~10 lines
into the **Notes** of scenario 4 with the offending `t0_ms` value,
and file a bd P0 bug citing `mc-long-form-stitched-losslessly`.

---

## Appendix C — What "Fail" means

Failing a scenario means **STOP the QA pass**, file a P0 bug, and do
NOT proceed to the next scenario until the bug is triaged. The
scenarios build on each other (later ones assume earlier paths work);
a fail in scenario 2 invalidates scenarios 3 onwards.

The exception is scenario 5, which is independent of 1–4 and can be
run in isolation if needed (e.g. a hotfix shipped and you want to
re-verify endurance without re-running the recording paths).

---

## Appendix D — Re-running post-judges (Wave 6+)

Once `phase-mc-complete` is tagged and the five `mc-*` judges have
landed, this QA matrix becomes a **regression check** rather than the
sole verification path. Re-run it after any non-trivial change to:

- the chunker (`src-tauri/src/meetings/chunker.rs`)
- the formatter (`src-tauri/src/meetings/formatter.rs`)
- the runtime (`src-tauri/src/meetings/runtime.rs`)
- migrations 011+ touching meeting tables

For non-meeting changes (dictation, injection, learning), only the
pre-flight `mc-dictation-untouched` smoke test is required.

---

## Run log (append-only — one section per QA pass)

> 🐶 Each completed QA pass appends a `### YYYY-MM-DD run` section
> below with the filled-in Actuals + verdicts. Don't overwrite prior
> runs — they form the regression history.

<!-- First run goes here. Format:

### 2026-MM-DD run (Dustin, build $HASH)

- Pre-flight: pass
- Scenario 1: ☐ Pass — notes:
- Scenario 2: …
- Scenario 3: …
- Scenario 4: …
- Scenario 5: …
- Bugs filed: bd-xxx, bd-yyy
- LESSONS appended: yes/no

-->
