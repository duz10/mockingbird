# Installing Mockingbird

**On macOS (Apple Silicon)?** The tiers below are the Windows path. Jump
straight to [macOS (Apple Silicon)](#macos-apple-silicon) — three install
methods (Homebrew, `.dmg`, or from source) with the full dictation +
meeting-capture experience.

On Windows, three tiers, pick whichever fits. (On macOS? Skip the tiers and use the
[macOS (Apple Silicon)](#macos-apple-silicon) section instead.)

- **Tier 1: Easy.** MSI installer. Dictation works out of the box. No cleanup LLM.
- **Tier 2: Standard.** MSI installer plus local Ollama for cleanup. Recommended, but optional: dictation works fully without it.
- **Tier 3: From source.** Clone, build, run. For developers and forkers.

System requirements live in [`PREREQS.md`](./PREREQS.md). Check them first if you're not sure your machine is supported.

---

## Tier 1: Easy

### Pick your installer

The [Releases](../../releases) page lists two MSI installers per release. Pick the one that matches your hardware:

- **`Mockingbird_x.y.z_x64_en-US.msi`** (about 9 MB download). CPU-only Whisper. Works on any 64-bit Windows machine. Transcription is slower than the GPU variant but has no hardware prereqs beyond a recent x86 CPU.
- **`Mockingbird-CUDA_x.y.z_x64_en-US.msi`** (about 580 MB download). NVIDIA GPU Whisper via cuBLAS. The MSI bundles NVIDIA's CUDA runtime libraries so you do NOT need to install the CUDA Toolkit separately. The only user-side prereq is an NVIDIA driver, which ships with every NVIDIA GPU and auto-updates via Windows Update or GeForce Experience.

If you have an NVIDIA GPU and you care about transcription speed, install the CUDA variant. If you have an AMD or Intel GPU, no GPU, or you want the smallest possible download, install the CPU variant. Installing the CUDA variant on a non-NVIDIA machine will fail at launch with a missing-DLL error; in that case, uninstall it and install the CPU variant.

The two variants register under distinct Add/Remove Programs entries (`Mockingbird` for CPU, `Mockingbird-CUDA` for GPU) so you can switch between them without manual cleanup.

### Install steps

1. Open the [Releases](../../releases) page and download whichever MSI you picked above.
2. Run the MSI. Windows SmartScreen will probably show a blue warning that says "Windows protected your PC". This is normal for unsigned Windows apps from independent developers. Click "More info", then "Run anyway". (Code signing is not on the roadmap for the beta.)
3. Accept the default install location. The installer registers Mockingbird as a startup app; you can disable that later in Settings.
4. Launch Mockingbird. On first run the app downloads a Whisper model into `%USERPROFILE%\mockingbird_models\`. This is between 500 MB and 2 GB depending on the variant you pick in Settings, and it is a separate download from the MSI itself.
5. Press Right Alt to record. Release to paste the transcript into the focused app.

That's it. Dictation is working. No optional LLM is configured yet, so you will see Whisper's raw transcript with no cleanup pass.

---

## Tier 2: Standard with local cleanup

Adds an optional local LLM that polishes filler words, punctuation, and capitalization. Everything still runs on your machine.

1. Complete the four Tier 1 steps above.
2. Install [Ollama](https://ollama.com/download) for Windows.
3. Open PowerShell and run:

   ```pwsh
   ollama pull qwen2.5:7b-instruct-q4_K_M
   ```

   This is about 4.7 GB. The "q4_K_M" suffix is a 4-bit quantization that runs comfortably on 8 GB of RAM. If you have a CUDA-capable NVIDIA GPU, Ollama will use it automatically.

4. Launch Mockingbird, open Settings, go to the Dictation tab, and set "Cleanup mode" to `normal` (or `casual` or `formal`, whichever you prefer). Confirm the Ollama tab shows the qwen model as available.
5. Dictate as before. The cleanup pass adds 1 to 3 seconds of latency depending on your hardware.

If you want to use the Anthropic Claude API instead of Ollama, paste your API key into Settings -> Dictation -> Cloud cleanup. The key is encrypted via Windows DPAPI and tied to your Windows user account.

---

## Setting up mobile sync (optional)

With mobile sync configured, you can capture notes from your iPhone via an iOS Shortcut and have them automatically picked up by Mockingbird on your PC. The mechanism is a folder synced between your phone and your desktop; Mockingbird watches the folder and routes new files into either the knowledge graph or the dictation pipeline depending on which inbox the file lands in.

1. Pick a sync provider that mirrors a folder between your iPhone and your PC. iCloud Drive, OneDrive, Dropbox, and Google Drive all work. Use whichever you already have set up.
2. Inside your Obsidian vault (or wherever you point Mockingbird's vault path), make sure the `Inbox/` and `Knowledge Graph/Inbox/` subfolders exist. Mockingbird creates these on first vault bootstrap; if you started before that landed, create them by hand.
3. In Mockingbird, open Settings -> Vault and confirm the vault path points at that synced folder. The vault watcher activates automatically.
4. On your iPhone, install the iOS Shortcut recipe from [`docs/mobile/`](./docs/mobile/). There is one recipe for general capture (routes to dictation) and one for knowledge graph capture (routes to KG).
5. Test it: fire the Shortcut on your phone, wait for your sync provider to mirror the file to your PC, and watch Mockingbird pick it up.

If nothing happens, check that the file actually arrived on the PC (sync providers have their own latency) and that Mockingbird's vault watcher path matches the folder your provider is syncing to.

---

## Tier 3: From source

For developers, forkers, and anyone who wants to build Mockingbird themselves.

### One-time setup

1. Install Rust 1.77 or newer via [rustup](https://rustup.rs/).
2. Install Node 20 or newer via [nvm-windows](https://github.com/coreybutler/nvm-windows) or the Node installer.
3. Install Git and Visual Studio Build Tools (the "Desktop development with C++" workload). MSVC is required by some Rust crates.
4. *(Optional but recommended)* Install the [CUDA Toolkit 12.8](https://developer.nvidia.com/cuda-12-8-0-download-archive). The build will use CUDA-accelerated Whisper if it detects a working install. Without CUDA, Whisper falls back to CPU (slower but works).
5. Install [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) if you're on Windows 10. Windows 11 ships with it preinstalled.

### Build

```pwsh
git clone https://github.com/duz10/mockingbird.git
cd mockingbird

# Sets up the toolchain, installs npm deps with --ignore-scripts, etc.
powershell -File scripts\setup-dev.ps1

# Build the release binary. The wrapper script imports MSVC + CUDA env first.
powershell -File scripts\dev\cargo-with-cuda.ps1 build --release

# Fetch the runtime model files (ONNX Runtime + Whisper GGUF).
powershell -File scripts\download-onnxruntime.ps1
powershell -File scripts\download-models.ps1

# Launch. Always go through the launcher; it sets ORT_DYLIB_PATH and
# prepends the CUDA bin dir to PATH so the binary can find onnxruntime.dll
# and cudart64_12.dll at process start.
powershell -File scripts\run-mockingbird.ps1
```

Build time on a warm cache is about 3 minutes. First build, including the CUDA Whisper artifacts, can take 15 to 30 minutes. Be patient.

### Running tests

```pwsh
powershell -File scripts\dev\cargo-with-cuda.ps1 test --release --no-run
cd ui ; npm test ; cd ..
```

There is a known issue where `cargo test --release` on the canonical Windows dev box exits with `STATUS_ENTRYPOINT_NOT_FOUND` (0xc0000139) at runner load time. The test binaries themselves link clean, which validates the type system and trait surface, but runner execution is blocked on this particular hardware configuration. On Linux and macOS dev boxes, plain `cargo test` works fine. The shipping binary is unaffected.

### Packaging an MSI

```pwsh
powershell -File scripts\dev\cargo-with-cuda.ps1 tauri build
```

The MSI lands under `target\release\bundle\msi\`. It is unsigned. If you want a signed build, configure your own code-signing certificate in `src-tauri\tauri.conf.json`.

---

## macOS (Apple Silicon)

macOS runs the **full experience** — voice dictation *and* meeting capture,
both with local LLM cleanup — on Apple Silicon Macs running **macOS 15
(Sequoia) or newer** (the ScreenCaptureKit floor for meeting system-audio
capture). Whisper runs on the **Metal** GPU backend.

There is **no Apple-signed installer** (no Apple Developer account), so
every download path is unsigned — the difference is only *how much*
Gatekeeper you touch. Three methods, cleanest first:

1. **Homebrew cask (recommended).** Homebrew (6.x) **adds** `com.apple.quarantine` on cask
   install — it does **not** strip it, and the cask cannot opt out (there
   is no `quarantine false` stanza in current Homebrew). The app is ad-hoc
   signed (not notarized), so a plain install hits a one-time Gatekeeper
   prompt on first launch:
   ```bash
   brew install --cask duz10/mockingbird/mockingbird
   ```
   Clear that one-time prompt via
   [First launch: Gatekeeper](#first-launch-gatekeeper) below. To skip it
   at install time, set `HOMEBREW_CASK_OPTS` (current Homebrew no longer
   accepts a bare `--no-quarantine` flag on `brew install`):
   ```bash
   HOMEBREW_CASK_OPTS="--no-quarantine" brew install --cask duz10/mockingbird/mockingbird
   ```
   (First `brew tap duz10/mockingbird https://github.com/duz10/mockingbird`
   if you haven't tapped it. See
   [`docs/macos-port/homebrew-tap.md`](./docs/macos-port/homebrew-tap.md).)

2. **Direct `.dmg` download.** Download `Mockingbird_<version>_aarch64.dmg` from the
   [Releases](https://github.com/duz10/mockingbird/releases) page, open
   it, and drag **Mockingbird.app** to **Applications**. Because it's
   unsigned you must clear Gatekeeper once — see
   [First launch: Gatekeeper](#first-launch-gatekeeper) below.

3. **Build from source.** No release required — clone and build the
   self-contained `.app` yourself (steps below).

> **Availability:** all three methods work today — the public release is
> live. They produce the same self-contained (~600 MB) app: the Whisper +
> Silero models and the ONNX Runtime dylib are bundled, so users never
> fetch models separately.
> macOS prerequisites for the source build are in
> [`PREREQS.md`](./PREREQS.md#building-from-source-on-macos-apple-silicon).

> **Windows-only for now.** Activity capture, the Knowledge Graph
> pipeline, and Mobile Sync are not wired on macOS yet — those surfaces
> show as "coming soon" in the Mac build. Dictation, meeting capture, and
> cleanup are at Windows parity.

### Build the `.app`

```bash
git clone https://github.com/duz10/mockingbird.git
cd mockingbird

# Toolchain (see PREREQS for details):
xcode-select --install     # Command Line Tools (compiler, git) if not present
brew install cmake jq      # cmake: whisper-rs-sys build.rs (whisper.cpp); jq: model fetch script
# Rust 1.77+ via https://rustup.rs and Node 20+ (e.g. `brew install node`)

# Fetch the runtime model files into ./models FIRST — the build bundles
# them into the .app, so they must be present before `tauri build`.
scripts/download-onnxruntime.sh   # libonnxruntime.dylib
scripts/download-models.sh        # Whisper GGUF + Silero VAD into ./models

# Build the self-contained .app. The macOS config overlay bundles the
# models (+ ORT dylib) into Contents/Resources/models/ and pins the
# macOS 15 floor; --bundles app produces just the .app (skips the DMG).
scripts/dev/cargo-mac.sh tauri build --config src-tauri/tauri.macos.conf.json --bundles app
```

The wrapper auto-injects `--features mockingbird/metal` (Metal GPU
Whisper). The bundle lands at
`target/release/bundle/macos/Mockingbird.app` — double-click-and-go, no
Xcode and no dev env vars needed at runtime.

> **Toolchain note:** `cmake` is a hard prerequisite — `whisper-rs-sys`'s
> `build.rs` shells out to it to compile the bundled `whisper.cpp`. Unlike
> the Windows path (where Visual Studio ships CMake), macOS Command Line
> Tools does **not** include it, so `brew install cmake` is required. You
> also need Rust 1.77+ (via [rustup](https://rustup.rs/)) and Node 20+.

### Run from source instead (developer loop)

If you just want to iterate on the code rather than produce a shippable
`.app`, run the dev server directly:

```bash
scripts/dev/cargo-mac.sh tauri dev
```

The dev build is **not** bundled, so it reads the models from `./models`
at runtime (the wrapper exports `MODEL_PATH` + `ORT_DYLIB_PATH` for you) —
which is why the `download-*.sh` scripts above are required for the dev
loop too. See the permissions note below for the dev-vs-`.app` TCC quirk.

### First launch: Gatekeeper

Applies to the **`.dmg` download**, a **locally built `.app`**, and a
plain **Homebrew cask** install (method 1) — Homebrew adds quarantine on
install, so unless you set `HOMEBREW_CASK_OPTS="--no-quarantine"` you clear
Gatekeeper once here too.

The `.app` is unsigned, so on first open macOS Gatekeeper refuses a plain
double-click ("Mockingbird can't be opened because Apple cannot check it
for malicious software"). On **macOS 15 (Sequoia)** the old
right-click → Open shortcut is **gone**. Clear it one of two ways (one
time only; later launches open normally):

- **System Settings** → **Privacy & Security** → scroll to the blocked-app
  notice → **Open Anyway** → confirm. (You may need to double-click the
  app once first to trigger the notice.)
- **Or the one-liner** (strips the quarantine flag directly):
  ```bash
  xattr -dr com.apple.quarantine /Applications/Mockingbird.app
  ```
  (Point it at wherever the `.app` lives if not in `/Applications`.)

(Code signing is not on the roadmap for the beta.)

### Local cleanup (Ollama) on macOS

Cleanup works the same as on Windows — install [Ollama for
macOS](https://ollama.com/download) and pull a model:

```bash
ollama pull qwen2.5:7b-instruct-q4_K_M   # ~4.7 GB, parity cleanup model
```

Mockingbird selects the cleanup model **based on your Mac's unified
memory** (ADR 0064):

- **16 GB or more** → the full **7B** model, byte-identical cleanup quality
  to the Windows path. Pull `qwen2.5:7b-instruct-q4_K_M` as above.
- **8 GB (or any &lt; 16 GB)** → auto-downshifts to a **3B** model so it
  coexists with Whisper-Metal in RAM. Pull it with:
  ```bash
  ollama pull qwen2.5:3b-instruct-q4_K_M   # ~1.9 GB
  ```

Without Ollama running, dictation still works — you just get Whisper's
raw transcript with **no cleanup pass** (passthrough). The Anthropic
Claude API cloud option also works on macOS; the key is stored in the
macOS **Keychain** (the Mac equivalent of Windows DPAPI).

### Granting permissions (Privacy & Security / TCC)

Mockingbird needs **four** macOS permissions for the full experience.
Three are for dictation; the fourth (Screen Recording) is what lets
meeting capture record system audio:

- **Microphone** — to record your voice (dictation + meetings).
- **Input Monitoring** — to see the Right Option hotkey globally (dictation).
- **Accessibility** — to paste the transcript into the focused app, and
  for the secure-input guard (dictation).
- **Screen Recording** — required by ScreenCaptureKit to capture system
  audio during **meeting capture**. Dictation works without it; meeting
  capture's system-audio channel does not.

**Which app you grant depends on how you launched Mockingbird — and this
trips people up.** macOS attributes the Input Monitoring and Accessibility
requests to the *responsible process*, which is **not always Mockingbird**:

| How you launched it | What appears in the permission list | What to grant |
|---|---|---|
| **Built `.app`** (`cargo tauri build`, then open `target/release/bundle/macos/Mockingbird.app`) | **`Mockingbird`** | Grant **Mockingbird**. Stable identity — the grant sticks across runs. This is the real end-user path. |
| **Dev build** (`cargo tauri dev` / `scripts/dev/cargo-mac.sh tauri dev`) | the **terminal that launched it** (e.g. **iTerm** or **Terminal**) — *not* "Mockingbird" | Grant the **terminal**. The unbundled dev binary inherits the terminal's TCC identity, so macOS lists the terminal, not Mockingbird. |

For the dev build, if the terminal isn't listed, click **`+`** in the
Privacy pane and add it (e.g. `/Applications/iTerm.app`), or add the dev
binary directly via `Cmd+Shift+G` →
`<repo>/target/debug/mockingbird`.

**TCC grants only take effect at process start.** After toggling a
permission you must **fully quit and relaunch** Mockingbird (for the dev
build, `Ctrl+C` the dev server and re-run it from the same terminal).
Microphone is unaffected by this quirk — its prompt attributes correctly.

> Note: the in-app first-launch permissions panel's "jump to the right
> pane, then come back" copy is accurate for the **built `.app`** (where
> the entry is *Mockingbird*). For a **dev build** the entry you actually
> toggle is your **terminal** — keep that in mind while developing.

For a representative sign-off (what a real user sees), validate against
the **built `.app`**, not the dev binary — its TCC identity is stable and
the permission entries read "Mockingbird".
