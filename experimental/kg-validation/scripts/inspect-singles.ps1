$j = Get-Content runs\run-a-baseline\SCORE.json | ConvertFrom-Json
$singles = $j.per_dictation | Where-Object { $_.expected_entry_count -eq 1 -and -not $_.is_junk }
Write-Host "Single-item non-junk cases: $($singles.Count)"
Write-Host ""
foreach ($s in $singles) {
    $seg = if ($s.segmentation_correct) { 'OK' } else { 'BAD' }
    if ($s.per_entry.Count -eq 0) {
        Write-Host ("{0,-25} seg={1} actual={2} -> NO ENTRIES" -f $s.dictation_id, $seg, $s.actual_entry_count)
        continue
    }
    $e = $s.per_entry[0]
    $tag = ("cat={0} type={1} date={2} inv={3}" -f $e.category_correct, $e.entry_type_correct, $e.date_match, $e.date_invented)
    Write-Host ("{0,-25} seg={1} actual={2} -> {3}" -f $s.dictation_id, $seg, $s.actual_entry_count, $tag)
}
