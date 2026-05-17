# LESSONS

Append-only notes from prior iterations. Each entry: date, phase tag,
short title, and the non-obvious finding. Search this before starting
a new iteration in the same area.

Format:
```
## YYYY-MM-DD [phase/iteration] short title
- Context: what we were doing
- Finding: the non-obvious thing
- Action: what to do differently next time
```

---

## 2026-05-17 [phase5-postship-9-followup] stale UI bundle embedded in release binary

- **Symptom:** shipped Wave 2 backend + UI source changes, ran my
  usual `cargo-with-cuda.ps1 build --release`, launched, and the
  Modes page showed the OLD layout — Normal/Verbose/Fragment in
  the transcription section + Casual/Formal incorrectly grouped
  under AI command modes alongside Rewrite/Expand/Summarize. The
  database had the right rows (`prompt_id=10`, `model=qwen2.5:7b`,
  `temperature=0.1` in the orchestrator log), the IPC returned
  them, but the partitioning logic on the JS side was using the
  pre-Wave-2 `TRANSCRIPTION_SLUGS = ["normal", "verbose",
  "fragment"]` allowlist. Stale frontend.
- **Root cause:** Tauri's `beforeBuildCommand`
  (`"npm --prefix ../ui run build"` in `tauri.conf.json`) ONLY
  runs under `cargo tauri build`. Plain `cargo build --release`
  skips it. So when I edited `ui/src/lib/types.ts` + friends and
  then ran `cargo-with-cuda.ps1 build --release`, the Rust binary
  re-linked against whatever `ui/dist/` was sitting on disk —
  which was the bundle from the last `cargo tauri build` or `npm
  run build`. New TypeScript sources, old bundled output.
- **Second trap on top of the first:** even after running
  `npm run build` manually to refresh `ui/dist/`, a subsequent
  `cargo build --release` didn't re-link the binary, because
  tauri-build's `cargo:rerun-if-changed=` directives don't always
  detect content changes inside `frontendDist`. The build said
  "Finished" instantly but the .exe timestamp didn't move. Had
  to `touch src-tauri/src/lib.rs` to force a re-link that
  re-embedded the assets.
- **Fix (long-term):** new `scripts/build-release.ps1` wraps the
  full three-step dance: `npm run build` + touch `lib.rs` +
  `cargo build --release`. Use this any time UI sources changed.
  `cargo-with-cuda.ps1 build` (without the wrapper) is fine for
  pure-backend iteration.
- **Patterns burned in:**
  - **"If the schema/IPC says one thing and the UI shows another,
    suspect a stale bundle BEFORE suspecting a logic bug."** The
    backend logs (`orchestrator config resolved mode=normal
    prompt_id=10`) were the smoking gun — they proved the
    backend was correct; therefore the discrepancy had to be
    upstream of the bundle.
  - **"Build tools that conditionally run hooks are a footgun."**
    Tauri's `beforeBuildCommand` is meant to be helpful but its
    silent skip on plain `cargo build` creates a class of bug
    that's invisible at build time and surfaces only when the
    user opens the page. A wrapper script that ALWAYS does the
    full dance removes the conditional.
  - **"Trust your eyes, not the build success message."** Both
    `npm run build` and `cargo build --release` reported success.
    The bug was in the gap between them.

---

## 2026-05-17 [phase5-postship-9] three focused modes + 7B default — Wave 2 of ADR 0022

- **The Santa-list regression** (2026-05-17 seventh smoketest):
  with the preprocessor in place, raw input `"I'm making a list
  of things and checking it twice. And I'm going to find out
  who's naughty or nice. And to do that I need to know these
  important things. Who has stolen something? Who has lied to
  their friends? Who has lied to their mom?"` came out cleaned as
  just three bulleted questions — the four sentences of preamble
  were dropped. The v3 prompt explicitly said "do not summarize"
  but the 3B-q4 model decided the questions were "the point"
  and editorialized everything else away.
- **Root cause is attention budget, not prompt wording.** The v3
  prompt was ~4.8 KB. A 3B-q4 model has finite attention; rules
  buried below the first KB of a long prompt get statistically
  ignored. **Load-bearing rules must go FIRST.** Same lesson as
  Wave 1's "move work out of the LLM" — every byte of prompt the
  model doesn't have to attend to is attention budget freed for
  the actual judgment work.
- **"Preserve every sentence" must be NON-NEGOTIABLE and FIRST.**
  All three Wave-2 prompts (casual_v1, normal_v4, formal_v1)
  start with a section literally titled
  `## NON-NEGOTIABLE RULES` whose first rule is
  `**PRESERVE EVERY SENTENCE.**` Followed by the reason: cleanup
  ≠ summarization, every sentence the speaker said matters. Then
  the rest of the prompt fits in ~1 KB. With this structure the
  rule lives in the highest-attention region of the context
  window. Small model can't miss it.
- **Bigger model when content fidelity matters.** qwen2.5:7b-q4
  follows instructions markedly better than 3b-q4 — the model-size
  effect on rule-following is real and large. The 7B cold-loads
  in ~17 s on the user's RTX-2060/6 GB rig (with Whisper-large
  already resident, ~1 GB free) and runs warm calls in ~3 s vs
  ~1.5 s for 3B. **2× latency for ~10× rule-following reliability
  is a great trade** for normal/formal modes. Casual stays on 3B
  because Wave 3 will skip the LLM entirely for short casual
  utterances anyway.
- **REQUEST_TIMEOUT must bracket the worst-case cold-load.** Old
  value 30 s covered 3B cold-load (~6 s) with 5× headroom. Same
  budget on a 7B cold-load is only 1.2× headroom; one moment of
  Whisper-Ollama-Tauri startup contention can blow through it.
  Bumped to 60 s. Steady-state warm calls pay nothing extra.
  Lesson: when changing default model SIZE, recheck every
  timeout downstream of it.
- **Migration `ON CONFLICT DO UPDATE WHERE` is exactly the right
  tool for soft state migration.** Migration 008 needed to rescue
  any user whose `dictation.active_mode_slug` setting pointed at
  the now-disabled `verbose` or `fragment`. The SQLite idiom:
  ```sql
  INSERT INTO settings VALUES ('dictation.active_mode_slug', 'normal')
  ON CONFLICT(key) DO UPDATE SET value = 'normal'
  WHERE value IN ('verbose', 'fragment');
  ```
  This is one statement that handles all four cases: row absent →
  insert with 'normal'; row present + verbose/fragment → update
  to 'normal'; row present + something else → WHERE excludes, no
  update; row present + already 'normal' → WHERE matches but
  UPDATE is a no-op. No `if exists` ceremony, no client-side
  branching.
- **Shared `<datalist>` is the DRY way to wire N comboboxes from
  one source.** The Modes editor has one model `<input>` per
  mode card, but all of them autocomplete from the same
  installed-models list. Browsers de-dup automatically when
  multiple inputs reference the same `list=` id, so the cleanest
  pattern is: render the `<datalist id="...">` ONCE at the top
  of the page, point every input at it via `list=`. The shared
  ID lives in a const at the top of the file (`MODELS_DATALIST_ID`)
  so producer + consumer can't drift.
- **Patterns burned in:**
  - **"Load-bearing rules go first." This applies to prompts,
    docstrings, function signatures, and anything else attention-
    budgeted. The most important rule deserves the highest-
    attention slot — front of the section, front of the line,
    bold/uppercase if the medium allows.
  - **"Smaller prompts > more rules."** The v3→v4 prompt shrank
    from 4.8 KB to 1.5 KB despite ADDING the preservation rule.
    How? Removed every rule the deterministic preprocessor now
    handles (fillers, punctuation, capitalization, layout cues).
    The LLM only sees the SHAPE of cleanup work it actually has
    to do. Same architectural move as Wave 1 — push work down
    the stack so the LLM-shaped portion shrinks.
  - **"Test the regression you fixed."** Each Wave-2 prompt's
    `## Examples` section contains the exact Santa-list utterance
    that triggered the regression. The example shows the model
    the right answer for the input that previously broke. Next
    test against the same input is a high-confidence pass.

---

## 2026-05-17 [phase5-postship-8] deterministic preprocessor — Wave 1 of ADR 0022

- **Context:** sixth-smoketest screenshot showed the LLM emitting
  `` ```ery keyboard supplies: `` (hallucinated intro), wrapping
  output in fences explicitly forbidden by the prompt, and dropping
  the speaker's framing. Cleanup latency was 3198 ms — 70 % of
  end-to-end. Root cause: asking a 3B-q4 model to do 100 % of
  cleanup inside a 5 KB prompt blows its attention budget.
- **Architectural fix:** new `cleanup/preprocessor.rs` runs BEFORE
  the LLM call. Handles the rule-shaped 80 % (fillers, stutters,
  self-corrections, verbal punctuation/quote/layout cues,
  capitalisation, terminal punctuation) in ~5 ms. The LLM now sees
  pre-cleaned text and is asked only to do judgment work in later
  waves. See ADR 0022 for the full pipeline rationale.
- **The `regex` crate trapped me twice** while porting the rule
  table from a 'this is just regexes' first pass:
  - **No lookaround.** I'd written `(?:^|\s)cue(?=\s|$|[.,!?])`
    for standalone-token matching of multi-word verbal cues. Rust's
    `regex` is the safe non-backtracking implementation and
    explicitly rejects `(?=...)` / `(?<=...)`. Fix: consume the
    boundary instead of looking past it — `(?:^|\s)cue\b\s?`. \b
    IS supported because it's zero-width-but-not-backtracking-y.
    Use `fancy-regex` only if you genuinely need lookaround; the
    safe crate is faster and you can usually refactor.
  - **No backreferences.** Stutter collapse ("the the the" → "the")
    naturally wants `\b(\w{1,4})(?:\s+\1\b){1,}` — match a word,
    then re-match the same word. `\1` isn't supported by `regex`
    either. Fix: do it as a manual `split_whitespace` token walk
    with last-token-comparison. O(n), comparable cost to a regex
    pass, and arguably more readable.
- **The ordering trap.** First version put layout-cue rendering
  (which inserts `\n\n`) BEFORE stutter collapse (which calls
  `split_whitespace`). `split_whitespace` eats newlines, so every
  inserted paragraph break vanished. Two tests failed loudly with
  `"first thought second thought"` where I expected the break. The
  invariant is now pinned in the `process()` docstring: **any pass
  that injects newlines MUST run AFTER any pass that uses
  `split_whitespace`.** Subtle, easy to violate, worth a comment.
- **Tier-2 filler stripping is load-bearing on prosody.** First
  version of the regex stripped "you know" at any sentence start.
  That broke `keeps_you_know_when_not_bounded` ("You know nothing
  about it" → "Nothing about it"). The fix: ONLY strip when the
  speaker also said a comma ("You know, it's true" has the
  prosodic marker; "You know nothing" doesn't). The trailing comma
  is the SOLE differentiator between filler and content. Same
  rule for `like`, `basically`, etc. — strip only when prosody
  (STT-rendered commas) flags them.
- **DLL-load issue forced a workaround:** `cargo test --lib` on
  this box dies with STATUS_ENTRYPOINT_NOT_FOUND in ntdll because
  ORT/CUDA DLLs aren't on the test binary's PATH at process load.
  Setting PATH + ORT_DYLIB_PATH env vars didn't help (the wrong
  ABI version was being picked up). Workaround: created a tiny
  throwaway crate at `C:\Users\dboyd\AppData\Local\Temp\preproc_test\`
  containing only the preprocessor source + regex dep, and ran
  `cargo test --lib` there. Found two real bugs in two iterations.
  Worth keeping the recipe documented for future pure-rust modules.
- **Patterns burned in:**
  - **"Move the load-bearing work out of the LLM, into the
    deterministic layer."** Every byte of prompt the LLM doesn't
    have to attend to is attention freed up for the actual
    judgment. Wisprflow's 'just-knows' magic is almost certainly
    not a better model; it's a fatter rule layer in front of it.
  - **Provenance suffix > new column.** Encoding the preprocessor
    version into the existing `model_used` string
    (`qwen2.5:3b-q4+preproc@v1`) gives full provenance without a
    migration. ADR 0008's append-only-migration invariant prefers
    schema stability over schema purity.
  - **Test rigs beat test runners when the runner is broken.**
    The throwaway-crate trick is a 5-minute reliable test loop
    even when the main crate's test infrastructure is unwell. If
    the module under test has bounded dependencies it's worth it.

---

## 2026-05-17 [phase5-postship-7] clipboard snapshot crashed the process when a bitmap was on the clipboard (STATUS_HEAP_CORRUPTION)

- **Context:** First dictation after `phase5-postship-6` ship. User
  said "it crashed when recording the test vtt". Process gone. Log
  ended mid-paste with `inject begin decision=Proceed(Paste)
  text_len=73 focus_drifted=false` and no matching `inject end`.
  No Rust-level error — process died beneath the tracing layer.
- **Diagnosis:** Windows Event Log Application Error showed
  `Faulting module name: ntdll.dll, Exception code: 0xc0000374`.
  That's `STATUS_HEAP_CORRUPTION` — ntdll's heap-validation
  tripwire fires when something has scribbled on the process heap
  metadata. Process death is immediate; tracing never gets to log
  it.
- **Root cause:** `injection/paste.rs::copy_format_bytes` iterated
  every format `EnumClipboardFormats` returned and called
  `GlobalSize(handle)` / `GlobalLock(handle)` on each one. The
  comment in the code claimed "GlobalSize returns 0 if not
  HGLOBAL" — **that's wrong**. Per MS docs, calling `GlobalSize` on
  a handle that wasn't allocated by `GlobalAlloc` is undefined
  behaviour. `CF_BITMAP` returns an `HBITMAP`, `CF_ENHMETAFILE`
  returns an `HENHMETAFILE`, etc. `GlobalSize` reads what it
  THINKS is the moveable-memory header at handle's address; on a
  GDI object that's other GDI metadata, and the call either
  scribbles or returns garbage that the next `GlobalLock` writes
  through.
- **Reproduction:** between the user's last successful paste at
  16:18 and the crashing one at 16:47, they took a screenshot for
  the bug report. The clipboard now held `CF_BITMAP` + `CF_DIB`
  alongside the text. Snapshot enumeration tried `CF_BITMAP`,
  passed its `HBITMAP` to `GlobalSize`, corrupted the heap. Next
  allocation tripped the tripwire and ntdll killed the process.
  Verified reproducer: place a bitmap on the clipboard via
  `[System.Windows.Forms.Clipboard]::SetImage`, trigger any paste
  dictation → immediate process death.
- **Fix:** allowlist. New `is_hglobal_format(fmt: u32) -> bool` is
  the gatekeeper for the snapshot loop. Anything not on the
  allowlist is logged at debug + skipped without EVER passing the
  handle to `GlobalSize` or `GlobalLock`. Allowlisted formats are
  the ones MS documents as HGLOBAL-backed: `CF_TEXT`, `CF_SYLK`,
  `CF_DIF`, `CF_TIFF`, `CF_OEMTEXT`, `CF_DIB`, `CF_UNICODETEXT`,
  `CF_HDROP`, `CF_LOCALE`, `CF_DIBV5`. Notably, `CF_DIB` IS
  HGLOBAL (header + pixels in moveable memory) while `CF_BITMAP`
  is NOT (HBITMAP) — they look related but live in different
  storage worlds. Registered formats (`>= 0xC000`) are
  app-defined; docs RECOMMEND HGLOBAL but don't require it, so
  we're conservative until a Phase 9 per-app deny list lands. Net
  effect: user temporarily loses round-trip of bitmaps + custom
  formats around a paste dictation. That's a paper cut; heap
  corruption is a project-ender.
- **Patterns burned in:**
  - **"It returns 0 if X" is a load-bearing claim that needs an
    MSDN citation, not a comment.** The old code had the comment
    `// GlobalSize returns 0 if handle isn't an HGLOBAL` directly
    above the UB call. The comment was wrong and lived for months
    because nobody had a screenshot on their clipboard during a
    dictation. Heap-corruption bugs are silent until they aren't.
  - **Allowlist > sniff-and-skip for FFI handles.** You cannot
    safely "try and recover" with raw Win32 handles — the act of
    asking "are you an HGLOBAL?" requires already treating you as
    one. The only safe move is to know up front. Allowlists are
    the right primitive whenever a wrong guess crashes the
    process.
  - **Crash logs > error logs for diagnosing process death.**
    Standard reflex (`Get-Content app.log -Tail`) doesn't help
    when the kernel kills the process; tracing buffers never get
    flushed. `Get-WinEvent -LogName Application | Where ProviderName
    -like '*Application Error*'` is the right tool. Pinning this
    in the project's smoketest runbook would have saved 10 minutes
    of head-scratching.
  - **A user-friendly bug-report workflow is itself a fuzzer.**
    The crash trigger was a screenshot taken to file the previous
    bug — i.e., the user's act of helping us hit a latent UB the
    test suite never tickled. The interactive-desktop ignored
    tests in `paste.rs` only exercise the happy text-text path;
    they never plant a bitmap and re-enumerate. New unit tests
    pin the full GDI-handle-rejection table so a regression here
    is caught without needing live Win32.

---

## 2026-05-17 [phase5-postship-6] active-mode selector + prompt v3 (preserve list context)

- **Context:** Sixth Dustin smoketest pass. Three pieces of feedback:
  (a) "Everything seems to default to normal mode and I don't know
  how I can change it", (b) the v2 prompt rendered bullets but
  dropped the introductory framing ("list of keyboard supplies" →
  three bare bullets), (c) per-mode hotkey chords (Ctrl+Win,
  Ctrl+Shift+Win, etc.) shown on the Modes page were confusing
  noise — the user wanted to just pick a mode and have Right-Alt
  use it.
- **Two-class mode model.** Mockingbird has two distinct mode
  classes that needed different UX treatments:
   - *Transcription modes* (`normal`, `verbose`, `fragment`): exactly
     ONE is active at a time. Right-Alt always uses the active one.
     The Modes page now shows them as a radio-style selector — each
     card has a "Use this mode" button, the active card gets an
     accent border + "Active" pill, and the per-mode hotkey badge is
     hidden (Right-Alt is global).
   - *AI command modes* (`rewrite`, `expand`, `summarize`): act on
     existing clipboard/selection text via their OWN hotkeys when
     enabled. They keep the legacy enable/disable toggle + hotkey
     badge. They are NOT eligible to be set as the active mode —
     there's no audio input concept to attach them to.
  The categorisation lives in `ui/src/lib/types.ts` as a fixed
  `TRANSCRIPTION_SLUGS` const + an `isTranscriptionSlug` predicate,
  mirrored on the Rust side in
  `src-tauri/src/commands/active_mode.rs`. The Rust
  `set_active_mode` IPC rejects any slug outside that list — the UI
  can't accidentally point Right-Alt at `summarize`.
- **Storage: settings table, not a new schema column.** The active
  mode is a single string under `settings.dictation.active_mode_slug`.
  No migration needed (the orchestrator falls back to `"normal"` if
  the row is missing). One source of truth, no cache to invalidate.
  Per-session lookup is one indexed-PK query — negligible vs.
  STT/cleanup latency. **Net effect: a `set_active_mode` call takes
  effect on the NEXT Right-Alt hold with zero restart/signalling/
  refcount dance.** This is the simplest possible mechanism that
  satisfies the user requirement.
- **Orchestrator: session-pinned mode, not config-time mode.** The
  old `OrchestratorConfig` set `mode_id` / `mode_slug` / `prompt_id`
  once at boot. I added `ResolvedMode { mode_id, slug, prompt_id }`
  plus `SessionState.active_mode: Option<ResolvedMode>`. At
  `start_capture` we resolve fresh and pin for the whole session;
  `complete()` + both `insert_session_row` helpers read from
  `current_mode()` which falls back to `self.config` if no session
  is active. Pinning AT `start_capture` (not at insert) means a
  `set_active_mode` call mid-dictation can't split one session
  across two modes (the cleanup prompt would mismatch the DB-
  recorded `mode_id` and we'd violate provenance). Resolution has
  two graceful fallbacks (poisoned mutex → config; missing settings
  row OR modes lookup fails → config) so the user never loses a
  dictation to a flaky DB read.
- **Prompt v3: "preserve framing" rule.** v2 was too eager to strip
  introductory phrases. The example I shipped in v2 — `"um so make
  a list first thing is apples"` → bare bullets — set the wrong
  precedent. v3 keeps that example (no intro spoken → no intro
  rendered) but adds three NEW examples where the speaker DID name
  the list ("I'm going to put together a list of keyboard supplies")
  and the cleaned output keeps that as a one-line lead-in followed
  by a blank line, then the bullets. ADR 0008 compliant: new file
  `normal_v3.md`, new migration 007 that INSERTs `prompts` row v3
  and repoints `modes.normal.prompt_id` — v1 + v2 stay addressable
  for every historical session that referenced them.
- **Migration test rig.** Added `scripts/test_migrations.py` because
  the Rust test runner has the pre-existing STATUS_ENTRYPOINT_NOT_FOUND
  DLL-load issue (ORT/CUDA path) and won't run unit tests at all on
  this box. The Python rig substitutes prompt-body tokens the same
  way `prompt_loader.rs` does (including the apostrophe escape and
  the leftover-token guard regex), applies every migration
  sequentially to a `:memory:` SQLite, and prints the resulting
  `prompts` + `modes` rows. Reproduced the working state in <1 s.
  Worth keeping in the repo for future migration iteration; pays
  for itself the first time you don't have to wait on a 4-minute
  release build to know whether your SQL is valid.
- **Patterns burned in:**
  - **"Active selection" is a UX problem, not a permissions problem.**
     Enable/disable toggles imply "on means this mode runs in the
     background" — which makes no sense for a transcription mode
     that runs on user-initiated hotkey. Radio-style selection makes
     the contract obvious: ONE is in use, click to switch. Reach
     for the right primitive; don't bend toggles to a single-select
     job.
  - **Resolve config per-session, not per-process.** Boot-time
     config is great for things that physically can't change without
     restart (audio device, model files). For anything the user can
     change from the UI, lookup fresh at use-time + pin for the
     duration of a single in-flight operation. Settings table +
     one indexed query is enough; no need for shared `Arc<RwLock<_>>`
     state or event broadcasters.
  - **The first example in a prompt sets the tone for everything
     the model generates.** v2's only list-example was the
     bare-bullet case. The model interpreted that as "strip
     EVERYTHING and emit bullets". v3 leads with the intro-preserving
     example and demotes the bare-bullet case to example three with
     an explicit "no intro phrase was spoken, so no lead-in is
     invented" annotation. Order + emphasis in few-shot examples
     matter as much as the rule prose.

---

## 2026-05-17 [phase5-postship-5] migration 006 crashed the entire app at boot with cryptic `near 'voice': syntax error`

- **Context:** Fifth Dustin smoketest (sigh). User ran `run-mockingbird.ps1`,
  terminal said "Started in background", but no tray icon, no main window,
  nothing. `Get-Process mockingbird` returned empty — the process was
  dead. The log file showed only `Mockingbird starting` and then
  silence. `database ready` (the next log line) never printed, so the
  crash was somewhere inside `db::apply_migrations`.
- **Finding:** Reproduced the migration outside the app via a tiny
  Python script (`sqlite3.executescript`) and got
  `OperationalError: near "voice": syntax error`. The word "voice"
  appears in the v2 prompt body ("preserve the speaker's voice"). My
  initial assumption — unescaped apostrophe in `speaker's` — was
  wrong; `prompt_loader::sql_escape` correctly doubles all single
  quotes. Real root cause was geometrically worse:

  Migration 006's own `--` comment block included the literal token
  `` `__PROMPT_NORMAL_V2_BODY__` `` as documentation of where the
  body gets substituted. `prompt_loader::substitute_prompt_bodies`
  is a blanket text replace — it doesn't care whether the token sits
  inside a SQL string literal, a `--` comment, or a `/* */` block.
  So the entire v2 prompt body (3.7KB of markdown with embedded
  newlines, apostrophes, fenced code blocks, etc.) got injected into
  the middle of a comment line. The body's first `\n` terminated
  the `--` comment early, and everything from there onward (
  including escaped apostrophes that were `''` inside the intended
  string literal but are now bare unmatched quotes in raw SQL
  context) hit the parser. Hence "syntax error near 'voice'".

  Migrations 003 and 005 dodged this by accident: both refer to the
  token family as `__PROMPT_*_BODY__` (with a literal asterisk), which
  doesn't match any concrete substitution key. I wrote 006's comment
  with the exact token literal because it felt more precise. It was.
  Precisely catastrophic.
- **Action (two-layer fix):**
  1. **Migration 006 comment** rewritten to use `__PROMPT_*_BODY__`
     style (matches the 003/005 precedent), with a multi-line
     `DO NOT` warning explaining what happens if you write the
     exact token. The next person to copy this file gets the
     warning at the same time they get the example.
  2. **Defensive leftover-token guard** in
     `prompt_loader::substitute_prompt_bodies`: after the chained
     `.replace()` calls, scan for any surviving `__PROMPT_` substring
     and panic loudly with the offending token + a hint about the
     two ways this happens (forgot to register, OR wrote it in a
     comment). Costs microseconds at boot; saves the next hour of
     "why does it say 'near voice'" debugging.
- **DB state preserved by SQLite transaction semantics.** Migration
  006 starts with `BEGIN TRANSACTION` and ends with `COMMIT`. When
  the syntax error fired mid-script, SQLite implicitly rolled back
  the open transaction. Verified post-incident: live DB still at
  schema_version=5, prompts table still has only v1 rows, no audit
  trigger noise. **This is exactly why every multi-statement
  migration must be wrapped in a transaction.** Phase 1 made this a
  rule (in 002, 003, 005); 006 honoured it; it saved the user from
  a partially-applied migration that would have required manual
  recovery.
- **Patterns burned in:**
  - **Text-substitution tooling treats source files as opaque
    strings.** Comments do not protect against substitution. If
    the substitution payload can contain syntactically-significant
    characters (newlines, quotes, semicolons), it can break out of
    any host-language construct that was structurally protecting
    the surrounding code. Either:
    (a) make the substitution syntax-aware (parse SQL, only
        substitute inside string literals), OR
    (b) make the token name impossible to confuse with prose (e.g.
        `<<<INSERT_NORMAL_V2_HERE>>>` with rare ASCII), OR
    (c) add a post-substitution sanity check (the path we took —
        cheapest, catches everything blanket-replace can miss).
  - **"Syntax error near X" usually means the parser entered the
    wrong state several tokens earlier.** Don't trust the column
    number. Dump the actual SQL that hit the parser; scan backward
    for the first place the lexer would have changed state
    incorrectly (unterminated string, broken comment, missing
    semicolon).
  - **Silent process death after a single log line is almost always
    a panic before tracing flushes.** First instinct should be
    "reproduce outside the app and capture the real error", not
    "hunt through the source for what runs between those two log
    statements." The reproduction took two minutes; the visual
    inspection would have taken much longer.

---

## 2026-05-17 [phase5-postship-4] History metadata showed `—` for Model / Prompt / Dictionary even though all three were populated in the DB

- **Context:** Fourth Dustin smoketest. Ollama-cleaned dictation worked
  cleanly (Cleaned=689ms, Inject=63ms). History row had different Raw
  vs Cleaned text — proof that LlmCleaner ran. But the Metadata panel
  showed `Model: —`, `Prompt: —`, `Dictionary: —` for all sessions,
  old and new.
- **Finding (instrumented via Python sqlite3 on the live DB):** The
  data WAS in the DB. `transcripts.model_used` = `'qwen2.5:3b-instruct-q4_K_M'`.
  `sessions.prompt_id` = 1. `sessions.dictionary_snapshot_id` = 1.
  Running the EXACT backend query manually in sqlite3 returned
  `('qwen2.5:3b-instruct-q4_K_M', 1, 1)`. So the bug was in how Rust
  read the row, not in what was written.

  Looked at the rusqlite visitor:
  ```rust
  r.get::<_, Option<String>>(0)?,  // model_used: TEXT  → fine
  r.get::<_, Option<String>>(1)?,  // p.version: INTEGER → ERROR
  r.get::<_, Option<i64>>(2)?,     // dict_id: INTEGER  → fine
  ```

  `prompts.version` is INTEGER (schema in migration 003), not TEXT.
  Asking rusqlite to read INTEGER as `Option<String>` returns
  `Err(InvalidColumnType)`. The visitor uses `?`, so the WHOLE
  closure returns Err. The outer code was `.unwrap_or((None, None,
  None))` — silently swallowing the error and returning a tuple of
  all-NULLs. THREE pieces of metadata vanished from the UI because
  of ONE type mismatch in ONE column, and the swallow-the-error
  pattern made it invisible to logs.

  Worst part: `commands/modes.rs` had a five-line comment WARNING
  about this exact pitfall (with the same column!), and used
  `'v' || p.version` in its SQL to force TEXT affinity. The author
  of `commands/sessions.rs` (also me) didn't know about that
  precedent, didn't search for it, and re-introduced the same bug.
- **Action (two fixes):**
  1. **SQL-side:** changed `p.version` → `'v' || p.version` in the
     sessions query. Matches the modes.rs precedent. Produces "v1",
     "v2", … — same shape the UI shows.
  2. **Rust-side:** replaced `.unwrap_or((None, None, None))` with
     `.unwrap_or_else(|e| { tracing::warn!(...); (None, None, None) })`.
     Same fallback behaviour for the user, but the error surfaces in
     logs within seconds instead of producing a mystery UI bug that
     survives three smoketest rounds.
- **Patterns burned in:**
  - `unwrap_or((None, None, None))` for ANY multi-column SQL result
     is a footgun: a single column-type error silently corrupts every
     field in the tuple. Always `unwrap_or_else` with a log.
  - When the same gotcha bites you twice (modes.rs comment, then
     sessions.rs query), the comment is in the wrong place. The fix
     belongs at the boundary that produces the gotcha (DTO/serde
     layer), not at each call site. Future Phase 6 polish: write a
     `Stringy<T>` newtype that auto-stringifies INTEGER columns into
     Option<String>, OR migrate prompts.version to TEXT in a v3
     migration.
  - When the symptom is "backend looks right but UI shows —", the
     bug is almost always in the visitor closure, not the SQL.
     Reproduce the query in `sqlite3` first — if it returns data,
     skip straight to inspecting the row decode.

---

## 2026-05-17 [phase5-postship-4] normal-mode prompt didn't render lists when user said "make a list"

- **Context:** Same smoketest. User dictated "...make a list here.
  First thing is apples and then eggs and then berries." Cleaned
  output was "Here's a list: first thing is apples, then eggs, and
  then berries." — reasonable English but NOT a list. Wisprflow
  (competitor) would render bullets.
- **Finding:** v1 of normal.md was deliberately structure-averse
  ("do not invent structure that isn't implied by the speech"). The
  rule is right for ambiguous cases but too strict when the speaker
  EXPLICITLY says "make a list". Dustin's expectation is "my words
  produce the structure I asked for, in markdown if needed."
- **Action (ADR 0008 compliant):**
  - Did NOT edit `normal.md` in place (initial reflex — caught
     myself before commit). ADR 0008 binds prompts to append-only
     versioning. Editing v1's source file would silently change what
     gets seeded into fresh-install DBs WITHOUT bumping a version,
     breaking provenance for any user whose existing session row
     points to prompt_id=1 with the OLD body.
  - Created `cleanup/prompts/normal_v2.md` with the new body
     (structure cues + 4 worked examples). v1 file stays frozen as
     the on-disk record of what shipped.
  - Added `PROMPT_NORMAL_V2` const + `__PROMPT_NORMAL_V2_BODY__`
     substitution token in `db/prompt_loader.rs`.
  - Created `db/migrations/006_prompt_normal_v2.sql` that INSERTs
     a new prompts row (mode_slug='normal', version=2) and UPDATEs
     modes.normal.prompt_id to point at v2. v1 row stays in the
     prompts table forever; existing session rows pointing at
     prompt_id=1 still resolve to the v1 body for provenance.
  - Registered migration in `db/migrations.rs`; bumped expected
     `schema_version` from "5" to "6" in tests.
- **Pattern (reusable):** the file-naming convention `{mode}.md` for
  v1, `{mode}_v2.md` for v2, etc., scales linearly with prompt edits.
  For phase 6+ when prompt iteration accelerates, consider switching
  to a directory layout: `prompts/normal/v1.md`, `prompts/normal/v2.md`,
  with a build-script that auto-generates the include_str! const list.
  Not worth it for 1-2 edits per phase; revisit at 5+.
- **Whisper aside:** Dustin also noted "I said ums and filler, raw
  doesn't show them at all." Whisper-large-v3 auto-suppresses
  disfluencies before producing the raw transcript — a model-level
  behavior we can't fully disable without switching models. Knobs to
  explore in Phase 6 if requested: `condition_on_prev_tokens=False`,
  smaller model size (medium/small suppresses less), or a custom
  initial prompt. For now: the "raw" we persist is Whisper's output,
  which is already lightly cleaned. The user-facing concept "raw"
  matches Whisper-raw, not microphone-raw. Documented here for
  future me when someone files this as a bug.

---

## 2026-05-17 [phase5-postship-3] inject reported `outcome=Ok` but nothing pasted into Notepad

- **Context:** Third Dustin smoketest. Pipeline ran end-to-end. New
  inject-lifecycle logs (added in postship-2) showed clean handshake:
  `inject begin decision=Proceed(Paste) text_len=23` →
  `inject end injection_latency_ms=67 outcome=Ok`. History row
  persisted with the correct raw + cleaned + injected text. App
  metadata showed `App: Notepad`. But Notepad was empty — no paste.
- **Finding:** ADR 0020 covers focus changes BETWEEN key-down and
  key-up (permissive: inject into key-up app). It does NOT cover
  focus changes BETWEEN key-up and inject. Sequence:
  1. Hold RightAlt with Notepad focused → fg_keydown = Notepad.
  2. Release RightAlt → fg_keyup snapshot captures Notepad.
  3. Cleanup hangs 30s on cold-load Ollama → user gets bored, clicks
     on Mockingbird's main History window to see if anything's
     happening. Mockingbird is now the foreground.
  4. Cleanup finally returns. Injector runs SetClipboardData +
     SendInput(Ctrl+V) against the CURRENT foreground = Mockingbird
     main window. Ctrl+V in our React UI is a no-op (no input field
     focused). Clipboard ops returned success, SendInput returned
     success → outcome=Ok. Nothing actually appeared anywhere
     visible. The injector has NO concept of "the target was
     Notepad; verify before pasting."
- **Action (three layers, all in this commit):**
  1. **Re-snapshot foreground before inject.** New post-cleanup
     check in `dictation::complete()`: re-call `window_ctx.foreground()`
     right before the inject step. If `process_name` differs from
     `fg_keyup.process_name` (case-insensitive, basename only —
     ignore HWND and title to avoid false alarms from window cycles
     and title edits), set `outcome = AbortedFocusChanged` and skip
     the injector call entirely. Raw + cleaned still persist for
     provenance; only the `final` transcript stage is omitted. User
     gets a clear History row showing "aborted, you navigated away"
     instead of a silent wrong-window paste.
  2. **Reused the existing `AbortedFocusChanged` variant** rather
     than minting a new `AbortedFocusDrift`. Semantics are close
     enough ("focus changed, we declined to paste") and the DB
     CHECK constraint in migrations/004 already allows the string.
     A new variant would require a migration AND a DB constraint
     update AND a UI badge, which is out of scope for the bugfix.
     Future: if we want to disambiguate "focus changed between
     key-down and key-up" (legacy, never emitted under ADR 0020)
     from "focus drifted during slow cleanup" (this fix), mint a
     new variant in Phase 6 polish.
  3. **Warm Ollama on boot** to make the slow-cleanup case rare.
     `dictation/runtime.rs::spawn_ollama_warmup` fires a tiny
     `/api/chat` (num_predict=1) on a dedicated thread right after
     the health-check succeeds. Pays the 30-60s cold-load cost
     while the user is opening their target app. First real
     dictation hits a warm model. Errors ignored — worst case the
     first real cleanup still cold-loads, which is just today's
     behavior.
- **Pattern:** ANY OS-targeted side effect (paste, click, focus,
  hotkey-grab) that runs AFTER an unbounded asynchronous step must
  re-validate its target before firing. The captured-at-key-up
  snapshot is stale the moment the next async op runs. Re-validation
  is cheap (one GetForegroundWindow + GetWindowThreadProcessId +
  K32GetModuleBaseNameW); silent wrong-target action is expensive
  (user trust + maybe a security-sensitive paste into the wrong app).

---

## 2026-05-17 [phase5-postship-3] pill still had a dark rectangular halo even after filling the window

- **Context:** Third smoketest. Postship-2 made the pill fill the
  entire window (`width:100%`, `height:100%`), which fixed the
  "transparent corners showing dark" issue. Pill is now a proper
  capsule. BUT — there's STILL a dark rectangle visible around the
  pill in the screenshot. Sharp corners. Hugs the pill on all four
  sides like a halo.
- **Finding:** The `.pill` rule still had `box-shadow: var(--shadow-3)`
  from the original design (when the pill was a smaller centered
  child of the window). Since the pill now fills 100%×100% of the
  window, the box-shadow extends OUTWARD from the rounded pill — but
  the area outside the pill is INSIDE the window's rectangular
  bounds. WebView2 happily paints the shadow there. Result: a sharp-
  cornered dark rectangle of shadow, hugging the rounded pill, that
  the previous fixes couldn't eliminate because they were targeting
  the wrong artifact (we kept blaming WebView2 transparency).
- **Action:** Remove the pill's `box-shadow`. Tauri's `shadow: true`
  on the recording window (already set in tauri.conf.json) gives a
  real OS-level DWM shadow that renders OUTSIDE the window bounds —
  the only place a shadow belongs on a frameless popup. CSS shadows
  are for elements with breathing room around them.
- **Reusable rule:** When a CSS element fills 100% of its container,
  it can't have a CSS `box-shadow` that extends outward — the shadow
  will clip to the container's bounds and produce a sharp-cornered
  artifact that looks NOTHING like a shadow. Either give the element
  margin/padding inside the container, or move the shadow to the
  container (OR, for windows, to the OS shadow API).

---

## 2026-05-17 [phase5-postship-2] pill stayed up + app crashed when Ollama cold-loaded a model past our 30s timeout

- **Context:** Phase 5 second smoketest. Right Alt + dictate. Capture
  finished cleanly. Pill went CLEANING. Then a 31-second gap in the
  logs ending with `WARN cleanup failed; falling back to raw
  transcript error=transport: http://localhost:11434/api/chat:
  Network Error: Error encountered in the status line: A c...`. After
  that: zero further log lines, pill stuck on screen forever,
  Mockingbird eventually crashed (process gone).
- **Finding 1 (cleanup hang root cause):** Ollama loads the model into
  VRAM on the FIRST `/api/chat` request. For qwen2.5:3b-q4 on a fresh
  Ollama process, that cold load can take 30-60 seconds. Our
  `REQUEST_TIMEOUT` in `cleanup/ollama.rs` is exactly 30s, so we sit
  RIGHT at the edge and frequently lose by a hair. The `/api/tags`
  health probe passes instantly (no model load involved), so the
  app's startup health check shows green even when the first cleanup
  is doomed to time out.
- **Finding 2 (pill stuck after cleanup hang):** `LlmCleaner::clean`
  catches the timeout error and returns `Ok(raw)` per the fallback
  rule — the WARN line in the log proves we reached that branch. But
  no logs after that means either the inject path hung silently OR
  the process panicked between cleanup-fallback and inject. Either
  way, `complete()` never reached its explicit `self.recording_window
  .hide()` at the bottom. The pill has no self-defense mechanism.
- **Finding 3 (why the process eventually crashed):** Unproven but
  most likely: the WebView2 child process died after our many
  emit-while-no-listener events with no logging path (WebView2 uses
  its own process group; when it dies, AppHandle::emit silently
  no-ops, but if the death was during an emit's actual IPC handshake
  the parent can get a broken-pipe panic propagating up the Tauri
  internals).
- **Action (defense in depth):**
  1. **Rust Drop guard.** Added `PillHideGuard` (RAII) at the top of
     `complete()`. Wraps a clone of `RecordingWindow` (cheap — it's
     Arc<AtomicBool> + Arc<Mutex<Option<AppHandle>>>). On Drop, if
     the window is still visible, hide it. Disarmed right before the
     explicit success-path hide() so we don't double-fire. Skips the
     warning log when window is already hidden (idempotent persist_
     failed_* paths beat us to it).
  2. **React watchdog.** Recording overlay now tracks time since the
     last `dictation:state` event. If >60s passes (= Rust
     orchestrator dead or terminally hung), the webview hides itself
     via `getCurrentWindow().hide()`. Doesn't need IPC to a dead
     parent. This is the only line of defense that works when the
     ENTIRE Rust process has crashed.
  3. **Inject logging.** Added `tracing::info!` at cleanup-begin,
     cleanup-end, inject-begin, inject-end. Next time something
     hangs we'll see exactly where.
  4. **Did NOT touch the 30s timeout** (in scope: defensive UI; the
     timeout itself is a separate ADR conversation — raising it
     punishes the user with longer hangs, lowering it more risks
     legit slow first-calls). Future work: a one-time "warm Ollama"
     ping on app boot that does a dummy /api/chat to pay the
     model-load cost in the background, so the first user dictation
     hits a hot model.
- **Pattern for future Tauri overlays:** ANY OS-managed visual
  affordance (overlay window, tray flyout, status badge) needs:
  - A Drop guard on the Rust side guaranteeing the affordance gets
    cleared on early return / panic.
  - A watchdog timer on the webview side that self-clears when no
    state update arrives within N seconds, using the webview's own
    API rather than IPC. The parent process might be dead.
  Together these handle (a) Rust-level errors, (b) Rust panics, (c)
  full Rust process crashes — the only failure they don't cover is
  WebView2 itself dying, in which case the user gets to ALT-F4 the
  empty window like any other ghost.

---

## 2026-05-17 [phase5-postship-2] `prompts.version INTEGER` failed to deserialize as `String`

- **Context:** Same smoketest. Modes page error box (after our prior
  silent-spinner fix exposed it): `Invalid column type Integer at
  index: 8, name: COALESCE(p.version, 'v1')`.
- **Finding:** `prompts.version` is `INTEGER` in 001_initial.sql. The
  `ModeDto.prompt_version` is `String`. The fallback literal `'v1'`
  IS a string, which made the COALESCE return type ambiguous —
  rusqlite picks the first non-null branch's affinity at row time,
  and on rows where `p.version` was non-null it returned Integer.
  rusqlite's `String` deserializer rejects Integer columns hard.
- **Action:** Concatenate to force TEXT affinity:
  `COALESCE('v' || p.version, 'v1')`. SQLite's `||` operator coerces
  both operands to TEXT. Produces strings like "v1", "v2", … which
  is what the UI was already rendering. No schema change needed
  (and forbidden post-Phase-1-seal anyway).
- **Reusable rule:** Whenever an SQL fallback literal differs in type
  from the column it's replacing, the COALESCE result type is
  per-row-dependent. Either cast both sides or change the DTO to
  match the column type. Default to casting (DTO contracts cross
  process boundaries; column types are internal).

---

## 2026-05-17 [phase5-postship] release binary baked in `localhost:5173` — webview shows "can't reach this page"

- **Context:** First end-to-end Dustin smoke test of the Phase 5 build.
  Tray icon left-click started working (after our prior fix), main
  window opened... and rendered the Edge/WebView2 default error page:
  *"Hmmm… can't reach this page — localhost refused to connect…
  ERR_CONNECTION_REFUSED"*. Pipeline still worked; only the visual
  surface was dead.
- **Finding:** In Tauri 2, the choice between `devUrl`
  (`http://localhost:5173`) and bundled-asset `frontendDist` is gated
  on the `tauri/custom-protocol` cargo feature, NOT on
  `cfg(debug_assertions)`. `cargo tauri build` enables `custom-protocol`
  implicitly; plain `cargo build --release` does NOT. So a vanilla
  `cargo build --release` produces a binary that, in production,
  literally tries to fetch the UI from a dev server that isn't running.
  Confirmed by string-searching the .exe: `localhost:5173` was right
  there, baked in.
- **Action:** Add `default = ["custom-protocol"]` +
  `custom-protocol = ["tauri/custom-protocol"]` to `src-tauri/Cargo.toml
  [features]`. Now both `cargo build --release` (our wrapper path,
  because tauri-cli doesn't propagate CUDA env reliably) AND
  `cargo tauri build` produce a binary that uses the bundled UI. The
  override pattern `cargo build --release --no-default-features` still
  works if someone genuinely wants the dev-server path in a release
  binary (weird but supported).
- **Diagnostic snippet** worth keeping:
  ```powershell
  $bytes = [IO.File]::ReadAllBytes('target\release\mockingbird.exe')
  $text = [System.Text.Encoding]::ASCII.GetString($bytes)
  if ($text -match 'localhost:5173') { 'devUrl LEAKED into release binary' }
  ```

---

## 2026-05-17 [phase5-postship] tray left-click did nothing + recording overlay rendered blank

- **Context:** Same Dustin smoke test. Two visible bugs:
  (1) left-clicking the tray icon did nothing (the menu opens on
  right-click, the main window never appeared); (2) the recording
  overlay appeared as a blank rounded box — not the pretty pill from
  the Playwright baselines.
- **Finding (tray):** `tray.rs` had `.on_menu_event(…)` (right-click
  menu) but ZERO `.on_tray_icon_event(…)` (left-click). And the
  `open_history` / `settings` / `pause` menu items were still Phase 1
  stubs that just `tracing::info!`d "(stub, Phase 5)". Easy miss
  during the UI sprint because the orchestrator + DB work absorbed all
  attention; the tray surface was never re-touched after Phase 1.
- **Finding (overlay):** Two stacked race conditions:
  - **Emit-before-listen.** `RecordingWindow::show()` calls
    `w.show()` then immediately `self.emit(LISTENING, …)`. On the
    first show the webview cold-starts (~50–500ms: WebView2 process
    spawn + JS bundle load + React mount + `listen()` registration).
    The single emit fires WHILE React is still mounting and the
    listener doesn't exist yet — event is lost forever, React stays at
    its initial `state: "idle"` value.
  - **Missing `modeLabel` in payload.** Rust `StateEventPayload` had
    `state + mode_slug + error`. React renders the mode badge
    conditionally on `event.modeLabel` — which was always undefined.
  - **Naive event-replace on the React side.** Mid-pipeline emits
    (`transcribing`, `cleaning`, …) only carry the new `state`, no
    `modeSlug` / `modeLabel`. A bare `setEvent(e.payload)` was wiping
    the mode badge on every transition.
- **Action (tray):** Add `.on_tray_icon_event` matching `MouseButton::Left`
  + `MouseButtonState::Up`, toggle the main window's visibility +
  focus. Wire `open_history` and `settings` menu items to call the
  same show-main-window helper. (Deep-linking to specific pages
  needs an `app:navigate` event — follow-up.)
- **Action (overlay):** Three layers of defense, all cheap:
  1. Rust: spawn a 3-emit burst at 50/200/500ms after the first
     `show()`, gated on `was_hidden`, bailing if visibility flips off.
  2. Rust: add `mode_label: Option<String>` to the payload, derived
     from `mode_slug` via title-case fallback.
  3. React: change initial state to `"listening"` (not `"idle"`) so
     the pretty pill renders even if every emit somehow misses;
     change `setEvent(e.payload)` to `setEvent(prev => ({ …prev, …e.payload }))`
     so mid-pipeline emits preserve the mode badge.
- **Pattern for future Tauri overlays:** ALWAYS pair an event-driven
  initial-state push with EITHER a query-on-mount IPC command OR a
  re-emit burst. The webview-cold-start emit race is silent and
  reproducible; debugging it without a console is brutal because
  transparent + `focus: false` windows are awkward to attach DevTools to.

---

## 2026-05-17 [phase5-wave-I] `cargo test --lib` exits 0xc0000139 even with cargo-with-cuda wrapper

- **Context:** Phase 5 Wave I wiring `RecordingWindow` to the real Tauri
  webview + `Emitter`. Wanted to run `cargo test --lib recording_window`
  (pure tests, no DLL deps in my code) to confirm new unit tests pass.
- **Finding:** Test binary exits with `STATUS_ENTRYPOINT_NOT_FOUND`
  (0xc0000139) even when `pwsh scripts/cargo-with-cuda.ps1 test --lib`
  is used. The earlier `0xc0000135 STATUS_DLL_NOT_FOUND` was solvable
  by putting `target\debug\` on PATH; this one is one level deeper —
  a DLL loaded successfully but an expected export is missing
  (likely `whisper.dll` ABI drift between debug+release builds, or
  `onnxruntime.dll` version mismatch). Affects ALL lib tests, not just
  mine — `cargo test --lib hotkey::state` (pure state-machine, no
  whisper/ort touch) also exits 0xc0000139. So it's a process-load-time
  failure, not a test failure.
- **Action:** Verify Rust code via `cargo build --lib` +
  `cargo test --lib --no-run` instead — both confirm the type system,
  borrow checker, and trait bounds without needing to actually launch
  the test exe. If a Phase 5/6 wave needs live test execution, the
  fix is to copy `whisper.dll` + `onnxruntime.dll` into
  `target\debug\deps\` (where the test exe lives), not just
  `target\debug\`. The launcher script doesn't do this today.

---

## 2026-05-17 [phase3-wave4.8] Silero v5 ONNX needs an UNDOCUMENTED 64-sample context buffer

- **Context:** End-to-end dictation produced empty Whisper output. Tracing
  showed the audio pipeline was healthy (capture worked, resampler worked,
  WAV-dump of the post-resample buffer sounded perfect to a human), but
  `vad_trim` kept returning zero samples because Silero VAD scored every
  frame as non-speech (max confidence ~0.0031 across 155 frames of clear
  speech vs. the 0.5 threshold).
- **Finding:** The Silero v5 ONNX model published at
  `snakers4/silero-vad/master/src/silero_vad/data/silero_vad.onnx`
  requires the caller to maintain a 64-sample "context" buffer (last 64
  samples of the previous frame, initialised to zeros) and **prepend it
  to every 512-sample frame before inference**, making the actual model
  input shape `[1, 576]`, not `[1, 512]`. Without this:
  - The model still runs (no error)
  - The output tensor still has the right shape `[1, 1]`
  - The output is essentially CONSTANT regardless of input: silence,
    speech, pure tones, and white noise all score ~0.001–0.003. The
    sigmoid output sits permanently at "definitely silence."
  This is **not in the ONNX schema metadata**. The schema declares
  `input: Float32 [-1, -1]` (any batch, any samples) — accepts 512
  samples without complaint. The requirement lives only in
  `silero-vad/src/silero_vad/utils_vad.py`'s `__call__` method:
  ```python
  context_size = 64 if sr == 16000 else 32
  if not len(self._context):
      self._context = torch.zeros(batch_size, context_size)
  x = torch.cat([self._context, x], dim=1)   # PREPEND
  ort_inputs = {'input': x.numpy(), ...}
  self._context = x[..., -context_size:]     # SAVE LAST 64 FOR NEXT CALL
  ```
  Misleading red herrings on the path here:
  - The `sr` input is declared `Tensor { ty: Int64, shape: [] }` (scalar).
    Both `ort 2.0.0-rc.10` and ONNX Runtime accept shape `[1]` (1-d,
    1-element) silently. The `silero-rs` Rust crate also uses `[1]`.
    Trying "true" scalar shapes (`()`, `[0_usize; 0]`) actually made the
    output WORSE, not better — these paths through ort + onnxruntime
    appear to mis-handle 0-d Int64 scalars. Stick with `[1]`.
  - ORT `GraphOptimizationLevel::Level3` vs `Level1` makes no difference
    once the context buffer is correct.
  - The model file hash matched the official upstream — it wasn't
    corrupted, just being called wrong.
- **Action:**
  1. Always maintain a `context: Vec<f32>` of size 64 in any Silero v5
     wrapper. Init to zeros, update with the last 64 samples of every
     new frame, reset in `reset()`.
  2. **When integrating a vendor ONNX model, find and read the
     reference Python `__call__` end-to-end** before trusting the
     schema. Schemas describe tensor shapes, not protocol semantics.
     The schema cannot tell you "this model expects to be called with
     overlapping windows."
  3. **Test models against KNOWN INPUTS with known expected outputs.**
     Our test suite only asserted "silence scores low" — which the
     broken impl still satisfied. Now we have a regression test
     (`silero_output_has_dynamic_range`) that asserts the model
     produces *meaningfully different* outputs for structurally
     different signals (silence vs. swept sine). Without the context
     buffer, all confidences collapse to ~0.001 and the test fails.
  4. **Add a `last_capture.wav` dump on every dictation** (single-slot
     overwrite) — this was the diagnostic that proved the audio was
     fine and isolated the bug to Silero. Cheap, low-overhead, and
     pays for itself the first time you need it.
  5. ORT `Tensor::from_array` silently accepts wrong-shape inputs for
     scalar parameters. Don't trust "no error" as "correct config" —
     verify behaviour with end-to-end output assertions.


## 2026-05-17 [phase-3-wave-4.5] `target/release/mockingbird.exe` fails with `STATUS_DLL_NOT_FOUND` from any cwd that isn't the build dir

- **Context:** Wave 4.5 smoke-tested the wired-up binary by running it directly. Got exit code `0xC0000135` (DLL_NOT_FOUND) within 100 ms, no logs, no panic message.
- **Finding 1:** `[lib] crate-type = ["staticlib", "cdylib", "rlib"]` means the exe links against `mockingbird_lib.dll` (cdylib), which Windows looks for via standard DLL search order. Running the exe from any cwd other than `target\release\` fails the load — the cdylib isn't on PATH.
- **Finding 2:** Even with cwd = `target\release\`, the binary still failed because `whisper-rs = { features = ["cuda"] }` (root `Cargo.toml` line 66) makes the build dlopen `cudart64_*.dll` at process start. CUDA 12.8 is installed at `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.8\` but its `bin\` is NOT on system PATH — only the `cargo-with-cuda.ps1` wrapper adds it for cargo invocations.
- **Finding 3:** The diagnostic was painful because `windows_subsystem = "windows"` (set in `main.rs` for release builds) suppresses console output. Loader-time failures show only the exit code, with no stderr text.
- **Action:** Created `scripts/run-mockingbird.ps1` that (a) sets cwd to `target\release`, (b) prepends CUDA 12.8 `bin\` to PATH, (c) sets `ORT_DYLIB_PATH`, (d) sets `SILERO_VAD_PATH` + `WHISPER_MODEL_PATH` for log clarity. Phase 6 packaging will copy the CUDA + ONNX DLLs next to the exe so this is moot in the installer; for dev workflow, use the launch script.
- **Generalisation:** When diagnosing `STATUS_DLL_NOT_FOUND` on a Tauri-cdylib release binary, the suspect order is: (1) the cdylib next to the exe, (2) any `features = ["cuda"]` deps, (3) `ORT_DYLIB_PATH` for `ort` crate, (4) Visual C++ runtime. Loader Snaps (`gflags /i mockingbird.exe +sls`) would print each DLL probe to a debugger — overkill for dev but invaluable if (1)-(4) don't explain it.

## 2026-05-17 [phase-3-wave-4.5] `cargo test --lib` doesn't see `ensure_ort_dylib_set()` so VAD tests need explicit env

- **Context:** After Wave 4.5 patches, 3 VAD tests failed with `ort 2.0.0-rc.10 is not compatible with the ONNX Runtime binary found at \`onnxruntime.dll\`; expected GetVersionString to return '1.22.x', but got '1.17.1'`.
- **Finding:** `dictation::runtime::ensure_ort_dylib_set()` only runs from `lib.rs::run()` (the Tauri entry point). `cargo test` is a separate binary that links `mockingbird_lib` directly and exercises `audio::vad::tests::*` without going through `run()`. So the env autodiscovery doesn't fire and Windows resolves `onnxruntime.dll` via default search order — finding a stale 1.17.1 DLL somewhere on the system.
- **Action:** Document the env requirement: `cargo test` needs `ORT_DYLIB_PATH` set explicitly. Wave 5 (or Phase 6) should either add a `#[ctor]`-style init for tests OR mark the affected VAD tests as `#[ignore]` to keep `cargo test` clean. For now: the launch script + cargo wrapper both set the env; bare `cargo test` is the only caller that needs manual setup.
- **Generalisation:** Env-var autodiscovery in `lib.rs::run()` covers the production path but creates a silent test-only gap. If a runtime env var is required, it should be enforced uniformly — either via a shared `init_runtime_env()` function called from both `run()` and a test-init hook, or via test attributes (`#[ignore]` for env-dependent tests).



## 2026-05-17 [phase-3-wave-4] ADR scope ≠ task brief scope

- **Context:** The Wave 4 brief said "no migration 004 — ADR 0010 binding." Implementation discovered the orchestrator needs per-row `injection_status` persistence — without storage, the cross-app QA matrix can't be objectively verified after the fact.
- **Finding:** Re-read ADR 0010. It says raw-`transcripts` rows are immutable; says NOTHING about migration count. The "no migration 004" line in the brief was the brief author (me) conflating two different bindings: "raw transcripts immutable" (real) + "migrations append-only" (real but means "don't EDIT existing migrations, new ones fine"). Asked Dustin via `ask_user_question`; he picked (A) "add migration 004."
- **Action:** Migration 004 lands the nullable `injection_status` column. Brief discipline TODO: when writing a brief that cites an ADR as binding, **paste the binding sentence verbatim from the ADR into the brief**. Paraphrasing introduces drift.

## 2026-05-17 [phase-3-wave-4] `windows-rs` import names drift between consts

- **Context:** Implementing `paste.rs` test for `CF_UNICODETEXT`. First attempt: `use windows::Win32::System::DataExchange::CLIPBOARD_FORMAT;` to wrap our constant in the typed newtype. Build broke.
- **Finding:** In `windows-rs 0.56`, `CLIPBOARD_FORMAT` lives in `Win32::System::DataExchange` as a `pub struct CLIPBOARD_FORMAT(pub u16)` — but it's NOT re-exported at the module's top level for direct `use`. You have to import via its enclosing alias path or just use raw u32. For internal-only test usage, a raw `u32` constant + a comment-encoded invariant is simpler.
- **Action:** Dropped the typed wrapper test; kept `CF_UNICODETEXT_ID: u32 = 13` as a plain constant with a comment locking in the Win32 ABI guarantee (CF_UNICODETEXT has been 13 since NT 3.51). **Generalisation:** when a windows-rs type ISN'T at the top-level of its module, don't fight it for one test — fall back to documented raw values.

## 2026-05-17 [phase-3-wave-4] `&mut dyn Trait` arguments are easy to miss in trait composition

- **Context:** Orchestrator `complete()` first attempted `trim_speech(&samples, self.audio.sample_rate(), &TrimConfig::default())`. Build failed: `trim_speech` takes `&mut dyn VoiceActivityDetector`, not `u32 (sample_rate)`.
- **Finding:** I had glossed the VAD shape based on partial recall. The orchestrator needs to own a `Box<dyn VoiceActivityDetector>` AS A SEPARATE FIELD from the audio capture. The two are independent traits that happen to share a `sample_rate` constant (16 kHz). Conflating them was a category error.
- **Action:** Added `vad: Box<dyn VoiceActivityDetector>` to `DictationOrchestrator`. **Generalisation:** when composing multiple traits in an orchestrator, list them as bullet points in a docstring BEFORE writing the struct fields — forces clear thinking about which trait owns what.

## 2026-05-17 [phase-3-wave-4] Hard-coded timestamps in tests are guaranteed wrong

- **Context:** Wrote `assert_eq!(format_secs_as_iso(1_779_926_400), "2026-05-17T00:00:00Z")`. Test failed; my arithmetic was off by 11 days.
- **Finding:** Mental arithmetic on Unix epoch seconds is reliably wrong. Even careful pen-and-paper attempts miscount leap years (especially the 2000 case).
- **Action:** Corrected the value (1_778_976_000) by deriving from a known anchor (2024-01-01 = 1_704_067_200) plus precisely-counted intervening days. Added a second test using 2024-02-29 to catch leap-year regressions specifically. **Generalisation:** when a pure-math test fails, the test is the bug 90% of the time. Derive expected values from KNOWN anchors + arithmetic; never from "I think it's around…"

---


## 2026-05-17 [phase-3-wave-3] WH_KEYBOARD_LL thread-local discipline

- **Context:** Implementing `hotkey/windows.rs` real `SetWindowsHookEx(WH_KEYBOARD_LL, ...)` per ADR 0015. The callback is `unsafe extern "system" fn` — no captures, no closures, no `&self`. State has to live in a `static` somewhere.
- **Finding:** The natural temptation is `static SENDER: Mutex<Option<Sender>> = ...`. **Don't.** The callback fires on the hook-installing thread (the only thread that ever touches it), so a `Mutex` adds zero safety and risks the 300 ms hook-watchdog timeout if any other code on the same process ever locks it. The right tool is `thread_local!(static SENDER: RefCell<Option<Sender>> = RefCell::new(None))`. Uncontended by construction.
- **Action:** Used three `thread_local!` cells: `CALLBACK_TX` (the channel sender), `CALLBACK_VK` (configured VK for filtering), `CALLBACK_HHOOK` (owned hook handle for RAII unhook). All set on hook-thread entry, all cleared on exit. Drop order matters — unhook BEFORE dropping the sender (so a stray late callback can still no-op cleanly).

## 2026-05-17 [phase-3-wave-3] Pure-vs-OS split makes WH_KEYBOARD_LL testable

- **Context:** Without care, every test of `LowLevelKeyboardProc` would have to install a real hook + use real `SendInput`. Slow, flaky, and impossible on headless CI.
- **Finding:** The callback's actual work is "decide if this message + VK is interesting, and if so emit which `HotkeyEvent`". That's a pure function: `fn classify_keystroke(wparam, vk_code, configured_vk, at) -> Option<HotkeyEvent>`. The OS-side glue (read `KBDLLHOOKSTRUCT`, `try_send`, `CallNextHookEx`) is a 10-line shim with no decisions in it. **9 of the 11 unit tests in `hotkey/windows.rs` exercise the pure helper** with synthesised message/VK pairs; the 2 remaining are `#[ignore]` live SendInput round-trips.
- **Action:** Establish the pattern for the rest of Wave 3+: ANY new OS-bound module should split into a pure-helper file (or function) covered by fast unit tests, plus a thin OS shim that's `#[ignore]`-tested live. `hotkey/probe.rs` follows the same recipe (`probe_with` is pure, `probe_live` is the OS wrapper).

## 2026-05-17 [phase-3-wave-3] State-driver tick cadence is not "as fast as possible"

- **Context:** Designing `hotkey/driver.rs` — the loop that pulls from the listener channel + synthesises `HotkeyEvent::Tick` events when no real event arrives. First instinct: tick every 1 ms for sub-millisecond resolution.
- **Finding:** 1 ms ticks are pointless. The §6.1 thresholds we care about are 80 ms (hold), 300 s (max session), 30 s (cancel-threshold), 3 s (confirm-timeout). The TIGHTEST resolution we ever need is 80 ms / 4 = 20 ms. Lower cadence means: less CPU wake-up overhead on battery, less syscall churn through `recv_timeout`, and the LL-hook watchdog logger (every 250th tick = 5 min @ 20 ms) gets a clean integer count.
- **Action:** Default `DEFAULT_TICK_INTERVAL = Duration::from_millis(20)`. Tests in `driver.rs` use `Duration::from_millis(5)` for faster wall-clock test runs but the production constant stays 20 ms. **Generalisation:** when designing a tick loop, derive the cadence from the tightest deadline / (4 to 8), not from "feels precise."

---


## 2026-05-17 [phase-3-wave-2] windows-rs 0.56 HWND is isize, not pointer

- **Context:** Implementing `window_context/windows.rs` + `injection/secure_guard.rs`. First attempt used `.is_null()` and `*mut c_void` patterns that work in newer `windows-rs` releases.
- **Finding:** In `windows-rs 0.56` (our pinned version per ADR 0011 + Cargo.lock), `HWND` is `pub struct HWND(pub isize);` and `HANDLE` is `pub struct HANDLE(pub isize);`. Null check is `hwnd.0 == 0`, not `hwnd.0.is_null()`. Conversion to/from `ForegroundWindow.hwnd: isize` is the trivial `.0` access. **`windows-rs 0.61+` switched to `*mut c_void`-backed pointer types** for HWND/HANDLE — but we're not on 0.61+ and won't be in Phase 3.
- **Action:** Established pattern: pass `isize` across thread boundaries (in `ForegroundWindow.hwnd`), wrap as `HWND(isize_value)` at the OS boundary, compare against zero for null. If we ever upgrade `windows-rs`, this is one of the breaking points to watch.

## 2026-05-17 [phase-3-wave-2] GUI_SECUREINPUT is not a real Win32 constant

- **Context:** ADR 0017 specified three signals for `SecureInputGuard`, including `GetGUIThreadInfo(GUI_SECUREINPUT)`. Wave 2 implementor went to look up the constant value in `windows-rs 0.56` to use it.
- **Finding:** `GUI_SECUREINPUT` does not exist in `windows-rs` AND does not exist in the official Win32 SDK `winuser.h`. The full list of `GUITHREADINFO_FLAGS` is `GUI_CARETBLINKING (0x1)`, `GUI_INMOVESIZE (0x2)`, `GUI_INMENUMODE (0x4)`, `GUI_SYSTEMMENUMODE (0x8)`, `GUI_POPUPMENUMODE (0x10)`. The ADR author (me) conflated this with macOS's `IsSecureEventInputEnabled()` which IS a real API. The Windows reality is different: UAC consent prompts run on a separate **secure desktop** that our process can't enumerate; `GetForegroundWindow()` returns NULL during them, which trips the null-foreground guard in `window_context/windows.rs` BEFORE we ever reach the secure-input check.
- **Action:** Amended ADR 0017 with an "Update — 2026-05-17 (Wave 2)" section dropping signal 1. The remaining two signals (class-name allowlist + `ES_PASSWORD` on focused edit) are sufficient because: (a) UAC / Hello / BitLocker / Ctrl-Alt-Del trip the null-foreground guard, (b) Credential UI is caught by class name, (c) Win32 password edits are caught by `ES_PASSWORD`. WebView2 password fields remain documented as out-of-scope and are mitigated via per-app `Abort` overrides in ADR 0016.
- **Carry-forward:** When an ADR references an API, **the implementor MUST validate the API exists in our pinned dep version before sealing the ADR**. Add this to the planning-agent's "ADR review checklist".

## 2026-05-17 [phase-3-wave-2] State-machine precedence: hard stop wins

- **Context:** `HotkeyStateMachine::handle` for `(ConfirmingCancel, Tick)` originally checked `confirm_timeout` first, then `max_session`. A unit test (`max_session_overrides_confirm_cancel`) caught the latent bug.
- **Finding:** When two timeouts fire on the same tick, the precedence matters. The 300 s `max_session` is a HARD ceiling per PLAN §6.1 — without it a user who sits on the confirm-cancel toast past 300 s would have their recording grow indefinitely. The 3 s `confirm_timeout` is a SOFT revert. Hard stops must always win.
- **Action:** Reordered the branches; added a code comment explaining the precedence + a test that explicitly hits both timeouts simultaneously. **Generalisation:** state-machine code with multiple time-dependent transitions out of one state should always order the branches by "severity" — hard stops first, soft transitions after.

---


## 2026-05-17 [phase-3-wave-1] Build env, parallelism, and PowerShell stream/arg parsing

- **Context:** Phase 3 Wave 1 — five ADRs + module scaffolds + first cargo gate post-Phase-2. Hit six separate Windows toolchain papercuts before the gate went green.
- **Finding 1 — `scripts/cargo-with-cuda.ps1` is now the one-call wrapper for ALL cargo invocations in this project.** Imports MSVC env via `vcvars64.bat`, pins `CUDA_PATH` + `CUDA_PATH_V12_8` to v12.8 (ADR 0011), prepends cmake to PATH, caps `CMAKE_BUILD_PARALLEL_LEVEL=4`, then forwards args through `cmd.exe /c "cargo ... 2>&1"`. Replaces the prior "set env inline before every cargo call" pattern from Phase 2. Always invoke via `-File`, never `-Command` (see Finding 4).
- **Finding 2 — whisper-rs-sys CUDA compile OOMs at `--parallel 16` on a 16 GB machine.** Each `fattn-mma` template instance can use 2–4 GB resident RAM. With 16 cores, MSBuild fires 16 nvcc processes in parallel and one or more get killed silently, leaving 0-byte `.obj` files. The downstream `Lib.exe` then fails with `LNK1136: invalid or corrupt file`. **Fix:** export `CMAKE_BUILD_PARALLEL_LEVEL=4` (now baked into the wrapper script). Build time goes from ~5 min (when it works) to ~10 min, but the OOM is gone.
- **Finding 3 — Em-dashes in PowerShell scripts break `-File` invocations.** `powershell -File` reads the script as system code page (cp1252 on US Windows), not UTF-8. UTF-8 em-dashes (U+2014, three bytes `E2 80 94`) get split into bogus tokens (e.g. `'\libnvvp'`), parser fails downstream of the actual problem with a misleading "Unexpected token" error. **Fix:** stick to ASCII hyphens in `.ps1` files. Markdown / Rust source / ADRs can use em-dashes freely.
- **Finding 4 — `powershell -Command` eats the `--` argument delimiter; `-File` preserves it.** This kills `cargo clippy ... -- -D warnings`-style invocations. PowerShell's `-Command` parser silently swallows the `--`, sending `-D warnings` to cargo-clippy directly, which forwards it to `cargo check`, which errors out with `unexpected argument '-D'`. **Fix:** the wrapper script uses no `param` block and no `[CmdletBinding()]` — it grabs everything from `$args` — and all callers invoke via `-File`, not `-Command`.
- **Finding 5 — cmd.exe `%ERRORLEVEL%` expands at PARSE time, not run time, across `&` separators.** Writing `powershell ... & echo exit:%ERRORLEVEL%>exit.log` captures the previous-iteration exit code (typically 0), NOT the just-finished powershell's exit. **Fix:** use `call echo exit:%^ERRORLEVEL%` — the `^` escapes the `%` past parse-time, and `call` re-parses the line at run-time. Now exit codes propagate correctly.
- **Finding 6 — PowerShell pipelines treat native-command stderr as terminating errors under various `Tee-Object` / `*>&1` combinations.** Cargo writes "Compiling …" progress lines to stderr; under the wrong stream config, PowerShell promotes these to `NativeCommandError` and kills cargo mid-build. **Fix:** the wrapper invokes cargo via `& cmd.exe /c "cargo ARGS 2>&1"` — merging streams INSIDE cmd.exe means PowerShell only sees a unified text stream, no error promotion. `$ErrorActionPreference = 'Continue'` immediately before the cargo call adds belt-and-braces protection.
- **Action — all six findings now codified in `scripts/cargo-with-cuda.ps1`.** Future iterations should call `pwsh scripts/cargo-with-cuda.ps1 <cargo-args>` rather than reinventing the env-setup wheel.

---

## 2026-05-17 [phase-3-wave-1] Wave 1 retrospective

**Delivered:** 5 ADRs (0015–0019), 16 module scaffolds across `hotkey/`, `injection/`, `window_context/`, `AppError::Hotkey` + `AppError::Injection` variants, `phf` workspace dep, broader `windows-rs` feature set (UI_WindowsAndMessaging + UI_Input_KeyboardAndMouse + System_DataExchange + System_Memory + System_Threading + System_ProcessStatus), and a reusable `scripts/cargo-with-cuda.ps1` build wrapper.

**Surprised:** six separate PowerShell / cmd.exe / cmake / nvcc papercuts before the cargo gate went green. Each was one specific subtlety — ASCII-only scripts, parallelism cap, `-File` vs `-Command`, `%^ERRORLEVEL%`, cmd.exe stream merging, $ErrorActionPreference scope. Lost about 90 minutes here. None of these would have surfaced from "just write Rust code" planning — they only show up the first time a fresh shell tries to run the gate after Phase 2.

**Deferred:** none. All Wave 1 deliverables landed in one iteration.

**Carry-forward:**
- Wave 2 implementor (injection-author + code-puppy) MUST use `pwsh scripts/cargo-with-cuda.ps1` for every cargo call. No inline env setup; the script is the contract.
- Default for `WinKeyboardHook` is a non-derived impl that sets `vk = VK_RMENU`. Wave 3 must reconsider when the conflict probe (ADR 0019) resolves the binding — the constructor should accept a `vk` parameter.
- The `block-bare-paste` hook (`scripts/hooks/warn-bare-clipboard-set.py`) is shell-side only. Rust-side static enforcement of "only `injection/paste.rs` calls `SetClipboardData`" is deferred to a clippy lint or rust-analyzer rule in a later wave — YAGNI for Wave 1.

**Numbers:** 13 new tests (164 / 164 passing). 16 new files. ~600 net lines of code (counted in implementation files; ADRs are separate ~1100 lines). ADRs 0015–0019 sealed. bd tasks closed: 6 of 24 (mb-q1z, mb-3az, mb-anl, mb-jzm, mb-dlo, mb-rne).

---

## 2026-05-16 [phase-2] CUDA 12.8 install + GPU re-enable success story
- **Context:** Wave 4 punted CUDA because chocolatey only ships CUDA 13.2.1, which is too new (deprecated ggml archs + empty MSBuild `CudaToolkitDir`). Wave 5 finale: install CUDA 12.8 manually from developer.nvidia.com, side-by-side with the existing 13.2.
- **Finding 1 — Side-by-side works fine.** CUDA Toolkit installations live in version-suffixed dirs (`v12.8\`, `v13.2\`) under `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\`. The installer asks about this politely (the "Environment Variable Check" dialog at the end is informational, not an action item) — pick custom install, uncheck Nsight / Documentation / Display Driver / HD Audio (the driver components would DOWNGRADE you from a newer driver), KEEP `Development` + `Runtime` + **`Visual Studio Integration`** under CUDA. Visual Studio Integration is the one that ships `.targets`/`.props` files into the VS BuildTools BuildCustomizations dir — without it cmake's VS generator can't build CUDA at all.
- **Finding 2 — MSBuild picks the LATEST `.targets` file alphabetically.** Even after CUDA 12.8 installs cleanly, cmake-rs's VS 2022 generator was still reading `CUDA 13.2.targets` (broken) instead of `CUDA 12.8.targets` (working) because MSBuild auto-imports all CUDA `.targets` files and tries the highest version. Fix: physically move (not delete — backup is reversible) the v13.2 `.props/.targets/.xml/.dll` files out of `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\MSBuild\Microsoft\VC\v170\BuildCustomizations\` to a backup folder. Requires admin (UAC). See `scripts/disable-cuda13-msbuild.ps1`.
- **Finding 3 — `CMAKE_GENERATOR_TOOLSET=cuda=...` does NOT override cmake-rs.** Tried setting this env var to force cmake to use CUDA 12.8; cmake-rs is hard-coded to set `toolset=host=x64` and complained about a duplicate toolset spec. The MSBuild integration file route is the only viable path on Windows.
- **Finding 4 — Child PowerShell processes do NOT inherit User/Machine env from the registry.** When the agent spawns a fresh PowerShell, that shell inherits env from the agent's PARENT process (which started before the CUDA install). New `CUDA_PATH`, `CUDA_PATH_V12_8`, and `PATH` entries set via `[Environment]::SetEnvironmentVariable(..., 'User')` aren't visible in spawned shells until the parent process restarts. Workaround: explicitly assign `$env:CUDA_PATH = '...v12.8'` AND `$env:CUDA_PATH_V12_8 = '...v12.8'` at the top of every PowerShell invocation that calls cargo. Persisted env vars matter for future shells; the running session needs `$env:` assignments.
- **Finding 5 — `cargo clean -p whisper-rs-sys` doesn't always trigger a fresh CUDA rebuild.** Cargo computes a per-feature-set hash for each crate's build dir; two simultaneous hashes (`6bf2b1cc..` CPU-only and `9140b77c..` CUDA-on) coexisted in `target/release/build/`. The stale binary linked against the CPU artifact even though the CUDA artifact had built successfully — because the previous build's link step timed out before re-linking `stt_test.exe`. **Just run `cargo build` again after a timeout; cargo's incremental rebuild figures it out.**
- **Finding 6 — clippy uses the DEBUG profile by default, which requires a fresh debug cmake configure.** After cuda landed in `target/release/`, running plain `cargo clippy` triggered a brand-new debug build of whisper-rs-sys (~10 min, including all CUDA kernels recompiled in debug). Fix: `cargo clippy --release --all-targets`. Reuses the release artifacts; takes 60 s instead of 10+ min.
- **Finding 7 — Whisper hallucinates `"Thank you."` from pure silence.** Whisper's training data includes thousands of YouTube outro phrases; the model occasionally fabricates one from silence. **This is not a regression** — it's a documented Whisper artifact. The non-fabrication test should assert text length under ~50–100 chars, not text equality. Real VAD will trim silent buffers down to nothing before they reach Whisper in production, so this affects only `--no-vad` paths.
- **Action — Wave 5 final commit landed `phase-2-complete` tag.** 151/151 tests pass on GPU. Latency on silent.wav: 716 ms total (~600 ms is cold model load to GPU; subsequent transcribes will be sub-100 ms).

---

## 2026-05-16 [phase-2-retrospective] Phase 2 retrospective (Waves 1–5; ✅ SEALED)

**Delivered (5 waves):**
- Wave 1: 4 ADRs (0011 whisper-rs CUDA, 0012 ort runtime, 0013 cpal/ringbuf, 0014 model storage), `AudioCapture` + `VoiceActivityDetector` + `SpeechToText` traits, model-resolver + 224-token cap constant, scaffolds with `todo!()` bodies, model download script (BITS-resumable + SHA-256-verified).
- Wave 2: `CpalCapture` (cpal 0.15 + ringbuf 0.4, 16 kHz mono i16, 1 MB SPSC, 30 ms frames, start/stop idempotent), synthetic WAV fixture generator + 3 fixtures committed (silent / sine_440 / mixed), 8 integration tests.
- Wave 3: Silero VAD via ort `2.0.0-rc.10` with `load-dynamic + ndarray` features (sidesteps MSVC 2022 STL static-link demand), 512-sample frames + LSTM carry-through, `vad_trim` helper with lead-in/hangover/min-speech, 4 unit + 4 integration tests.
- Wave 4: `WhisperStt` (whisper-rs 0.16, CPU path), `prompt_builder` (recency × frequency × app-match, hand-rolled ISO-8601 parser, 224-token greedy pack), `stt_test` CLI (pretty + JSON), criterion bench skeleton, 4 whisper integration tests, 12 prompt_builder unit tests.
- Wave 5: 3 judge cards (`stt-correct`, `cuda-verified`, `perf-stt`) + 3 entries in `judges-template.json`, this retrospective. Initial Wave 5 commit held back the seal tag pending GPU verification. **Wave 5 finale (same day):** CUDA 12.8 installed side-by-side with CUDA 13.2, MSBuild integration sorted, `whisper-rs cuda` feature re-enabled, stt_test verified `gpu_used=true` on RTX 2060 — **`phase-2-complete` tag APPLIED.**

**Test count growth:** 101 (Phase 1) → 122 (W2) → 134 (W3) → 151 (W4). +50 tests over the phase, target was +40–50 — hit.

**What worked:**
- **The brief pattern keeps paying.** End-of-wave briefs nailed Wave 2/3/4 first-try compile-and-pass with one or two trivial clippy fixes. ~95% first-run pass rate.
- **Skipping gracefully > skipping silently.** Tests gated on runtime resources (`silero_runtime_available`, `whisper_model_present`) skip with a `eprintln!` — keeps CI green without `#[ignore]` hiding regressions.
- **ADRs upstream of dependency decisions.** ADR 0011 named the CUDA-fallback design BEFORE we hit the CUDA-13 chasm. When the chasm appeared the answer was "the architecture already handles this; ship CPU and re-enable later," not panic.

**What surprised us:**
- **Chocolatey's CUDA package is current-only.** No version pins available; you get whatever's latest. For tightly-coupled toolchains (CUDA 12 vs 13 is a different ABI generation), this is a footgun.
- **whisper-rs 0.13.2 ships internally inconsistent.** 71 build errors before we even touched CUDA — the wrapper accessed fields the `-sys` crate's bindgen explicitly hid as opaque. Newer crate version solved it; lesson is *don't blind-pin -sys-coupled crates*.
- **whisper-rs 0.16 API renames silently.** `full_n_segments()` → returns `i32` (was `Result`); `full_get_segment_text(i)` → `get_segment(i)` returning `Option<Segment>` with `.to_str_lossy()`. Caught at compile time but the brief was based on 0.13 shapes.
- **PowerShell parses em-dashes (U+2014) as multi-byte garbage in scripts.** Strip to ASCII before saving any `.ps1`. Already burnt this in Phase 1 — recurring.

**What we deferred:**
- **A real-speech 10s WAV fixture.** Wave 4 ships synthetic sine/silent only; the `stt-correct` + `perf-stt` judges need `hello.wav` (or similar) with an `.expected.txt` sidecar. Helios delegation candidate (Windows `System.Speech.Synthesis`).
- **Phase 3 will own the global hotkey + injection paths.** No keyboard hooks landed; the trait stubs for cross-app injection are NOT in Phase 2.

**Carry-forward to Phase 3 (cross-app injection):**
- The brief pattern: write `docs/phases/phase3-wave1-brief.md` at the start, treat it as binding.
- The `Provenance is total` invariant is about to get pressed harder — sessions table rows go from "in-memory test only" to "every hotkey press writes a row."
- Test density target: ~10 tests / 500 LoC. Phase 1 hit it, Phase 2 hit it (~50 tests / ~2,500 new lines).
- AppError-string variants generalize well; Phase 3 will likely add `Injection(String)` + `Hotkey(String)`.

**Phase 2 numbers:**
- Tests: 151 / 151 ✅ (was 101 at Phase 1 seal)
- LoC added: ~2,500 (audio + stt + vad + bin + tests + benches + scripts + ADRs)
- ADRs: 4 new (0011–0014) — all Status=Accepted
- LESSONS entries: +18 (now ~30 total)
- bd tasks: 27 of 27 Phase-2 tasks closed (including the GPU-verification seal-blocker `mb-ltq`)
- Phase tag: **`phase-2-complete` APPLIED 2026-05-16.**

---

## 2026-05-16 [phase-2] CUDA 13 + whisper-rs 0.16's bundled ggml = chasm
- **Context:** Wave 4 installed CUDA Toolkit 13.2.1 (latest, only version on choco) plus VS 2022 BT, cmake, LLVM. Tried to build whisper-rs with the `cuda` feature.
- **Finding 1:** ggml hard-codes CUDA architectures `52;61;70;75`. CUDA 13 dropped pre-Turing support — those archs no longer compile.
- **Finding 2:** MSBuild's `CUDA 13.2.targets` integration file reads `CudaToolkitDir` from somewhere that's coming up empty post-install. Either a registry timing issue or the installer not registering correctly when invoked through chocolatey.
- **Finding 3:** Chocolatey only publishes `cuda 13.2.1`. Older versions (12.x) are NOT available through the default repo — would require manual download from developer.nvidia.com (~3 GB).
- **Action:** Shipped Wave 4 CPU-only by dropping the `cuda` feature from whisper-rs in `Cargo.toml`. **This is NOT a shortcut** — ADR 0011's runtime CPU fallback was designed for exactly this scenario. `WhisperStt::new` still has GPU-first/CPU-fallback semantics; without the cuda feature the GPU attempt fails immediately and the CPU path runs. When CUDA 12.x is installed side-by-side from developer.nvidia.com (CUDA toolkits coexist in `v12.x` / `v13.x` subdirs), flip the feature back on and the GPU path activates.
- **Follow-up:** bd `mb-ltq` tracks the GPU re-enable task.

## 2026-05-16 [phase-2] whisper-rs 0.13.x ships incompatible with its own -sys crate
- **Context:** Wave 4 originally pinned `whisper-rs = "0.13"`. Build failed with 71 errors of the form `no field grammar_penalty on type whisper_full_params`.
- **Finding:** whisper-rs 0.13.2 (the high-level wrapper) and whisper-rs-sys 0.11.1 (the bindings) were published incompatible. The -sys crate's bindgen produced opaque structs (`pub _address: u8` with `size_of=264` assertion) — bindgen's signal for blocklisted types. The 0.13.2 wrapper tries to access fields the bindings explicitly hid.
- **Action:** Bumped to `whisper-rs = "0.16"`. The 0.16/sys-0.15.0 pair compiles cleanly and the field-access pattern works. **Lesson:** when a Rust crate has a sibling `-sys` crate, the high-level and low-level versions are coupled tightly; trust the crate author's pairing and use the LATEST stable rather than version-pinning blind.

## 2026-05-16 [phase-2] whisper-rs 0.16's segment API renamed methods
- **Context:** Brief specified `state.full_get_segment_text(i)` (0.13 API) and `state.full_n_segments()` returning `Result<i32>`.
- **Finding 1:** 0.16 changed `full_n_segments()` to return `i32` directly (no Result).
- **Finding 2:** 0.16 introduced a `Segment` accessor: `state.get_segment(i)` returns `Option<Segment>` (not Result); use `.to_str_lossy()` on the segment to get UTF-8-safe text.
- **Action:** Brief updated mid-execution. The 0.16 API is cleaner; document the shape in `stt/whisper.rs` comments so future bumps catch the next API drift.

## 2026-05-16 [phase-2] chocolatey package paths: cmake hides inside VS 2019 BT
- **Context:** Pre-install reconnaissance showed `where cmake` returned nothing — yet a recursive search of `C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools` found `cmake.exe` already on disk.
- **Finding:** VS 2019/2022 BuildTools includes a cmake.exe inside `Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\`. It's NOT on PATH by default. Saved 50 MB of redundant install in option A's path; ultimately we installed standalone cmake anyway for explicit PATH access.
- **Lesson:** Before `choco install <tool>`, recursive-search the existing VS installs. Frequently the tool is already there.

## 2026-05-16 [phase-2] PowerShell em-dash bites in script files
- **Context:** `scripts/install-wave4-toolchain.ps1` failed to parse because I copy-pasted an em-dash ("—") inside a string literal. PowerShell's parser cascaded brace errors from the malformed string.
- **Finding:** PowerShell expects ASCII inside script files unless explicit BOM/UTF-8 encoding is declared. The em-dash (U+2014) and en-dash (U+2013) got mangled on save → garbled multi-byte sequences that broke string termination.
- **Action:** Strip both characters via `$c -replace [char]0x2014, '--' -replace [char]0x2013, '-'` before saving. Use ASCII hyphens in PowerShell scripts always.

## 2026-05-16 [phase-2] ort 2.0 is RC-only AND static-link demands MSVC 2022
- **Context:** Phase 2 Wave 3 added `ort` for Silero VAD. Compilation failed two ways.
- **Finding 1:** No stable ort 2.0 exists — only `2.0.0-rc.1` through `rc.12`. Plain `"2"` fails cargo ("prerelease must be specified explicitly"). Pin with `version = "=2.0.0-rc.10"`.
- **Finding 2:** With default features (`download-binaries`), ort-sys statically links a libonnxruntime.lib built with MSVC 2022 STL. On VS 2019 BuildTools, this triggers `LNK2001: unresolved external symbol __std_find_trivial_8` across dozens of `.obj` files.
- **Action 1:** Switched to `default-features = false, features = ["load-dynamic", "ndarray"]`. Skips the static lib; `dlopen`s `onnxruntime.dll` at runtime. **Version-locked:** rc.10 demands ONNX Runtime 1.22.x exactly. A 1.20.x DLL panics at startup with `expected GetVersionString to return '1.22.x', but got '1.20.1'`.
- **Action 2:** Wave 3 pinned `rc.10`, not `rc.12`. rc.12 has an internal compile bug under `load-dynamic` + default-features-off: `no field SessionOptionsAppendExecutionProvider_VitisAI on type &OrtApi`. Until that's fixed upstream, downgrade.
- **Action 3:** Added `scripts/download-onnxruntime.ps1` that fetches v1.22.0 specifically and tells you what to set `ORT_DYLIB_PATH` to. Production bundling (DLL alongside the .exe in Tauri's resources) is a Phase 4/5 concern.
- **Lesson:** When upstream is in RC, version-pin tightly and read release notes for every patch bump. ort's `load-dynamic` feature is a god-tier escape hatch when static link demands a newer toolchain than you have.

## 2026-05-16 [phase-2] Silero VAD model lives under `src/silero_vad/data/` now, not `files/`
- **Context:** Wave 1 manifest pointed at `https://github.com/snakers4/silero-vad/raw/master/files/silero_vad.onnx`. Download succeeded with HTTP 200 but the connection terminated mid-stream — the path is no longer canonical.
- **Finding:** snakers4 reorganized the repo around the Python `silero-vad` PyPI package. The ONNX file moved to `src/silero_vad/data/silero_vad.onnx`. The `raw.githubusercontent.com` host is more reliable than `github.com/.../raw/...` redirects.
- **Action:** Updated manifest URL + pinned the real SHA-256 (`1a153a22...`) and size (2,327,524 bytes). Documented the deprecation in the manifest `notes` field.

## 2026-05-16 [phase-2] `Box<dyn Trait>` brings trait methods into scope automatically
- **Context:** Wave 2 integration test imported both `make_default_capture` and `AudioCapture`. Clippy `-D warnings` flagged `AudioCapture` as unused even though I was calling `.start()`, `.drain()`, etc. on the returned `Box<dyn AudioCapture>`.
- **Finding:** When a value's type is `Box<dyn Trait>` (or `&dyn Trait`, etc.), the trait's methods are accessible **without** an explicit `use` for the trait. The trait is implicitly in scope via the type. Opposite of the usual "trait methods require trait in scope" rule for `impl Trait for ConcreteType` calls.
- **Action:** Drop redundant trait imports when working with `Box<dyn Trait>` return values. Rule of thumb: if you have a `Box<dyn Foo>` or `&dyn Foo`, you don't need `use crate::Foo;` to call Foo's methods.

## 2026-05-16 [phase-2] cpal::Stream is `!Send` on Windows
- **Context:** Wave 2 brief specified `AudioCapture: Send`. cpal's `Stream` on Windows is NOT `Send` (WASAPI handles are thread-bound).
- **Finding:** Wrapping a non-Send field in a struct that impls a `Send`-bounded trait fails with `*mut c_void` cannot be sent between threads safely`. The trait bound has to go.
- **Action:** Dropped `Send` from `AudioCapture`. Doc comment explains why; Phase 5 owns the recording thread. **Generalized lesson:** don't add `: Send` speculatively — add it when you need it, and be ready to discover an OS won't satisfy it.

## 2026-05-16 [phase-2] `cpal::Host` is `!Clone` — re-resolve in worker threads
- **Context:** Wave 2 device watcher needed Host access from a spawned thread. `let host = self.host.clone()` failed: `Host: !Clone`.
- **Finding:** cpal's `Host` is a per-platform singleton with no public clone path. Idiom: re-resolve via `cpal::default_host()` inside the thread (cheap; struct construction).
- **Action:** In `spawn_watcher`, call `let host = cpal::default_host();` inside the closure. No Host capture from outer scope.

## 2026-05-15 [bootstrap] bd (beads) lives next to STATUS.md, not instead of it
- **Context:** PLAN.md predates the decision to adopt `bd` for issue
  tracking. User asked during bootstrap if we were using beads.
- **Finding:** `bd` is the live, dependency-graph-aware task queue; STATUS.md
  is the human-readable phase snapshot the PLAN, judges, and hooks expect
  at iteration boundaries. They serve complementary roles — keep both in
  sync end-of-iteration.
- **Action:** Every iteration: `bd close <completed-ids>`, `bd create` for
  discovered work, AND update STATUS.md.

## 2026-05-15 [bootstrap] bd init is interactive and will timeout in a non-TTY
- **Context:** `bd init --prefix mb` ran for >60s because it prompted
  "Contributing to someone else's repo? [y/N]".
- **Finding:** The initial run *did* create `.beads/` before timing out;
  re-running fails because it sees the partial state.
- **Action:** Pipe `"N"` (PowerShell `'N' |`) to skip the prompt, or use a
  `--non-interactive` flag if/when bd adds one. If init is partial, just
  proceed with `bd status` — the partial state is usable.

## 2026-05-15 [bootstrap] PowerShell + native CLIs treat stderr as terminating
- **Context:** `bd create` emits a one-line warning to stderr
  ("beads.role not configured"); with `$ErrorActionPreference = "Stop"`
  PowerShell threw on the first call.
- **Finding:** PS 7's `$PSNativeCommandUseErrorActionPreference = $false`
  is the right escape hatch for native-command stderr noise.
- **Action:** In any PS script that wraps native CLIs, set
  `$PSNativeCommandUseErrorActionPreference = $false` at the top.

## 2026-05-15 [bootstrap] Hook scripts must decode subprocess stdout themselves on Windows
- **Context:** `session-start-briefing.py` crashed with cp1252 UnicodeDecodeError
  when reading `bd ready` output (em-dashes were not cp1252-encodable).
- **Finding:** `subprocess.run(..., text=True)` uses the locale codec on
  Windows (cp1252), not UTF-8.
- **Action:** Always pass `capture_output=True` without `text=True`, then
  decode `.stdout` as `utf-8, errors='replace'`. The shared
  `scripts/hooks/_lib.py` has examples.

## 2026-05-15 [phase-0] rust-toolchain.toml is a PIN, not an MSRV declaration
- **Context:** Added `rust-toolchain.toml` with `channel = "1.77"` thinking
  it would declare the project's MSRV. The next `rustc --version` call
  triggered rustup to install Rust 1.77 (a downgrade from the dev's
  installed 1.93), hanging the whole shell for ~40s.
- **Finding:** `rust-toolchain.toml` is a hard pin — rustup auto-installs
  the channel on *any* cargo/rustc invocation in that directory. MSRV
  (minimum supported) is a separate concept and belongs in
  `Cargo.toml`'s `[package] rust-version = "..."` field.
- **Action:** Do NOT commit `rust-toolchain.toml` unless you genuinely
  want every developer on the same exact Rust version. For "works on
  1.77+", use `rust-version = "1.77"` in `Cargo.toml` (added in Phase 1).
  Side lesson: `Get-Command rustc` in PowerShell will also block on the
  toolchain auto-install — diagnosis was confusing because the hang
  surfaces as a `Get-Command` hang, not a `rustup install` log.

## 2026-05-15 [bootstrap] Secret-scan hook needs a known-public-prefix allowlist
- **Context:** The Tauri updater public key in STATUS.md tripped the
  high-entropy heuristic in `block-secret-commit.py` (152-char base64 token).
- **Finding:** Public keys are *intended* to be in repos; scanning them as
  secrets is a false positive. Cleanest fix: an allowlist of well-known
  public-material prefixes (`dW50cnVzdGVkIGNvbW1lbnQ6` = Tauri/minisign,
  PEM `-----BEGIN PUBLIC KEY-----`, `ssh-rsa `, etc.) plus an inline pragma
  `pragma: allow-secret-scan` for human-vetted edge cases.
- **Action:** When adding a new "secrets you intentionally commit"
  category, extend `KNOWN_PUBLIC_PREFIXES` in `scripts/hooks/block-secret-commit.py`
  with a comment justifying the prefix. Never disable the high-entropy
  check wholesale.

## 2026-05-15 [bootstrap] PowerShell param defaults can't use $PSScriptRoot
- **Context:** `scripts/seed-judges.ps1` set a default param to
  `Join-Path $PSScriptRoot ...`; PS evaluates defaults before binding
  $PSScriptRoot, so the path was empty.
- **Finding:** Compute path defaults in the *body* of the script, not in
  the `param()` block. Also: `Join-Path` is two-arg only —
  `[IO.Path]::Combine(...)` is the n-arg version.
- **Action:** Pattern: `param([string]$X = "")` + `if (-not $X) { $X = ... }`.

## 2026-05-15 [phase-1] cargo fmt fights git autocrlf on Windows
- **Context:** Phase 1 Wave 1 first `cargo fmt --check` failed:
  `Incorrect newline style in src-tauri/src/lib.rs` even though the files
  were written with LF.
- **Finding:** Git's Windows default `core.autocrlf=true` converts LF to
  CRLF on checkout. rustfmt with `newline_style = "Unix"` then reads back
  CRLF and fails. The two settings fight each other.
- **Action:** Drop `newline_style` from `.rustfmt.toml` (default = Auto,
  accepts file as-is). Add `.gitattributes` with `*.rs text eol=lf` to
  pin LF cross-platform on next checkout. gitattributes is the single
  source of truth; rustfmt becomes ending-agnostic.

## 2026-05-15 [phase-1] rustup minimal toolchains do not include rustfmt or clippy
- **Context:** Fresh Rust install attempting `cargo fmt` produced
  `cargo-fmt.exe is not installed for the toolchain stable-x86_64-pc-windows-msvc`.
- **Finding:** rustup ships only the compiler by default; rustfmt and
  clippy are components, not bundled.
- **Action:** Always `rustup component add rustfmt clippy` as part of
  dev setup. Phase 1 Wave 5 task `p1-lefthook-verify` should add this
  to `setup-dev.ps1`.

## 2026-05-15 [phase-1] First cargo check with rusqlite-bundled takes ~4 minutes
- **Context:** First `cargo check --workspace` after Phase 1 Wave 1.
- **Finding:** 247 seconds (4m07s) cold-cache. `rusqlite` features=["bundled"]
  compiles SQLite from C source (~150k lines). One-time cost; incremental
  builds are seconds.
- **Action:** Budget the cold compile when planning iterations on a
  fresh checkout. CI should cache `target/` aggressively. Do NOT panic
  when cargo check appears to hang for 3-4 minutes on a fresh clone.

## 2026-05-15 [phase-1] Wave-specific briefs ship integration-test pass rates above 90% on first compile
- **Context:** Phase 1 Wave 2 — migrations 001-003 + runner + 7 integration tests.
  The wave was preceded by `docs/phases/phase1-wave2-brief.md` (~300 lines)
  written end-of-Wave-1 by code-puppy with fresh context, capturing every
  design decision PLAN §7 didn't pin down: audit-trigger SQL extrapolated
  to all 4 tables, runner file layout with function signatures,
  integration-test specs with exact assertion counts, PLAN bug flagged
  (`dictionary.OLD.enabled` doesn't exist).
- **Finding:** With the brief, migration-author delivered 4 files in one
  shot. Compile produced 9 trivial `From<rusqlite::Error>` errors (mechanical
  fix — add a variant to AppError). **Tests: 15/15 passed first run, including
  all 7 cross-crate integration tests.** Zero 5-attempt escalations. Zero
  surprise architectural decisions made under pressure.
- **Action:** **Pattern: at the end of every iteration, write a brief for
  the next wave** with full context. Briefs that work well: full SQL/code
  snippets (not just "do X"), exact assertion counts, flagged source-doc
  bugs, explicit deviations from canonical (PLAN) with reasons, visibility
  notes for cross-crate concerns. The cost (~one iteration of context to
  write) pays back ~3x in implementation efficiency. Adopt for Waves 3, 4,
  5 of Phase 1 and every multi-iteration phase going forward.

## 2026-05-15 [phase-1] `#[cfg(test)]` does NOT carry across crate boundaries
- **Context:** Wave 2 brief originally specified `#[cfg(test)]` on
  `Database::open_in_memory()`. migration-author flagged: integration tests
  in `src-tauri/tests/db_migrations.rs` are a **separate crate** from the
  `src-tauri` library crate, so `#[cfg(test)]` items in `src-tauri/src/`
  are invisible to them.
- **Finding:** `#[cfg(test)]` only enables items when the **current crate**
  is being compiled in test mode. Integration tests (`tests/*.rs`) build
  the library crate in **release mode** (not test mode), then link against
  it as a regular dependency. Items needed by integration tests must be
  `pub` (or `pub(crate)` if behind a shim).
- **Action:** For any helper that integration tests need (test-database
  fixtures, `open_in_memory`, etc.): make it plain `pub` with a doc
  comment marking it test-oriented. If you want to discourage production
  callers, gate behind a Cargo feature like `test-helpers` instead of
  `#[cfg(test)]`.

## 2026-05-15 [phase-1] AppError variants are added per-module as the modules come online
- **Context:** Wave 2 db module's first compile failed with 9 instances of
  `From<rusqlite::Error>` not implemented for AppError.
- **Finding:** I (code-puppy) preloaded AppError in Wave 1 with `Io` and
  `Tauri` variants only — the others get added when their source modules
  first compile. This is the right pattern (YAGNI: don't pre-declare error
  variants for modules that don't exist yet) and the fix is mechanical
  (add one `#[error("sqlite error: {0}")] Sqlite(#[from] rusqlite::Error)`
  variant).
- **Action:** When a new module fails to compile with `From<...>` errors,
  the fix is always: add a `#[from]` variant to `AppError` in `error.rs`.
  Don't refactor to module-local error types — the AppError aggregator is
  the explicit project-wide pattern (per `.code_puppy/AGENTS.md` Rust
  conventions). When in doubt, check `error.rs` first.



### Delivered (5 waves, 5 commits + 4 brief commits + seal)

- **Wave 1** (`8e70d7c`): scaffolding, error aggregator, ADR 0004, Cargo workspace, tauri.conf.json. 5 tests.
- **Wave 2** (`b1f39ff`): migrations 001-003 (4 files), runner with PRAGMA + integrity_check + foreign_key_check, prompt_loader with token substitution. **15/15** tests first run.
- **Wave 3** (`7dada9d`): 7 DB repository modules (transcripts, prompts, dictionary, examples, search, sessions, audit) + `tests/db_repos.rs`. **77/77** tests after 2 trivial test-only fixes (raw-string quote count, SQL UNIQUE+NULL gotcha).
- **Wave 4** (`c7d3faa`): logging (rolling appender + PII scrub), settings (typed facade + 8-key registry), tray (placeholder menu), commands (3 IPC handlers), app wire. **101/101** tests **first run** — zero fixes needed.
- **Wave 5** (this commit): docs/CONTRIBUTING.md, docs/SETTINGS.md (binding), 3 judge cards, `#![warn(missing_docs)]` re-enabled, retrospective, seal commit + `phase-1-complete` tag.

### Final test count

**101 tests** across the workspace, all green:
- 88 unit tests inside `src-tauri/src/`
- 7 integration tests in `tests/db_migrations.rs`
- 6 integration tests in `tests/db_repos.rs`

### What worked

1. **The brief pattern.** End-of-wave handoff briefs (`docs/phases/phase1-waveN-brief.md`) that specify types, function signatures, test specs, known risks, and explicit deviations from PLAN. Outcome: 3 consecutive ~100% first-run test pass rates. The pattern is now the documented default for any multi-iteration phase.
2. **AppError aggregator with `#[from]` variants.** New modules add a variant when they bring a new source error type. Mechanical, predictable, no abstraction debt.
3. **`Database::open_in_memory()` (plain `pub`, not `#[cfg(test)]`).** Bridged the cross-crate test boundary; integration tests get a fully-migrated DB in ~5ms.
4. **Typed registries.** `SettingKey` enum + `default_value` + `try_parse` + `all()` makes adding a setting a 4-step mechanical edit with no string-typing.
5. **`AuditedTable` enum gating dynamic SQL.** Zero SQL-injection surface in the audit/rollback path despite needing to UPDATE/INSERT/DELETE arbitrary tables.
6. **Provenance-is-total enforced at API layer, not schema.** `NewSession` requires `i64` (not `Option<i64>`) for FKs that SQL leaves nullable. The schema and API deliberately disagree.

### What surprised us

1. **`#[cfg(test)]` doesn't carry across crate boundaries.** Integration tests in `tests/*.rs` are a separate crate; `pub` is required for helpers they consume.
2. **SQL UNIQUE treats NULL as distinct.** Two rows with `app_context: None` both pass a `UNIQUE(term, app_context)` constraint. Fix: test with non-null values, or use a partial INDEX with COALESCE.
3. **SQLite `CURRENT_TIMESTAMP` has 1-second granularity.** Audit-rollback tests would race within the same second. Workaround: `pin_latest_at` test helper that overwrites the `at` column to a synthetic timestamp after the trigger fires.
4. **`#![warn(missing_docs)]` is hostile to repo modules with self-documenting fields.** 163 warnings for fields like `pub id: i64`. Resolution: keep the lint at the crate level, allow at the module level for repo modules, doc the small-API modules (commands, tray, logging) properly.
5. **Rolling 4-minute cold `cargo check`** because `rusqlite-bundled` compiles SQLite from C. One-time cost. Document so future contributors don't panic.
6. **PowerShell `Select-String` matches inside comments** when counting code patterns. Anchor with `^` or run via SQLite for ground truth.
7. **`tracing_subscriber::try_init` is once-per-process.** Test isolation matters; only call inside test code that's certain it's the first.

### What we deferred (intentional, captured in phase ownership)

- **Mockall trait abstractions** (Wave 3 brief — YAGNI; Wave 4 didn't need them either). Reintroduce only when a specific command/UI surface needs to mock a repo.
- **DBOS** (bootstrap step 3 — skipped per project owner).
- **Pack agents** (deprecated upstream — `no-pack-agents` judge enforces).
- **Operator-aware FTS5 query parsing** (Phase 6 history viewer brief). Phase 1 ships conservative phrase escaping.
- **Audio retention enforcement** (Phase 5).
- **Real example ranking + auto-selection** (Phase 8 learning loop).
- **`ClaudeApiKeyRef` actual Credential Manager lookup** (Phase 4).
- **Tray icon state transitions** (Phase 5 recording lifecycle).
- **Cross-app injection** (Phase 3 — requires human at keyboard).
- **Lefthook live-fire verification** — lefthook binary not on dev machine PATH this iteration. Config in `lefthook.yml` looks correct. Install (`scoop install lefthook` or equivalent) and run a real commit through pre-commit; append observations here.
- **`missing_docs` polish for repo modules** — applied `#[allow]` at module level rather than doc-ing every self-evident field. Phase-6 UI work may add field-level docs where they matter.

### Carry-forward for Phase 2+

- **Brief pattern is now the default.** Every multi-iteration wave gets `docs/phases/phaseN-waveM-brief.md` written end-of-current-iteration with full context.
- **LESSONS.md is institutional memory.** Append non-obvious findings as you hit them, not at retrospective time.
- **STATUS.md is the canonical handoff document.** Resume instructions, last-judge line, cost line, blocked-on section all live there.
- **AppError aggregator pattern** generalizes. Phase 2 will add `Stt`, `Audio` variants; Phase 3 will add `Injection`; Phase 4 will add `Claude`.
- **Provenance-is-total at the API layer** is a project-wide principle, not a Phase 1 quirk. Future repos honor it.
- **The `phase-N-complete` tag SEALS its migrations.** Phase 2 ships migration 004+; the previous numbers are now frozen forever.
- **Test-density target:** ~10 tests per ~500 lines of code. Phase 1 hit ~100 tests / ~5,000 lines.

### Numbers for posterity

- **Files created:** ~30 (modules) + ~10 (docs) + ~10 (judges/briefs).
- **Lines of code:** ~5,000 Rust + ~1,500 SQL + ~3,000 markdown.
- **bd tasks closed:** 25/25 Phase 1 tasks (plus 11 Phase 0 tasks).
- **Commits:** 9 (bootstrap + Phase 0 + Wave-1-brief + Wave 1 + Wave-2-brief + Wave 2 + Wave-3-brief + Wave 3 + Wave-4-brief + Wave 4 + Wave-5-brief + Wave 5 + seal).
- **Test pass rates per wave:** W1 5/5, W2 15/15, W3 75→77 (2 test fixes), W4 101/101 first run, W5 101/101 still.

---

## 2026-05-15 [phase-1] SQL UNIQUE treats NULL as distinct (`NULL != NULL`)
- **Context:** Wave 3 dictionary repo test `unique_term_app_context_is_enforced`
  inserted two rows with `term='Foo', app_context=NULL` expecting the UNIQUE
  constraint to fire. Both inserts succeeded.
- **Finding:** Standard SQL semantics: `NULL != NULL` for purposes of
  UNIQUE constraints. Two rows with NULL in the same UNIQUE column are
  considered distinct and both allowed. This is a famous SQLite gotcha
  (also true in Postgres, MySQL, etc).
- **Action:** For null-equal-null semantics, use a partial UNIQUE INDEX
  on `COALESCE(col, '')` or similar — that's a schema change requiring
  a future migration. For Phase 1 we test the constraint with a
  non-null value where UNIQUE actually fires. Phase 6 dictionary UI
  may want the null-equal-null behavior.

## 2026-05-15 [phase-1] SQLite `CURRENT_TIMESTAMP` has 1-second granularity
- **Context:** Wave 3 audit-rollback tests insert→update→rollback. Each
  audit-trigger fire timestamps with `CURRENT_TIMESTAMP` which only has
  per-second resolution. Two operations within the same second get
  identical `at` values, breaking the `state_at` algorithm's ordering.
- **Finding:** Sleeping ≥1s between ops works but makes tests slow.
  Cleaner: after each real operation, UPDATE the just-created history
  row's `at` field to a known synthetic timestamp. The audit table has
  no constraint preventing this — it's an internal-record-of-fact
  table, not a contract. Pattern (added as `pin_latest_at` helper):
  ```rust
  conn.execute("UPDATE _history_X SET at = ?1 WHERE id = (SELECT MAX(id) FROM _history_X)", [ts])?;
  ```
- **Action:** Use synthetic `at` values for any test that depends on
  temporal ordering. Keep this trick test-only — production code
  trusts `CURRENT_TIMESTAMP`.

## 2026-05-15 [phase-1] `#![warn(missing_docs)]` is hostile to repo modules with self-documenting fields
- **Context:** Wave 1 added `#![warn(missing_docs)]` at the top of
  `lib.rs`. Wave 3 added 7 repository modules with ~60 public structs/
  enums/fields where the field name IS the documentation (`pub id: i64`,
  `pub term: String`, etc.). Clippy spammed 60+ missing-doc warnings
  and `clippy -D warnings` refused to ship.
- **Finding:** Mandatory module-level docs are valuable. Mandatory
  field-level docs are noise when the field name is self-evident.
- **Action:** Demoted `missing_docs` from `warn` to nothing for now;
  Wave 5 polish task will (a) add doc comments to non-self-documenting
  public items, (b) re-enable the lint, (c) `#[allow(missing_docs)]`
  on the obvious cases like `pub id: i64`. Don't blanket-enable lints
  faster than you can comply with them.

## 2026-05-15 [phase-1] PowerShell Select-String matches inside comments — grep regexes need context
- **Context:** Sanity-checking the trigger count after Wave 2: I expected 14
  triggers (per the brief), but `Select-String -Pattern 'CREATE TRIGGER'`
  returned 15.
- **Finding:** One of those matches was inside a `--` SQL comment in
  `002_audit_triggers.sql` ("-- new migration that CREATE TRIGGER IF NOT
  EXISTS-replaces the offender"). Substring match doesn't distinguish
  code from comments.
- **Action:** For exact code counts, anchor the pattern: e.g.
  `Select-String -Pattern '^CREATE TRIGGER'` (line starts with) or
  `'^\s*CREATE TRIGGER'` (optional indent). Or use `sqlite3 :memory: < file.sql`
  followed by `SELECT COUNT(*) FROM sqlite_master WHERE type='trigger'`
  for the ground truth. The integration test asserts the ground truth
  (`trigger_count_is_14`) and that's the canonical check.


## 2026-05-16 — Phase 3 Wave 4.8 — Test-only callers are landmines

**Symptom.** First hold-release dictation cycle worked end-to-end.
Second + third hold did nothing: no beep, no log entry, no audio
captured. The LL hook was firing fine, the orchestrator was alive,
the audio layer was restartable (Wave 4.8 fix #1) — but the §6.1
state machine sat in `Processing` forever and
`(Processing, _) => StateAction::None` silently dropped every event.

**Root cause.** `HotkeyStateMachine::complete_processing()` was the
designated `Processing → Idle` transition method. It had unit tests
(`complete_processing_returns_to_idle`,
`complete_processing_in_idle_is_noop`). It had docstrings. The
state-machine module looked complete.

It had **zero production callers**. `grep complete_processing`
across the whole codebase returned only the method definition + its
tests. The orchestrator never signalled "pipeline done" back to the
driver, so the machine never transitioned.

**Why it slipped through review.**
1. The state-machine unit tests called `complete_processing()`
   directly — they exercised the transition without exercising the
   wiring.
2. The orchestrator's own tests (`pipeline::tests`) tested decision
   logic, not the full action-loop.
3. There was no end-to-end smoke test that ran two consecutive
   hold-release cycles through the real runtime. Phase 3 Wave 5 is
   when integration testing was planned.

**The fix.**
- Added `HotkeyEvent::PipelineComplete` variant — back-channel from
  dictation thread to driver thread.
- State machine routes `PipelineComplete` to the existing
  `complete_processing()` method (idempotent: no-op outside
  `Processing`).
- Orchestrator's `handle()` wraps `StopCapture` + `DiscardAudio`
  with a `signal_pipeline_complete()` call AFTER the inner method
  returns — guaranteed signal even on pipeline error, no risk of
  losing the signal in an early-return error path.
- `PauseHandle::sender_clone()` exposes the existing hotkey channel
  Sender for the dictation thread to use (same channel as
  PauseToggle — preserves event ordering wrt KeyUp).

**Lessons.**
1. **`grep <method_name>` looking only at non-test results is a
   useful pre-commit check** for any method that's supposed to be
   wired into production. If the only callers are tests, it's not
   wired — regardless of how complete it looks.
2. **State-machine handshake protocols need a counterpart end-to-
   end smoke test**, not just unit tests of each transition. Two
   consecutive sessions is the minimum useful smoke for any state
   machine that has a "completion ack" step.
3. **Centralise mandatory signals in the dispatch site**, not the
   per-branch helpers. `handle()` wrapping
   `let r = self.complete(); self.signal_pipeline_complete(); r`
   guarantees the signal fires on every code path through complete,
   including the four `persist_failed_*` early-returns. Sprinkling
   the signal at every return site of complete() would have been
   DRY-violating and bug-prone.
4. **Audio capture restartability (Wave 4.8 fix #1) was a real bug
   masked by this one.** Both needed to be fixed. The producer-slot
   bug would have surfaced on cycle 2 if the state machine had
   actually transitioned out of Processing. Lucky-ish that both
   showed at once — could have been a long debugging session if
   they'd shown up sequentially.

---

## 2026-05-17 [phase3-wave4.9] K32GetModuleBaseNameW silently returns 0 under PROCESS_QUERY_LIMITED_INFORMATION

- **Context:** Bug B in the Wave-4 QA matrix — `sessions.foreground_app`
  was always empty string. Tracing showed `OpenProcess` succeeded,
  `QueryFullProcessImageNameW` returned the full exe path correctly,
  but `K32GetModuleBaseNameW` returned 0 → empty string → all
  downstream strategy resolution, per-app overrides, and audit
  joins silently broke.
- **Finding:** `K32GetModuleBaseNameW` (and the older
  `GetModuleBaseNameW`) require `PROCESS_QUERY_INFORMATION +
  PROCESS_VM_READ` on the process handle. Mockingbird opens with
  the lighter `PROCESS_QUERY_LIMITED_INFORMATION` (necessary to
  read protected processes — System, csrss, anti-cheat-protected
  game windows). Under that mask, `K32GetModuleBaseNameW` does
  NOT error — it returns 0 (= "0 chars copied") and sets nothing.
  The MSDN page mentions the access requirement only in passing
  and most online examples show `OpenProcess(PROCESS_ALL_ACCESS, ...)`
  which masks the issue. Stack Overflow answers that recommend
  the function rarely flag the access-mask trap.
- **Action:** Derive process basename from
  `QueryFullProcessImageNameW`'s output via
  `std::path::Path::file_name` instead. Same information, one
  fewer Win32 call, no access-mask surprises. The
  `basename_from_path` helper is pure + trivially unit-testable.
  Lesson generalises: any Win32 helper that returns "0 = failure"
  with no error-code path is suspect — assume access-mask
  sensitivity until proven otherwise.

## 2026-05-17 [phase3-wave4.9] Clipboard sequence baseline must be measured AFTER our write, not before

- **Context:** Bug C in the Wave-4 QA matrix — after a successful
  paste, the user's pre-dictation clipboard contents were lost
  (the dictation text remained). The
  `SequenceAnalysis::classify(seq_before_set, seq_after_paste)`
  classifier treated `seq_after_paste == seq_before_set + 1` or
  `+ 2` as "safe to restore". In practice we observed `+ 2` after
  our own writes (no target write yet) and `+ 3` after a normal
  paste → classifier returned `Diverged` → restore skipped.
- **Finding:** `EmptyClipboard` + `SetClipboardData` together
  advance the sequence number by an OS-dependent amount. Windows
  may fold consecutive ops into a single bump or count them
  separately; the count appears to vary across builds and
  possibly across clipboard-format-list state. Baselining off
  `seq_before_set` made the classifier brittle to that fold.
  Baselining off `seq_after_set` (measured immediately AFTER
  `write_unicode_text` returns) eliminates the dependency entirely
  — the classifier now answers only the question we actually care
  about: "did anything OTHER than our write happen?"
- **Action:** Always baseline post-mutation, not pre-mutation, when
  the mutation's own sequence cost is OS-internal. The classifier
  is correspondingly simpler:
  - `seq_after == seq_after_set` → target read-only paste (common).
  - `seq_after == seq_after_set + 1` → target also wrote (clipboard
    managers, dedupe).
  - Anything else → another writer; skip restore.

  Also: dropped the `wait_for_paste_sentinel` polling loop in
  favour of a fixed `PASTE_CONSUME_GRACE` (30 ms) sleep before
  the post-paste seq read. Read-only paste never advances seq,
  so the poll could only ever time out on the common case —
  burning 250 ms instead of 30. Deterministic sleep > clever
  poll when the "clever" signal is absent for the dominant path.

## 2026-05-17 [phase3-wave4.9] Hard-coded test count is a bad smoke test for "did the refactor work"

- **Context:** During Wave-4.9 I almost grepped for "303 passed"
  as the success criterion after refactoring transcripts +
  classifier + permissive focus. The refactor changed the test
  count (removed 5 tests, added 9 → net +4) so a count match
  would have been a false negative.
- **Finding:** `cargo test`'s "N passed; 0 failed" line is the
  real gate. The count is only meaningful when compared against
  the expected delta for the iteration. A grep for "0 failed"
  is correct; a grep for a specific passed count is brittle.
- **Action:** Smoke commands should grep `FAILED|panicked|0 failed`
  (the first two are negative checks, the third confirms the
  test runner finished), never a hard-coded `N passed` literal.

## 2026-05-17 [phase3-wave4.9] Running mockingbird.exe locks itself; `cargo build --release` fails with os error 5

- **Context:** During Wave-4.9 verification I asked Dustin to re-launch
  after a release rebuild. He hit
  `error: failed to remove file ... mockingbird.exe / Caused by:
  Access is denied. (os error 5)` on cargo's pre-link cleanup.
- **Finding:** Windows holds an exclusive lock on a running .exe's
  image file. cargo's link step tries to overwrite
  `target/release/mockingbird.exe` in-place, which fails until the
  running process exits. Unix devs are surprised by this because
  Linux allows overwriting open files via inode swap (the running
  process keeps its open inode; new file gets a fresh one). The
  Windows error message doesn't mention "running process" — it
  just says "Access is denied", which is a misleadingly generic
  diagnostic for what is fundamentally a per-OS semantic
  difference.
- **Action:** `scripts/run-mockingbird.ps1` now accepts a `-Force`
  flag that kills any running `mockingbird.exe` before launching,
  with a 300 ms sleep to let Windows release the file lock.
  Canonical rebuild-and-relaunch dance is now:
  1. `pwsh scripts/cargo-with-cuda.ps1 build --release`
  2. `pwsh scripts/run-mockingbird.ps1 -Force`

  If step 1 fails with os error 5, that's the signal a previous
  instance is still running — `taskkill /F /IM mockingbird.exe`
  and retry, or just use `-Force` on the next `run-mockingbird`
  invocation. Generalises: any Rust binary that does `dlopen` of
  cudart / onnxruntime / etc. hits this on rebuild; the
  `-Force`-style kill is the standard Windows answer.

## 2026-05-18 [phase-3-wave-5] Orchestrator integration tests want stubbed traits, not a real `DictationRuntime` spawn

- **Context:** Wave 5 needed end-to-end tests that drive `DictationOrchestrator::run` through a full `StartCapture → StopCapture` cycle so the new judges (`e2e-injection`, `db-provenance`, `secure-input-respected`) had real CI targets instead of just wrapping the pure `pipeline::decide` unit tests.
- **First instinct (wrong):** spawn the real `DictationRuntime` from `dictation/runtime.rs` and use `HotkeyEvent::PipelineComplete` for synchronisation. That pulls in a real low-level keyboard hook, a real `WinSecureInputGuard`, and a real `GetForegroundWindow` — none of which work on a headless test runner.
- **What actually works:** put the test in `src-tauri/tests/dictation_orchestrator.rs` (separate crate, depends on `mockingbird_lib` like any consumer). Stub every trait the orchestrator takes (`AudioCapture` / `VoiceActivityDetector` / `SpeechToText` / `Cleaner` / `Injector` / `WindowContext` / `SecureInputGuard`). Use `Database::open_in_memory()` for SQLite + `default_normal_config` for FK seeding. Use a plain `std::sync::mpsc` pair for `StateAction`. The orchestrator's `run(rx)` iterates `rx.iter()` and terminates when the sender drops — so the test pushes its two actions, drops `tx`, then calls `run(rx)` inline. No threads, no synchronisation primitives beyond the channels the orchestrator already owns.
- **Generalisation:** when an orchestrator has `Box<dyn Trait>` deps for every I/O surface, integration tests should mirror the pattern: in-memory DB + stub traits + drop-the-sender to terminate the loop. The Phase 4 `LlmCleaner` integration test should follow the same recipe, swapping `PassthroughCleaner` for a `StubLlmCleaner` that returns a deterministic transformation, so the e2e-injection judge can prove the LLM is actually in the loop.
- **Gotcha:** `rustfmt` rewrites long `assert_eq!` lines aggressively. Always run `pwsh scripts/cargo-with-cuda.ps1 fmt` (no `--check`) before pushing — a fmt-check failure blocks the `mb-quality-bar` judge AND surfaces noise about unrelated files (e.g. Wave 4.9's `examples/verify_wave49.rs` was already not-clean and got swept up in Wave 5's pre-tag check).


## 2026-05-18 [phase5/6 UI sprint] CSS Modules need a vite-env.d.ts to satisfy tsc

- **Context:** UI bundle compiled fine via Vite alone, but `tsc --noEmit`
  (the first step of `npm run build`) flagged every `import styles from
  "./Foo.module.css"` as TS2307. Even `@vitejs/plugin-react` doesn't add
  the module ambient declaration on its own — that's `vite/client` types
  via a triple-slash reference.
- **Finding:** A `ui/src/vite-env.d.ts` with `/// <reference types="vite/client" />`
  plus ambient declarations for `*.module.css`, `*.module.scss`, `*.css`
  is what unblocks `tsc`. Vite's docs imply this file is auto-created
  by `npm create vite`, but a hand-rolled Phase 5 scaffold needs it
  explicitly.
- **Action:** When standing up a new Vite + TS workspace, drop a
  `vite-env.d.ts` next to `main.tsx` BEFORE writing the first
  `.module.css` import. Saves a round-trip through Playwright/CI to
  notice.

## 2026-05-18 [phase5/6 UI sprint] Tauri `commands.rs` (file) vs `commands/` (dir) conflict

- **Context:** Splitting the IPC surface into per-feature sub-modules
  (`commands/insights.rs`, `commands/sessions.rs`, ...) collided with the
  Phase 1 `commands.rs` file that held `AppState` + `get_setting` /
  `set_setting` / `fts_smoke_test`. Rust 2024 module resolution
  rejects having both `foo.rs` and `foo/mod.rs` at the same depth.
- **Finding:** Delete the old file and move its contents into a
  `commands/legacy.rs` sub-module. Keeping both command surfaces
  side-by-side (typed `SettingKey`-JSON vs flat string/string) is fine
  because Tauri command names are globally unique — `get_setting` vs
  `get_settings` (different by one letter) is enough. The `AppState`
  struct stays re-exported from `commands/mod.rs` so the call site
  `use crate::commands::AppState` in `lib.rs` keeps working unchanged.
- **Action:** Treat `pub mod commands` as a directory from day one in
  Tauri projects, even when it contains only one file initially.
  Cheaper than the inevitable refactor when the IPC surface grows past
  3 commands.

## 2026-05-18 [phase5/6 UI sprint] `tauri::AppHandle` in command signatures bounces with CommandArg error

- **Context:** Wanted `#[tauri::command] pub fn app_paths(app: AppHandle) -> ...`
  so the path resolver could return canonical app-data + log dirs.
  Compiler bounced it: `the trait Deserialize<'_> is not implemented for AppHandle`.
- **Finding:** Inside a `#[tauri::command]` fn the runtime extracts
  `AppHandle<R>` from the invoke context, NOT from JS-side args. The
  `generate_handler!` macro should detect that and skip the
  `CommandArg` impl — but in our setup (tauri 2.x with the runtime
  generic erased at the registration site) it didn't.
- **Action:** For app-data / logs / models paths, bypass `AppHandle`
  entirely and read `APPDATA` + `USERPROFILE` env vars the same way
  `logging::init` and `lib::run` do. Matches the rest of the runtime's
  resolution logic. If we hit a case where Tauri's `path_resolver` has
  overrides we truly need, revisit by adding the runtime generic to
  the command: `pub fn app_paths<R: tauri::Runtime>(app: tauri::AppHandle<R>)`.

## 2026-05-18 [phase5/6 UI sprint] rusqlite closure type inference needs explicit annotations under `?`-chains returning String

- **Context:** Several new command modules did this pattern:
  ```rust
  let mut stmt = conn.prepare(SQL).map_err(|e| e.to_string())?;
  let rows = stmt.query_map([], |r| Ok(MyRow { ... })).map_err(|e| e.to_string())?;
  ```
  Half the closures failed with E0282 ("type annotations needed"). The
  outer `?` operator was wired to `Result<_, String>` via `map_err`
  but the closure return type couldn't be inferred backwards from
  there.
- **Finding:** Two cleaner fixes:
  1. Hint the closure: `|r: &rusqlite::Row<'_>| -> rusqlite::Result<MyRow> { ... }`.
  2. Use a generic `into_err<E: Display>(e) -> String` helper instead of
     `|e| e.to_string()` closures everywhere — the function pointer's
     type signature does the inference.
  We picked (2) because it also dedupes the `map_err` body and reads better.
- **Action:** Add `commands::into_err` (or similar) to any
  command-module crate before writing the first `.map_err`. Future
  authors will copy the pattern without thinking about closure
  inference.


## 2026-05-17 — Vite CSS `@import` does not resolve absolute `/public` paths

- **Context:** Design Language Phase Wave 1 (mb-9pw). Adding self-hosted Latin WOFF2s under `ui/public/fonts/` plus a generated `fonts.css` next to them. Wanted to pull `fonts.css` into the bundle from `ui/src/design/global.css` with `@import "/fonts/fonts.css";` so a single CSS entry kept all design-system styles together.
- **Symptom:** TS build wouldn't have flagged it (we run `tsc --noEmit && vite build` and CSS imports go through Vite's plugin), but Vite's CSS resolver treats `@import` strings as module specifiers, not URL paths. An absolute path like `/fonts/fonts.css` either gets resolved against the source tree (not the `public/` root) or silently dropped from the bundle, depending on Vite version. Either way the WOFF2 references in the imported CSS end up broken in production.
- **Finding:** `public/` is for files you want served verbatim. Reference them with `<link rel="stylesheet" href="/fonts/fonts.css">` in `index.html` (and `recording.html` for the recording overlay window) — the browser fetches them as plain static assets and the relative `url(./DMSans-latin.woff2)` references inside resolve correctly against the served URL.
- **Action:** Both Tauri webview entry points (`ui/index.html`, `ui/recording.html`) now `<link>` the fonts stylesheet. The token + typography CSS still lives in `src/design/` and gets `@import`ed via the JS-imported `global.css` — that path works fine because both files are inside `src/` and Vite's resolver is happy with relative source-tree paths. Future rule: anything under `public/` is loaded via `<link>` / `<script>` in HTML, never via JS-imported CSS `@import`.
