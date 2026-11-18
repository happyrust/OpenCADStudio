# Walks the sample SmartPlant backups in both interfaces and sums the walks up.
#
# Usage: walk-samples.ps1 [-Plant <dir>] [-Small <dir>] [-SmallZip <zip>] [-SqlServer <dir>]
#                         [-PlantExpect 53,2,1] [-SmallExpect 7,1,1] [-SqlServerExpect 12,11,2]
#                         [-Only plant,small,small-zip,sqlserver] [-Out <dir>] [-Lang zh-CN]
#
# Eight walks, one after another: the plant backup, the small backup from its
# folder and from its zip, and the SQL Server backup, each once in the classic
# interface and once in the P&ID module. A walk is the ignored test
# app::commands::pidbackup::audit, held to its expectation: three numbers, the
# drawings, the drawings whose items may fail to read at most, the drawings that
# may fail to import at most. walk_summary.py, next to this script, then reads
# the eight reports: one table, the two interfaces of each backup compared, the
# zip compared with its folder, and the verdict.
#
# The samples are not in the repository. The defaults are where they lie on the
# machine the plan was written on (docs/plans/2026-11-17-sppid-backup-publish/
# 01-shared-conventions.md, section 2); the default expectations are the limits
# that plan settled (its readings.md, section 3.9).
#
# -Out receives everything: one folder per walk (report.txt, the pictures,
# cargo-test.log), runs.json and summary.md. It must lie outside the
# repository; the default is D:\scratch\ocs-sppid\<date>-walk-samples.
# -Only walks some of the four sources; the summary then says it is only a part.
#
# cargo runs in the repository this script lies in, offline and with the lock
# file, whatever the current folder is. All eight walks take about nine
# minutes once the test program is built. The table is Chinese Markdown, ready
# for readings.md; summary.md holds the same text as UTF-8.
#
# Exit code: 0 when every walk passes and every comparison agrees; 1 when
# something does not; 2 when the arguments cannot be used, the test program
# does not build, or the summary could not be made.
#
# ASCII only: Windows PowerShell 5.1 misreads UTF-8 without a BOM.
param(
    [string]$Plant = 'D:\work\plant-code\cad\pid-test-data',
    [string]$Small = 'D:\work\plant-code\cad\pid-parse\test-file\backup-test\DWG-0202GP06-01_p',
    [string]$SmallZip = '',
    [string]$SqlServer = 'D:\work\plant-code\cad\pid-parse\test-file\backup-test\TEST02_p',
    [string[]]$PlantExpect = @('53,2,1'),
    [string[]]$SmallExpect = @('7,1,1'),
    [string[]]$SqlServerExpect = @('12,11,2'),
    [string[]]$Only = @(),
    [string]$Out = '',
    [string]$Lang = 'zh-CN'
)

function Stop-Usage([string]$message) {
    [Console]::Error.WriteLine("walk-samples: $message")
    exit 2
}

function Get-Expectation([string]$name, [string[]]$value) {
    # 7,1,1 typed in PowerShell arrives as three values; from -File as one.
    $text = ($value -join ',') -replace '\s', ''
    if ($text -notmatch '^\d+,\d+,\d+$') {
        Stop-Usage "-$name is '$text'; it takes three numbers, as in 53,2,1."
    }
    return $text
}

function Get-FullPath([string]$path) {
    # Against the current location of PowerShell, whether or not the path exists.
    $full = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($path)
    if ($full.Length -gt 3) { $full = $full.TrimEnd('\', '/') }
    return $full
}

foreach ($tool in 'cargo', 'python') {
    if (-not (Get-Command $tool -CommandType Application -ErrorAction SilentlyContinue)) {
        Stop-Usage "$tool is not on PATH."
    }
}
$expectPlant = Get-Expectation 'PlantExpect' $PlantExpect
$expectSmall = Get-Expectation 'SmallExpect' $SmallExpect
$expectSqlServer = Get-Expectation 'SqlServerExpect' $SqlServerExpect
if (-not $SmallZip) { $SmallZip = (Get-FullPath $Small) + '.zip' }
$sources = @(
    @{ Name = 'plant'; Package = 'plant'; Form = 'folder'; Source = (Get-FullPath $Plant); Expect = $expectPlant },
    @{ Name = 'small'; Package = 'small'; Form = 'folder'; Source = (Get-FullPath $Small); Expect = $expectSmall },
    @{ Name = 'small-zip'; Package = 'small'; Form = 'zip'; Source = (Get-FullPath $SmallZip); Expect = $expectSmall },
    @{ Name = 'sqlserver'; Package = 'sqlserver'; Form = 'folder'; Source = (Get-FullPath $SqlServer); Expect = $expectSqlServer }
)
$known = @($sources | ForEach-Object { $_.Name })
$asked = @($Only | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim().ToLowerInvariant() } | Where-Object { $_ })
foreach ($name in $asked) {
    if ($known -notcontains $name) {
        Stop-Usage "-Only takes $($known -join ', '); '$name' is none of them."
    }
}
$picked = @($sources | Where-Object { $asked.Count -eq 0 -or $asked -contains $_.Name })

$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path.TrimEnd('\')
if (-not $Out) { $Out = 'D:\scratch\ocs-sppid\{0}-walk-samples' -f (Get-Date -Format 'yyyy-MM-dd') }
$Out = Get-FullPath $Out
if (($Out + '\').StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase)) {
    Stop-Usage "-Out is $Out, inside the repository; the reports and pictures stay outside it."
}
try {
    New-Item -ItemType Directory -Force -Path $Out -ErrorAction Stop | Out-Null
} catch {
    Stop-Usage "-Out $Out cannot be made: $($_.Exception.Message)"
}

# The walk reads these; whatever the caller had set comes back at the end.
$variables = 'OCS_PID_BACKUP_AUDIT', 'OCS_PID_BACKUP_AUDIT_OUT', 'OCS_PID_BACKUP_AUDIT_LANG',
    'OCS_PID_BACKUP_AUDIT_EXPECT', 'OCS_PID_BACKUP_AUDIT_SHELL', 'OCS_PID_BACKUP_AUDIT_SKIP_IMPORT'
$before = @{}
foreach ($name in $variables) { $before[$name] = [Environment]::GetEnvironmentVariable($name) }
$runs = @()
$count = 2 * $picked.Count
Write-Host "$count walks, one after another; everything goes to $Out"
Push-Location -LiteralPath $repo
try {
    $at = 0
    foreach ($sample in $picked) {
        foreach ($interface in 'classic', 'module') {
            $at++
            $folder = Join-Path $Out ('{0}-{1}' -f $sample.Name, $interface)
            New-Item -ItemType Directory -Force -Path $folder | Out-Null
            # What an earlier run left here must not pass for this walk.
            foreach ($left in 'report.txt', 'cargo-test.log') {
                $path = Join-Path $folder $left
                if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force }
            }
            $run = [ordered]@{
                package = $sample.Package
                form = $sample.Form
                interface = $interface
                source = $sample.Source
                expect = $sample.Expect
                folder = $folder
                exit_code = $null
                seconds = $null
                note = ''
            }
            $label = '[{0}/{1}] {2} {3}' -f $at, $count, $sample.Name, $interface
            if (-not (Test-Path -LiteralPath $sample.Source)) {
                $run.note = 'the source does not exist'
                Write-Host "${label}: not run, $($sample.Source) does not exist"
            } else {
                $env:OCS_PID_BACKUP_AUDIT = $sample.Source
                $env:OCS_PID_BACKUP_AUDIT_OUT = $folder
                $env:OCS_PID_BACKUP_AUDIT_LANG = $Lang
                $env:OCS_PID_BACKUP_AUDIT_EXPECT = $sample.Expect
                if ($interface -eq 'module') {
                    $env:OCS_PID_BACKUP_AUDIT_SHELL = '1'
                } else {
                    Remove-Item Env:OCS_PID_BACKUP_AUDIT_SHELL -ErrorAction SilentlyContinue
                }
                Remove-Item Env:OCS_PID_BACKUP_AUDIT_SKIP_IMPORT -ErrorAction SilentlyContinue
                $log = Join-Path $folder 'cargo-test.log'
                $watch = [Diagnostics.Stopwatch]::StartNew()
                cmd /c "cargo test --offline --locked --lib app::commands::pidbackup::audit -- --ignored --nocapture > `"$log`" 2>&1"
                $run.exit_code = $LASTEXITCODE
                $run.seconds = [math]::Round($watch.Elapsed.TotalSeconds, 1)
                Write-Host ('{0}: exit={1} seconds={2}' -f $label, $run.exit_code, $run.seconds)
                # cargo names the test program before it runs it. Without that
                # line the program did not build, and no later walk fares better.
                $ran = Select-String -LiteralPath $log -Pattern '^\s+Running ' -Quiet -ErrorAction SilentlyContinue
                if ($run.exit_code -ne 0 -and -not $ran) {
                    Get-Content -LiteralPath $log -Tail 20 -ErrorAction SilentlyContinue |
                        ForEach-Object { [Console]::Error.WriteLine($_) }
                    Stop-Usage "the test program does not build; see $log"
                }
            }
            $runs += $run
        }
    }
} finally {
    Pop-Location
    foreach ($name in $variables) { [Environment]::SetEnvironmentVariable($name, $before[$name]) }
}

$manifest = [ordered]@{ partial = ($picked.Count -lt $sources.Count); runs = $runs }
$runsFile = Join-Path $Out 'runs.json'
[IO.File]::WriteAllText($runsFile, ($manifest | ConvertTo-Json -Depth 4), (New-Object Text.UTF8Encoding($false)))

$summary = Join-Path $Out 'summary.md'
if (Test-Path -LiteralPath $summary) { Remove-Item -LiteralPath $summary -Force }
# The summary is Chinese: python writes UTF-8, and the console is told to expect it.
$encoding = $env:PYTHONIOENCODING
$console = $null
try {
    $console = [Console]::OutputEncoding
    [Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
} catch {
    $console = $null
}
$env:PYTHONIOENCODING = 'utf-8'
try {
    python (Join-Path $PSScriptRoot 'walk_summary.py') $runsFile --out $summary
    $verdict = $LASTEXITCODE
} finally {
    $env:PYTHONIOENCODING = $encoding
    if ($console) { try { [Console]::OutputEncoding = $console } catch { } }
}
if (-not (Test-Path -LiteralPath $summary)) {
    Stop-Usage "the summary could not be made from $runsFile"
}
Write-Host "summary: $summary"
exit $verdict
