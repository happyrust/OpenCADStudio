# Shared by run19.ps1, run_all.ps1 and run_cross.ps1: where things are.
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
