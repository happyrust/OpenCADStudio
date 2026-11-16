# Lists the rustfmt --check hunks of a file that its baseline copy does not have.
# Usage: fmt-newhunks.ps1 <baseline-file> <current-file> [-Rustfmt <path>]
# rustfmt is the one on PATH, unless -Rustfmt names another.
param([string]$Base, [string]$Current, [string]$Rustfmt = '')
if (-not $Rustfmt) {
    $found = Get-Command rustfmt -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $found) { throw 'rustfmt is not on PATH; give -Rustfmt <path>.' }
    $Rustfmt = $found.Source
}
function Get-Hunks([string]$path) {
    $tmp = [System.IO.Path]::GetTempFileName()
    cmd /c "`"$Rustfmt`" --check --edition 2021 --config skip_children=true `"$path`" > `"$tmp`" 2>&1"
    $lines = Get-Content $tmp -Encoding UTF8
    Remove-Item $tmp
    $hunks = @()
    $body = $null
    $head = ''
    foreach ($l in $lines) {
        $clean = $l -replace '\x1b\[[0-9;]*m', ''
        if ($clean -match '^Diff in .*:(\d+):?$' -or $clean -match '^Diff in .* at line (\d+):') {
            if ($null -ne $body) { $hunks += [pscustomobject]@{ Head = $head; Body = ($body -join "`n") } }
            $head = $Matches[1]
            $body = @()
        } elseif ($null -ne $body) {
            $body += $clean
        }
    }
    if ($null -ne $body) { $hunks += [pscustomobject]@{ Head = $head; Body = ($body -join "`n") } }
    return $hunks
}
$before = Get-Hunks $Base
$after = Get-Hunks $Current
$known = @{}
foreach ($h in $before) { $known[$h.Body] = 1 + [int]$known[$h.Body] }
$new = @()
foreach ($h in $after) {
    if ([int]$known[$h.Body] -gt 0) { $known[$h.Body] = $known[$h.Body] - 1 } else { $new += $h }
}
"before=$($before.Count) after=$($after.Count) new=$($new.Count)"
foreach ($h in $new) { "@@ line $($h.Head)"; $h.Body }
