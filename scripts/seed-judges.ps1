# Merge .code_puppy/judges-template.json into ~/.code_puppy/judges.json.
# Idempotent: judges keyed by id; existing entries are preserved unless
# --force is passed, in which case the template wins.
#
# Run from repo root:
#   pwsh scripts/seed-judges.ps1
#   pwsh scripts/seed-judges.ps1 -Force

[CmdletBinding()]
param(
    [switch]$Force,
    [string]$TemplatePath = "",
    [string]$TargetPath   = ""
)

$ErrorActionPreference = "Stop"

if (-not $TemplatePath) {
    $TemplatePath = [IO.Path]::Combine($PSScriptRoot, "..", ".code_puppy", "judges-template.json")
}
if (-not $TargetPath) {
    $TargetPath = [IO.Path]::Combine($env:USERPROFILE, ".code_puppy", "judges.json")
}

if (-not (Test-Path $TemplatePath)) {
    throw "Template not found: $TemplatePath"
}

$template = Get-Content -Raw -Path $TemplatePath | ConvertFrom-Json

$targetDir = Split-Path -Parent $TargetPath
if (-not (Test-Path $targetDir)) {
    New-Item -ItemType Directory -Path $targetDir -Force | Out-Null
}

if (Test-Path $TargetPath) {
    $existing = Get-Content -Raw -Path $TargetPath | ConvertFrom-Json
} else {
    $existing = [PSCustomObject]@{ version = 1; judges = @() }
}

if (-not $existing.judges) {
    $existing | Add-Member -NotePropertyName judges -NotePropertyValue @() -Force
}

$existingIds = @{}
foreach ($j in $existing.judges) { $existingIds[$j.id] = $true }

$added   = 0
$skipped = 0
$updated = 0

foreach ($j in $template.judges) {
    if ($existingIds.ContainsKey($j.id)) {
        if ($Force) {
            # Replace the existing entry
            $existing.judges = @($existing.judges | Where-Object { $_.id -ne $j.id }) + $j
            $updated++
        } else {
            $skipped++
        }
    } else {
        $existing.judges = @($existing.judges) + $j
        $added++
    }
}

$existing | ConvertTo-Json -Depth 10 | Set-Content -Path $TargetPath -Encoding UTF8

Write-Host "Merged judges template into $TargetPath"
Write-Host "  Added:   $added"
Write-Host "  Updated: $updated"
Write-Host "  Skipped (use -Force to overwrite): $skipped"
