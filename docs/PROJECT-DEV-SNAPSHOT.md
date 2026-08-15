# Mockingbird Project Development Snapshot

**Snapshot of:** commit `ce4d1b5b2a5424ea8187eb8b3d0c27ea60465800`, release tag `v0.3.0-beta.3`.

## 1. Purpose, and how to use this file

This file is written **for an AI coding agent**, not for a human to read
start to finish.

The intended workflow is: you fork Mockingbird, and before your agent
writes a single line you tell it *"read `docs/PROJECT-DEV-SNAPSHOT.md`
first."* The agent then starts with the maintainer's context: what this
project is actually for, which seams were designed to be replaced, which
invariants are load-bearing, and which traps already cost somebody a day.

Three things to hold in mind:

1. **This is a snapshot, not a living document.** It reflects `main` at
   the commit and tag named above. The code moves; this file does not.
   If the code and this file disagree, the code is right.
2. **This is the maintainer's view.** It encodes one person's opinions
   about scope, product shape, and what is worth building. It is not
   neutral and does not try to be.
3. **You should disagree with it where your fork diverges.** If you are
   forking to build something the maintainer chose not to build, the
   "non-goals" section is a list of things to ignore, not obey. Take the
   invariants and the lessons; treat product opinions as context.

The maintainer's own working notes (status logs, lesson journals, phase
plans, decision records) are gitignored and not in your clone. This file
is the distilled, portable subset of them, and nothing here links to a
file you cannot see.

## 2. What Mockingbird is, and what it is NOT

Mockingbird is one binary that does three things:

1. **Voice dictation.** Push-to-talk on a global hotkey. Whisper
   transcribes locally, an optional local LLM tidies the text, the result
   is pasted into whatever app has focus, and your previous clipboard
   contents are restored afterwards.
2. **Meeting capture.** A chord toggles long-form recording. Microphone
   and system audio are captured in parallel, transcribed in rolling
   30-second windows, and merged into a two-speaker Markdown transcript.
3. **Personal knowledge engine capture.** Fast notes get classified,
   entity-extracted, and projected into an Obsidian vault as wiki-linked
   Markdown.

Everything runs on the user's machine. There is no telemetry, no
analytics, no crash reporting, no account system.

### The North Star: Mockingbird is the capture layer, NOT the wiki author

**This is the single most important thing in this file.** If you miss
it, you will build the wrong product, and you will build it competently,
which is worse.

Mockingbird's knowledge-engine work follows the Karpathy "LLM Wiki"
pattern and Alvin Clark's *Building a Personal Knowledge Engine with LLMs
and Obsidian*. The lineage goes back to Vannevar Bush's Memex (1945). The
part Bush could not solve was who does the maintenance. An LLM does the
maintenance. That is the whole idea.

That gives you a **two-agent split**, and the split is the architecture:

- **Mockingbird** is the capture and first-pass synthesis layer. It
  captures audio or text, cleans it, classifies it, extracts entities,
  emits wiki-links, auto-generates stub pages for entities, projects, and
  tags, and maintains an index and an append-only log. It works
  per-segment, in isolation, with no cross-vault reasoning, calibrated
  for roughly one minute of intake latency because it sits between a
  person having a thought and that thought being filed.
- **The user's chat-LLM** (Claude Code, Cursor, or whatever they use) is
  the **wiki author**. It reads the schema contract that Mockingbird
  ships into the vault and performs the three deep operations: **Ingest**
  (drop a source in, ripple the consequences across ten or fifteen
  pages), **Query** (answer a question with citations, then file the
  answer back as a new page), and **Lint** (find contradictions, orphans,
  stale claims, missing concepts).
- **The Obsidian vault is the knowledge codebase.** Obsidian is the IDE.
  The vault is plain Markdown on disk, which is exactly why a chat-LLM
  can work on it: it is a codebase, and coding agents are good at
  codebases.

The vault is organized in three layers. **Layer 1** is raw and immutable:
Mockingbird writes it, nobody edits it. **Layer 2** is the LLM-maintained
wiki: Mockingbird authors a shallow first pass (entries plus stub pages
for entities, projects, and tags) and the user and their chat-LLM own
everything after that. **Layer 3** is the schema contract, a
vault-resident file that is write-once and user-owned and tells any LLM
how to maintain this particular vault. Preferences travel with the file,
not with the model, so the vault stays portable across chat-LLM products.
The shipped source of that contract is
`src-tauri/src/kg/assets/SCHEMA.md`.

**Do not build Mockingbird-side ingest-ripple, query, or lint engines.**
Those three operations belong to the chat-LLM, which already has the
tools, the context window, and the whole-vault view to do them properly.
Mockingbird's "ingest" is deliberately shallow.

The knowledge vocabulary is nine shapes: `source`, `note`, `concept`,
`entity`, `project`, `question`, `decision`, `reference`, `observation`.
Notably **not** `task` and **not** `event`.

### What Mockingbird is NOT

- **Not a task manager.** No Kanban boards, no due-date workflows, no
  status-cascading projects. Obsidian's grain is knowledge-graph-first,
  not task-management-first, and fighting that grain produces a mediocre
  Things clone instead of an excellent capture layer. Users who want
  tasks can opt into the Obsidian Tasks plugin; that is an ecosystem fit,
  not a load-bearing default.
- **Not a wiki author.** See above. This bears repeating because it is
  the most tempting wrong turn in the codebase.
- **Not a cloud product.** No accounts, no sync service, no server-side
  anything. Cross-device knowledge sharing happens through the user's own
  vault sync (iCloud, Dropbox, whatever), not through infrastructure the
  project operates.
- **Not a platform.** There is no plugin API and no extension surface.
  The codebase is the extension surface. Fork it.
- **Not remotely configurable.** No feature flags, no A/B tests, no
  server-driven config. What ships is what runs.

### One piece of portable design guidance from the KG research

"Tags" and "entities" are **different object types** and should not be
collapsed into one field. A tag is an open-vocabulary topical label. An
entity is a typed, referenceable thing that deserves its own page and its
own backlinks. They have different cardinalities, different lifecycles,
and different normalization rules. Modelling them as one string list is
convenient for about a week and then permanently wrong. Closed-vocabulary
tagging and entity extraction were both researched at length against a
private corpus; those results are only meaningful against that corpus.
The type distinction is the portable part. The rest was calibration.

## 3. Current state by subsystem and platform

Version at this snapshot: **0.3.0-beta.3** (matches `CHANGELOG.md` and
`Casks/mockingbird.rb`).

### Platform matrix

| Subsystem | Windows 10/11 | macOS 15+ (Apple Silicon) |
|---|---|---|
| Voice dictation | Ships | Ships |
| Meeting capture | Ships | Ships |
| Local LLM cleanup | Ships | Ships |
| Cloud LLM cleanup (opt-in) | Ships | Ships |
| Activity capture | Ships | Not built |
| Knowledge graph / vault projection | Ships | Not built |
| Mobile sync / inbox courier | Ships | Not built |
| STT acceleration | CUDA (optional) or CPU | Metal |
| Secrets storage | DPAPI | Keychain |
| Installer | Unsigned MSI (CPU and CUDA variants) | Ad-hoc signed `.dmg` + Homebrew cask |

**Be honest about this when planning work:** activity capture, the
knowledge graph, and mobile sync are **Windows-only**. They are not
"mostly ported"; they are not ported. If your fork is Mac-first and wants
the knowledge engine, that is real work, not a config flag.

### Distribution facts

- Windows ships two MSIs per release: a CPU build (about 9 MB download)
  and a CUDA build (about 580 MB download) that bundles the CUDA runtime
  DLLs so no separate Toolkit install is needed.
- macOS ships `Mockingbird_x.y.z_aarch64.dmg` (Apple Silicon only, no
  Intel build) plus a Homebrew cask in `Casks/mockingbird.rb`.
- Every release tag builds **both** platforms. A release whose changelog
  only mentions macOS fixes still ships fresh Windows installers.
- Neither platform is properly code-signed. Windows MSIs are unsigned and
  trip SmartScreen. The macOS `.app` is **ad-hoc** signed, which stops
  the "damaged and should be uninstalled" error but is not a Developer ID
  and is not notarized, so Gatekeeper still needs one manual approval.
- Release builds come from `.github/workflows/release.yml` on a `v*.*.*`
  tag and land as a **draft** release for manual review before publish.

### Maturity notes

Dictation and meeting capture are the mature surfaces; they have been in
daily use the longest. The knowledge graph pipeline is complete end to
end (segment, classify, extract, extract entities, normalize, file,
project to vault, reconcile edits back) but expects a reasonably capable
local model. The reverse-watcher reconciles individual Obsidian edits at
roughly three seconds median latency. The Tauri updater is wired but
disabled, because enabling an update channel before the builds are signed
would be a bad trade.

## 4. Architecture and the intentional seams

Mockingbird is a Tauri 2 app: a Rust backend hosting a React 19 +
TypeScript frontend. Rust owns audio, STT, storage, the global hotkey
hook, paste injection, and all filesystem IO. TypeScript owns the UI and
the overlay windows. They talk over Tauri's `invoke` boundary through
typed commands in `src-tauri/src/commands/`.

`ARCHITECTURE.md` in the repo root is the full subsystem map. This
section covers only the **seams**: the places where the architecture was
deliberately shaped so you could replace a piece without touching
anything else.

### Seam 1: the cleanup provider

`src-tauri/src/cleanup/`

`CleanupProvider` in `src-tauri/src/cleanup/provider.rs` is the trait.
Two implementations ship: `ollama.rs` (local, the default) and
`claude.rs` (cloud, opt-in, bring your own key). There is also a
higher-level `Cleaner` trait in `src-tauri/src/cleanup/mod.rs`.

To add OpenAI, Gemini, llama.cpp, or an in-process model: implement
`CleanupProvider`, add a provider enum variant, add a settings tab. No
other subsystem needs to know you did it. This is the cheapest useful
fork and a good first change to prove your build works.

Prompts are versioned and live in the database, seeded by migrations,
with the Markdown sources under `src-tauri/src/cleanup/prompts/`. Three
dictation modes (casual, normal, formal) each pick a provider, a model, a
prompt version, and a per-pass system header. Short utterances bypass
cleanup entirely as a latency guard, and a length-ratio shrink fallback
catches LLM runaway.

### Seam 2: the STT wrapper

`src-tauri/src/stt/`

`SpeechToText` in `src-tauri/src/stt/mod.rs` is the trait. The
whisper.cpp binding lives behind it in `src-tauri/src/stt/whisper.rs`.
The trait surface is small on purpose: audio in, transcript events out.

Voice activity detection is a separate seam. `VoiceActivityDetector` in
`src-tauri/src/audio/vad.rs` wraps Silero VAD running on ONNX Runtime.
VAD gates Whisper invocations and trims leading and trailing silence.

A fork targeting Groq, Deepgram, faster-whisper, or a remote Whisper
server implements `SpeechToText`. Be aware that the meeting-capture path
depends on segment-level output for its deterministic merge, so a
provider that only returns a flat string will need work there.

### Seam 3: capture surfaces

Dictation, meeting capture, and knowledge-graph capture are three
capture surfaces over a shared spine. Adding a fourth means following the
dictation orchestrator pattern:

```
trigger -> capture session -> STT -> optional cleanup -> sink
```

The sink is the interesting part. Dictation's sink is a clipboard paste.
Meeting capture's sink is a merged Markdown transcript in SQLite. The
knowledge-graph sink is a filing queue that feeds a vault projector.
Yours could be a file, an HTTP call, or a database row.

Relevant directories, all publicly tracked: `src-tauri/src/audio/`
(microphone capture, resampling, VAD), `src-tauri/src/dictation/` and
`src-tauri/src/dictation.rs`, `src-tauri/src/meetings/`,
`src-tauri/src/activity/` (foreground polling, block summarization),
`src-tauri/src/kg/` (the five-pass pipeline), `src-tauri/src/vault/`
(Obsidian projection), `src-tauri/src/inbox/` (mobile capture courier),
and `src-tauri/src/command_center/` (the unified recording front door).

Note the deliberate sibling-not-generalization pattern: meeting capture,
activity capture, and the knowledge graph are **siblings** of dictation,
not generalizations of it. Each one that tried to become a generalization
got pushed back to a sibling. Dictation is the hot path and the one users
notice when it regresses; keeping it uncoupled is worth some duplication.

### Seam 4: the platform layer behind `#[cfg(target_os)]`

Every platform-specific concern sits behind a trait with per-OS
implementations in separate files. This is the seam that made the macOS
port tractable: the platform specifics slotted in behind existing traits
and the shared orchestration was untouched.

| Concern | Trait and module | Implementations |
|---|---|---|
| Global hotkey | `HotkeyListener` in `src-tauri/src/hotkey/mod.rs` | `hotkey/windows.rs`, `hotkey/macos/mod.rs`, `hotkey/linux.rs` |
| Paste injection | `Injector` in `src-tauri/src/injection/mod.rs` | `injection/windows.rs`, `injection/macos.rs`, `injection/linux.rs` |
| Secure-input detection | `SecureInputGuard` in `src-tauri/src/injection/secure_guard.rs` | UI Automation on Windows, AX API on macOS |
| Secrets | `SecretStore` in `src-tauri/src/secrets/mod.rs` | `secrets/windows.rs` (DPAPI), `secrets/macos.rs` (Keychain), `secrets/stub.rs` |
| Window context | `WindowContext` in `src-tauri/src/window_context/mod.rs` | `window_context/windows.rs`, `window_context/macos.rs`, `window_context/linux.rs` |
| Audio capture | `AudioCapture` in `src-tauri/src/audio/mod.rs` | `cpal` for microphone everywhere; system-audio loopback is the hard part |

The Linux files exist as stubs and scaffolding, not as a working port.
Treat them as the shape to fill in, not as a running implementation.

System-audio loopback is the genuinely hard platform problem. Windows
uses WASAPI loopback. macOS uses ScreenCaptureKit in a unified
single-session capture, which is why the macOS floor is 15.0 (Sequoia)
and not lower. Linux will need a display-server-specific answer.

`docs/research/macos-implementation-notes.md` is tracked and public and
was written specifically to enable a forker doing this kind of work.

### Seam 5: storage

`src-tauri/src/db/`

SQLite via `rusqlite`. Migrations are numbered files in
`src-tauri/src/db/migrations/` (`001` through `030` at this snapshot),
driven by `src-tauri/src/db/migrations.rs`. Full-text search is FTS5,
maintained by triggers.

### The UI side

React 19, strict TypeScript, Tailwind v4. Routes are flat with a sidebar
and a page switch through a Zustand store, deliberately with no router
framework and no `@tanstack/*` packages. Design tokens live in
`ui/src/design/tokens-v2.css` and primitives in
`ui/src/design/components/`. Pages are in `ui/src/pages/`. Standalone
overlay windows (the dictation pill, the meeting pill, the command
center) are separate HTML entry points.

## 5. Invariants you must not break

These are not style preferences. Each one exists because breaking it
produces a specific, bad outcome. Several are enforced by git hooks
configured in `lefthook.yml` and implemented in `scripts/dev/hooks/`, so
you will find out mechanically if you violate them.

1. **Zero telemetry, ever.** No analytics, no crash reporting that leaves
   the machine, no phone-home of any kind. Crashes log to local files and
   stay there. *Why:* it is the entire value proposition. A dictation app
   sees everything the user says and everything they paste into. The
   moment any of that can leave the machine, the product is a different
   product. This is also why the landing page self-hosts its fonts rather
   than loading them from a CDN: a font request leaks every visitor's IP.

2. **Raw transcripts are immutable.** Rows in `transcripts` with
   `stage='raw'` are never UPDATEd. New facts about an utterance go into
   a new row. *Why:* provenance. If the raw layer can be rewritten, you
   can never prove what the model actually heard, and every downstream
   quality investigation becomes unfalsifiable. Enforced by the
   `block-raw-transcript-edit` hook plus application discipline. Note
   the honest detail: on the dictation `transcripts` table this is
   **hook-enforced, not trigger-enforced**. `001_initial.sql` says so
   in its header and records that a belt-and-suspenders trigger was
   deferred to a later migration. Later tables did get SQL triggers
   (activity events and the knowledge-graph mention tables raise
   `ABORT` on update). If you want belt and braces on `transcripts`,
   adding the trigger in a new migration is a reasonable fork
   improvement.

3. **Provenance is total.** Every session row pins the exact prompt
   version, dictionary snapshot, and example set that produced it.
   *Why:* prompt tuning here is empirical. Without pinned provenance you
   cannot tell whether last week's output was better because the prompt
   changed, the model changed, or the dictionary grew.

4. **Clipboard save and restore around every paste.** Save the user's
   clipboard, write your text, synthesize the paste, wait for the target
   to consume it, restore the original. *Why:* the user's clipboard is
   not your scratch space. Silently eating whatever they had copied is
   the kind of small betrayal that makes people uninstall. A
   `warn-bare-clipboard-set` hook watches for naked clipboard writes.

5. **Secure-input fields abort injection.** Before injecting, query the
   focused field. If it reports as a password or other credential input,
   abort and surface a toast. *Why:* pasting a mistranscription into a
   password manager is bad. Pasting a transcript into a password field
   that then gets logged somewhere is worse. Fail closed.

6. **Never run `npm install` without `--ignore-scripts`.** Enforced twice
   over: `ignore-scripts=true` in `.npmrc` plus the `block-unsafe-npm`
   hook. *Why:* postinstall scripts are the dominant npm supply-chain
   attack vector, and the failure mode is arbitrary code execution on a
   machine that holds signing material and a user's transcripts.

7. **Migrations are append-only.** Once a migration ships in a tagged
   release, it never changes. Defects get fixed forward by a new
   migration. *Why:* an edited migration means two users with the same
   version number have different schemas, and you cannot tell which is
   which. A hook blocks edits to the earliest migrations outright.

8. **File size cap: 600 lines**, Rust and TypeScript alike. *Why:* it
   forces module boundaries to be discovered early rather than
   retrofitted. Do not split purely to hit the number if it hurts
   cohesion, but treat crossing it as a signal.

9. **Layers are replaceable.** No provider or platform specifics outside
   their module. Platform code lives behind `#[cfg(target_os)]` and a
   trait even when only one platform is implemented. *Why:* this is the
   invariant that made the macOS port possible at all. It costs a little
   indirection continuously and saves an enormous refactor once.

10. **No shortcuts on testability.** If something is hard to test,
    refactor until it is testable. Hard-to-verify is itself the bug.

If your fork intends to break one of these, break it deliberately: rip
out the hook, write down why, move on. What you should not do is break
one by accident and find out from a user's bug report.

## 6. Hard-won lessons that travel

This is the payload of the file. Each entry is a trap, its root cause,
and what to do instead. These were all learned the expensive way.

### 6.1 Tauri 2: register managed state on the Builder, before `.setup()`

**The trap.** You write the canonical-looking thing:

```rust
tauri::Builder::default()
    .setup(|app| {
        app.manage(my_state);   // compiles, logs fine, does not work
        Ok(())
    })
```

It compiles. It runs. `app.try_state::<T>()` returns `Some` immediately
after the `manage()` call **and** again at the end of the setup closure.
Every probe says the state is registered. Then an IPC command with a
`State<T>` extractor fails at dispatch time with:

```
state not managed for field <name> on command <command>
```

**Root cause.** `App::manage()` called inside `.setup()` does not
reliably propagate to post-setup, webview-bound IPC `State<T>`
extractors. The two registration sites, `Builder::manage()` and
`App::manage()` inside setup, look interchangeable in the API, log
identically under tracing, and pass identical static analysis. They are
not interchangeable.

**What to do instead.** Construct your state **before** you call
`tauri::Builder::default()`, and register it on the builder chain
**before** `.setup()`:

```rust
let app_state = AppState::new(Arc::clone(&shared_conn));
let builder = tauri::Builder::default()
    .manage(app_state)          // must be pre-.setup()
    .plugin(...)
    .setup(move |app| { /* other init */ Ok(()) });
```

The wiring problem this creates: your state constructor probably needs
the app data directory, which is only available from the `App` handle
inside `.setup()`. Break that dependency by resolving the data directory
yourself from the platform environment plus your app identifier, and add
a unit test asserting your constant matches
`src-tauri/tauri.conf.json`'s `identifier` so the two cannot drift. That resolution lives in
`src-tauri/src/lib.rs`.

**Cost to learn: four rounds of diagnostic spiral, about six hours.** The
first three rounds re-litigated static analysis, which could never have
found this. What broke the spiral was adding four disjoint runtime probes
proportional to the hypothesis surface, not another speculative refactor.

**The generalized rule** (this applies well beyond Tauri): if
`register()` and `extract()` live in different lifecycle phases, prove
visibility with a closed-loop test that does **both**, through the
framework's actual runtime. Do not trust "the docs say register once"
plus "the registration logs true."

**Meta-lesson worth internalizing:** when round N+1 evidence contradicts
round N's conclusion, add observables proportional to the hypothesis
surface area. Do not re-argue the static analysis. Instrumentation that
feels like a detour is what turns the next round into a one-hour fix.

### 6.2 The PE static-import closure trap

**The trap.** You want to omit a large DLL from a Windows bundle. You
grep the entire dependency tree, confirm your code never calls a single
symbol from it, and conclude it is safe to drop. In our case: whisper.cpp
provably never calls a cuBLAS-Lt API, so surely we can ship without the
660 MB `cublasLt64_12.dll`.

**Root cause.** That finding is true and completely irrelevant.
`dumpbin /DEPENDENTS` on `cublas64_12.dll` shows `cublasLt64_12.dll` as a
**static PE import in cuBLAS's own header**. The Windows loader walks the
entire static-import closure at process-map time, *before* `main()` runs.
If any node in that closure is unreachable, the OS fails the process
launch with `STATUS_DLL_NOT_FOUND` (`0xc0000135`). Your code's
non-use of the API never gets a chance to matter, because your code never
executes.

Runtime confirmation was unambiguous: a test binary in an isolated
directory with the other DLLs staged, the system CUDA path stripped,
exited `0xc0000135` before printing a single byte.

**What to do instead.** Before deciding any Windows DLL is omittable:

1. Understand that source-level grep tells you what *your code* calls. It
   tells you nothing about the PE static-import closure of the DLLs you
   ship.
2. Run `dumpbin /DEPENDENTS` **transitively**. Walk every node in the
   import graph of every DLL you plan to ship. Anything that surfaces is
   mandatory.
3. Confirm with a runtime probe. Stage your candidate set in an isolated
   directory, strip the system path, launch. If the loader fails, your
   closure is incomplete.
4. Remember: "my source never calls this API" is a **necessary but not
   sufficient** condition for omission. The sufficient condition is that
   the transitive closure is empty.

The counter-pattern, if the size cost is genuinely prohibitive and the
DLL is genuinely never called: relink the *importing* DLL with
`/DELAYLOAD`. That requires building the importing DLL from source, which
is usually out of scope.

This near-miss would have shipped a CUDA installer that fails to launch
on every NVIDIA machine on earth.

### 6.3 Linking test binaries is not the same as building the app

**The trap.** Your test gate runs `cargo test --release --no-run`. It
passes. Types check, traits resolve, everything links. You seal the work,
the user relaunches the app, and nothing has changed.

**Root cause.** `--no-run` links the **test** binaries. The application
binary is a different cargo target with a different linker invocation.
Linking the test targets does not write through to
`target/release/<app>.exe`. The user was running a stale executable that
predated the change entirely, so migrations that had been authored,
tested, and sealed simply were not in the running binary.

**What to do instead.** `--no-run` is fine as a *gate* (it genuinely
proves type, trait, and link validity, which matters when your box cannot
execute the test runner). It is not a substitute for producing the
runtime artifact. After any change that touches:

- database migrations,
- worker or pipeline code paths, or
- the IPC surface (a new command or handler registration),

explicitly run `cargo build --release` and **verify the executable's
mtime is newer than your change**. Then smoke it.

Generalized: know which artifact your gate actually produces. A green
gate that builds a different target than the one you ship is a green
light on the wrong road.

### 6.4 Never use `*-latest` runner images in CI

**The trap.** Your workflow says `runs-on: windows-latest`. It works for
months. Then one day, with no change on your side, every build fails:

```
generator Visual Studio 18 2026 does not match the generator used
previously: Visual Studio 17 2022
```

**Root cause.** GitHub rolls the `*-latest` runner images forward on
their schedule, not yours. When the image advanced from Visual Studio 17
to Visual Studio 18, a restored cargo cache containing a CMake build
directory stamped with the *old* generator landed in a build driven by
the *new* one. CMake refuses to reuse it. Silent until the image rolls,
then broken everywhere at once.

**What to do instead.** Pin the explicit image label (`windows-2022`, not
`windows-latest`) in every workflow; both `.github/workflows/ci.yml` and
`.github/workflows/release.yml` do this. Fold the runner image label into
your cache key, so a future deliberate bump auto-invalidates the stale
cache instead of poisoning the new build. And bump the image in a commit,
so the toolchain change and the cache invalidation land together and are
reviewable.

This shapes every workflow you will ever author. Reproducible builds and
`*-latest` are mutually exclusive.

### 6.5 A `@media` block adds zero specificity

**The trap.** You write a perfectly reasonable accessibility override:

```css
@media (prefers-reduced-motion: reduce) {
  .dl-mark ellipse { animation: none; }
}
```

It does nothing. No warning, no console error, no lint failure. The
animation keeps running for motion-sensitive users.

**Root cause.** A media query contributes **zero** specificity. It gates
*whether* a rule applies; it does not strengthen it. The animation was
applied via `.dl-mark ellipse:nth-of-type(n)`, specificity (0,2,1). The
bare `.dl-mark ellipse` override is (0,1,1) and silently loses the
cascade. The override has to out-specify the rule it is overriding
entirely on its own merits.

**What to do instead.** Match or exceed the specificity of the rule you
are overriding:

```css
@media (prefers-reduced-motion: reduce) {
  .dl-mark ellipse:nth-of-type(1),
  .dl-mark ellipse:nth-of-type(2),
  .dl-mark ellipse:nth-of-type(3) { animation: none; }
}
```

Prefer matching specificity over `!important` so the block stays
overridable in the normal cascade. `!important` is defensible when you
are blanketing many selectors at once rather than a known few.

**Why this one matters disproportionately:** it was a real WCAG 2.2
SC 2.3.3 failure that shipped and was only caught in manual QA. It fails
*quietly*. Nothing in the toolchain tells you that your accessibility
override is inert. The live code and the reasoning are preserved in the
comment above the rule in `docs/site/styles.css`.

The same class of quiet cascade failure bites on source order: a block
with equal specificity that is moved *earlier* in the file silently
loses. If you have a rule whose correctness depends on its position,
write that down next to it.

### 6.6 Stale comments propagate into documentation

**The trap.** A header comment in `.github/workflows/release.yml`
described the installer filenames and download sizes. It was out of date.
Those wrong filenames and wrong sizes were then copied, in good faith,
into three separate user-facing documents. Users following the install
docs looked for files that did not exist.

**Root cause.** Comments near authoritative-looking machinery get treated
as authoritative. A workflow file that genuinely produces the artifacts
feels like the source of truth, so nobody re-derives the facts; they copy
the comment. One wrong comment becomes N wrong documents, and each copy
looks independently corroborated.

**What to do instead.** When you find a wrong fact in documentation,
**find the source of the wrong fact and fix that too**; fixing only the
copies guarantees the next person re-derives the same error. Grep the
whole repo for the wrong string before you call it fixed (three documents
had it; a spot fix would have found one). Verify artifact facts such as
filenames, sizes, and hashes against a published build, not against a
comment describing the build. And be suspicious of any comment stating a
value a machine could compute: those go stale silently, because nothing
ever executes a comment.

### 6.7 Static gates do not catch live-OS integration regressions

**The trap.** Every automated check is green. Unit tests pass, contract
assertions pass, lints pass, the diff checks out. You ship, and four
user-visible regressions land at once: a hotkey chord collision, a button
stuck in the wrong state, a stubbed source probe, and an overlay that
never appears.

**Root cause.** Static, unit, and file-diff assertions prove that
*contracts* hold. They do not prove that *integrations* work on a real
machine with real OS hooks, real audio devices, and real window
lifecycle. Nothing in the gate ever pressed the hotkey.

**What to do instead.** Any change touching OS hooks, audio devices, or
live event delivery gets a **five-minute human smoke test before the
commit that seals it**, not after. The smoke test is load-bearing
infrastructure, not a nicety. Write the click-path down so it is
repeatable and a future agent can hand it to a human as a checklist.

Worth internalizing if you are an agent: you cannot press the hotkey. Say
so and ask for the smoke test rather than declaring work done on the
strength of green checks.

### 6.8 Sub-agent session continuity is a foot-gun on serial handoffs

**The trap.** You are chaining work across several dispatches to a
sub-agent: stage one, then stage two, then stage three. Reusing the same
session id seems obviously right, since continuity should help.

It does the opposite. Each dispatch in an accumulating session re-runs
the sub-agent's session-start orientation, which triages "the kickoff
prompt" against the state on disk. In a long session, "the kickoff
prompt" anchors on the **first** user message. That first charter has
already shipped, so the sub-agent correctly concludes "this asks for work
that is already on disk, therefore the prompt is stale, therefore stop
and escalate." Re-authorizing does not help; each re-dispatch re-anchors
on the same stale first message and stops again. This produced an
eight-attempt loop before escalation.

**Root cause.** A session id is for **conversational refinement of one
task**: clarify, ask, respond. It is not a mechanism for serial task
handoff. The two patterns are not interchangeable and the API makes them
look like they are.

**What to do instead.**

- For serial handoffs (stage N to stage N+1), **omit the session id.**
  Each dispatch gets a fresh session with a clean anchor.
- Keep dispatch prompts short and pointer-style: "implement X per
  `<path-on-disk>`" rather than embedding the whole specification. The
  spec lives on disk, not in the prompt. Long embedded specs invite
  false-positive stale-prompt detections even in genuinely fresh
  sessions.

### 6.9 Two kinds of work seal two different ways

Mockingbird distinguishes **planned phases** (new subsystems, multi-wave,
sealed with a git tag) from **lateral epics** (coherent multi-piece work
that does not reopen a sealed phase, sealed with an accepted
architecture decision record and a status update, **no tag**).

**The trap** is reaching for phase machinery when a smaller container
fits, or worse, creating a new phase tag for work that is actually a
lateral epic against an already-sealed phase. That makes the tag
namespace lie about what shipped when.

**What to do instead.** Pick the smallest container that fits: a single
issue for anything small and self-contained; a lateral epic chartered by
a written decision record for coherent multi-piece work spanning a
session or three (the macOS port shipped this way, with no phase tag); a
planned phase with a tag only for a genuinely new subsystem; a standing
priority for never-completing quality loops.

The portable part: **decide your sealing ritual up front and make the
artifact namespace mean one thing.** If tags mean "phase complete," then
never tag anything else, or the tags stop being evidence.

Related: if you take one convention from this project, take **write the
decision down before you implement it.** Not after. A decision record
written before the work exists forces you to state what you are choosing
against, and that is the part you will want in six months.

### 6.10 A note on the workspace-root build output

Minor but portable for Tauri 2: the release executable lands at the
**workspace root** `target/release/`, not `src-tauri/target/release/`. If
your tooling, launcher scripts, or packaging steps assume the latter,
they will silently operate on a stale or nonexistent binary.

### 6.11 Writer strictness must not outrun the producer

**The trap.** You tighten a vocabulary, taxonomy, or contract on the
**writer** side (the serializer, emitter, or projection) while the
**producer** side (the classifier, extractor, or upstream service) still
emits the old values. The moment the strict writer ships, you get
deterministic failures. Worse, if something silently downgrades the old
values instead of failing, you get *undebuggable* failures.

**Root cause.** Parser-tolerant plus serializer-strict is an asymmetric
pairing, and the asymmetry is invisible until deployment order bites.

**What to do instead.** During any contract pivot, pick one: **both
tolerant, with a warning log** on every legacy value seen (the safe
default, and the warning count tells you when it is safe to tighten), or
**both strict**, but only after the producer is fully migrated. Never
strict-writer plus tolerant-parser while the producer is mid-migration.
And if you do bridge legacy values, make the bridge **loud**: return a
sidecar indicating a legacy value was translated, and log it. Silent
downgrades are the actual bug, not the downgrade itself.

## 7. Known gaps and deliberate non-goals

### Deliberate non-goals

These are not backlog items. They are choices.

- **No telemetry, analytics, or remote crash reporting.** Not "not yet."
- **No accounts, sign-in, or identity layer.** The app is per-OS-user by
  virtue of the platform keystore.
- **No bundled models on Windows.** Users fetch their own Whisper model
  at first launch. macOS is the exception: the `.dmg` bundles it.
- **No A/B experimentation, remote flags, or server-driven config.**
- **No plugin or extension API.** The codebase is the extension surface.
- **No auto-update by default.** The updater is wired and disabled.
- **No cross-machine database sync.** Knowledge travels through the
  user's own vault sync, not project-operated infrastructure.
- **No Mockingbird-side ingest, query, or lint engines.** See section 2.

### Known issues at this snapshot

- **No code signing on either platform.** Windows MSIs are unsigned and
  trip SmartScreen. The macOS build is ad-hoc signed only, with no
  Developer ID and no notarization, so Gatekeeper needs a one-time manual
  approval. This is a cost decision, not an oversight.
- **Activity capture, the knowledge graph, and mobile sync are
  Windows-only.** Not partially ported. Not ported.
- **Landing-site layout overflows horizontally below about 320px.** The
  design-language page's small-viewport rules engage at `max-width:
  860px`, but several components keep fixed horizontal floors below that
  (for example a search field with `min-width: 280px` that the mobile
  block never resets), so at very narrow widths the page scrolls sideways.
- **A layout dead band just above the mobile breakpoint** (reported at
  roughly 861px to 916px). Above `860px` the desktop rules apply in full:
  a fixed `240px` sidebar, `64px` of horizontal main padding, and
  fixed-width grid tracks such as the `140px 1fr 120px` typography
  specimen rows. In that band the chrome plus fixed tracks exceed the
  viewport before the mobile stacking rules are allowed to help.
- **IBM Plex Mono is missing weights.** `docs/site/fonts/fonts.css`
  declares `@font-face` blocks for weights 400, 500, and 600, but all
  three point at the same two `.woff2` files. Only one real weight is
  present, so 500 and 600 render synthetically. Regenerating with
  `scripts/dev/download-design-fonts.ps1` and keeping the per-weight
  files distinct is the fix.
- **Reverse-watcher has no full-vault sweep.** Individual Obsidian edits
  reconcile at roughly three seconds median; a nightly full sweep is not
  built.
- **The knowledge graph needs a reasonably capable local model.** Small
  models degrade to a tags-only mode rather than failing loudly.
- **The test runner does not execute on the maintainer's Windows box.**
  `cargo test --release` exits `STATUS_ENTRYPOINT_NOT_FOUND`
  (`0xc0000139`) during runner load. Test binaries link clean and the
  shipping binary is unaffected; on Linux and macOS the runner works
  normally. Fork onto a Linux or Mac box and you get a real test suite.
  Take advantage of that.
- **ESLint is not currently running.** The flat-config migration is
  outstanding, so the git hook type-checks with `tsc --noEmit` instead.

## 8. How this project is built

Stated plainly because it helps a forker's agent match the existing
conventions, and because it is already public on the project site.

- **Primary model:** Claude, primarily Opus 4.8.
- **Coding agent:** code-puppy.
- **Issue tracking:** Beads. Work is bead-first: if a task is not already
  an issue, it becomes one before the first edit. Issue ids are
  referenced in commit messages.
- **Session scoping:** a `/goals` pass turns a rough intent into a
  bounded set of tasks before any code is written.
- **Architecture decision records** for every non-trivial decision, and
  written *before* implementing, not after.
- **Wave briefs** before multi-iteration work: a short written plan
  naming the scope, the gates, and the seal condition, so a multi-session
  effort has one artifact to check progress against.
- **Git hooks are law.** Configured in `lefthook.yml`, implemented in
  `scripts/dev/hooks/`. Never bypassed with `--no-verify`.
- **Branch discipline.** No direct commits to `main`. Work happens on a
  branch and lands through a reviewed merge.
- **Commit messages must match the diff.** Every claim in the message
  must be present in the diff, and every substantive change in the diff
  must be reflected in the message. The rule exists because two
  overclaiming messages got caught.
- **Escalate after five failed attempts** on one problem, not ten. If
  five honest attempts have not moved, the specification or the test is
  probably the thing that is wrong.

None of this is required to fork successfully. It is offered so that if
you point an agent at this codebase, the agent's output looks like the
rest of the codebase.

## 9. Where to start if you are forking

Read in this order:

1. **`README.md`** for what the app does and how a user installs it.
2. **`ARCHITECTURE.md`** for the subsystem map. One page, covers every
   directory. Mind the stale cross-platform paragraph noted above.
3. **`CONTRIBUTING.md`** for build instructions, toolchain, and code
   style. Its "Common fork ideas" section maps onto section 4 here.
4. **`PRIVACY.md`** and **`SECURITY.md`** for the non-negotiable user
   guarantees. A fork that breaks these should pick a different name.
5. **`PREREQS.md`** and **`INSTALL.md`** for the from-source path,
   including the macOS build.

Then pick a concrete first change. Good ones, in ascending difficulty:

- **Add a cleanup provider.** Implement `CleanupProvider` in
  `src-tauri/src/cleanup/`. Small, self-contained, and it proves your
  whole toolchain works end to end.
- **Change a prompt.** Markdown sources live in
  `src-tauri/src/cleanup/prompts/` and versioning goes through the
  database. Teaches you the provenance model.
- **Swap the STT engine.** Implement `SpeechToText` in
  `src-tauri/src/stt/`. Watch out for the meeting-capture merge, which
  needs segment-level output.
- **Port a platform surface.** Pick a trait from the section 4 table and
  fill in the Linux implementation. `src-tauri/src/secrets/` is the
  smallest; hotkey and injection are the hard ones.
- **Bring a Windows-only subsystem to macOS.** Activity capture, the
  knowledge graph, or the inbox courier. Real work, and the
  highest-value contribution to a Mac-first fork.

Useful pointers for orientation:

- `src-tauri/src/lib.rs` is where the app is assembled. Start there to
  see how everything is wired, and note the pre-`.setup()` state
  registration described in section 6.1.
- `src-tauri/src/commands/` is the entire IPC surface. If you want to
  know what the frontend can ask the backend to do, it is all here.
- `src-tauri/src/db/migrations/` read in numeric order is a decent
  history of how the data model actually evolved.
- `docs/research/macos-implementation-notes.md` was written to enable
  exactly this kind of work, and `docs/mobile/` has the iOS Shortcut
  recipes for the mobile capture routes.

One last thing. The maintainer's position is explicit in `README.md` and
`CONTRIBUTING.md`: this is a reference implementation, pull requests are
welcome but not prioritized, and forking is the supported path. That is a
capacity statement, not a brush-off, and it means you should feel free to
make opinionated changes. Take the invariants in section 5 seriously,
take the lessons in section 6 as free money, and treat the rest as
negotiable.

## Keeping this current

This file is a **snapshot**, not a living document. It is regenerated at
each release tag from the maintainer's working documents (status notes,
the lessons journal, the product-state reference, the agent conventions
file, the plan, and the architecture decision records), all of which are
gitignored and not present in your clone.

Being a snapshot is deliberate. A point-in-time document that honestly
states its commit and tag is more useful to an agent than a continuously
half-updated one that cannot be trusted at any given moment. If the
header names an old tag, that is information, not a defect: read it as
"this was true at that tag" and check the code for whatever matters to
you.
