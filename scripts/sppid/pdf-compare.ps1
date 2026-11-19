# Exports the sample SmartPlant backups as PDF, compares the PDFs with the ones
# of pid_plot, measures the vendor's sheets, and sums it all up.
#
# Usage: pdf-compare.ps1 [-Plant <dir>] [-Small <dir>] [-SmallZip <zip>] [-SqlServer <dir>]
#                        [-Vendor <dir>] [-Fonts <dir>] [-Tools <dir>]
#                        [-PlantExpect 53,53] [-SmallExpect 7,7] [-SqlServerExpect 12,12]
#                        [-VendorExpect 11] [-PlantLimit 600]
#                        [-Only plant,small,small-zip,sqlserver,vendor] [-Out <dir>]
#
# Five steps, one after another. For the plant backup, the small backup from
# its folder and from its zip, and the SQL Server backup: the program opens the
# backup and exports it twice (the ignored test app::commands::pidbackuppdf::
# tests::a_real_backup_exports_the_same_pages_twice_and_stays_as_it_was), then
# pid_plot --batch plots the restored backup the program exported, against the
# reference folder the program took. For the vendor's sheets: the program plots
# every .pid of the vendor's folder that has a .pdf beside it (the ignored test
# io::pid_pdf::tests::the_vendors_sheets_are_plotted_where_they_lie), then
# pid_plot_compare measures each plot against the vendor's PDF. pdf_compare.py,
# next to this script, reads what the steps left: one table of the exports, the
# zip compared with its folder, one table of the vendor's sheets beside the
# records of pid-plot's own acceptance, and the verdict.
#
# An expectation is two numbers: the drawings of the backup and the PDFs the
# program writes. -VendorExpect is the number of the vendor's sheets.
# -PlantLimit is how many seconds one export of the plant backup may take.
#
# Both sides plot with the same fonts: PID_PLOT_FONTS is set to -Fonts, by
# default the folder fonts of the plant backup. PID_PLOT_CORPUS, where the
# program turns for the border templates a project lacks, is set to -Plant.
# PID_PLOT_CJK_FONT and PID_SYMBOL_LIBRARY are cleared. Whatever the caller had
# set comes back at the end.
#
# pid_plot.exe and pid_plot_compare.exe are taken from -Tools, by default
# D:\scratch\ocs-sppid\tools\<the first seven digits of the pid-plot commit
# that Cargo.toml pins>, where build-pid-tools.ps1 leaves them.
#
# The samples are not in the repository. The defaults are where they lie on the
# machine the plan was written on (docs/plans/2026-11-17-sppid-backup-publish/
# 01-shared-conventions.md, section 2); the default expectations are what that
# plan measured (its readings.md, section 15).
#
# -Out receives everything: export-<name> (the restore, the two exports,
# report.json, cargo-test.log), batch-<name> (what pid_plot --batch wrote),
# vendor-plots, vendor-compare, the logs, runs.json and summary.md. It must lie
# outside the repository; the default is D:\scratch\ocs-sppid\<date>-pdf-compare.
# Those folders of an earlier run in -Out are removed first. -Only runs some of
# the five steps; the summary then says it is only a part.
#
# cargo runs in the repository this script lies in, offline and with the lock
# file, whatever the current folder is. All five steps take about five minutes
# once the test program is built. The table is Chinese Markdown, ready for
# readings.md; summary.md holds the same text as UTF-8.
#
# Exit code: 0 when everything passes; 1 when something does not; 2 when the
# arguments cannot be used, the test program does not build, or the summary
# could not be made.
#
# ASCII only: Windows PowerShell 5.1 misreads UTF-8 without a BOM.
param(
    [string]$Plant = 'D:\work\plant-code\cad\pid-test-data',
    [string]$Small = 'D:\work\plant-code\cad\pid-parse\test-file\backup-test\DWG-0202GP06-01_p',
    [string]$SmallZip = '',
    [string]$SqlServer = 'D:\work\plant-code\cad\pid-parse\test-file\backup-test\TEST02_p',
    [string]$Vendor = '',
    [string]$Fonts = '',
    [string]$Tools = '',
    [string[]]$PlantExpect = @('53,53'),
    [string[]]$SmallExpect = @('7,7'),
    [string[]]$SqlServerExpect = @('12,12'),
    [int]$VendorExpect = 11,
    [int]$PlantLimit = 600,
    [string[]]$Only = @(),
    [string]$Out = ''
)

$exportTest = 'app::commands::pidbackuppdf::tests::a_real_backup_exports_the_same_pages_twice_and_stays_as_it_was'
$vendorTest = 'io::pid_pdf::tests::the_vendors_sheets_are_plotted_where_they_lie'

function Stop-Usage([string]$message) {
    [Console]::Error.WriteLine("pdf-compare: $message")
    exit 2
}

function Get-Expectation([string]$name, [string[]]$value) {
    # 53,53 typed in PowerShell arrives as two values; from -File as one.
    $text = ($value -join ',') -replace '\s', ''
    if ($text -notmatch '^\d+,\d+$') {
        Stop-Usage "-$name is '$text'; it takes two numbers, as in 53,53."
    }
    return $text
}

function Get-FullPath([string]$path) {
    # Against the current location of PowerShell, whether or not the path exists.
    $full = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($path)
    if ($full.Length -gt 3) { $full = $full.TrimEnd('\', '/') }
    return $full
}

function Remove-Left([string]$path) {
    # What an earlier run left here must not pass for this run.
    if (-not (Test-Path -LiteralPath $path)) { return }
    try {
        Remove-Item -LiteralPath $path -Recurse -Force -ErrorAction Stop
    } catch {
        Stop-Usage "$path is left from an earlier run and cannot be removed: $($_.Exception.Message)"
    }
}

function Stop-Unbuilt([int]$code, [string]$log) {
    # cargo names the test program before it runs it. Without that line the
    # program did not build, and no later step fares better.
    $ran = Select-String -LiteralPath $log -Pattern '^\s+Running ' -Quiet -ErrorAction SilentlyContinue
    if ($code -ne 0 -and -not $ran) {
        Get-Content -LiteralPath $log -Tail 20 -ErrorAction SilentlyContinue |
            ForEach-Object { [Console]::Error.WriteLine($_) }
        Stop-Usage "the test program does not build; see $log"
    }
}

foreach ($tool in 'cargo', 'python') {
    if (-not (Get-Command $tool -CommandType Application -ErrorAction SilentlyContinue)) {
        Stop-Usage "$tool is not on PATH."
    }
}
$expectPlant = Get-Expectation 'PlantExpect' $PlantExpect
$expectSmall = Get-Expectation 'SmallExpect' $SmallExpect
$expectSqlServer = Get-Expectation 'SqlServerExpect' $SqlServerExpect
if ($VendorExpect -lt 0) { Stop-Usage "-VendorExpect is $VendorExpect; it takes the number of sheets." }
if ($PlantLimit -le 0) { Stop-Usage "-PlantLimit is $PlantLimit; it takes a number of seconds." }
$Plant = Get-FullPath $Plant
if (-not $SmallZip) { $SmallZip = (Get-FullPath $Small) + '.zip' }
# The folder is named in Chinese: yan shou shu ju, the data to accept against.
if (-not $Vendor) { $Vendor = 'D:\work\cad\pid-test-data\' + (-join [char[]](0x9A8C, 0x6536, 0x6570, 0x636E)) }
$Vendor = Get-FullPath $Vendor
if (-not $Fonts) { $Fonts = Join-Path $Plant 'fonts' }
$Fonts = Get-FullPath $Fonts
$sources = @(
    @{ Name = 'plant'; Package = 'plant'; Form = 'folder'; Source = $Plant; Expect = $expectPlant; Limit = $PlantLimit },
    @{ Name = 'small'; Package = 'small'; Form = 'folder'; Source = (Get-FullPath $Small); Expect = $expectSmall; Limit = $null },
    @{ Name = 'small-zip'; Package = 'small'; Form = 'zip'; Source = (Get-FullPath $SmallZip); Expect = $expectSmall; Limit = $null },
    @{ Name = 'sqlserver'; Package = 'sqlserver'; Form = 'folder'; Source = (Get-FullPath $SqlServer); Expect = $expectSqlServer; Limit = $null }
)
$known = @($sources | ForEach-Object { $_.Name }) + 'vendor'
$asked = @($Only | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim().ToLowerInvariant() } | Where-Object { $_ })
foreach ($name in $asked) {
    if ($known -notcontains $name) {
        Stop-Usage "-Only takes $($known -join ', '); '$name' is none of them."
    }
}
$picked = @($sources | Where-Object { $asked.Count -eq 0 -or $asked -contains $_.Name })
$withVendor = ($asked.Count -eq 0 -or $asked -contains 'vendor')

$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path.TrimEnd('\')
if (-not $Tools) {
    $pinned = Select-String -LiteralPath (Join-Path $repo 'Cargo.toml') -Pattern '^pid-plot\s*=.*\brev\s*=\s*"([0-9a-f]{7})' |
        Select-Object -First 1
    if (-not $pinned) {
        Stop-Usage 'Cargo.toml pins no pid-plot commit; name the folder of pid_plot.exe with -Tools.'
    }
    $Tools = 'D:\scratch\ocs-sppid\tools\' + $pinned.Matches[0].Groups[1].Value
}
$Tools = Get-FullPath $Tools
$pidPlot = Join-Path $Tools 'pid_plot.exe'
$pidPlotCompare = Join-Path $Tools 'pid_plot_compare.exe'
if ($picked.Count -gt 0 -and -not (Test-Path -LiteralPath $pidPlot -PathType Leaf)) {
    Stop-Usage "no pid_plot.exe in $Tools; scripts\sppid\build-pid-tools.ps1 builds it."
}
if ($withVendor -and -not (Test-Path -LiteralPath $pidPlotCompare -PathType Leaf)) {
    Stop-Usage "no pid_plot_compare.exe in $Tools; scripts\sppid\build-pid-tools.ps1 builds it."
}
if (-not (Test-Path -LiteralPath $Fonts -PathType Container)) {
    Stop-Usage "-Fonts $Fonts is no folder; both sides must plot with the same fonts."
}

if (-not $Out) { $Out = 'D:\scratch\ocs-sppid\{0}-pdf-compare' -f (Get-Date -Format 'yyyy-MM-dd') }
$Out = Get-FullPath $Out
if (($Out + '\').StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase)) {
    Stop-Usage "-Out is $Out, inside the repository; the PDFs and reports stay outside it."
}
try {
    New-Item -ItemType Directory -Force -Path $Out -ErrorAction Stop | Out-Null
} catch {
    Stop-Usage "-Out $Out cannot be made: $($_.Exception.Message)"
}

# The tests and the tools read these; whatever the caller had set comes back at the end.
$variables = 'OCS_PID_BACKUP_PDF_REAL', 'OCS_PID_BACKUP_PDF_REAL_OUT', 'OCS_PID_PDF_VENDOR', 'OCS_PID_PDF_VENDOR_OUT',
    'PID_PLOT_FONTS', 'PID_PLOT_CORPUS', 'PID_PLOT_CJK_FONT', 'PID_SYMBOL_LIBRARY'
$before = @{}
foreach ($name in $variables) { $before[$name] = [Environment]::GetEnvironmentVariable($name) }
$packages = @()
$vendorRun = $null
$build = ''
$count = $picked.Count + [int]$withVendor
Write-Host "$count step(s), one after another; everything goes to $Out"
Push-Location -LiteralPath $repo
try {
    $env:PID_PLOT_FONTS = $Fonts
    $env:PID_PLOT_CORPUS = $Plant
    Remove-Item Env:PID_PLOT_CJK_FONT -ErrorAction SilentlyContinue
    Remove-Item Env:PID_SYMBOL_LIBRARY -ErrorAction SilentlyContinue
    $at = 0
    foreach ($sample in $picked) {
        $at++
        $folder = Join-Path $Out ('export-' + $sample.Name)
        $batchFolder = Join-Path $Out ('batch-' + $sample.Name)
        $batchLog = Join-Path $Out ('batch-' + $sample.Name + '.log')
        Remove-Left $folder
        Remove-Left $batchFolder
        Remove-Left $batchLog
        New-Item -ItemType Directory -Force -Path $folder | Out-Null
        $batch = [ordered]@{ folder = $batchFolder; exit_code = $null; seconds = $null; project = '' }
        $run = [ordered]@{
            package = $sample.Package
            form = $sample.Form
            source = $sample.Source
            expect = $sample.Expect
            folder = $folder
            exit_code = $null
            seconds = $null
            limit_seconds = $sample.Limit
            note = ''
            batch = $batch
        }
        $label = '[{0}/{1}] {2}' -f $at, $count, $sample.Name
        if (-not (Test-Path -LiteralPath $sample.Source)) {
            $run.note = 'the source does not exist'
            Write-Host "${label}: not run, $($sample.Source) does not exist"
        } else {
            $env:OCS_PID_BACKUP_PDF_REAL = $sample.Source
            $env:OCS_PID_BACKUP_PDF_REAL_OUT = $folder
            $log = Join-Path $folder 'cargo-test.log'
            $watch = [Diagnostics.Stopwatch]::StartNew()
            cmd /c "cargo test --offline --locked --lib $exportTest -- --ignored --nocapture --exact > `"$log`" 2>&1"
            $run.exit_code = $LASTEXITCODE
            $run.seconds = [math]::Round($watch.Elapsed.TotalSeconds, 1)
            Write-Host ('{0}: export exit={1} seconds={2}' -f $label, $run.exit_code, $run.seconds)
            Stop-Unbuilt $run.exit_code $log
            if (-not $build) {
                $finished = Select-String -LiteralPath $log -Pattern '^\s*Finished ' | Select-Object -First 1
                if ($finished) { $build = $finished.Line.Trim() }
            }
            # pid_plot plots the restore the program exported, against the
            # reference folder the program took: the restore when it holds
            # Ref, else the backup when that does, else the restore.
            $restored = ''
            $root = ''
            $report = Join-Path $folder 'report.json'
            if (Test-Path -LiteralPath $report) {
                try {
                    $told = Get-Content -LiteralPath $report -Raw -Encoding UTF8 | ConvertFrom-Json
                    $restored = [string]$told.restored
                    $root = [string]$told.package
                } catch {
                    $restored = ''
                }
            }
            if ($restored -and (Test-Path -LiteralPath $restored -PathType Container)) {
                $project = $restored
                if (-not (Test-Path -LiteralPath (Join-Path $restored 'Ref'))) {
                    if ($root -and (Test-Path -LiteralPath (Join-Path $root 'Ref'))) { $project = $root }
                }
                $batch.project = $project
                $watch = [Diagnostics.Stopwatch]::StartNew()
                cmd /c "`"$pidPlot`" --batch `"$restored`" `"$batchFolder`" `"$project`" > `"$batchLog`" 2>&1"
                $batch.exit_code = $LASTEXITCODE
                $batch.seconds = [math]::Round($watch.Elapsed.TotalSeconds, 1)
                Write-Host ('{0}: pid_plot --batch exit={1} seconds={2}' -f $label, $batch.exit_code, $batch.seconds)
            } else {
                Write-Host "${label}: pid_plot --batch not run, the export left no restore to plot"
            }
        }
        $packages += $run
    }

    if ($withVendor) {
        $at++
        $plots = Join-Path $Out 'vendor-plots'
        $readings = Join-Path $Out 'vendor-compare'
        Remove-Left $plots
        Remove-Left $readings
        New-Item -ItemType Directory -Force -Path $plots | Out-Null
        New-Item -ItemType Directory -Force -Path $readings | Out-Null
        $compares = @()
        $vendorRun = [ordered]@{
            folder = $Vendor
            expect = $VendorExpect
            plots = $plots
            readings = $readings
            exit_code = $null
            seconds = $null
            compare_seconds = $null
            note = ''
            compares = $compares
        }
        $label = '[{0}/{1}] vendor' -f $at, $count
        if (-not (Test-Path -LiteralPath $Vendor -PathType Container)) {
            $vendorRun.note = 'the folder does not exist'
            Write-Host "${label}: not run, $Vendor does not exist"
        } else {
            $env:OCS_PID_PDF_VENDOR = $Vendor
            $env:OCS_PID_PDF_VENDOR_OUT = $plots
            $log = Join-Path $plots 'cargo-test.log'
            $watch = [Diagnostics.Stopwatch]::StartNew()
            cmd /c "cargo test --offline --locked --lib $vendorTest -- --ignored --nocapture --exact > `"$log`" 2>&1"
            $vendorRun.exit_code = $LASTEXITCODE
            $vendorRun.seconds = [math]::Round($watch.Elapsed.TotalSeconds, 1)
            Write-Host ('{0}: plots exit={1} seconds={2}' -f $label, $vendorRun.exit_code, $vendorRun.seconds)
            Stop-Unbuilt $vendorRun.exit_code $log
            if (-not $build) {
                $finished = Select-String -LiteralPath $log -Pattern '^\s*Finished ' | Select-Object -First 1
                if ($finished) { $build = $finished.Line.Trim() }
            }
            $sheet = 0
            $watch = [Diagnostics.Stopwatch]::StartNew()
            foreach ($plot in @(Get-ChildItem -LiteralPath $plots -Filter '*.pdf' | Sort-Object Name)) {
                $sheet++
                $name = $plot.BaseName
                $reference = Join-Path $Vendor ($name + '.pdf')
                $drawing = Join-Path $Vendor ($name + '.pid')
                $compareLog = Join-Path $readings ('compare-{0:d2}.log' -f $sheet)
                cmd /c "`"$pidPlotCompare`" `"$reference`" `"$($plot.FullName)`" `"$readings`" `"$drawing`" `"$Vendor`" > `"$compareLog`" 2>&1"
                $compares += [ordered]@{ name = $name; exit_code = $LASTEXITCODE }
            }
            $vendorRun.compares = $compares
            $vendorRun.compare_seconds = [math]::Round($watch.Elapsed.TotalSeconds, 1)
            $failed = @($compares | Where-Object { $_.exit_code -ne 0 }).Count
            Write-Host ('{0}: pid_plot_compare on {1} sheet(s), {2} failed, seconds={3}' -f $label, $sheet, $failed,
                $vendorRun.compare_seconds)
        }
    }
} finally {
    Pop-Location
    foreach ($name in $variables) { [Environment]::SetEnvironmentVariable($name, $before[$name]) }
}

$manifest = [ordered]@{
    partial = ($count -lt $known.Count)
    build = $build
    fonts = $Fonts
    reference = $Plant
    tools = $Tools
    packages = $packages
    vendor = $vendorRun
}
$runsFile = Join-Path $Out 'runs.json'
[IO.File]::WriteAllText($runsFile, ($manifest | ConvertTo-Json -Depth 6), (New-Object Text.UTF8Encoding($false)))

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
    python (Join-Path $PSScriptRoot 'pdf_compare.py') $runsFile --out $summary
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
