# Verify the local dev environment has every tool Mockingbird needs.
#
# Usage:
#   pwsh scripts/verify-environment.ps1          # report-only, exit 0
#                                                  unless required-now tool is missing
#   pwsh scripts/verify-environment.ps1 -Strict  # exit 1 if ANY tool
#                                                  (including phase-2/4) missing

[CmdletBinding()]
param(
    [switch]$Strict
)

$PSNativeCommandUseErrorActionPreference = $false
$ErrorActionPreference = "Continue"

$results = [System.Collections.ArrayList]::new()

function Add-Result {
    param($Name, $Category, $Status, $Version, $InstallUrl)
    [void]$results.Add([PSCustomObject]@{
        Tool = $Name; Category = $Category; Status = $Status
        Version = $Version; InstallUrl = $InstallUrl
    })
}

function Probe {
    param(
        [string]$Name,
        [string]$Exe,
        [string[]]$ProbeArgs,
        [string]$Category,
        [string]$InstallUrl
    )
    $resolved = Get-Command $Exe -ErrorAction SilentlyContinue
    if (-not $resolved) {
        Add-Result $Name $Category "MISSING" $null $InstallUrl
        return
    }
    $version = $null
    try {
        $output = & $Exe @ProbeArgs 2>$null | Select-Object -First 1
        if ($output) { $version = ([string]$output).Trim() }
    } catch { }
    if ($version) {
        Add-Result $Name $Category "OK" $version $InstallUrl
    } else {
        Add-Result $Name $Category "ERROR" $null $InstallUrl
    }
}

Write-Host "=== Mockingbird environment check ===" -ForegroundColor Cyan

Probe -Name "rustc"       -Exe "rustc"  -ProbeArgs @("--version") -Category "required-now" -InstallUrl "https://rustup.rs/"
Probe -Name "cargo"       -Exe "cargo"  -ProbeArgs @("--version") -Category "required-now" -InstallUrl "https://rustup.rs/"
Probe -Name "node"        -Exe "node"   -ProbeArgs @("--version") -Category "required-now" -InstallUrl "https://nodejs.org/"
Probe -Name "npm"         -Exe "npm"    -ProbeArgs @("--version") -Category "required-now" -InstallUrl "https://nodejs.org/"
Probe -Name "git"         -Exe "git"    -ProbeArgs @("--version") -Category "required-now" -InstallUrl "https://git-scm.com/"
Probe -Name "python"      -Exe "python" -ProbeArgs @("--version") -Category "required-now" -InstallUrl "https://www.python.org/downloads/"
Probe -Name "cargo-tauri" -Exe "cargo"  -ProbeArgs @("tauri", "--version") -Category "required-now" -InstallUrl "https://v2.tauri.app/start/prerequisites/"
Probe -Name "bd (beads)"  -Exe "bd"     -ProbeArgs @("--version") -Category "required-now" -InstallUrl "https://github.com/steveyegge/beads"

Probe -Name "cmake"  -Exe "cmake"  -ProbeArgs @("--version") -Category "required-phase-2" -InstallUrl "https://cmake.org/download/"
Probe -Name "nvcc"   -Exe "nvcc"   -ProbeArgs @("--version") -Category "required-phase-2" -InstallUrl "https://developer.nvidia.com/cuda-downloads"
Probe -Name "ollama" -Exe "ollama" -ProbeArgs @("--version") -Category "required-phase-4" -InstallUrl "https://ollama.com/download"

# WebView2 — registry probe (no CLI surface).
$wv2Key = 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
$wv2 = Get-ItemProperty -Path $wv2Key -ErrorAction SilentlyContinue
if (-not $wv2) {
    $wv2 = Get-ItemProperty -Path ($wv2Key.Replace('WOW6432Node\','')) -ErrorAction SilentlyContinue
}
if ($wv2 -and $wv2.pv) {
    Add-Result "WebView2 runtime" "required-now" "OK" $wv2.pv "https://developer.microsoft.com/microsoft-edge/webview2/"
} else {
    Add-Result "WebView2 runtime" "required-now" "MISSING" $null "https://developer.microsoft.com/microsoft-edge/webview2/"
}

$results | Format-Table Tool, Category, Status, Version -AutoSize

$missingNow = @($results | Where-Object { $_.Status -ne "OK" -and $_.Category -eq "required-now" })
$missingP2  = @($results | Where-Object { $_.Status -ne "OK" -and $_.Category -eq "required-phase-2" })
$missingP4  = @($results | Where-Object { $_.Status -ne "OK" -and $_.Category -eq "required-phase-4" })

if ($missingNow.Count) {
    Write-Host "MISSING (required for Phase 0/1):" -ForegroundColor Red
    foreach ($m in $missingNow) { Write-Host "  $($m.Tool) - install: $($m.InstallUrl)" }
}
if ($missingP2.Count) {
    Write-Host "MISSING (required for Phase 2):" -ForegroundColor Yellow
    foreach ($m in $missingP2) { Write-Host "  $($m.Tool) - install: $($m.InstallUrl)" }
}
if ($missingP4.Count) {
    Write-Host "MISSING (required for Phase 4):" -ForegroundColor Yellow
    foreach ($m in $missingP4) { Write-Host "  $($m.Tool) - install: $($m.InstallUrl)" }
}
if (-not ($missingNow.Count + $missingP2.Count + $missingP4.Count)) {
    Write-Host "All tools present." -ForegroundColor Green
}

if ($Strict -and ($missingNow.Count + $missingP2.Count + $missingP4.Count) -gt 0) { exit 1 }
if ($missingNow.Count -gt 0) { exit 1 }
exit 0
