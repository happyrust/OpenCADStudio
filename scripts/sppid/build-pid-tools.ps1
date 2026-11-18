# Build the pid-parse command-line tools that the PDF and export checks
# compare against, from the commit this repository pins.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\sppid\build-pid-tools.ps1
#
# What it does:
#   1. Reads the rev of every pid-parse git dependency in Cargo.toml. They
#      must all be one full commit id, and Cargo.lock must build the same one.
#   2. Exports that commit with `git archive` from your pid-parse checkout to
#      <ToolsRoot>\<first 7 of the commit>\src. The checkout is only read: no
#      branch switch, no file touched. If it does not hold the commit, the
#      copy cargo fetched of it is used instead.
#   3. Builds pid_plot, pid_plot_compare and pid_publish_xml there: release,
#      offline, with the commit's own Cargo.lock.
#   4. Leaves the three programs and BUILD.txt in <ToolsRoot>\<first 7>.
#
# A commit that is built is not built again: the three programs are checked
# against the hashes in BUILD.txt and the script exits 0. -Force exports and
# runs cargo again; cargo itself still reuses what it has compiled before, and
# BUILD.txt says how many packages it compiled this time.
#
# The source stays where it was compiled. The programs look there for the
# symbols and typefaces the commit carries (test-file\Ref\Symbols,
# test-file\fonts), the way the editor's own build looks in cargo's copy of
# the commit; without the folder they would find less than the editor does.
#
# Exit codes: 0 = the three programs are there; 1 = the export or the build
# failed; 2 = it could not start (an argument, a tool not on PATH, pins that
# disagree). The build never goes online: when cargo's cache lacks a
# dependency the script says so and stops, and fetching it is your call.
#
# ASCII only: Windows PowerShell 5.1 misreads UTF-8 without a BOM.
param(
    # The manifest whose pins are read. Default: this repository's Cargo.toml.
    [string]$Manifest = '',
    # Your pid-parse checkout. Only read.
    [string]$PidParse = 'D:\work\plant-code\cad\pid-parse',
    # Where the tools go, one folder per commit. Outside the repository.
    [string]$ToolsRoot = 'D:\scratch\ocs-sppid\tools',
    # Build even if this commit is already built.
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

$programs = @('pid_plot', 'pid_plot_compare', 'pid_publish_xml')
$builds = @(
    'cargo build --release --offline --locked -p pid-plot --bin pid_plot --bin pid_plot_compare',
    'cargo build --release --offline --locked -p pid-parse --bin pid_publish_xml'
)
$pinPattern = 'git\s*=\s*"https://github\.com/happyrust/pid-parse(\.git)?/?"'
$lockPattern = '^source = "git\+https://github\.com/happyrust/pid-parse(\.git)?/?\?[^#"]*#([0-9a-f]{40})"'

function Say([string]$message) {
    Write-Host "build-pid-tools: $message"
}

function Stop-Usage([string]$message) {
    [Console]::Error.WriteLine("build-pid-tools: $message")
    exit 2
}

function Stop-Failed([string]$message) {
    [Console]::Error.WriteLine("build-pid-tools: $message")
    exit 1
}

function Get-FullPath([string]$path) {
    $full = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($path)
    if ($full.Length -gt 3) { $full = $full.TrimEnd('\', '/') }
    return $full
}

function Get-Sha256([string]$path) {
    return (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
}

# Run a command line through cmd with everything it prints sent to $log, so
# that what a tool says on stderr is text in a file and not a PowerShell
# error. Returns the exit code.
function Invoke-Logged([string]$commandLine, [string]$log) {
    cmd /c "$commandLine > `"$log`" 2>&1" | Out-Null
    return $LASTEXITCODE
}

function Remove-Tree([string]$path) {
    if (-not (Test-Path -LiteralPath $path)) { return }
    cmd /c "rd /s /q `"$path`"" | Out-Null
    if (Test-Path -LiteralPath $path) { Stop-Failed "could not remove $path" }
}

$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path.TrimEnd('\')
if (-not $Manifest) { $Manifest = Join-Path $repo 'Cargo.toml' }
$manifestPath = Get-FullPath $Manifest
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    Stop-Usage "-Manifest $manifestPath is not a file"
}
foreach ($tool in 'git', 'cargo', 'rustc') {
    if (-not (Get-Command $tool -CommandType Application -ErrorAction SilentlyContinue)) {
        Stop-Usage "$tool is not on PATH"
    }
}

# The pins. Every line that names the pid-parse repository has to carry its
# rev, and all of them have to name one commit.
$pins = [ordered]@{}
$pinLines = 0
$number = 0
foreach ($line in [IO.File]::ReadAllLines($manifestPath)) {
    $number++
    if ($line -match '^\s*#' -or $line -notmatch $pinPattern) { continue }
    if ($line -notmatch '\brev\s*=\s*"([^"]*)"') {
        Stop-Usage "$manifestPath line ${number}: a pid-parse dependency with no rev on its line"
    }
    $rev = $Matches[1].ToLowerInvariant()
    if (-not $pins.Contains($rev)) { $pins[$rev] = @() }
    $pins[$rev] += $number
    $pinLines++
}
if ($pinLines -eq 0) { Stop-Usage "$manifestPath names no pid-parse git dependency" }
if ($pins.Count -gt 1) {
    $said = @("$manifestPath pins pid-parse at $($pins.Count) different commits; " +
        "its $pinLines rev lines must name one:")
    foreach ($rev in $pins.Keys) { $said += "  $rev  on line $($pins[$rev] -join ', ')" }
    Stop-Usage ($said -join [Environment]::NewLine)
}
$commit = [string]@($pins.Keys)[0]
if ($commit -notmatch '^[0-9a-f]{40}$') {
    Stop-Usage "the rev '$commit' in $manifestPath is not a full 40-digit commit id"
}

# Cargo.lock is what the editor is really built from; it has to agree.
$lockNote = 'no Cargo.lock beside it to check'
$lockPath = Join-Path (Split-Path -Parent $manifestPath) 'Cargo.lock'
if (Test-Path -LiteralPath $lockPath -PathType Leaf) {
    $lockLines = 0
    $others = @()
    foreach ($line in [IO.File]::ReadAllLines($lockPath)) {
        if ($line -notmatch $lockPattern) { continue }
        $lockLines++
        if ($Matches[2] -ne $commit -and $others -notcontains $Matches[2]) { $others += $Matches[2] }
    }
    if ($others.Count -gt 0) {
        Stop-Usage "$lockPath builds pid-parse from $($others -join ', '), but $manifestPath pins $commit"
    }
    $lockNote = "Cargo.lock builds the same commit in $lockLines places"
    if ($lockLines -eq 0) { $lockNote = 'Cargo.lock names no pid-parse source' }
}
Say "$manifestPath pins pid-parse at $commit ($pinLines rev lines; $lockNote)"

$short = $commit.Substring(0, 7)
$root = Get-FullPath $ToolsRoot
if ($root -eq $repo -or $root.StartsWith("$repo\", [StringComparison]::OrdinalIgnoreCase)) {
    Stop-Usage "-ToolsRoot $root is inside the repository; the tools and their source belong outside it"
}
$dest = Join-Path $root $short
$src = Join-Path $dest 'src'
$target = Join-Path $src 'target'
$buildTxt = Join-Path $dest 'BUILD.txt'
$scratch = Join-Path $dest 'scratch.txt'

# What keeps the folder from counting as built, or $null when it does: its
# BUILD.txt is for this commit, each program is the file BUILD.txt records,
# and the source the programs were compiled in is still there.
function Get-Unbuilt {
    if (-not (Test-Path -LiteralPath $buildTxt -PathType Leaf)) { return 'it has no BUILD.txt' }
    $recorded = @{}
    $of = ''
    foreach ($line in [IO.File]::ReadAllLines($buildTxt)) {
        if ($line -match '^commit ([0-9a-f]{40})$') { $of = $Matches[1] }
        if ($line -match '^program (\S+) sha256 ([0-9a-f]{64}) ') { $recorded[$Matches[1]] = $Matches[2] }
    }
    if ($of -ne $commit) { return "its BUILD.txt is for commit '$of'" }
    foreach ($program in $programs) {
        $exe = Join-Path $dest "$program.exe"
        if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { return "$program.exe is missing" }
        if ($recorded[$program] -ne (Get-Sha256 $exe)) {
            return "$program.exe is not the file BUILD.txt records"
        }
    }
    if (-not (Test-Path -LiteralPath (Join-Path $src 'pid-plot\Cargo.toml') -PathType Leaf)) {
        return 'the source the programs were compiled in is gone'
    }
    return $null
}

if (-not $Force) {
    $unbuilt = Get-Unbuilt
    if ($null -eq $unbuilt) {
        Say "$short is already built, nothing to do: $dest"
        exit 0
    }
    if (Test-Path -LiteralPath $dest) { Say "$dest does not count as built ($unbuilt); building it again" }
}

# Start clean: nothing an earlier run wrote here may pass for this run's,
# its logs included.
New-Item -ItemType Directory -Force -Path $dest | Out-Null
$stale = @($buildTxt) + @($programs | ForEach-Object { Join-Path $dest "$_.exe" })
$stale += @(Get-ChildItem -LiteralPath $dest -File |
        Where-Object { $_.Name -match '^(export|build-\d+)\.log$' } |
        ForEach-Object { $_.FullName })
foreach ($old in $stale) {
    if (Test-Path -LiteralPath $old) { Remove-Item -LiteralPath $old -Force }
}
Remove-Tree $src
Remove-Tree $target

# The commit's files go to $src: from the checkout if it holds the commit,
# else from the copy cargo fetched to build the editor.
$checkout = Get-FullPath $PidParse
$git = "git -C `"$checkout`""
$log = Join-Path $dest 'export.log'

# HEAD and the status of the checkout as one string, or $null when git cannot
# say. --no-optional-locks: asking must not even refresh the index.
function Get-CheckoutState {
    $state = ''
    foreach ($ask in 'rev-parse HEAD', '--no-optional-locks status --porcelain') {
        if ((Invoke-Logged "$git $ask" $scratch) -ne 0) { return $null }
        $state += [IO.File]::ReadAllText($scratch)
    }
    return $state
}

# The folder cargo checked this commit out to, or $null.
function Find-CargoCopy {
    $cargoHome = $env:CARGO_HOME
    if (-not $cargoHome) { $cargoHome = Join-Path $env:USERPROFILE '.cargo' }
    $fetched = Join-Path $cargoHome 'git\checkouts'
    $dbs = @(Get-ChildItem -LiteralPath $fetched -Directory -Filter 'pid-parse-*' -ErrorAction SilentlyContinue)
    foreach ($db in $dbs) {
        $copy = Join-Path $db.FullName $short
        if (-not (Test-Path -LiteralPath (Join-Path $copy 'Cargo.toml') -PathType Leaf)) { continue }
        if ((Invoke-Logged "git -C `"$copy`" rev-parse HEAD" $scratch) -ne 0) { continue }
        if ([IO.File]::ReadAllText($scratch).Trim() -eq $commit) { return $copy }
    }
    return $null
}

try {
    $held = (Test-Path -LiteralPath $checkout -PathType Container) -and
        ((Invoke-Logged "$git cat-file -e `"$commit^{commit}`"" $log) -eq 0)
    if ($held) {
        $from = "git archive from $checkout"
        Say "exporting $short from $checkout (git archive; the checkout is only read)"
        $before = Get-CheckoutState
        $zip = Join-Path $dest 'src.zip'
        if ((Invoke-Logged "$git archive --format=zip -0 -o `"$zip`" $commit" $log) -ne 0) {
            Stop-Failed "git archive failed; see $log"
        }
        try {
            Add-Type -AssemblyName System.IO.Compression.FileSystem
            [IO.Compression.ZipFile]::ExtractToDirectory($zip, $src, [Text.Encoding]::UTF8)
        } catch {
            Stop-Failed "could not unpack the export to ${src}: $($_.Exception.Message)"
        }
        Remove-Item -LiteralPath $zip -Force
        if ($null -eq $before -or $before -cne (Get-CheckoutState)) {
            Say ("warning: HEAD or the status of $checkout is not what it was when the export began; " +
                'this script only reads it')
        } else {
            Say 'the checkout is as it was: same HEAD, same status'
        }
    } else {
        $copy = Find-CargoCopy
        if (-not $copy) {
            Stop-Failed ("commit $commit is not in the checkout $checkout, and cargo holds no copy " +
                "of it (git\checkouts\pid-parse-*\$short under CARGO_HOME)")
        }
        $from = "cargo's copy $copy (the checkout $checkout does not hold the commit)"
        Say "the checkout $checkout does not hold $short; copying the source cargo fetched: $copy"
        New-Item -ItemType Directory -Force -Path $src | Out-Null
        foreach ($entry in @(Get-ChildItem -LiteralPath $copy -Force)) {
            if ($entry.Name -eq '.git' -or $entry.Name -eq '.cargo-ok') { continue }
            Copy-Item -LiteralPath $entry.FullName -Destination $src -Recurse -Force
        }
        $git = "git -C `"$copy`""
    }
    $listed = -1
    if ((Invoke-Logged "$git ls-tree -r --name-only $commit" $scratch) -eq 0) {
        $listed = @([IO.File]::ReadAllLines($scratch)).Count
    }
} finally {
    if (Test-Path -LiteralPath $scratch) { Remove-Item -LiteralPath $scratch -Force }
}
$files = @(Get-ChildItem -LiteralPath $src -Recurse -File -Force).Count
if ($files -ne $listed) {
    Stop-Failed "the export holds $files files and the commit lists $listed; see $src"
}
Say "exported $files files to $src"

# The build runs in $src, so that only the commit's own files and cargo's
# home configuration shape it. Its target folder is $src\target, where cargo
# puts it unasked; CARGO_TARGET_DIR says so too, so that a value set elsewhere
# cannot send the build into another project's folder. (A cargo that ignores
# the variable and always uses .\target ends up in the same place.) The folder
# is removed once the programs are copied out.
$watch = [Diagnostics.Stopwatch]::StartNew()
$savedTarget = $env:CARGO_TARGET_DIR
$env:CARGO_TARGET_DIR = $target
Push-Location -LiteralPath $src
try {
    $toolchain = @("$(rustc --version)", "$(cargo --version)")
    $compiled = 0
    $step = 0
    foreach ($build in $builds) {
        $step++
        $log = Join-Path $dest "build-$step.log"
        Say "$build  (log: $log)"
        $code = Invoke-Logged $build $log
        $lines = @([IO.File]::ReadAllLines($log))
        if ($code -eq 0) {
            $compiled += @($lines | Where-Object { $_ -match '^\s*Compiling ' }).Count
            continue
        }
        $first = [Math]::Max(0, $lines.Count - 40)
        for ($i = 0; $i -lt $lines.Count; $i++) {
            if ($lines[$i] -match '^error') { $first = $i; break }
        }
        for ($i = $first; $i -lt $lines.Count -and $i -lt $first + 40; $i++) {
            [Console]::Error.WriteLine("  $($lines[$i])")
        }
        Stop-Failed ("the offline build failed (cargo exit $code; log: $log). Nothing was fetched. " +
            "If the lines above name a dependency that cargo's cache lacks, getting it takes one " +
            'online run, and that is your call.')
    }
} finally {
    Pop-Location
    $env:CARGO_TARGET_DIR = $savedTarget
}
$seconds = [int]$watch.Elapsed.TotalSeconds

$record = @(
    'pid-parse command-line tools, built by scripts\sppid\build-pid-tools.ps1',
    "commit $commit",
    "pinned-by $manifestPath ($pinLines rev lines)",
    "built $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss zzz')",
    "cargo-work $seconds s, $compiled packages compiled (cargo does not compile again what it built before)",
    "source $from",
    "source-dir $src ($files files; the programs look here for test-file\Ref\Symbols and test-file\fonts, keep it)",
    "toolchain $($toolchain[0])",
    "toolchain $($toolchain[1])",
    "build-dir $src (CARGO_TARGET_DIR=$target)"
)
foreach ($build in $builds) { $record += "command $build" }
foreach ($program in $programs) {
    $built = Join-Path $target "release\$program.exe"
    if (-not (Test-Path -LiteralPath $built -PathType Leaf)) {
        Stop-Failed "cargo reported success but $built is not there"
    }
    $exe = Join-Path $dest "$program.exe"
    Copy-Item -LiteralPath $built -Destination $exe -Force
    $file = Get-Item -LiteralPath $exe
    $record += "program $program sha256 $(Get-Sha256 $exe) bytes $($file.Length) " +
        "written $($file.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss'))"
}
[IO.File]::WriteAllLines($buildTxt, $record, (New-Object Text.UTF8Encoding($false)))
Remove-Tree $target
Say ("built $short (cargo ran $seconds s and compiled $compiled packages): " +
    "$($programs -join ', ') and BUILD.txt are in $dest")
exit 0
