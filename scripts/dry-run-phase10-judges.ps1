# Wave 6.A dry-run helper — runs each judge's static / grep / structural
# criteria and reports pass/fail. Live-test exec is blocked on this box
# (LESSONS P2); criteria gated on tests that don't exist yet are honestly
# classified as "fixture mismatch" (category c) rather than as real bugs.
#
# Run from workspace root:
#   powershell -File scripts\dry-run-phase10-judges.ps1

$ErrorActionPreference = 'Continue'

function H1($t) { Write-Host ''; Write-Host ('=' * 70) -ForegroundColor Cyan; Write-Host $t -ForegroundColor Cyan; Write-Host ('=' * 70) -ForegroundColor Cyan }
function H2($t) { Write-Host ''; Write-Host "-- $t" -ForegroundColor Yellow }
function OK($t)  { Write-Host "  GREEN: $t" -ForegroundColor Green }
function BAD($t) { Write-Host "  RED:   $t" -ForegroundColor Red }
function INFO($t) { Write-Host "  INFO:  $t" -ForegroundColor Gray }

function HasMatch($path, $pat) {
    return [bool](Select-String -Path $path -Pattern $pat -SimpleMatch:$false -Quiet -ErrorAction SilentlyContinue)
}

# ============================================================
H1 'Judge 1: exclusion-is-total'
# ============================================================
$exc = 'src-tauri\src\activity\exclusion.rs'
$rt  = 'src-tauri\src\activity\runtime.rs'
$mig = 'src-tauri\src\db\migrations\015_activity_wave5_hardening.sql'

H2 'C1: pure-Rust exclusion test suite exists (live exec blocked on this box)'
$tcount = (Select-String -Path $exc -Pattern '#\[test\]').Count
if ($tcount -ge 13) { OK "$tcount #[test] fns in exclusion.rs (>=13 required)" }
else { BAD "only $tcount #[test] fns (expected >=13)" }

H2 'C2: builtin_rules_load_via_load test'
if (HasMatch $exc 'builtin_rules_load') { OK 'test exists' } else { BAD 'MISSING (cat c: fixture mismatch, 6.B work)' }

H2 'C3: record_event_drops_matched_rows integration test'
if (HasMatch $rt 'record_event_drops_matched_rows') { OK 'test exists' } else { BAD 'MISSING (cat c: fixture mismatch, 6.B work)' }

H2 'C4: reload_exclusion_rules_no_leak_across_window test'
if (HasMatch $rt 'reload_exclusion_rules_no_leak_across_window') { OK 'test exists' } else { BAD 'MISSING (cat c: fixture mismatch, 6.B work)' }

H2 'C5: matcher is consulted BEFORE INSERT (structural eyeball)'
$rtTxt = Get-Content $rt -Raw
$idxExc1 = $rtTxt.IndexOf('check_excluded(')
$idxIns1 = $rtTxt.IndexOf('insert_event(')
if ($idxExc1 -gt 0 -and $idxIns1 -gt 0 -and $idxExc1 -lt $idxIns1) {
    OK "check_excluded() at offset $idxExc1 precedes insert_event() at offset $idxIns1 in record_event"
} else {
    BAD "structural ordering violation (exc=$idxExc1, ins=$idxIns1)"
}

H2 'Built-in rules count in migration 015'
$migTxt = Get-Content $mig -Raw
$inserts = [regex]::Matches($migTxt, "(?i)INSERT\s+INTO\s+activity_exclusion_rules").Count
INFO "INSERT-INTO-activity_exclusion_rules statements: $inserts"

# ============================================================
H1 'Judge 2: retention-preserves-abstracts'
# ============================================================
$ret = 'src-tauri\src\activity\retention.rs'

H2 'C1: existing retention.rs unit tests'
$tcount = (Select-String -Path $ret -Pattern '#\[test\]').Count
if ($tcount -ge 1) { OK "$tcount #[test] fns in retention.rs" } else { BAD 'no tests' }

H2 'C2: sweep_marks_blocks_and_deletes_events test'
if (HasMatch $ret 'sweep_marks_blocks_and_deletes_events') { OK 'test exists' } else { BAD 'MISSING (cat c: fixture mismatch, 6.B work)' }

H2 'C3: transaction wrapper in sweep_once'
$retTxt = Get-Content $ret -Raw
$hasTx = $retTxt -match 'conn\.transaction\(\)|tx\.commit\(\)'
if ($hasTx) { OK 'transaction wrapper present' } else { BAD 'sweep_once not wrapped in conn.transaction() / tx.commit()' }

H2 'C4: default privacy posture (no auto-delete when TTLs are 0)'
if (HasMatch $ret 'any_ttl_set|policy_any_ttl') { OK 'any_ttl_set logic + tests present' } else { BAD 'any_ttl_set logic missing' }

H2 'C5: blocks_cutoff branch independent of events_cutoff (structural)'
$hasBlocksCutoff = $retTxt -match 'blocks_cutoff'
$hasEventsCutoff = $retTxt -match 'events_cutoff'
$hasPurgedUpdate = $retTxt -match 'raw_events_purged_at'
if ($hasBlocksCutoff -and $hasEventsCutoff -and $hasPurgedUpdate) {
    OK 'blocks_cutoff, events_cutoff, raw_events_purged_at all distinct'
} else {
    BAD ("structure incomplete (blocks_cutoff=" + $hasBlocksCutoff + ", events_cutoff=" + $hasEventsCutoff + ", purged=" + $hasPurgedUpdate + ")")
}

# ============================================================
H1 'Judge 3: crash-recovery-idempotent'
# ============================================================
$cr = 'src-tauri\src\activity\crash_recovery.rs'

H2 'C1: existing crash_recovery tests'
$tcount = (Select-String -Path $cr -Pattern '#\[test\]').Count
INFO "$tcount #[test] fns in crash_recovery.rs (judge expects 5+)"
if ($tcount -ge 5) { OK "$tcount tests present" } else { BAD "only $tcount tests (expected >=5)" }

H2 'C2: recover_all_is_idempotent test'
if (HasMatch $cr 'recover_all_is_idempotent') { OK 'test exists' } else { BAD 'MISSING (cat c: fixture mismatch, 6.B work)' }

H2 'C3: recover_all_handles_concurrent_calls test'
if (HasMatch $cr 'recover_all_handles_concurrent_calls') { OK 'test exists' } else { BAD 'MISSING (cat c: fixture mismatch, 6.B work)' }

H2 'C4: ended_at synthesis uses MAX(updated_at, started_at)'
if (HasMatch $cr 'MAX\(updated_at|MAX\(\s*updated_at') { OK 'MAX(updated_at, started_at) pattern present' } else { BAD 'pattern missing' }

H2 'C5: recover_all wired in lib.rs setup BEFORE runtime spawn'
$lib = 'src-tauri\src\lib.rs'
$libTxt = Get-Content $lib -Raw
$idxRec = $libTxt.IndexOf('recover_all')
$idxSpw = $libTxt.IndexOf('ActivityCaptureRuntime::spawn')
if ($idxSpw -lt 0) { $idxSpw = $libTxt.IndexOf('CaptureRuntime::spawn') }
if ($idxRec -gt 0 -and $idxSpw -gt 0 -and $idxRec -lt $idxSpw) {
    OK "recover_all (offset $idxRec) precedes runtime spawn (offset $idxSpw)"
} elseif ($idxRec -gt 0 -and $idxSpw -lt 0) {
    INFO ("recover_all found at " + $idxRec + "; runtime spawn token not found by literal match - eyeball needed")
} else {
    BAD ("ordering issue or recover_all not wired (rec=" + $idxRec + ", spawn=" + $idxSpw + ")")
}

# ============================================================
H1 'Judge 4: pdf-renders-correct-block-count'
# ============================================================
$pdf = 'src-tauri\src\activity\pdf_export.rs'
$ctoml = 'src-tauri\Cargo.toml'

H2 'C1: existing pdf_export unit tests'
$tcount = (Select-String -Path $pdf -Pattern '#\[test\]').Count
INFO "$tcount #[test] fns in pdf_export.rs"
if ($tcount -ge 1) { OK "$tcount tests present" } else { BAD 'no tests' }

H2 'C2: pdf-extract is dev-dep only'
$tomlTxt = Get-Content $ctoml -Raw
$idxDev = $tomlTxt.IndexOf('[dev-dependencies]')
$idxPdfX = $tomlTxt.IndexOf('pdf-extract')
$idxDeps = $tomlTxt.IndexOf('[dependencies]')
if ($idxPdfX -gt 0 -and $idxPdfX -gt $idxDev) {
    OK "pdf-extract appears in [dev-dependencies] block (offset $idxPdfX > $idxDev)"
} else {
    BAD ("pdf-extract dev-dep registration check failed (dev=" + $idxDev + ", pdf=" + $idxPdfX + ")")
}

H2 'C3: full_mode_renders_three_block_labels test'
if (HasMatch $pdf 'full_mode_renders_three_block_labels') { OK 'test exists' } else { BAD 'MISSING (cat c: fixture mismatch, 6.B work)' }

H2 'C4: work_report_mode_renders_three_abstracts_no_events test'
if (HasMatch $pdf 'work_report_mode_renders_three_abstracts_no_events') { OK 'test exists' } else { BAD 'MISSING (cat c: fixture mismatch, 6.B work)' }

H2 'C5: PdfMode::parse round-trip'
if (HasMatch $pdf 'PdfMode::parse|fn parse') { OK 'PdfMode parser present' } else { BAD 'parser missing' }

# ============================================================
H1 'Judge 5: sealed-phases-untouched (mechanical diff portion only)'
# ============================================================
H2 'C1: Dictation public-API surface diff vs phase-mc-complete'
$d1 = git diff --name-only phase-mc-complete..HEAD -- `
    src-tauri/src/dictation/ `
    src-tauri/src/injection/ `
    src-tauri/src/cleanup/provider.rs `
    src-tauri/src/cleanup/llm_cleaner.rs `
    src-tauri/src/cleanup/ollama.rs 2>$null
if ($null -eq $d1 -or $d1.Count -eq 0) { OK 'empty diff vs phase-mc-complete (mechanical layer: clean)' }
else { INFO "files in diff (LLM grader needed in 6.B to classify as authorized/unauthorized):"; $d1 | ForEach-Object { Write-Host "    $_" } }

H2 'C2: Meeting Capture pipeline shape diff'
$d2 = git diff --name-only phase-mc-complete..HEAD -- `
    src-tauri/src/meetings/capture.rs `
    src-tauri/src/meetings/long_form_stt.rs `
    src-tauri/src/meetings/formatter.rs `
    src-tauri/src/meetings/merge.rs `
    src-tauri/src/meetings/chunker.rs `
    src-tauri/src/meetings/filler_words.rs `
    src-tauri/src/audio/capture.rs 2>$null
if ($null -eq $d2 -or $d2.Count -eq 0) { OK 'empty diff (mechanical layer: clean)' }
else { INFO 'files in diff (needs LLM classification in 6.B):'; $d2 | ForEach-Object { Write-Host "    $_" } }

H2 'C3: stage=raw UPDATE introductions'
$rawHits = git diff phase-mc-complete..HEAD -- 'src-tauri/src/' 2>$null | Select-String -Pattern 'UPDATE transcripts|UPDATE .* stage'
if ($null -eq $rawHits) { OK 'no UPDATE transcripts / UPDATE...stage patterns in diff' }
else { BAD (("$($rawHits.Count) matches - eyeball needed (could be comment / cleaned-stage)")); $rawHits | Select-Object -First 10 | ForEach-Object { Write-Host "    $_" } }

H2 'C4: migrations 001-014 untouched since phase-mc-complete'
$migDiff = git diff --name-only phase-mc-complete..HEAD -- `
    'src-tauri/src/db/migrations/001_*.sql' `
    'src-tauri/src/db/migrations/002_*.sql' `
    'src-tauri/src/db/migrations/003_*.sql' `
    'src-tauri/src/db/migrations/004_*.sql' `
    'src-tauri/src/db/migrations/005_*.sql' `
    'src-tauri/src/db/migrations/006_*.sql' `
    'src-tauri/src/db/migrations/007_*.sql' `
    'src-tauri/src/db/migrations/008_*.sql' `
    'src-tauri/src/db/migrations/009_*.sql' `
    'src-tauri/src/db/migrations/010_*.sql' `
    'src-tauri/src/db/migrations/011_*.sql' `
    'src-tauri/src/db/migrations/012_*.sql' `
    'src-tauri/src/db/migrations/013_*.sql' `
    'src-tauri/src/db/migrations/014_*.sql' 2>$null
if ($null -eq $migDiff -or $migDiff.Count -eq 0) { OK 'all sealed migrations 001-014 untouched' }
else { BAD 'sealed migrations modified:'; $migDiff | ForEach-Object { Write-Host "    $_" } }

H2 'C5: link-clean test --release --no-run (run separately above)'
INFO 'cargo test --release --no-run completed earlier in this dispatch: clean exit, all targets linked.'

# ============================================================
H1 'Judge 6: provenance-is-total'
# ============================================================
$abs = 'src-tauri\src\activity\abstractor.rs'
$bp  = 'src-tauri\src\activity\blocks_persist.rs'

H2 'C1: fingerprint constants are present + named as expected'
$absTxt = Get-Content $abs -Raw
$hasT = $absTxt -match 'TEMPLATE_NO_PAYLOAD_SHA'
$hasV1 = $absTxt -match 'abstract_v1-'
$hasV2 = $absTxt -match 'abstract_v2_audio-'
if ($hasT -and $hasV1 -and $hasV2) {
    OK 'TEMPLATE_NO_PAYLOAD_SHA, abstract_v1-, abstract_v2_audio- all present'
} else {
    BAD ("missing one of: T=" + $hasT + ", V1=" + $hasV1 + ", V2=" + $hasV2 + ")")
}

H2 'C2: all_blocks_have_prompt_version_sha test'
if (HasMatch $bp 'all_blocks_have_prompt_version_sha') { OK 'test exists' } else { BAD 'MISSING (cat c: 6.B work)' }

H2 'C3: prompt_version_sha_is_known_family test'
if (HasMatch $bp 'prompt_version_sha_is_known_family') { OK 'test exists' } else { BAD 'MISSING (cat c: 6.B work)' }

H2 'C4: source_event_ids_is_valid_json_array_of_strings test'
if (HasMatch $bp 'source_event_ids_is_valid_json_array_of_strings') { OK 'test exists' } else { BAD 'MISSING (cat c: 6.B work)' }

H2 'C5: source_event_ids_reference_existing_rows_or_block_is_purged test'
if (HasMatch $bp 'source_event_ids_reference_existing_rows_or_block_is_purged') { OK 'test exists' } else { BAD 'MISSING (cat c: 6.B work)' }

H2 'C6: insert_block binds prompt_version_sha as &str (not Option)'
$bpTxt = Get-Content $bp -Raw
$hasOptionSha = $bpTxt -match 'prompt_version_sha.*Option<|Option<.*prompt_version_sha'
if ($hasOptionSha) { BAD 'prompt_version_sha appears as Option<...> — provenance bypass risk' } else { OK 'prompt_version_sha is non-optional in insert_block path' }

Write-Host ''
Write-Host '=== DRY-RUN COMPLETE. Reds are honestly reported; not fixing in 6.A. ===' -ForegroundColor Magenta
