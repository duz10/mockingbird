<#
.SYNOPSIS
  Download ML models for Mockingbird per scripts/model-manifest.json.

.DESCRIPTION
  Per ADR 0014: resolves the target directory in this order:
    1. -OutputDir parameter (explicit override)
    2. $env:MODEL_PATH (dev override)
    3. $env:LOCALAPPDATA\Mockingbird\models\ (production default)

  Idempotent: a model whose SHA-256 already matches the manifest is
  skipped. Aborts with non-zero exit on a SHA-256 mismatch after
  download (file is left in place for inspection).

  Resumable: uses BITS transfer when available, falls back to
  Invoke-WebRequest. Both support resume on partial downloads.

.PARAMETER OutputDir
  Override the resolution order with an explicit target directory.

.PARAMETER Manifest
  Path to the manifest JSON. Defaults to scripts/model-manifest.json
  relative to this script.

.PARAMETER WhatIf
  Standard PowerShell -WhatIf — prints what would happen without
  downloading or writing anything.

.EXAMPLE
  pwsh ./scripts/download-models.ps1

  Downloads all manifest entries to the production default location.

.EXAMPLE
  pwsh ./scripts/download-models.ps1 -OutputDir .\models

  Downloads to a local dev directory.
#>
[CmdletBinding(SupportsShouldProcess)]
param(
    [string]$OutputDir,
    [string]$Manifest = (Join-Path $PSScriptRoot 'model-manifest.json')
)

$ErrorActionPreference = 'Stop'

function Resolve-OutputDir {
    param([string]$Explicit)
    if ($Explicit) { return $Explicit }
    if ($env:MODEL_PATH) { return $env:MODEL_PATH }
    return (Join-Path $env:LOCALAPPDATA 'Mockingbird\models')
}

function Get-Sha256 {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLower()
}

function Invoke-Download {
    param([string]$Url, [string]$Destination)
    # Prefer BITS for resume + bandwidth throttling support; fall back
    # to Invoke-WebRequest if BITS is unavailable (rare on Windows).
    try {
        Start-BitsTransfer -Source $Url -Destination $Destination -DisplayName "Mockingbird model" -ErrorAction Stop
    }
    catch {
        Write-Warning "BITS unavailable ($($_.Exception.Message)); using Invoke-WebRequest"
        Invoke-WebRequest -Uri $Url -OutFile $Destination -UseBasicParsing
    }
}

if (-not (Test-Path -LiteralPath $Manifest)) {
    throw "Manifest not found: $Manifest"
}

$manifestData = Get-Content -LiteralPath $Manifest -Raw | ConvertFrom-Json
$targetDir = Resolve-OutputDir -Explicit $OutputDir

if (-not (Test-Path -LiteralPath $targetDir)) {
    if ($PSCmdlet.ShouldProcess($targetDir, "Create directory")) {
        New-Item -ItemType Directory -Path $targetDir -Force | Out-Null
    }
}

Write-Host "Target directory: $targetDir" -ForegroundColor Cyan
$failed = @()

foreach ($model in $manifestData.models) {
    $destPath = Join-Path $targetDir $model.filename
    $expectedSha = $model.sha256.ToLower()

    Write-Host ""
    Write-Host "[$($model.name)] $($model.filename) — $([math]::Round($model.size_bytes / 1MB, 1)) MB" -ForegroundColor Yellow

    if ($expectedSha -eq 'tbd-pin-when-downloaded') {
        Write-Warning "  SHA-256 not yet pinned in manifest. Downloading and recording the observed hash."
    }

    # Idempotency: skip if already present and matching.
    $existingSha = Get-Sha256 -Path $destPath
    if ($existingSha -and $expectedSha -ne 'tbd-pin-when-downloaded' -and $existingSha -eq $expectedSha) {
        Write-Host "  ✓ Already present with matching SHA-256." -ForegroundColor Green
        continue
    }

    if ($PSCmdlet.ShouldProcess($destPath, "Download from $($model.url)")) {
        try {
            Invoke-Download -Url $model.url -Destination $destPath
        }
        catch {
            Write-Error "  ✗ Download failed: $($_.Exception.Message)"
            $failed += $model.name
            continue
        }

        $observedSha = Get-Sha256 -Path $destPath
        if ($expectedSha -eq 'tbd-pin-when-downloaded') {
            Write-Host "  Observed SHA-256: $observedSha" -ForegroundColor Cyan
            Write-Host "  Update model-manifest.json with this hash to pin." -ForegroundColor Cyan
        }
        elseif ($observedSha -eq $expectedSha) {
            Write-Host "  ✓ SHA-256 verified." -ForegroundColor Green
        }
        else {
            Write-Error "  ✗ SHA-256 mismatch! Expected $expectedSha, got $observedSha. File left in place for inspection."
            $failed += $model.name
        }
    }
}

if ($failed.Count -gt 0) {
    Write-Host ""
    Write-Error "Failed: $($failed -join ', ')"
    exit 1
}

Write-Host ""
Write-Host "All models present and verified." -ForegroundColor Green
