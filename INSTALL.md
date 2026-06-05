# Installing Mockingbird

Three tiers, pick whichever fits.

- **Tier 1: Easy.** MSI installer. Dictation works out of the box. No cleanup LLM.
- **Tier 2: Standard.** MSI installer plus local Ollama for cleanup. Recommended.
- **Tier 3: From source.** Clone, build, run. For developers and forkers.

System requirements live in [`PREREQS.md`](./PREREQS.md). Check them first if you're not sure your machine is supported.

---

## Tier 1: Easy

1. Open the [Releases](../../releases) page and download the latest `Mockingbird-Setup-x.y.z.msi`.
2. Run the MSI. Windows SmartScreen will probably show a blue warning that says "Windows protected your PC". This is normal for unsigned Windows apps from independent developers. Click "More info", then "Run anyway". (Code signing is not on the roadmap for the beta.)
3. Accept the default install location. The installer registers Mockingbird as a startup app; you can disable that later in Settings.
4. Launch Mockingbird. On first run the app downloads a Whisper model into `%USERPROFILE%\mockingbird_models\`. This is between 500 MB and 2 GB depending on the variant you pick in Settings.
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
