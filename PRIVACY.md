# Privacy

Mockingbird is local-first software. This document spells out exactly what data the app handles, where it goes, and what it does not do.

## The short version

- Your voice never leaves your machine unless you opt in to a cloud cleanup provider.
- Your transcripts never leave your machine unless you opt in to a cloud cleanup provider.
- The app has zero telemetry. There is no maintainer-controlled server, ever.
- Cloud surfaces are opt-in, off by default, and disclosed clearly when you enable them.

## What the app records

### Voice audio

- Captured from your microphone when you press the dictation hotkey or start a meeting capture session.
- Transcribed locally by Whisper.
- **Discarded immediately** after transcription, unless you have explicitly turned on audio retention in Settings -> Privacy.
- Meeting capture is a special case: chunk WAV files exist on disk for the duration of the session and are deleted after merge, again unless you have turned on retention.

If you turn retention on, audio files live under `%LOCALAPPDATA%\Mockingbird\audio\` on your machine only.

### System audio (meeting capture only)

- Captured via WASAPI loopback during a meeting capture session. This is the same audio coming out of your speakers.
- Mixed with the microphone channel for transcription.
- Same retention rules as microphone audio.

### Transcripts and meeting notes

- Stored in a local SQLite database at `%LOCALAPPDATA%\Mockingbird\mockingbird.db`.
- Never transmitted anywhere unless you opt in to a cloud cleanup provider.
- You can delete individual sessions from the History page in the app, or wipe the database file if you want a clean slate.

### Knowledge graph notes

- Stored locally in the SQLite database.
- Projected to your configured Obsidian vault as Markdown files, if you have configured one.
- If your Obsidian vault is synced to a cloud service (Obsidian Sync, Dropbox, OneDrive, iCloud, Syncthing, etc.), the Markdown files travel through that service per its own privacy policy. Mockingbird itself does not sync to any cloud.

### Activity capture

- If you enable the activity capture feature, Mockingbird polls foreground window titles and UI Automation snapshots on a configurable interval.
- Snapshots are summarized into per-block descriptions by your configured local LLM.
- Snapshots and summaries are stored only in the local SQLite database. They are never transmitted anywhere.
- An exclusion list lets you mark specific apps that should never be captured (password managers, banking apps, whatever you want).

### API keys

- Anthropic and Unsplash API keys are encrypted via Windows DPAPI before being persisted.
- The encryption is tied to your Windows user account. Another user logging in to the same machine cannot read them. They are not portable across machines.

## Cloud surfaces (opt-in, off by default)

### Anthropic Claude API (cleanup)

- **When it activates:** Only if you have entered a Claude API key in Settings AND routed a cleanup mode to Claude.
- **What is sent:** The raw Whisper transcript text for the current dictation or meeting cleanup pass. No audio, no metadata beyond the prompt itself.
- **Where it goes:** Directly from your machine to `api.anthropic.com` over HTTPS. No proxy.
- **What Anthropic does with it:** See [Anthropic's privacy policy](https://www.anthropic.com/legal/privacy). The maintainer of Mockingbird has no business relationship with Anthropic and receives no data from them about your usage.

### Unsplash API (ambient backgrounds)

- **When it activates:** Only if you have entered an Unsplash API key in Settings -> Appearance.
- **What is sent:** Your search terms (if you customize the background category) and your IP address (as a normal consequence of making an HTTP request to a third party).
- **Where it goes:** Directly from your machine to `api.unsplash.com` and `images.unsplash.com` over HTTPS.
- **What Unsplash does with it:** See [Unsplash's privacy policy](https://unsplash.com/privacy).

### Ollama (local LLM cleanup)

- **Not a cloud surface.** Ollama runs entirely on your machine. The HTTP to `http://localhost:11434` never leaves the loopback interface.
- Listed here only to be explicit: enabling Ollama cleanup does not create any external network traffic.

## What the app does not do

- **No telemetry.** Period. The app does not connect to any maintainer-controlled server. There is no usage tracking, no anonymous metrics, no error reporting service.
- **No crash reporting.** Crashes log to local files under `%LOCALAPPDATA%\Mockingbird\logs\`. Nothing is sent anywhere.
- **No A/B testing.** Features behave the same for every user.
- **No advertising identifiers, no third-party trackers.**
- **No remote model updates.** Whisper models and LLM prompts ship with the binary (or are pulled from public sources at first install) and only change when you install a new version of Mockingbird.

## Your controls

- **Settings -> Privacy** is the canonical place to review and change retention, cloud opt-ins, and capture toggles.
- **Settings -> Dictation -> Cleanup mode** controls whether each mode uses Ollama (local), Claude (cloud), or no cleanup at all.
- **Settings -> Activity Capture -> Exclusion list** lets you mark apps that should never be captured.
- **History page** lets you delete individual sessions.
- **`%LOCALAPPDATA%\Mockingbird\`** is the canonical local data directory. Deleting it gives you a clean slate. Uninstalling the app does not delete this directory automatically; you keep your data.

## Questions

For privacy questions specifically (not security vulnerabilities), open a [Discussion](../../discussions) on this repository. For vulnerabilities, see [`SECURITY.md`](./SECURITY.md).
