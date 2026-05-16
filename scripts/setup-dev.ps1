# One-shot developer onboarding. Idempotent: re-runs are safe.
#
# Steps:
#   1. Verify environment (calls verify-environment.ps1, non-strict).
#   2. Ensure beads workspace is initialized.
#   3. Seed judges (idempotent merge into ~/.code_puppy/judges.json).
#   4. Print the next-step menu.

[CmdletBinding()]
param(
    [switch]$Strict
)

$PSNativeCommandUseErrorActionPreference = $false
$ErrorActionPreference = "Continue"

# Resolve repo root from this script's location (LESSONS line 6:
# don't rely on $PSScriptRoot in param defaults; compute in body).
$RepoRoot       = Resolve-Path (Join-Path $PSScriptRoot "..")
$VerifyScript   = Join-Path $PSScriptRoot "verify-environment.ps1"
$SeedJudges     = Join-Path $PSScriptRoot "seed-judges.ps1"

Write-Host "=== Mockingbird dev setup ===" -ForegroundColor Cyan
Write-Host "Repo root: $RepoRoot"

# Step 1: verify environment
Write-Host ""
Write-Host "[1/4] Verifying environment..." -ForegroundColor Cyan
if ($Strict) {
    & $VerifyScript -Strict
} else {
    & $VerifyScript
}
$envExit = $LASTEXITCODE
if ($Strict -and $envExit -ne 0) {
    Write-Host "Strict mode: environment check failed. Aborting setup." -ForegroundColor Red
    exit 1
}

# Step 2: beads workspace
Write-Host ""
Write-Host "[2/4] Checking beads workspace..." -ForegroundColor Cyan
$bdStatus = bd status 2>&1 | Out-String
if ($bdStatus -match "Total Issues:") {
    Write-Host "Beads workspace already initialized."
} else {
    Write-Host "Beads workspace not found. Run 'bd init --prefix mb' in this directory."
}

# Step 3: judges seed
Write-Host ""
Write-Host "[3/4] Seeding judges (idempotent)..." -ForegroundColor Cyan
if (Test-Path $SeedJudges) {
    & $SeedJudges
} else {
    Write-Host "seed-judges.ps1 not found (unexpected). Skipping."
}

# Step 4: next steps
Write-Host ""
Write-Host "[4/4] Next steps:" -ForegroundColor Cyan
Write-Host "  - Read PLAN-mockingbird-v2.md (the spine)"
Write-Host "  - Read .code_puppy/AGENTS.md"
Write-Host "  - Read STATUS.md for current phase"
Write-Host "  - bd ready    # see unblocked tasks"
Write-Host "  - bd prime    # full workflow brief"
Write-Host ""
Write-Host "Done." -ForegroundColor Green
exit 0
