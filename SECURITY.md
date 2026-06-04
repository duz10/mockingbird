# Security Policy

## Reporting a vulnerability

Please use [GitHub Security Advisories](../../security/advisories/new) (private vulnerability reporting) on this repository. Do not file a public issue for security bugs.

The maintainer is solo and works on this project in spare time. Response time is best effort. For non-critical issues, expect a few days. For critical issues (remote code execution, privilege escalation, secret exfiltration), I will try to acknowledge within 48 hours but cannot guarantee a fix on a fixed schedule. If you need a fix faster than I can provide, the project is MIT licensed and fork-friendly.

Please include in the report:

- A clear description of the issue.
- Steps to reproduce, ideally with a minimal proof of concept.
- The affected version (look at `Help -> About` in the app, or the git commit if you built from source).
- Your suggested severity rating.
- Whether you would like credit in the eventual fix announcement.

## Supported versions

Only the latest tagged release receives security fixes. The project is in public beta; there is no LTS branch.

## Security model

Mockingbird is a local-first Windows desktop app. The threat model is shaped accordingly.

### Data at rest

- **SQLite database** lives under `%LOCALAPPDATA%\Mockingbird\`. This directory is per-user and protected by standard Windows ACLs. The database itself is not encrypted at rest; it relies on disk-level encryption (BitLocker, if you have it enabled) for confidentiality against offline attackers.
- **Whisper models** live under `%USERPROFILE%\mockingbird_models\` and are public weights downloaded from official sources at first launch. No secrets there.
- **API keys** (Anthropic Claude, Unsplash) are encrypted via [Windows DPAPI](https://learn.microsoft.com/en-us/windows/win32/seccrypto/cryptprotectdata) tied to your Windows user account. They are not portable to another user account or another machine, and they are not stored in plaintext on disk.
- **Recorded audio** is held in memory during transcription and discarded immediately unless you have opted in to retention (Settings -> Privacy -> Audio retention). Meeting capture stores chunk WAVs on disk during the session and finalizes them according to your retention policy.

### Data in transit

Mockingbird makes outbound network requests only when you have opted in to a cloud surface.

- **Anthropic Claude API.** HTTPS to `api.anthropic.com`. Used only for the cleanup pass on dictation transcripts, and only if you have set a Claude API key and routed a mode to Claude.
- **Unsplash API.** HTTPS to `api.unsplash.com` and `images.unsplash.com`. Used only if you have set an Unsplash API key in Settings -> Appearance.
- **Ollama.** Local-only HTTP to `http://localhost:11434`. Never leaves your machine. Mockingbird treats Ollama as a trusted local service.

No proxy, no middleware, no maintainer-controlled server is ever involved.

### Permissions

- **Microphone.** Requested via the standard Windows microphone consent prompt the first time you trigger dictation or meeting capture.
- **System audio (WASAPI loopback).** Used only for meeting capture. Captured via the default render device. No additional Windows permission prompt is involved.
- **Window context (UIA).** Used to detect secure-input fields (password boxes, BitLocker prompts) so the app can abort paste injection. Also used by the activity capture feature to summarize what apps you were using.
- **No screen recording.** Mockingbird does not capture screen video.
- **No keystroke logging.** The global hotkey hook listens specifically for the configured hotkey and does not forward keystrokes anywhere.

### Updater

- The Tauri updater is **disabled by default in the beta.** No automatic update checks are performed.
- The updater configuration is built with a public signing key, so when updates do ship (post-beta), they will be verified via [minisign](https://jedisct1.github.io/minisign/) before installation.
- Users will be able to opt in to updates via Settings in a future release.

### What the app does not do

- No telemetry. No analytics. No crash reporting.
- No A/B testing. No feature flags driven by a remote server.
- No autoupdate of models or prompts from a maintainer-controlled endpoint.
- No advertising identifiers, no fingerprinting, no third-party JavaScript in the WebView2 frontend.

See [`PRIVACY.md`](./PRIVACY.md) for the user-facing data flow guarantees in more depth.

## Dependency hygiene

- `cargo audit` and `npm audit` are run during CI; advisories at moderate severity or above block release.
- npm installs always use `--ignore-scripts` to defend against postinstall-script supply-chain attacks.
- Tauri 2 is kept on its latest stable release line. Major dependency bumps go through a release branch with a full smoke pass.

If you spot a dependency advisory we have missed, please report it via the same security advisory flow above.
