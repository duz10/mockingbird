<#
.SYNOPSIS
    Download the ONNX Runtime DLL required by `ort = "=2.0.0-rc.10"`.

.DESCRIPTION
    Phase 2 Wave 3 uses ort's `load-dynamic` feature to side-step the
    static-link MSVC 2022 dependency (ort-sys built against MSVC 2022
    STL won't link against VS 2019 BuildTools). This means we must
    provide `onnxruntime.dll` at runtime via the `ORT_DYLIB_PATH` env
    var.

    This script downloads Microsoft's official ONNX Runtime release
    matching the version ort expects (1.22.x for rc.10), extracts the
    DLL, and prints the value to set `ORT_DYLIB_PATH` to.

    Updates here MUST track ort version bumps in Cargo.toml.

.PARAMETER OutputDir
    Where to download to. Defaults to `$env:LOCALAPPDATA\Mockingbird\models\`.

.PARAMETER Version
    ONNX Runtime version to fetch (must be the one ort expects).

.EXAMPLE
    pwsh scripts/download-onnxruntime.ps1
    # or:
    powershell -File scripts/download-onnxruntime.ps1
#>
[CmdletBinding()]
param(
    [string]$OutputDir,
    [string]$Version = '1.22.0'
)

$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

if (-not $OutputDir) {
    if ($env:MODEL_PATH) {
        $OutputDir = $env:MODEL_PATH
    } else {
        $OutputDir = Join-Path $env:LOCALAPPDATA 'Mockingbird\models'
    }
}

New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
$dllPath = Join-Path $OutputDir 'onnxruntime.dll'

if (Test-Path $dllPath) {
    Write-Host "onnxruntime.dll already present at $dllPath — skipping download." -ForegroundColor Green
    Write-Host ""
    Write-Host "Set: `$env:ORT_DYLIB_PATH = '$dllPath'"
    return
}

$zipUrl = "https://github.com/microsoft/onnxruntime/releases/download/v$Version/onnxruntime-win-x64-$Version.zip"
$zipPath = Join-Path $OutputDir "ort-$Version.zip"

Write-Host "Downloading ONNX Runtime $Version from $zipUrl ..." -ForegroundColor Cyan
Invoke-WebRequest -Uri $zipUrl -OutFile $zipPath -UseBasicParsing -TimeoutSec 300
Write-Host "Downloaded $((Get-Item $zipPath).Length) bytes."

Write-Host "Extracting onnxruntime.dll ..." -ForegroundColor Cyan
$tempExtract = Join-Path $OutputDir "ort-$Version-extracted"
if (Test-Path $tempExtract) { Remove-Item -Recurse -Force $tempExtract }
Expand-Archive -Path $zipPath -DestinationPath $tempExtract -Force

$candidate = Get-ChildItem -Path $tempExtract -Recurse -Filter onnxruntime.dll |
    Where-Object { $_.FullName -like "*$Version*" -and $_.FullName -notlike '*providers*' } |
    Select-Object -First 1

if (-not $candidate) {
    throw "onnxruntime.dll v$Version not found inside the downloaded archive"
}

Copy-Item $candidate.FullName $dllPath -Force
Remove-Item -Recurse -Force $tempExtract
Remove-Item -Force $zipPath

Write-Host "DLL ready at $dllPath ($((Get-Item $dllPath).Length) bytes)." -ForegroundColor Green
Write-Host ""
Write-Host "Add the following to your environment (PowerShell):"
Write-Host "  `$env:ORT_DYLIB_PATH = '$dllPath'"
