# Generate experimental/kg-validation/judge-calibration/synonym-map.json
# v1 from three input sources, per ADR 0048 section G7.
#
#   1. Auto-seed: every distinct tag in corpus/answer-keys/*.json
#      acceptable_topic_tag_sets, normalized through the pipeline
#      normalize rules. Each becomes a canonical with empty variants,
#      source=auto-seed-answer-key.
#
#   2. Bernard hand-augmented seed list (project-knowledge domain
#      coverage from the Wave 3 dispatch brief). Source=bernard-seed.
#      Variants that auto-seeded as their own canonical are demoted
#      to variants of the Bernard-chosen canonical.
#
#   3. Code-puppy diff-driven additions discovered by comparing the
#      pipeline tag vocabulary in runs/run-a-baseline/structured/*.json
#      against the canonical+variant set. Source=diff-driven-codepuppy.
#
# Output sorted by canonical for stable diffs across regenerations.
#
# Usage (from experimental/kg-validation/):
#   powershell -File scripts\generate-synonym-map.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$answerKeysDir = Join-Path $root "corpus\answer-keys"
$outPath = Join-Path $root "judge-calibration\synonym-map.json"

# Mirror src/passes/normalize.rs::normalize_one. Pure function so the
# auto-seed canonical matches what the pipeline emits.
function Normalize-Tag([string]$raw) {
    if ([string]::IsNullOrWhiteSpace($raw)) { return "" }
    $s = $raw.ToLower()
    $sb = New-Object System.Text.StringBuilder
    foreach ($c in $s.ToCharArray()) {
        if ([char]::IsWhiteSpace($c) -or $c -eq '_') { [void]$sb.Append('-') }
        else { [void]$sb.Append($c) }
    }
    $s = $sb.ToString()
    while ($s.Contains('--')) { $s = $s -replace '--', '-' }
    $s = $s.Trim('-')

    function Singularize([string]$w) {
        if ($w.Length -le 3) { return $w }
        if ($w.EndsWith('ies') -and $w.Length -gt 3) { return ($w.Substring(0, $w.Length - 3) + 'y') }
        if (-not $w.EndsWith('s')) { return $w }
        if ($w.EndsWith('ss') -or $w.EndsWith('sh') -or $w.EndsWith('ch') -or $w.EndsWith('us')) { return $w }
        if ($w.EndsWith('xes') -or $w.EndsWith('zes')) { return $w.Substring(0, $w.Length - 2) }
        $prior = $w[$w.Length - 2]
        if ('sxzuio'.Contains($prior)) { return $w }
        return $w.Substring(0, $w.Length - 1)
    }

    $dash = $s.LastIndexOf('-')
    if ($dash -ge 0) {
        $head = $s.Substring(0, $dash + 1)
        $last = $s.Substring($dash + 1)
        return $head + (Singularize $last)
    }
    return Singularize $s
}

# ---- 1. Auto-seed from answer keys ----
$autoSeed = New-Object 'System.Collections.Generic.SortedSet[string]'
Get-ChildItem -Path (Join-Path $answerKeysDir "*.json") | ForEach-Object {
    $j = Get-Content -Raw -Path $_.FullName | ConvertFrom-Json
    foreach ($entry in $j.entries) {
        foreach ($set in $entry.acceptable_topic_tag_sets) {
            foreach ($tag in $set) {
                $n = Normalize-Tag $tag
                if ($n) { [void]$autoSeed.Add($n) }
            }
        }
    }
}

# entries: ordered hashtable, key = canonical, value = entry
$entries = [ordered]@{}
foreach ($c in $autoSeed) {
    $entries[$c] = [ordered]@{
        canonical = $c
        variants = New-Object 'System.Collections.Generic.List[string]'
        rationale = "Auto-seeded from corpus answer-keys vocabulary; identity canonical."
        source = "auto-seed-answer-key"
    }
}

function Add-Mapping {
    param(
        [string]$Canonical,
        [string[]]$Variants,
        [string]$Rationale,
        [string]$Source
    )
    $canon = Normalize-Tag $Canonical
    if (-not $entries.Contains($canon)) {
        $entries[$canon] = [ordered]@{
            canonical = $canon
            variants = New-Object 'System.Collections.Generic.List[string]'
            rationale = $Rationale
            source = $Source
        }
    } else {
        if ($entries[$canon].source -eq "auto-seed-answer-key") {
            $entries[$canon].source = $Source
            $entries[$canon].rationale = $Rationale
        }
    }
    foreach ($v in $Variants) {
        $vn = Normalize-Tag $v
        if (-not $vn) { continue }
        if ($vn -eq $canon) { continue }
        if ($entries.Contains($vn)) {
            $existingVariants = @($entries[$vn].variants)
            foreach ($carry in $existingVariants) {
                if ($carry -ne $canon -and -not $entries[$canon].variants.Contains($carry)) {
                    [void]$entries[$canon].variants.Add($carry)
                }
            }
            $entries.Remove($vn)
        }
        if (-not $entries[$canon].variants.Contains($vn)) {
            [void]$entries[$canon].variants.Add($vn)
        }
    }
}

# ---- 2. Bernard hand-augmented seed list ----
# Stored as a data table to sidestep PowerShell positional-parsing weirdness
# with parens inside double-quoted strings passed to a function.
$bernardSeeds = @(
    @{ C="car-repair";        V=@("auto-maintenance","auto-repair","car-maintenance","vehicle-repair","auto"); R="Automotive maintenance domain. Bernard seed." }
    @{ C="daycare";           V=@("childcare","kid-care","child-care"); R="Daycare and childcare synonyms. Bernard seed." }
    @{ C="meal-planning";     V=@("meal-prep","meal-preparation"); R="Meal-prep and meal-planning synonyms. Bernard seed." }
    @{ C="groceries";         V=@("grocery-shopping","shopping-list","food-shopping"); R="Grocery shopping artifacts collapse. Bernard seed." }
    @{ C="doctor-appointment";V=@("medical-appointment","dr-appointment"); R="Doctor-appointment synonyms. Bernard seed. NOTE: NOT a collapse target for pediatrician - pediatric specificity preserved." }
    @{ C="home-maintenance";  V=@("household","chores","house-chores"); R="Household maintenance domain. Bernard seed. Canonical chosen as home-maintenance since answer-key uses home-maintenance." }
    @{ C="finance";           V=@("money","personal-finance"); R="General financial-management synonyms. Bernard seed. Per caveat: budget / budgeting / budget-revision intentionally NOT collapsed - different concept than the finance domain." }
    @{ C="meeting";           V=@("check-in","sync","standup","1on1","one-on-one","all-hands"); R="Meeting-type synonyms incl. cadence variants. Bernard seed. Abstraction-level collapse: a 1on1 IS a meeting for filing purposes." }
    @{ C="email";             V=@("correspondence","outreach","email-followup"); R="Email and outreach synonyms. Bernard seed." }
    @{ C="marketing";         V=@("promotion","advertising"); R="Marketing channel synonyms. Bernard seed." }
    @{ C="planning";          V=@("project-planning","quarterly-planning","q3-planning","q4-planning"); R="Planning abstraction-level collapse. Bernard seed. Quarterly variants collapse to general planning." }
    @{ C="tools";             V=@("equipment","supplies","gear"); R="Tradesperson tool and supply synonyms. Bernard seed." }
    @{ C="client";            V=@("customer"); R="Client and customer synonyms. Bernard seed." }
    @{ C="invoice";           V=@("billing","invoicing"); R="Invoicing synonyms. Bernard seed." }
    @{ C="school";            V=@("kids-school","school-pickup","school-dropoff"); R="School-context synonyms. Bernard seed. All resolve to the school-coordination domain." }
    @{ C="pediatrician";      V=@("kids-doctor","kid-doctor"); R="Pediatrician synonyms. Bernard seed. Discipline: does NOT collapse to general doctor - pediatric specificity matters." }
)
foreach ($e in $bernardSeeds) {
    Add-Mapping -Canonical $e.C -Variants $e.V -Rationale $e.R -Source "bernard-seed"
}

# ---- 3. Diff-driven discovery (conservative; each cleared G7 self-review) ----
$diffSeeds = @(
    @{ C="farmers-market";  V=@("farmer's-market"); R="Punctuation variant: pipeline emits possessive apostrophe; answer key omits it. Normalizer does not strip apostrophes, so synonym map closes the gap. Safe collapse, identical referent." }
    @{ C="chen";            V=@("mrs-chen"); R="Honorific stripping for the same person; same persona-03 reference." }
    @{ C="roth";            V=@("roth-ira"); R="Roth IRA abbreviation; pipeline expands, answer key contracts. Same retirement-account referent." }
    @{ C="side-business";   V=@("side-work"); R="Side-hustler synonyms; same persona-04 economic activity." }
    @{ C="smith";           V=@("the-smith"); R="Family-name reference: pipeline emits the-Smith; answer key uses plural Smiths which normalizes to smith. Same family referent. Per G7 this is a person-name match within the same household, not a person-domain collapse." }
    @{ C="wholesale";       V=@("wholesaler"); R="Activity vs. actor synonym for the same wholesale-purchasing context in persona-04 corpus." }
)
foreach ($e in $diffSeeds) {
    Add-Mapping -Canonical $e.C -Variants $e.V -Rationale $e.R -Source "diff-driven-codepuppy"
}

# ---- Serialize ----
$sortedCanonicals = @($entries.Keys) | Sort-Object

$synonymList = New-Object 'System.Collections.Generic.List[object]'
foreach ($c in $sortedCanonicals) {
    $e = $entries[$c]
    $sortedVariants = @($e.variants | Sort-Object)
    $synonymList.Add([ordered]@{
        canonical = $e.canonical
        variants = $sortedVariants
        rationale = $e.rationale
        source = $e.source
    })
}

$rootObj = [ordered]@{
    version = "v1"
    schema_version = "synonym-map-v1"
    generated_from = @(
        "corpus/answer-keys/*.json (auto-seed)",
        "Wave 3 dispatch brief (bernard-seed)",
        "runs/run-a-baseline/structured/*.json (diff-driven-codepuppy)"
    )
    discipline_notes = "Per ADR 0048 G7: person names NEVER collapse to domain tags; specificity preserved when extra info is irreducible; domain overlap is NOT equivalence; when in doubt, leave out."
    synonyms = $synonymList
}

$json = $rootObj | ConvertTo-Json -Depth 8
[System.IO.File]::WriteAllText($outPath, $json, [System.Text.UTF8Encoding]::new($false))

$count = $synonymList.Count
$bySource = $synonymList | Group-Object -Property source
Write-Host "Wrote $outPath"
Write-Host "Total canonicals: $count"
Write-Host "By source:"
foreach ($g in $bySource) { Write-Host ("  {0}: {1}" -f $g.Name, $g.Count) }
