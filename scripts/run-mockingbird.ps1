# run-mockingbird.ps1 — launch the release build with the correct env.
#
# Why this script exists: the bare `target/release/mockingbird.exe`
# needs CUDA 12.8's `bin/` on PATH (whisper-rs cuda feature) AND
# `ORT_DYLIB_PATH` pointing at the 1.22.x ONNX Runtime DLL. Forgetting
# either gives `STATUS_DLL_NOT_FOUND` at exe-load time with NO useful
# diagnostic. This script also pre-sets `SILERO_VAD_PATH` /
# `WHISPER_MODEL_PATH` so first-run with non-standard model locations
# Just Works.
#
# Usage (the script is cwd-independent — runs from any directory):
#
#   # Windows PowerShell 5.1 (built-in):
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\run-mockingbird.ps1
#
#   # PowerShell 7+ (if you have `pwsh` installed):
#   pwsh scripts/run-mockingbird.ps1
#
# Flags:
#   -Foreground            # run attached, see stdout/stderr live
#   -Force                 # kill any running mockingbird.exe before launching
#                          # (also unblocks `cargo build --release`, which can't
#                          #  overwrite the .exe while it's loaded → "Access is
#                          #  denied. (os error 5)")
#   -ModelsDir D:\models   # override the default %USERPROFILE%\mockingbird_models
#   -CudaRoot 'C:\...\v12.8'
#
# To stop: right-click tray → Quit, OR `taskkill /F /IM mockingbird.exe`,
#          OR `pwsh scripts/run-mockingbird.ps1 -Force` (kills + relaunches).

[CmdletBinding()]
param(
    [string]$ModelsDir   = (Join-Path $env:USERPROFILE 'mockingbird_models'),
    [string]$CudaRoot    = 'C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.8',
    [switch]$Foreground,
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

# 0. Optionally kill any previous instance. -Force is opt-in because
#    auto-killing on every launch would be a surprise (you might be
#    debugging a live session). With the flag, this is the canonical
#    "rebuild-and-relaunch" shortcut.
if ($Force) {
    $existing = Get-Process -Name 'mockingbird' -ErrorAction SilentlyContinue
    if ($existing) {
        Write-Host "Killing $($existing.Count) running mockingbird process(es) ..."
        $existing | Stop-Process -Force
        # Give Windows a moment to release the .exe file lock before any
        # subsequent caller (or this script's own launch) touches it.
        Start-Sleep -Milliseconds 300
    } else {
        Write-Host "No running mockingbird to kill."
    }
}

# 1. Resolve binary location.
$repoRoot = Split-Path -Parent $PSScriptRoot
$exe      = Join-Path $repoRoot 'target\release\mockingbird.exe'
if (-not (Test-Path -LiteralPath $exe)) {
    throw "mockingbird.exe not found at $exe -- run ``pwsh scripts/cargo-with-cuda.ps1 build --release`` first."
}

# 2. CUDA bin on PATH (whisper-rs cuda feature dlopens cudart at startup).
if (-not (Test-Path -LiteralPath $CudaRoot)) {
    Write-Warning "CUDA not at $CudaRoot -- whisper will run CPU-only if it loads at all."
} else {
    $cudaBin = Join-Path $CudaRoot 'bin'
    if ($env:PATH -notlike "*$cudaBin*") {
        $env:PATH = "$cudaBin;$env:PATH"
        Write-Host "PATH += $cudaBin"
    }
}

# 3. ONNX Runtime DLL (ort 2.0.0-rc.10 pins 1.22.x exactly).
$ortDll = Join-Path $ModelsDir 'onnxruntime.dll'
if (Test-Path -LiteralPath $ortDll) {
    $env:ORT_DYLIB_PATH = $ortDll
    Write-Host "ORT_DYLIB_PATH = $ortDll"
} else {
    Write-Warning "ONNX Runtime DLL not at $ortDll -- VAD will fail to load."
}

# 4. Model paths (the resolver already finds %USERPROFILE%\mockingbird_models
#    via the Wave 4.5 fallback, but explicit env makes log lines clearer).
$silero = Join-Path $ModelsDir 'silero_vad.onnx'
$whisper = Join-Path $ModelsDir 'whisper-large-v3-turbo-q5_0.bin'
if (Test-Path -LiteralPath $silero)  { $env:SILERO_VAD_PATH    = $silero }
if (Test-Path -LiteralPath $whisper) { $env:WHISPER_MODEL_PATH = $whisper }

$env:RUST_BACKTRACE = '1'

# 5. Launch. CWD must be target\release so the cdylib (mockingbird_lib.dll)
#    is loadable.
$cwd = Split-Path -Parent $exe
Write-Host "Launching $exe ..."
Write-Host "Logs: $env:APPDATA\com.dustin.mockingbird\logs\"
Write-Host ""

if ($Foreground) {
    Push-Location $cwd
    try { & $exe } finally { Pop-Location }
} else {
    Start-Process -FilePath $exe -WorkingDirectory $cwd
    Write-Host "Started in background. Hold RightAlt to dictate."
    Write-Host "Stop with: taskkill /F /IM mockingbird.exe"
}
