# Every script that starts refx.exe to collect evidence must run it with its
# OWN data folder (--data-root), never the user's.  (ROADMAP P5-9e)
#
# ---------------------------------------------------------------------------
# WHY THIS FILE EXISTS  (27 Sep 2026)
# ---------------------------------------------------------------------------
# For two months every scripted run wrote into %LOCALAPPDATA%\RefX -- the same
# folder the project owner's installed copy uses.  The day it mattered: his
# rc.1 test session left a recovery snapshot he had not answered yet, and the
# app stamps ".asked" the moment the recovery bar APPEARS.  The next scripted
# launch would have stripped that snapshot's "never delete before the user has
# seen it" protection.  Nobody would have noticed until the work was gone.
#
# It was also bad evidence: a "cold" measurement was only as cold as whatever
# happened to be in his cache that day (docs/08 3.9 item 16).
#
# ---------------------------------------------------------------------------
# Usage (dot-source it, then wrap every -ArgumentList)
# ---------------------------------------------------------------------------
#   . (Join-Path $PSScriptRoot 'refx-test-root.ps1')
#   $RefxRoot = New-RefxTestRoot 'checklist'
#   Start-Process refx.exe -ArgumentList (Add-RefxDataRoot $args $RefxRoot)
#
# ONE root per script run, shared by every launch in that run: the checklist
# kills the app and relaunches it to prove recovery works, which only means
# something if the second launch sees what the first one wrote.
#
# xtask/tests/refx_launchers.rs reads every script and fails if a
# Start-Process of refx is not wrapped in Add-RefxDataRoot.

# The two folders a scripted run must never write into.
function Get-RefxRealDataFolders {
    $folders = @()
    if ($env:LOCALAPPDATA) { $folders += (Join-Path $env:LOCALAPPDATA 'RefX') }
    if ($env:APPDATA)      { $folders += (Join-Path $env:APPDATA 'RefX') }
    return $folders
}

# Refuse a root that is, or sits inside, the user's real RefX folders.
function Assert-RefxRootIsNotRealData {
    param([Parameter(Mandatory = $true)][string]$Root)
    $full = [System.IO.Path]::GetFullPath($Root).TrimEnd('\') + '\'
    foreach ($real in Get-RefxRealDataFolders) {
        $realFull = [System.IO.Path]::GetFullPath($real).TrimEnd('\') + '\'
        if ($full.StartsWith($realFull, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "refusing --data-root=$Root : that is the user's real RefX data ($real)"
        }
    }
}

# The folder this script run WOULD use if no launch brings its own.
#
# NOT created and NOT printed here.  The first version did both, then a caller
# passed its own --data-root and the log proudly named a folder the app never
# touched (28 Sep 2026) -- evidence that points at the wrong place.  The
# folder is created, and the root announced, only by Add-RefxDataRoot, for
# the root each launch actually gets.
function New-RefxTestRoot {
    param([string]$Label = 'run')
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $root = Join-Path $env:TEMP ("refx-test-root\{0}-{1}-{2}" -f $stamp, $Label, $PID)
    Assert-RefxRootIsNotRealData $root
    return $root
}

# Say which root a launch really uses -- once per distinct root per run.
$script:RefxAnnounced = @{}
function Write-RefxRootInUse {
    param([string]$Root, [string]$How)
    if (-not $script:RefxAnnounced.ContainsKey($Root)) {
        $script:RefxAnnounced[$Root] = $true
        Write-Host "refx data root : $Root ($How)"
    }
}

# The only refx.exe processes a script may kill or drive: the ones started
# with --data-root, i.e. started by a script.  Never the user's own RefX.
#
# Before 27 Sep 2026 "kill" meant `Get-Process refx | Stop-Process -Force` and
# "attach" meant `Get-Process refx` -- both reach whatever RefX is running,
# including one the user opened with unsaved work in it.  That was only
# unavoidable while RefX was single-instance per user; with --data-root the
# test instance and the user's instance hold different locks and coexist.
function Get-RefxTestProcesses {
    $found = @(Get-CimInstance Win32_Process -Filter "Name = 'refx.exe'" |
        Where-Object { $_.CommandLine -match '--data-root=' })
    foreach ($p in $found) {
        $proc = Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue
        if ($proc) { $proc }
    }
}

# Add --data-root to a launch's arguments.  Accepts the string form
# (ui-drive.ps1) and the array form (checklist / measure).  A caller that
# already passes its own --data-root keeps it -- but it is still checked.
function Add-RefxDataRoot {
    param($ArgumentList, [Parameter(Mandatory = $true)][string]$Root)

    $items = @()
    if ($ArgumentList -is [array]) { $items = @($ArgumentList) }
    elseif ($ArgumentList)         { $items = @([string]$ArgumentList) }

    foreach ($item in $items) {
        # quoted value: up to the closing quote · unquoted: up to the next space
        # (the first version read an unquoted value to the END of the string, so
        # "--data-root=C:\x --lang=th" checked "C:\x --lang=th" -- wrong path)
        if ($item -match '--data-root=(?:"([^"]+)"|(\S+))') {
            $given = if ($Matches[1]) { $Matches[1] } else { $Matches[2] }
            Assert-RefxRootIsNotRealData $given
            Write-RefxRootInUse $given 'given by the caller'
            return $ArgumentList
        }
    }
    New-Item -ItemType Directory -Force -Path $Root | Out-Null
    Write-RefxRootInUse $Root 'fresh for this run'
    # Quoted: Windows PowerShell 5.1 does not quote array elements that contain
    # spaces, and %TEMP% can contain them.
    $flag = '--data-root="' + $Root + '"'
    if ($ArgumentList -is [array]) { return @($ArgumentList) + $flag }
    if ($ArgumentList)             { return ([string]$ArgumentList + ' ' + $flag) }
    return $flag
}
