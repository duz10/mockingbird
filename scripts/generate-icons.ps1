# Generate the Tauri icon set from assets/icons/mockingbird.svg.
#
# Strategy:
#   1. If `cargo tauri icon` is available (Tauri CLI 2.x), use it.
#   2. Else if ImageMagick (`magick`) is on PATH, fall back.
#   3. Else exit 0 with a message (soft prereq — Phase 1 regenerates
#      via `cargo tauri init`).
#
# Output: src-tauri/icons/{32x32,128x128,128x128@2x,Square*Logo,StoreLogo,icon}.{png,ico}

[CmdletBinding()]
param(
    [string]$SvgPath = "",
    [string]$OutDir  = ""
)

$PSNativeCommandUseErrorActionPreference = $false
$ErrorActionPreference = "Continue"

# LESSONS line: $PSScriptRoot isn't available in param() defaults — compute here.
if (-not $SvgPath) { $SvgPath = [IO.Path]::Combine($PSScriptRoot, "..", "assets", "icons", "mockingbird.svg") }
if (-not $OutDir)  { $OutDir  = [IO.Path]::Combine($PSScriptRoot, "..", "src-tauri", "icons") }

if (-not (Test-Path $SvgPath)) {
    Write-Host "Source SVG not found: $SvgPath" -ForegroundColor Red
    exit 1
}
if (-not (Test-Path $OutDir)) {
    New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
}

$tauriCli = Get-Command cargo -ErrorAction SilentlyContinue
$magick   = Get-Command magick -ErrorAction SilentlyContinue

if ($tauriCli) {
    Write-Host "Using cargo tauri icon..." -ForegroundColor Cyan
    & cargo tauri icon $SvgPath --output $OutDir 2>&1
    $code = $LASTEXITCODE
    if ($code -eq 0) {
        Write-Host "Icons generated under $OutDir." -ForegroundColor Green
        exit 0
    }
    Write-Host "cargo tauri icon exited $code; trying ImageMagick fallback..." -ForegroundColor Yellow
}

if ($magick) {
    Write-Host "Using ImageMagick fallback..." -ForegroundColor Cyan
    # Tauri's expected icon set (sizes per Tauri CLI 2.x convention).
    $sizes = @(32, 128, 256, 512)
    foreach ($s in $sizes) {
        $out = Join-Path $OutDir "${s}x${s}.png"
        & magick convert -background none -resize "${s}x${s}" $SvgPath $out
    }
    # .ico needs the multi-resolution package
    $ico = Join-Path $OutDir "icon.ico"
    & magick convert -background none $SvgPath -define icon:auto-resize=256,128,64,32,16 $ico
    Write-Host "Icons generated under $OutDir." -ForegroundColor Green
    exit 0
}

Write-Host ""
Write-Host "Neither 'cargo tauri icon' nor ImageMagick produced an icon set." -ForegroundColor Yellow
Write-Host "This is a SOFT prereq - Phase 1 will regenerate as part of 'cargo tauri init'." -ForegroundColor Yellow
Write-Host "To generate now, install ImageMagick: https://imagemagick.org/script/download.php" -ForegroundColor Yellow
exit 0
