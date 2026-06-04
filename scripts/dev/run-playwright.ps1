#requires -Version 5.1
<#
.SYNOPSIS
  One-button Playwright runner for the Mockingbird UI.

.DESCRIPTION
  Installs UI deps with `--ignore-scripts` (per standing rule), then
  separately installs the Playwright browser binaries (which needs
  scripts to run — that's a Playwright-specific dance, not a general
  npm postinstall chain). Then builds the Vite preview output and
  runs the Playwright test suite against it.

  Safe to run repeatedly. Reuses the existing browser install when
  present.

.PARAMETER UI
  Pass to open the Playwright HTML UI (interactive mode) instead of
  the headless run.

.PARAMETER Headed
  Pass to run headed (visible browser window) but still command-line.

.EXAMPLE
  pwsh scripts/dev/run-playwright.ps1
  pwsh scripts/dev/run-playwright.ps1 -UI
  pwsh scripts/dev/run-playwright.ps1 -Headed
#>

param(
  [switch]$UI,
  [switch]$Headed
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
$uiDir = Join-Path $repoRoot "ui"

if (-not (Test-Path $uiDir)) {
  Write-Error "UI directory not found at $uiDir"
  exit 1
}

Push-Location $uiDir
try {
  # 1. Dependency install (no lifecycle scripts).
  if (-not (Test-Path "node_modules")) {
    Write-Host "==> npm install --ignore-scripts" -ForegroundColor Cyan
    npm install --ignore-scripts
    if ($LASTEXITCODE -ne 0) { throw "npm install failed" }
  } else {
    Write-Host "==> node_modules present, skipping install" -ForegroundColor DarkGray
  }

  # 2. Playwright browser install — needed once per machine/version.
  #    `playwright install chromium` is the documented Playwright
  #    pathway; not the same as a general npm postinstall chain.
  $chromiumMarker = Join-Path $env:LOCALAPPDATA "ms-playwright"
  if (-not (Test-Path $chromiumMarker)) {
    Write-Host "==> npx playwright install --with-deps chromium" -ForegroundColor Cyan
    npx --yes playwright install --with-deps chromium
    if ($LASTEXITCODE -ne 0) { throw "playwright install failed" }
  } else {
    Write-Host "==> Playwright browsers present at $chromiumMarker" -ForegroundColor DarkGray
  }

  # 3. Run the suite. webServer block in playwright.config.ts handles
  #    `npm run build && npm run preview` so we don't double-up.
  if ($UI) {
    Write-Host "==> npx playwright test --ui" -ForegroundColor Cyan
    npx --yes playwright test --ui
  }
  elseif ($Headed) {
    Write-Host "==> npx playwright test --headed" -ForegroundColor Cyan
    npx --yes playwright test --headed
  }
  else {
    Write-Host "==> npx playwright test" -ForegroundColor Cyan
    npx --yes playwright test
  }
  $exit = $LASTEXITCODE
}
finally {
  Pop-Location
}

if ($exit -ne 0) {
  Write-Host ""
  Write-Host "Playwright exited with $exit. HTML report: ui/playwright-report/index.html" -ForegroundColor Yellow
  exit $exit
}

Write-Host ""
Write-Host "All Playwright specs passed." -ForegroundColor Green
