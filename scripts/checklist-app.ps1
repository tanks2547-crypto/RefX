# checklist-app.ps1 -- the docs/08 section 3 items that a script can decide (P5-2)
#
# WHY THIS EXISTS
#
#   A manual checklist that is too long is a checklist nobody finishes, and the
#   items that get skipped are the boring ones, which is where the silent bugs
#   live.  Every item that a script can decide belongs in a script.
#
#   What stays manual is the set that needs hardware or an OS event we cannot
#   cause from here: a real GPU driver update, sleep/resume, iGPU<->dGPU switch,
#   a genuinely full disk, and dragging from Explorer with the real mouse.
#
# WHAT IT CHECKS  (docs/08 section 3, "stability" + "security" groups)
#
#   broken-images   drop N unopenable files -> EVERY ONE gets a placeholder and
#                   the app stays usable
#   media-removed   yank the image folder out from under a loaded board -> the
#                   board survives and the app does not crash
#   damaged-refx    open a .refx with flipped bytes -> a clear message, no panic
#
# ASCII ONLY (docs/08 section 3.9 rule 9, 4th row).
#
# USAGE
#   powershell -File scripts/checklist-app.ps1 -Images E:/refx-bench-smoke
#
# Exit code 0 = every check passed.  Non-zero = at least one FAILED, and the
# line above it says which and why.

param(
    [Parameter(Mandatory = $true)][string]$Images,   # a folder of GOOD images
    [string]$Exe = "target/release/refx.exe",
    [string]$Work = "$env:TEMP\refx-checklist"
)

$ErrorActionPreference = "Stop"
if (-not (Test-Path $Exe)) { throw "binary not found at $Exe -- cargo build --release first" }
$ExePath = (Resolve-Path $Exe).Path

$already = @(Get-Process refx -ErrorAction SilentlyContinue)
if ($already.Count -gt 0) {
    Write-Output ("refx.exe is ALREADY RUNNING (pid " + ($already.Id -join ", ") + ") -- close it first")
    exit 1
}

New-Item -ItemType Directory -Force -Path $Work | Out-Null
$script:failures = 0

function Report {
    param([string]$Name, [bool]$Ok, [string]$Detail)
    $tag = if ($Ok) { "PASS" } else { "FAIL"; }
    if (-not $Ok) { $script:failures++ }
    Write-Output ("[{0}] {1,-14} {2}" -f $tag, $Name, $Detail)
}

# Launch, wait for the window, grab stdout, kill.  Same rule as measure-app.ps1:
# refx.exe never exits on its own, so we never wait for it to.
function Run-Refx {
    param([string[]]$RefxArgs, [int]$SettleSeconds = 6)
    $out = Join-Path $Work ("out-{0}.txt" -f [guid]::NewGuid())
    $err = [System.IO.Path]::ChangeExtension($out, ".err")
    $p = Start-Process -FilePath $ExePath -ArgumentList $RefxArgs `
        -RedirectStandardOutput $out -RedirectStandardError $err -PassThru
    $w = [System.Diagnostics.Stopwatch]::StartNew()
    while ((-not $p.HasExited) -and ($p.MainWindowHandle -eq 0) -and ($w.Elapsed.TotalSeconds -lt 30)) {
        $p.Refresh(); Start-Sleep -Milliseconds 5
    }
    Start-Sleep -Seconds $SettleSeconds
    $alive = -not $p.HasExited
    $stdout = ""; $stderr = ""
    if (Test-Path $out) { $stdout = Get-Content $out -Raw -Encoding UTF8 -ErrorAction SilentlyContinue }
    if (Test-Path $err) { $stderr = Get-Content $err -Raw -Encoding UTF8 -ErrorAction SilentlyContinue }
    return @{ Proc = $p; Alive = $alive; Stdout = $stdout; Stderr = $stderr }
}
function Stop-Refx {
    param($R)
    if ($R -and $R.Proc -and (-not $R.Proc.HasExited)) {
        [void]$R.Proc.CloseMainWindow()
        if (-not $R.Proc.WaitForExit(4000)) { try { $R.Proc.Kill() } catch { } }
    }
    Get-Process refx -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 700
}

# The app prints "<thai> R <thai> A <thai> T ms" = requested, ADDED, milliseconds.
#
# ! IT IS THE SECOND NUMBER THAT MATTERS.  The first version of this function
#   read the FIRST number and reported 20-of-20 for a drop where only 4 images
#   reached the board -- a green check that measured the count of files handed
#   IN, which is 20 by construction and can never disagree.  The app used to
#   print only that number too; both were fixed together.
function Added-Count {
    param([string]$Stdout)
    foreach ($l in ($Stdout -split "`r?`n")) {
        if ($l -match "([0-9]+)\D+([0-9]+)\D+([0-9.]+)\s*ms\s*$") { return [int]$Matches[2] }
    }
    return -1
}

Write-Output "checking against $Exe"
Write-Output ""

# ---------------------------------------------------------------------------
# 1. broken images -- docs/08 s3: "20 unopenable files -> ALL show as Failed,
#    the app is still usable"
# ---------------------------------------------------------------------------
$broken = Join-Path $Work "broken"
Remove-Item $broken -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $broken | Out-Null

$sample = @(Get-ChildItem -Path $Images -File)[0]
$good = [System.IO.File]::ReadAllBytes($sample.FullName)
$rand = New-Object System.Random 7
# five shapes of broken, four of each = 20 files
for ($i = 0; $i -lt 20; $i++) {
    $name = Join-Path $broken ("broken{0:d2}{1}" -f $i, $(if ($i % 2) { ".png" } else { ".jpg" }))
    switch ($i % 5) {
        0 { [System.IO.File]::WriteAllBytes($name, $good[0..([math]::Min(400 + $i * 37, $good.Length - 1))]) }
        1 { $b = New-Object byte[] 3000; $rand.NextBytes($b)
            [System.IO.File]::WriteAllBytes($name, ($good[0..63] + $b)) }
        2 { # a PNG that claims 60000x60000 -- the decompression bomb of docs/08 s3
            $ihdr = [byte[]](0,0,234,96, 0,0,234,96, 8,2,0,0,0)
            $b = New-Object byte[] 1000; $rand.NextBytes($b)
            [System.IO.File]::WriteAllBytes($name, ([byte[]](0x89,0x50,0x4E,0x47,0x0D,0x0A,0x1A,0x0A) + $ihdr + $b)) }
        3 { $b = New-Object byte[] 2000; $rand.NextBytes($b)
            [System.IO.File]::WriteAllBytes($name, ([byte[]](0x4D,0x5A) + $b)) }
        4 { [System.IO.File]::WriteAllBytes($name, (New-Object byte[] 0)) }
    }
}
$dropped = @(Get-ChildItem -Path $broken -File).Count
$r = Run-Refx @("--open-dir=$broken")
$shown = Added-Count $r.Stdout
$aliveAfter = $r.Alive
Stop-Refx $r

Report "broken-images" ($shown -eq $dropped) `
    ("dropped $dropped unopenable files, the board shows $shown" + $(if ($shown -ne $dropped) { " -- $($dropped - $shown) vanished with no message to the user" } else { "" }))
Report "app-survives" $aliveAfter "the app was still running after $dropped unopenable files"

# ---------------------------------------------------------------------------
# 2. media removed while in use -- docs/08 s3: "pull the USB stick"
# ---------------------------------------------------------------------------
$usb = Join-Path $Work "usb"
Remove-Item $usb -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $usb | Out-Null
@(Get-ChildItem -Path $Images -File) | Select-Object -First 8 | ForEach-Object {
    Copy-Item $_.FullName (Join-Path $usb $_.Name)
}
$r = Run-Refx @("--open-dir=$usb") 6
$loaded = Added-Count $r.Stdout
$gone = "$usb-GONE"
Remove-Item $gone -Recurse -Force -ErrorAction SilentlyContinue
Rename-Item $usb $gone -ErrorAction SilentlyContinue
$vanished = -not (Test-Path $usb)
Start-Sleep -Seconds 3
$r.Proc.Refresh()
$stillAlive = -not $r.Proc.HasExited
Stop-Refx $r
Remove-Item $gone -Recurse -Force -ErrorAction SilentlyContinue

Report "media-removed" ($vanished -and $stillAlive) `
    ("loaded $loaded images, deleted the folder underneath, app alive: $stillAlive")

# ---------------------------------------------------------------------------
# 3. a .refx with flipped bytes -- docs/08 s3: "clear error, no panic"
# ---------------------------------------------------------------------------
# Built from a real save so that everything except the flipped bytes is valid;
# a hand-assembled file would be rejected at the magic check and prove nothing
# (docs/08 s3.9 item 1b).
#
# ! THE CHECK MAKES ITS OWN FIXTURE (docs/08 s3.9 item 17, 8 Sep 2026).
#   This step used to grab the first *.refx it could find on E:\ and report a
#   FAIL when there was none.  That is a check whose result depends on what
#   happens to be lying around on ONE developer's disk: green here, red on every
#   other machine and on CI, for a reason nobody can see from the output.
#   Same shape as item 13.  So: drive the real app through a real Ctrl+S into
#   our own scratch folder, then flip bytes in THAT.
$source = Join-Path $Work "source.refx"
Remove-Item $source -Force -ErrorAction SilentlyContinue
$drive = Join-Path $PSScriptRoot "ui-drive.ps1"
& $drive -Steps @(
    "launch|$ExePath|--open-dir=$Images",
    "sleep|4000",
    "keydn|17", "key|83", "keyup|17",   # Ctrl+S
    "sleep|900",
    # ! a board that has never been saved asks HOW to store the images first
    #   ("Link to the image files" / "Store the images inside") -- the OS dialog
    #   only opens after that answer.  Driving Ctrl+S straight into 'dlgtype'
    #   fails with "NO DIALOG", which is what this line exists to prevent.
    "click|271|33",
    "sleep|1500",
    "dlgtype|$source",
    # ! the write happens on a worker and its RESULT is collected on the next
    #   frame -- and the app sleeps in ControlFlow::Wait (I-1), so with no input
    #   there is no next frame.  Nudging the mouse is what produces one.  Killing
    #   at this point without the nudge leaves no file at all, which reads as
    #   "Ctrl+S is broken" when the only thing broken is the test's timing.
    "move|600|400", "sleep|400", "move|620|420", "sleep|1500",
    "kill") | Out-Null
Stop-Refx $null

$refx = Join-Path $Work "damaged.refx"
if (-not (Test-Path $source)) {
    Report "damaged-refx" $false "the app did not write $source -- Ctrl+S itself is broken"
} else {
    $bytes = [System.IO.File]::ReadAllBytes($source)
    if ($bytes.Length -le 80) {
        Report "damaged-refx" $false "the saved file is only $($bytes.Length) bytes -- nothing to damage"
    } else {
        # ! flip bytes INSIDE the document body, past the 20-byte header, so the
        #   file still passes the magic/version check and fails at the checksum.
        #   Flipping the header would test a different branch entirely.
        $bytes[60] = $bytes[60] -bxor 0xFF; $bytes[70] = $bytes[70] -bxor 0xFF
        [System.IO.File]::WriteAllBytes($refx, $bytes)
        $r = Run-Refx @("--open=$refx") 4
        $alive = $r.Alive
        $panicked = ($r.Stderr -match "panicked")
        Stop-Refx $r
        Report "damaged-refx" ($alive -and (-not $panicked)) `
            ("saved $($bytes.Length) bytes, flipped 2, app alive: $alive, panic on stderr: $panicked")
    }
}
Remove-Item $source, $refx -Force -ErrorAction SilentlyContinue

Write-Output ""
if ($script:failures -gt 0) {
    Write-Output ("$($script:failures) check(s) FAILED -- see the lines above")
    exit 1
}
Write-Output "every scripted checklist item passed"
