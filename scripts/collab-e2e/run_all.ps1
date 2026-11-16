# Run the M6 validation scenarios one after another, each to its own console
# log, and check after each one that nothing it started is left running.
# No git commands in here.
#
#   powershell -File run_all.ps1 [-Only <name>[,<name>...]] [-New]
#                                [-EvidenceDir <dir>] [-BinDir <dir>] [-PortShift <n>]
#
# -Only picks from the eight scenarios listed below; with -New it names the
# scenarios to run itself (the harness's own: t00_smoke, t02_harness_rules,
# t03_vanish_retry, t04_entry_rerun).
# -EvidenceDir, -BinDir and -PortShift are explained in e2e_env.ps1; each can
# be given by its environment variable instead. Logs go to <EvidenceDir>\m6.
#
# A scenario that loses an application instance (exit code 75) is run once
# more and both results are kept: Invoke-E2eScenario in e2e_env.ps1.
param([string[]]$Only = @(), [switch]$New, [string]$EvidenceDir = '', [string]$BinDir = '', [string]$PortShift = '')
$Only = @($Only | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
$ErrorActionPreference = 'Continue'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$env:PYTHONIOENCODING = 'utf-8'
. (Join-Path $PSScriptRoot 'e2e_env.ps1')
$e2e = Resolve-E2eEnv -Here $PSScriptRoot -EvidenceDir $EvidenceDir -BinDir $BinDir -PortShift $PortShift
$root = Join-Path $e2e.Evidence 'm6'
New-Item -ItemType Directory -Force -Path (Join-Path $root 'logs') | Out-Null
$all = 't20_session', 't21_claims', 't23_stale', 't24_reconnect', 't25_offline', 't10_broadcast', 't11_same_user', 't22_ttl'
if ($New) { $all = $Only } elseif ($Only.Count -gt 0) { $all = $all | Where-Object { $Only -contains $_ } }
$summary = Join-Path $root 'logs\run_all.summary.txt'
"### $(Get-Date -Format s) run_all: $($all -join ', ')" | Out-File -Encoding utf8 -Append $summary
Push-Location $e2e.Scripts
try {
    foreach ($name in $all) { Invoke-E2eScenario -E2e $e2e -Suite 'm6' -Name $name -Summary $summary }
} finally {
    Pop-Location
}
