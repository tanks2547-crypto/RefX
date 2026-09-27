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

# A fresh, empty folder for this script run.  Printed so the evidence says
# where it came from.
function New-RefxTestRoot {
    param([string]$Label = 'run')
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $root = Join-Path $env:TEMP ("refx-test-root\{0}-{1}-{2}" -f $stamp, $Label, $PID)
    Assert-RefxRootIsNotRealData $root
    New-Item -ItemType Directory -Force -Path $root | Out-Null
    Write-Host "refx data root : $root"
    return $root
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
        if ($item -match '--data-root=("?)([^"]+)\1') {
            Assert-RefxRootIsNotRealData $Matches[2]
            return $ArgumentList
        }
    }
    # Quoted: Windows PowerShell 5.1 does not quote array elements that contain
    # spaces, and %TEMP% can contain them.
    $flag = '--data-root="' + $Root + '"'
    if ($ArgumentList -is [array]) { return @($ArgumentList) + $flag }
    if ($ArgumentList)             { return ([string]$ArgumentList + ' ' + $flag) }
    return $flag
}
