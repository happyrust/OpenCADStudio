# Shared by run19.ps1, run_all.ps1 and run_cross.ps1: where things are, and
# how one scenario is run (Invoke-E2eScenario, at the end of this file).
#
# No path is fixed in these scripts. Each value comes from the entry's
# parameter when it is given, otherwise from the environment variable,
# otherwise from the default:
#
#   -EvidenceDir  OCS_E2E_EVIDENCE_DIR  where logs, screenshots, databases and
#                                       drawing copies go. Required, and it
#                                       has to be outside the repository.
#   -BinDir       OCS_E2E_BIN_DIR       where OpenCADStudio.exe and
#                                       ocs_collab_server.exe are taken from.
#                                       Default: target\debug of this
#                                       repository.
#   -PortShift    OCS_E2E_PORT_SHIFT    added to every port. Default 0, which
#                                       is the contract's range: GUI 3210-3239,
#                                       server 3240. Give 100 to run beside
#                                       something that already uses that range.
#
# What is resolved is put back into the environment: the Python harness
# (m6h.py, m7h.py) and the entries that run19.ps1 starts read it from there.

function Resolve-E2eEnv {
    param([string]$Here, [string]$EvidenceDir, [string]$BinDir, [string]$PortShift)
    $repo = (Resolve-Path (Join-Path $Here '..\..')).Path.TrimEnd('\')
    $full = { param($p) $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($p).TrimEnd('\') }

    if (-not $EvidenceDir) { $EvidenceDir = $env:OCS_E2E_EVIDENCE_DIR }
    if (-not $EvidenceDir) {
        throw 'No evidence directory: give -EvidenceDir <dir> or set OCS_E2E_EVIDENCE_DIR. It has to be outside the repository.'
    }
    $evidence = & $full $EvidenceDir
    if (($evidence + '\').StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase)) {
        throw "The evidence directory is inside the repository ($evidence); it has to be outside."
    }

    if (-not $BinDir) { $BinDir = $env:OCS_E2E_BIN_DIR }
    if (-not $BinDir) { $BinDir = Join-Path $repo 'target\debug' }
    $bin = & $full $BinDir
    foreach ($exe in 'OpenCADStudio.exe', 'ocs_collab_server.exe') {
        if (-not (Test-Path (Join-Path $bin $exe))) {
            throw "$exe is not in $bin. Build it (cargo build --bin OpenCADStudio; cargo build -p ocs_collab_server) or give -BinDir <dir>."
        }
    }

    if (-not $PortShift) { $PortShift = $env:OCS_E2E_PORT_SHIFT }
    if (-not $PortShift) { $PortShift = '0' }
    $shift = 0
    if (-not [int]::TryParse($PortShift, [ref]$shift)) { throw "The port shift is not a whole number: $PortShift" }

    New-Item -ItemType Directory -Force -Path $evidence | Out-Null
    $env:OCS_E2E_EVIDENCE_DIR = $evidence
    $env:OCS_E2E_BIN_DIR = $bin
    $env:OCS_E2E_PORT_SHIFT = "$shift"
    # Nothing a run produces lands in the repository, Python's bytecode cache included.
    $env:PYTHONDONTWRITEBYTECODE = '1'
    [pscustomobject]@{ Scripts = $Here; Repo = $repo; Evidence = $evidence; Bin = $bin; PortShift = $shift }
}

# The OpenCADStudio.exe and ocs_collab_server.exe processes on this machine
# that were started from this run's two directories (what a scenario may have
# left behind) or, with -Other, the ones that were not (somebody else's).
function Get-E2eProcesses {
    param($E2e, [switch]$Other)
    $mine = ($E2e.Bin + '\'), ($E2e.Evidence + '\')
    @(Get-CimInstance Win32_Process -Filter "Name='OpenCADStudio.exe' OR Name='ocs_collab_server.exe'" | Where-Object {
        $path = "$($_.ExecutablePath)"
        $own = $false
        foreach ($prefix in $mine) {
            if ($path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { $own = $true }
        }
        if ($Other) { -not $own } else { $own }
    })
}

# Runs one scenario (<Name>.py in the current directory) with its console in
# <EvidenceDir>\<Suite>\logs\<Name>.console.txt, counts what it left behind
# and adds one line to the summary file.
#
# A scenario that ends with exit code 75 has lost an application instance:
# one that was ready and then was gone, though the scenario neither ended it
# nor told it to quit. The harness has written down the exit code, the tail of
# the log and the time by then (logs\vanished-*.json). Such a scenario is run
# once more, whole, with the same drawings on the same ports. Both results
# stay: the console and the result file of the first attempt are kept as
# <Name>.attempt1.console.txt and <Name>.attempt1.result.txt. There is no
# third attempt: a second attempt that loses an instance too is a failure.
# <Name>.runs.txt gets one line per attempt (attempt, exit code, seconds,
# leftover processes); run19.ps1 reads the exit code from there.
function Invoke-E2eScenario {
    param($E2e, [string]$Suite, [string]$Name, [string]$Summary, [switch]$Descriptors)
    $root = Join-Path $E2e.Evidence $Suite
    $logs = Join-Path $root 'logs'
    $log = Join-Path $logs "$Name.console.txt"
    $result = Join-Path $logs "$Name.result.txt"
    $runs = Join-Path $logs "$Name.runs.txt"
    $keptLog = Join-Path $logs "$Name.attempt1.console.txt"
    $keptResult = Join-Path $logs "$Name.attempt1.result.txt"
    Remove-Item $keptLog, $keptResult, $runs -ErrorAction SilentlyContinue
    foreach ($attempt in 1, 2) {
        $env:OCS_E2E_ATTEMPT = "$attempt"
        $sw = [Diagnostics.Stopwatch]::StartNew()
        python "$Name.py" *> $log
        $code = $LASTEXITCODE
        $seconds = [int]$sw.Elapsed.TotalSeconds
        $left = @(Get-E2eProcesses $E2e)
        $more = ''
        if ($Descriptors) {
            $found = @(Get-ChildItem (Join-Path $root 'work\appdata') -Recurse -Filter '*.json' -ErrorAction SilentlyContinue |
                Where-Object { $_.DirectoryName -match '\\OpenCADStudio\\automation$' })
            $more = " descriptors=$($found.Count)"
        }
        $again = ($code -eq 75) -and ($attempt -eq 1)
        $mark = ''
        if ($again) { $mark = ' attempt=1 (lost an instance: kept as attempt1, run once more)' }
        elseif ($attempt -eq 2 -and $code -eq 75) { $mark = ' attempt=2 (lost an instance again: not run a third time)' }
        elseif ($attempt -eq 2) { $mark = ' attempt=2' }
        $tail = (Get-Content $log -Encoding UTF8 | Select-String -Pattern '^\d\d:\d\d:\d\d === ' | Select-Object -Last 1).Line
        $desk = (Get-Content $log -Encoding UTF8 | Select-String -Pattern 'desktop (untouched|CHANGED)' | Select-Object -Last 1).Line
        $line = "$(Get-Date -Format s) $Name exit=$code ${seconds}s leftover=$($left.Count)$more$mark :: $tail :: $desk"
        $line | Out-File -Encoding utf8 -Append $Summary
        Write-Host $line
        foreach ($p in $left) { Write-Host "    LEFTOVER pid=$($p.ProcessId) $($p.CommandLine)" }
        "attempt=$attempt exit=$code seconds=$seconds leftover=$($left.Count)" | Out-File -Encoding ascii -Append $runs
        if (-not $again) { break }
        Move-Item -Force $log $keptLog
        if (Test-Path $result) { Move-Item -Force $result $keptResult }
    }
    Remove-Item Env:OCS_E2E_ATTEMPT -ErrorAction SilentlyContinue
}
