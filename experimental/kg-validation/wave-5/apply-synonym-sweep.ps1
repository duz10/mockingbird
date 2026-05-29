#requires -Version 5.1
# Wave 5 synonym map sweep -> v1.1
#
# Adds CONSERVATIVE collapses per ADR 0048 G7 discipline:
# - person names NEVER collapse to domain tags
# - specificity preserved when extra info is irreducible
# - domain overlap is NOT equivalence
# - when in doubt, leave out
#
# Source of observations: runs/run-a-baseline + runs/iter-{1..5}-{a,b}
# top-10 near-miss tables.

$ErrorActionPreference = 'Stop'
$mapPath = Join-Path $PSScriptRoot '..\judge-calibration\synonym-map.json'
$j = Get-Content $mapPath -Raw | ConvertFrom-Json

# Three safe additions only. Skipped (per discipline):
#  - after-school -> kid           (different concept: place vs entity)
#  - cake -> bakery / cake -> dad  (different concept: object vs vendor / person)
#  - brake -> car-repair           (specificity preserved; brake is its own thing)
#  - 401k -> retirement            (specificity preserved; both legit)
#  - budget -> meeting/slide-deck  (domain overlap, not equivalence)
$addPlan = @(
    @{ canonical = 'kid';              add = @('kids','children'); rationale = 'plural/collective collapse; person names not affected per discipline' }
    @{ canonical = 'apartment';        add = @('apartment-complex'); rationale = 'over-specification collapse; -complex is decorative' }
    @{ canonical = 'home-maintenance'; add = @('cleanup','home-cleanup'); rationale = 'action-noun collapses into containing domain; corpus context is residential' }
)

$applied = @()
foreach ($plan in $addPlan) {
    $entry = $j.synonyms | Where-Object { $_.canonical -eq $plan.canonical }
    if (-not $entry) {
        Write-Warning ('skip ' + $plan.canonical + ' (canonical missing)')
        continue
    }
    $existing = @($entry.variants)
    $toAdd = $plan.add | Where-Object { $existing -notcontains $_ }
    if ($toAdd.Count -eq 0) {
        Write-Host ('skip ' + $plan.canonical + ' (all variants already present)')
        continue
    }
    $entry.variants = @($existing + $toAdd)
    $applied += [pscustomobject]@{
        canonical = $plan.canonical
        added     = ($toAdd -join ',')
        rationale = $plan.rationale
    }
    Write-Host ('add ' + $plan.canonical + ' += ' + ($toAdd -join ','))
}

$j.version = 'v1.1'
$j | Add-Member -NotePropertyName 'wave_5_sweep' -NotePropertyValue @{
    sweep_date    = (Get-Date -Format 'yyyy-MM-dd')
    bead          = 'mb-ojm5'
    additions     = $applied
    skipped_count = 5
    skipped_note  = 'after-school/cake/brake/401k/budget held per ADR 0048 G7 discipline'
} -Force

# Write as UTF-8 WITHOUT BOM (Rust serde_json rejects BOM at line 1 col 1).
$utf8NoBom = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText($mapPath, ($j | ConvertTo-Json -Depth 100), $utf8NoBom)

Write-Host ('done: synonym-map version -> v1.1 (' + $applied.Count + ' new variant assignments)')
