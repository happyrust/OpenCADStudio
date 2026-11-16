# Runs the end-to-end scenarios in four groups, one after another, and writes
# one summary: the eleven conflict and cross-area scenarios (run_cross.ps1),
# the eight desktop-client scenarios (run_all.ps1), the later cross-area
# scenarios (every y<digit>*.py in this directory), then the server-process
# scenarios (every s<digit>*.py: the collaboration server and WS sessions
# only, no GUI).
#
#   powershell -File run19.ps1 [-Label <name>] [-Only19] [-OnlyNew] [-OnlyServer]
#                              [-EvidenceDir <dir>] [-BinDir <dir>] [-PortShift <n>]
#
# -Label     names this run: the summary is summary-<Label>-<time>.txt in the
#            evidence directory.
# -Only19    only the first two groups; -OnlyNew only the third; -OnlyServer
#            only the fourth. Given together, they add up.
# -EvidenceDir, -BinDir and -PortShift are explained in e2e_env.ps1; each can
# be given by its environment variable instead.
#
# Drawings the scenarios read but do not make are made first when missing: the
# M3 baseline drawing (by the ignored test write_the_m3_baseline_drawing, which
# needs cargo) and the two seed drawings (x00_seed.py, t01_seed.py). The
# server-process scenarios read no drawing, so -OnlyServer makes none.
# No process this run did not start is touched; the summary only records how
# many other instances of the two programs were running, to explain a failure by.
param([string]$Label = 'run', [switch]$Only19, [switch]$OnlyNew, [switch]$OnlyServer,
      [string]$EvidenceDir = '', [string]$BinDir = '', [string]$PortShift = '')
$picked = $Only19 -or $OnlyNew -or $OnlyServer
$run19 = (-not $picked) -or $Only19
$runNew = (-not $picked) -or $OnlyNew
$runServer = (-not $picked) -or $OnlyServer
$ErrorActionPreference = 'Continue'
try { [Console]::OutputEncoding = [Text.Encoding]::UTF8 } catch { }
$env:PYTHONIOENCODING = 'utf-8'
. (Join-Path $PSScriptRoot 'e2e_env.ps1')
$e2e = Resolve-E2eEnv -Here $PSScriptRoot -EvidenceDir $EvidenceDir -BinDir $BinDir -PortShift $PortShift
$root = $e2e.Evidence
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$out = Join-Path $root "summary-$Label-$stamp.txt"
$cross = Join-Path $e2e.Scripts 'run_cross.ps1'
$client = Join-Path $e2e.Scripts 'run_all.ps1'

function Say([string]$text) { $text | Out-File -Encoding utf8 -Append $out; Write-Host $text }

$version = (& (Join-Path $e2e.Bin 'OpenCADStudio.exe') --version 2>&1 | Select-Object -First 2) -join ' / '
Say "### $(Get-Date -Format s) label=$Label bin=$($e2e.Bin) port-shift=$($e2e.PortShift) :: $version"
Say "other instances running at start: $(@(Get-E2eProcesses $e2e -Other).Count)"

# Inputs the scenarios read but do not make.
$m7work = Join-Path $root 'm7\work'; $m6work = Join-Path $root 'm6\work'
$m7logs = Join-Path $root 'm7\logs'; $m6logs = Join-Path $root 'm6\logs'
New-Item -ItemType Directory -Force -Path $m7work, $m6work, $m7logs, $m6logs | Out-Null
$m3 = Join-Path $m7work 'PID-1101-m3.dwg'
$drawings = $run19 -or $runNew
if ($drawings -and -not (Test-Path $m3)) {
    $env:OCS_M3_BASELINE_OUT = $m3
    Push-Location $e2e.Repo
    cmd /c "cargo test --offline --locked --lib -- --ignored write_the_m3_baseline_drawing > `"$m7logs\m3_baseline.console.txt`" 2>&1"
    Say "m3 baseline: cargo exit=$LASTEXITCODE written=$(Test-Path $m3)"
    Pop-Location
    Remove-Item Env:OCS_M3_BASELINE_OUT
}
Push-Location $e2e.Scripts
if ($drawings -and -not (Test-Path (Join-Path $m7work 'PID-1101-seed.dwg'))) {
    python x00_seed.py *> (Join-Path $m7logs 'x00_seed.console.txt'); Say "x00_seed exit=$LASTEXITCODE"
}
if ($drawings -and -not (Test-Path (Join-Path $m6work 'PID-1101-seed.dwg'))) {
    python t01_seed.py *> (Join-Path $m6logs 't01_seed.console.txt'); Say "t01_seed exit=$LASTEXITCODE"
}
Pop-Location

$rows = @()
function Collect([string]$suite, [string[]]$names) {
    foreach ($name in $names) {
        $log = Join-Path $root "$suite\logs\$name.console.txt"
        $tail = if (Test-Path $log) { (Get-Content $log -Encoding UTF8 | Select-String -Pattern '=== .*: (\d+) PASS / (\d+) FAIL ===' | Select-Object -Last 1) } else { $null }
        if ($tail) {
            $script:rows += [pscustomobject]@{ Suite = $suite; Name = $name; Pass = [int]$tail.Matches[0].Groups[1].Value; Fail = [int]$tail.Matches[0].Groups[2].Value }
        } else {
            $script:rows += [pscustomobject]@{ Suite = $suite; Name = $name; Pass = 0; Fail = -1 }
        }
    }
}

if ($run19) {
    $m7 = 'c01_field_merge', 'c02_object_topology', 'c03_resolution', 'x01_journey', 'x02_claim_exclusive', 'x04_readonly_role',
          'x05_m3_persistence', 'x06_first_visit', 'x07_conflict_closure', 'x10_concurrent_insert', 'x12_after_session'
    $m6 = 't20_session', 't21_claims', 't23_stale', 't24_reconnect', 't25_offline', 't10_broadcast', 't11_same_user', 't22_ttl'
    & powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File $cross | ForEach-Object { Say "  $_" }
    Collect 'm7' $m7
    & powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File $client | ForEach-Object { Say "  $_" }
    Collect 'm6' $m6
}
if ($runNew) {
    $new = @(Get-ChildItem $e2e.Scripts -Filter 'y*.py' | Where-Object { $_.Name -match '^y\d' } |
        Sort-Object Name | ForEach-Object { $_.BaseName })
    if ($new.Count -gt 0) {
        & powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File $cross -Only ($new -join ',') -New | ForEach-Object { Say "  $_" }
        Collect 'm7' $new
    }
}
if ($runServer) {
    $server = @(Get-ChildItem $e2e.Scripts -Filter 's*.py' | Where-Object { $_.Name -match '^s\d' } |
        Sort-Object Name | ForEach-Object { $_.BaseName })
    if ($server.Count -gt 0) {
        & powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File $cross -Only ($server -join ',') -New | ForEach-Object { Say "  $_" }
        Collect 'm7' $server
    }
}

Say ''
foreach ($row in $rows) { Say ("{0,-3} {1,-28} pass={2,3} fail={3,2}" -f $row.Suite, $row.Name, $row.Pass, $row.Fail) }
$old = @($rows | Where-Object { $_.Name -notmatch '^[ys]\d' })
$newRows = @($rows | Where-Object { $_.Name -match '^y\d' })
$serverRows = @($rows | Where-Object { $_.Name -match '^s\d' })
$sum = { param($set) (($set | Measure-Object -Property Pass -Sum).Sum, (($set | Where-Object { $_.Fail -ne 0 }) | Measure-Object).Count) }
if ($old.Count) { $s = & $sum $old; Say ("old scenarios: {0} ({1} checks passed, {2} scenario(s) not clean)" -f $old.Count, $s[0], $s[1]) }
if ($newRows.Count) { $s = & $sum $newRows; Say ("new scenarios: {0} ({1} checks passed, {2} scenario(s) not clean)" -f $newRows.Count, $s[0], $s[1]) }
if ($serverRows.Count) { $s = & $sum $serverRows; Say ("server scenarios: {0} ({1} checks passed, {2} scenario(s) not clean)" -f $serverRows.Count, $s[0], $s[1]) }
Say "other instances running at end: $(@(Get-E2eProcesses $e2e -Other).Count)"
Say "### $(Get-Date -Format s) done -> $out"
