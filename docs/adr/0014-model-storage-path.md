# ADR-0014: Model storage path

- **Status:** Accepted
- **Date:** 2026-05-16
- **Deciders:** Dustin (project lead), code-puppy (implementor), planning-agent

## Context

Phase 2 ships with two ML models on disk:

- **Whisper.cpp** (`ggml-large-v3-turbo-q5_0.bin` or similar) —
  ~1.5 GB
- **Silero VAD** (`silero_vad.onnx`) — ~1.8 MB

Both must be located at runtime; both must be download-able
post-install (we don't ship them in the installer to keep MSI size
sane); both must survive across app updates. Dev workflow needs an
override path so contributors can point at locally-downloaded copies
without polluting the system folder.

## Decision

**Runtime location (release builds):**
`%LOCALAPPDATA%\Mockingbird\models\`

`%LOCALAPPDATA%` chosen over `%APPDATA%` because:
- Doesn't roam (we don't want a 1.5 GB roaming profile)
- No Active-Directory quota issues on enterprise machines
- Per-machine, per-user — correct privacy semantics

**Dev location:** `<repo-root>/models/` (gitignored).

**Resolution order** (first match wins):

1. `MODEL_PATH` env var (dev override; absolute path)
2. `<exe_dir>/models/` (portable install — Phase 7 may ship this)
3. `%LOCALAPPDATA%\Mockingbird\models\` (default release path)

The resolution helper lives in `src-tauri/src/stt/mod.rs` (free
function `models_dir() -> AppResult<PathBuf>`).

**Manifest:** `scripts/model-manifest.json` lists every model with:

```json
{
  "models": [
    {
      "name": "whisper-large-v3-turbo-q5_0",
      "filename": "ggml-large-v3-turbo-q5_0.bin",
      "size_bytes": 1574000000,
      "sha256": "<TBD-fill-when-pinning>",
      "url": "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin"
    },
    {
      "name": "silero-vad",
      "filename": "silero_vad.onnx",
      "size_bytes": 1810000,
      "sha256": "<TBD-fill-when-pinning>",
      "url": "https://github.com/snakers4/silero-vad/raw/master/files/silero_vad.onnx"
    }
  ]
}
```

**Download script:** `scripts/download-models.ps1` — resumable
(`Invoke-WebRequest -Resume` or equivalent), idempotent (skips when
SHA-256 already matches), and verifies SHA-256 on completion. Aborts
with non-zero exit on mismatch.

## Consequences

- **Positive:** standard Windows storage convention; dev workflow
  flexibility via `MODEL_PATH`; no installer bloat; reproducible
  builds via SHA-256 manifest.
- **Negative:** first-run requires running the download script
  (~30 s for Silero, ~5 min for Whisper on typical broadband).
  Phase 5 first-run wizard automates this.
- **Neutral:** the manifest is the canonical source of truth — any
  model swap is a manifest update + a script re-run.

## Alternatives considered

- **`%APPDATA%`:** roams; bad for 1.5 GB. Rejected.
- **`%PROGRAMDATA%`:** requires admin write on first install.
  Rejected (we want per-user installs).
- **Inside the MSI:** 1.5+ GB installer is unacceptable for a
  download. Rejected.
- **`include_bytes!`:** static-link 1.5 GB into the exe. Rejected.
- **Download on first run, cache in `%TEMP%`:** ephemeral; user
  would re-download after every disk clean. Rejected.

## Cross-references

- PLAN line 1366 (`scripts/download-models.ps1` — resumable,
  SHA-256-verified)
- PLAN line 421 (`scripts/generate-tauri-keys.ps1` as exemplar
  PowerShell script)
- `docs/phases/phase2.md` Wave 1
- ADR 0011 (whisper-rs build — finds the model via this resolution
  order)
- ADR 0012 (ort runtime — same)
