# Run the M7 scenarios one after another, each to its own console log, and
# check after each one that nothing it started is left running and that it
# left no automation descriptor behind. No git commands in here.
#
#   powershell -File run_cross.ps1 [-Only <name>[,<name>...]] [-New]
#                                  [-EvidenceDir <dir>] [-BinDir <dir>] [-PortShift <n>]
#
# -Only picks from the eleven scenarios listed below; with -New it names the
# scenarios to run itself (the later ones, y03 / y08 / y09).
# -EvidenceDir, -BinDir and -PortShift are explained in e2e_env.ps1; each can
# be given by its environment variable instead. Logs go to <EvidenceDir>\m7.
param([string[]]$Only = @(), [switch]$New, [string]$EvidenceDir = '', [string]$BinDir = '', [string]$PortShift = '')
$Only = @($Only | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
$ErrorActionPreference = 'Continue'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$env:PYTHONIOENCODING = 'utf-8'
. (Join-Path $PSScriptRoot 'e2e_env.ps1')
$e2e = Resolve-E2eEnv -Here $PSScriptRoot -EvidenceDir $EvidenceDir -BinDir $BinDir -PortShift $PortShift
$root = Join-Path $e2e.Evidence 'm7'
New-Item -ItemType Directory -Force -Path (Join-Path $root 'logs') | Out-Null
$all = 'c01_field_merge', 'c02_object_topology', 'c03_resolution',
       'x01_journey', 'x02_claim_exclusive', 'x04_readonly_role', 'x05_m3_persistence',
       'x06_first_visit', 'x07_conflict_closure', 'x10_concurrent_insert', 'x12_after_session'
if ($New) { $all = $Only } elseif ($Only.Count -gt 0) { $all = $all | Where-Object { $Only -contains $_ } }
$summary = Join-Path $root 'logs\run_cross.summary.txt'
"### $(Get-Date -Format s) run_cross: $($all -join ', ')" | Out-File -Encoding utf8 -Append $summary
Push-Location $e2e.Scripts
try {
    foreach ($name in $all) {
        $log = Join-Path $root "logs\$name.console.txt"
        $sw = [Diagnostics.Stopwatch]::StartNew()
        python "$name.py" *> $log
        $code = $LASTEXITCODE
        $left = @(Get-E2eProcesses $e2e)
        $descriptors = @(Get-ChildItem (Join-Path $root 'work\appdata') -Recurse -Filter '*.json' -ErrorAction SilentlyContinue |
            Where-Object { $_.DirectoryName -match '\\OpenCADStudio\\automation$' })
        $tail = (Get-Content $log -Encoding UTF8 | Select-String -Pattern '^\d\d:\d\d:\d\d === ' | Select-Object -Last 1).Line
        $desk = (Get-Content $log -Encoding UTF8 | Select-String -Pattern 'desktop (untouched|CHANGED)' | Select-Object -Last 1).Line
        $line = "$(Get-Date -Format s) $name exit=$code $([int]$sw.Elapsed.TotalSeconds)s leftover=$($left.Count) descriptors=$($descriptors.Count) :: $tail :: $desk"
        $line | Out-File -Encoding utf8 -Append $summary
        Write-Host $line
        foreach ($p in $left) { Write-Host "    LEFTOVER pid=$($p.ProcessId) $($p.CommandLine)" }
    }
} finally {
    Pop-Location
}
