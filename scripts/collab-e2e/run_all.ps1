# Run the M6 validation scenarios one after another, each to its own console
# log, and check after each one that nothing it started is left running.
# No git commands in here.
#
#   powershell -File run_all.ps1 [-Only <name>[,<name>...]]
#                                [-EvidenceDir <dir>] [-BinDir <dir>] [-PortShift <n>]
#
# -EvidenceDir, -BinDir and -PortShift are explained in e2e_env.ps1; each can
# be given by its environment variable instead. Logs go to <EvidenceDir>\m6.
param([string[]]$Only = @(), [string]$EvidenceDir = '', [string]$BinDir = '', [string]$PortShift = '')
$Only = @($Only | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
$ErrorActionPreference = 'Continue'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$env:PYTHONIOENCODING = 'utf-8'
. (Join-Path $PSScriptRoot 'e2e_env.ps1')
$e2e = Resolve-E2eEnv -Here $PSScriptRoot -EvidenceDir $EvidenceDir -BinDir $BinDir -PortShift $PortShift
$root = Join-Path $e2e.Evidence 'm6'
New-Item -ItemType Directory -Force -Path (Join-Path $root 'logs') | Out-Null
$all = 't20_session', 't21_claims', 't23_stale', 't24_reconnect', 't25_offline', 't10_broadcast', 't11_same_user', 't22_ttl'
if ($Only.Count -gt 0) { $all = $all | Where-Object { $Only -contains $_ } }
$summary = Join-Path $root 'logs\run_all.summary.txt'
"### $(Get-Date -Format s) run_all: $($all -join ', ')" | Out-File -Encoding utf8 -Append $summary
Push-Location $e2e.Scripts
try {
    foreach ($name in $all) {
        $log = Join-Path $root "logs\$name.console.txt"
        $sw = [Diagnostics.Stopwatch]::StartNew()
        python "$name.py" *> $log
        $code = $LASTEXITCODE
        $left = @(Get-E2eProcesses $e2e)
        $tail = (Get-Content $log -Encoding UTF8 | Select-String -Pattern '^\d\d:\d\d:\d\d === ' | Select-Object -Last 1).Line
        $desk = (Get-Content $log -Encoding UTF8 | Select-String -Pattern 'desktop (untouched|CHANGED)' | Select-Object -Last 1).Line
        $line = "$(Get-Date -Format s) $name exit=$code $([int]$sw.Elapsed.TotalSeconds)s leftover=$($left.Count) :: $tail :: $desk"
        $line | Out-File -Encoding utf8 -Append $summary
        Write-Host $line
        foreach ($p in $left) { Write-Host "    LEFTOVER pid=$($p.ProcessId) $($p.CommandLine)" }
    }
} finally {
    Pop-Location
}
