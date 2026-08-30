# measure-app.ps1 -- the app-level rows of docs/08 section 2 (P5-1)
#
# WHY THIS IS A SEPARATE TOOL FROM scripts/check-bench.sh
#
#   check-bench.sh covers the rows that are pure CPU: they run on any runner in
#   seconds and are enforced on every push.  The rows here need a real GPU, a
#   real window and a 1000-image dataset on disk, so they cannot be a per-push
#   gate without blowing the Actions budget.  They are measured on demand and
#   the numbers are recorded in HANDOFF.
#
# ============================================================================
# THREE RULES THIS SCRIPT LEARNED THE HARD WAY -- do not undo them
# ============================================================================
#
# 1. NEVER WAIT FOR refx.exe TO EXIT.  IT DOES NOT.
#
#    --bench-seconds prints its report and then goes back to sleep, because
#    I-1 says an idle app draws nothing and exits nothing.  The first version
#    of this script called $proc.StandardOutput.ReadToEnd(), which returns only
#    when the pipe closes, which happens only on exit.  It hung for 5.5 HOURS
#    while the app sat at 0.000s CPU delta having finished its work in ~2 min.
#    (The same lesson was already paid for once in P2-5.)
#
#    -> stdout goes to a FILE, we poll the file for the report line, then we
#       kill the process ourselves.  No ReadToEnd, no bare WaitForExit.
#
# 2. A LONG-RUNNING TOOL MUST PRINT PROGRESS.
#
#    "working" and "hung" were indistinguishable from outside.  That is the
#    same trap as docs/08 section 3.9 rule 9.  Every tick prints elapsed time,
#    RSS and CPU-seconds, so a stall is visible in the first 10 seconds.
#
#    -> PROGRESS USES [Console]::WriteLine, NOT Write-Output.  In PowerShell
#       anything a function writes to the output stream BECOMES PART OF ITS
#       RETURN VALUE.  The first version used Write-Output inside Invoke-Pass,
#       so every progress line was silently swallowed into $run (which still
#       "worked" by member enumeration, which is how it went unnoticed) and the
#       operator saw nothing at all for the whole run.  Rule 2 was written and
#       then broken by the very next line of code.
#
# 3. EVERY WAIT HAS A DEADLINE AND NAMES THE STEP IT DIED IN.
#
# ---------------------------------------------------------------------------
# READ THIS BEFORE BELIEVING A HIGH startup_to_window
# ---------------------------------------------------------------------------
# The FIRST launch after `cargo build` reads an 18 MB binary that is not in the
# OS file cache yet.  Measured on this box: 586 ms and 485 ms on the first
# launch after a build, then 65-74 ms on every launch after that (n=6).
#
# That is not flakiness to be averaged away and it is not a code regression
# either -- it is the number a user gets the first time they start a freshly
# installed or freshly updated RefX.  If the row goes OVER, check whether the
# binary was just rebuilt before deciding anything.
#
# ASCII ONLY, ON PURPOSE (docs/08 section 3.9 rule 9, 4th row): a Windows
# console defaults to cp1252 and dies when a script prints Thai.
#
# USAGE
#   powershell -File scripts/measure-app.ps1 -Images E:/refx-bench-1000
#   powershell -File scripts/measure-app.ps1 -Images ... -Seconds 15 -SkipWarm
#   powershell -File scripts/measure-app.ps1 -Images ... -Pan        <- frame_pan
#
# WHAT IT REPORTS
#   startup_to_window            process start -> window handle exists
#   open_board_..._warm_cache    printed by the app itself, measured by
#                                LoadTracker from the first frame that saw work
#                                pending to the frame the queue drained
#   frame p50 / p99              printed by the app itself (--bench-seconds)
#   vram_idle                    printed by the app itself (texture budget)
#   rss_idle                     WorkingSet64 from outside, after the report
#
#   WITHOUT -Pan the frame numbers are redraw-standing-still.  WITH -Pan a real
#   middle-button camera pan runs for the whole bench window, which is what the
#   docs/08 row frame_pan_1000 actually asks for.  The window is attached and
#   resized IN BOTH MODES so the two runs draw the same number of pixels and
#   can be compared (docs/08 3.9 item 7).

param(
    [Parameter(Mandatory = $true)][string]$Images,
    [int]$Seconds = 15,
    [switch]$SkipWarm,
    # -Pan drives a real middle-button camera pan for the whole bench window,
    # which is what the docs/08 row frame_pan_1000 actually asks for.  Without
    # it the frame numbers are redraw-standing-still.
    [switch]$Pan,
    [string]$Exe = "target/release/refx.exe",
    # deadline per pass.  cold decode of 1000 files is ~2 min on the dev box;
    # 12 min is generous enough that hitting it means something is really wrong.
    [int]$TimeoutSeconds = 720
)

# One pan step in ui-drive.ps1 costs ~660 ms (two 120 ms moves either side of a
# 150 ms button hold, twice).  Measured, not guessed -- see the report line the
# script prints at the end of the drive.
$PAN_STEP_MS = 660

$ErrorActionPreference = "Stop"

if (-not (Test-Path $Exe)) { throw "binary not found at $Exe -- cargo build --release first" }
if (-not (Test-Path $Images)) { throw "image folder not found: $Images" }
# RefX is single-instance: a leftover process makes every pass below exit
# instantly.  Say so here rather than letting it look like a measurement.
$already = @(Get-Process refx -ErrorAction SilentlyContinue)
if ($already.Count -gt 0) {
    Write-Output ("refx.exe is ALREADY RUNNING (pid " + ($already.Id -join ", ") + ")")
    Write-Output "RefX is single-instance, so every pass would exit instantly holding no lock."
    Write-Output "Close it (or: Get-Process refx | Stop-Process -Force) and run this again."
    exit 1
}

$count = @(Get-ChildItem -Path $Images -File).Count
Write-Output "dataset : $Images ($count files)"
Write-Output "binary  : $Exe"
Write-Output "timeout : $TimeoutSeconds s per pass"
Write-Output ""

# ---------------------------------------------------------------------------
# Run one pass.  Returns a hashtable of everything measured.
#
# The completion signal is the app's own report line ("p50 ... ms"), NOT
# process exit -- see rule 1 in the header.
# ---------------------------------------------------------------------------
function Invoke-Pass {
    param([string]$Label, [int]$BenchSeconds, [bool]$DoPan = $false)

    $outFile = Join-Path ([System.IO.Path]::GetTempPath()) ("refx-bench-{0}.out" -f [guid]::NewGuid())
    $errFile = [System.IO.Path]::ChangeExtension($outFile, ".err")

    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $proc = Start-Process -FilePath (Resolve-Path $Exe).Path `
        -ArgumentList @("--open-dir=$Images", "--bench-seconds=$BenchSeconds") `
        -RedirectStandardOutput $outFile -RedirectStandardError $errFile `
        -PassThru

    $startupMs = $null
    $peakRss = 0
    $reported = $false
    $lastTick = 0.0

    while ($true) {
        # RULE 4: the app dying is not the same as the app stalling.
        #
        # refx.exe exits immediately when another instance holds the
        # single-instance lock, and says so on stderr.  The first version threw
        # stderr away and reported "stuck while <blank>", which sent the
        # operator looking for a hang that never existed.  Whatever the app
        # said about its own death is the most useful line in the whole run.
        if ($proc.HasExited) {
            $why = ""
            if (Test-Path $errFile) {
                $why = (Get-Content $errFile -Raw -Encoding UTF8 -ErrorAction SilentlyContinue)
            }
            if (-not $why) { $why = "(nothing on stderr)" }
            [Console]::WriteLine(("  the app EXITED after {0:N1}s, exit code {1}" -f `
                $watch.Elapsed.TotalSeconds, $proc.ExitCode))
            [Console]::WriteLine(("  it said: {0}" -f $why.Trim()))
            Remove-Item $outFile, $errFile -ErrorAction SilentlyContinue
            return @{
                Ok = $false
                Stage = ("the app exited on its own (code {0}): {1}" -f $proc.ExitCode, $why.Trim())
                Startup = $startupMs; Stdout = ""
            }
        }
        $proc.Refresh()

        if (($null -eq $startupMs) -and ($proc.MainWindowHandle -ne 0)) {
            $startupMs = $watch.Elapsed.TotalMilliseconds
            [Console]::WriteLine(("  [{0,6:N1}s] window is up after {1:N0} ms" -f `
                $watch.Elapsed.TotalSeconds, $startupMs))

            # ---- drive the window -------------------------------------------
            # ATTACH RUNS IN BOTH MODES ON PURPOSE.  ui-drive's attach resizes
            # the window to 1296x839 and takes focus; doing it only in -Pan
            # would mean the pan run draws a different number of pixels than
            # the still run, and the two numbers could not be compared at all
            # (docs/08 3.9 item 7: compare the END STATE, not the input).
            $steps = @("attach")
            if ($DoPan) {
                # alternate the direction so the camera oscillates around the
                # board instead of wandering off it and panning over emptiness
                $n = [int]([math]::Ceiling(($BenchSeconds * 1000.0) / $PAN_STEP_MS))
                for ($i = 0; $i -lt $n; $i++) {
                    if ($i % 2 -eq 0) { $steps += "pan|420|300|880|620" }
                    else              { $steps += "pan|880|620|420|300" }
                }
                # ! ui-drive blocks for the whole pan, so the 10 s progress ticks
                #   below cannot run while it does.  Say how long the silence
                #   will last BEFORE it starts -- rule 2 is about the operator
                #   being able to tell working from hung, and a silent 3 minutes
                #   with no warning fails that just as badly as a hang.
                [Console]::WriteLine(("  [{0,6:N1}s] panning for the whole window: {1} steps, ~{2:N0}s of no output" -f `
                    $watch.Elapsed.TotalSeconds, $n, ($n * $PAN_STEP_MS / 1000.0)))
            }

            # ! capture, then print through [Console].  ui-drive writes a lot,
            #   and Write-Output from a call inside THIS function would be
            #   swallowed into our return value -- the exact bug rule 2 is about.
            $driveOut = (& (Join-Path $PSScriptRoot "ui-drive.ps1") -Steps $steps 2>&1 | Out-String)
            $driveCode = $LASTEXITCODE
            foreach ($l in ($driveOut -split "`r?`n")) {
                if ($l.Trim()) { [Console]::WriteLine("    ui-drive: " + $l.Trim()) }
            }
            if ($driveCode -and $driveCode -ne 0) {
                try { $proc.Kill() } catch { }
                Remove-Item $outFile, $errFile -ErrorAction SilentlyContinue
                return @{
                    Ok = $false
                    Stage = "ui-drive refused to continue (exit $driveCode) -- focus was lost, so any number here would be a lie"
                    Startup = $startupMs; Stdout = ""
                }
            }
        }
        try { if ($proc.WorkingSet64 -gt $peakRss) { $peakRss = $proc.WorkingSet64 } } catch { }

        # the app's own report line is the only reliable "done" signal
        if (Test-Path $outFile) {
            $sofar = Get-Content $outFile -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
            if ($sofar -and ($sofar -match "p50\s+[0-9.]+\s+ms")) { $reported = $true; break }
        }

        # RULE 2: make "working" and "hung" tell themselves apart
        if (($watch.Elapsed.TotalSeconds - $lastTick) -ge 10) {
            $lastTick = $watch.Elapsed.TotalSeconds
            $cpu = 0.0
            try { $cpu = $proc.CPU } catch { }
            [Console]::WriteLine(("  [{0,6:N1}s] working: rss {1,6:N1} MB  cpu {2,6:N1}s" -f `
                $watch.Elapsed.TotalSeconds, ($proc.WorkingSet64 / 1MB), $cpu))
        }

        # RULE 3: a deadline that names the step
        if ($watch.Elapsed.TotalSeconds -gt $TimeoutSeconds) {
            $stage = if ($null -eq $startupMs) { "waiting for the window to appear" }
                     else { "waiting for the --bench-seconds report line" }
            try { $proc.Kill() } catch { }
            [Console]::WriteLine(("  TIMEOUT after {0:N0}s while: {1}" -f $watch.Elapsed.TotalSeconds, $stage))
            return @{ Ok = $false; Stage = $stage; Startup = $startupMs; Stdout = "" }
        }

        # ! POLL FAST UNTIL THE WINDOW IS UP, SLOW AFTERWARDS.
        #
        #   startup_to_window is measured by this loop, so the sleep here IS
        #   the measurement's granularity.  A flat 100 ms sleep reported
        #   119-153 ms for a startup that a 2 ms poll measures at 32-50 ms --
        #   the tool was reporting its own latency and calling it the app's.
        #   Once the window is up nothing needs millisecond resolution, so the
        #   cadence drops back to 100 ms and stops burning a core.
        if ($null -eq $startupMs) { Start-Sleep -Milliseconds 2 }
        else                      { Start-Sleep -Milliseconds 100 }
    }

    $elapsed = $watch.Elapsed.TotalSeconds
    $rssIdle = 0
    if (-not $proc.HasExited) { $proc.Refresh(); $rssIdle = $proc.WorkingSet64 }

    # RULE 1: we close it, it never closes itself
    if (-not $proc.HasExited) {
        [void]$proc.CloseMainWindow()
        if (-not $proc.WaitForExit(5000)) { try { $proc.Kill() } catch { } }
    }
    [void]$proc.WaitForExit(5000)

    $stdout = ""
    if (Test-Path $outFile) { $stdout = Get-Content $outFile -Raw -Encoding UTF8 -ErrorAction SilentlyContinue }
    Remove-Item $outFile, $errFile -ErrorAction SilentlyContinue

    [Console]::WriteLine(("  {0} finished in {1:N1}s (report line seen: {2})" -f $Label, $elapsed, $reported))
    return @{
        Ok = $reported; Startup = $startupMs; PeakRss = $peakRss
        RssIdle = $rssIdle; Stdout = $stdout; Elapsed = $elapsed
    }
}

# ---- pass 1: warm the cache -------------------------------------------------
# A cold cache means the first run spends most of its time decoding, which
# would land inside the measurement window and turn the frame numbers into a
# measurement of the decoder instead of the renderer.
if (-not $SkipWarm) {
    Write-Output "pass 1/2: warming cache.sqlite (cold decode of $count files)"
    $warm = Invoke-Pass -Label "cold pass" -BenchSeconds $Seconds -DoPan $false
    if (-not $warm.Ok) {
        Write-Output "ABORT: cold pass did not reach its report line -- numbers would be meaningless"
        exit 1
    }
    Write-Output ("  cold: to-report {0:N1}s (bench window {1}s -> load <= {2:N1}s)" -f `
        $warm.Elapsed, $Seconds, ($warm.Elapsed - $Seconds))
    Write-Output ""
}

# ---- pass 2: the measured run ----------------------------------------------
Write-Output "pass 2/2: measured run (warm cache)"
$run = Invoke-Pass -Label "measured pass" -BenchSeconds $Seconds -DoPan ([bool]$Pan)
if (-not $run.Ok) {
    Write-Output ("ABORT: measured pass stuck while {0}" -f $run.Stage)
    exit 1
}

Write-Output ""
Write-Output "---- what the app printed ----"
foreach ($line in ($run.Stdout -split "`r?`n")) {
    if ($line -match "p50|quad|vram") { Write-Output $line }
}

# ---- table against the ceilings in docs/08 section 2 ------------------------
$p50 = $null; $p99 = $null; $vram = $null
if ($run.Stdout -match "p50\s+([0-9.]+)\s+ms\s+\|\s+p99\s+([0-9.]+)\s+ms") {
    $p50 = [double]$Matches[1]; $p99 = [double]$Matches[2]
}
if ($run.Stdout -match "vram\s+([0-9.]+)\s+MB") { $vram = [double]$Matches[1] }
# open_board_1000_warm_cache
#
# ! THE APP ALREADY MEASURES THIS.  It has printed
#       "<thai> N <thai> M ms"   (drag & drop -> every image on screen)
#   since P1-8, timed from the drop to the frame where the last image is up.
#   A LoadTracker-based second measurement was written for this row and then
#   thrown away on discovering the first one: two things measuring the same
#   quantity drift, and the older one is the one the app itself reports to the
#   user.  (They agreed to 0.1 ms while both existed: 303.9 vs 304.)
#
# Match on the SHAPE (two numbers, the second followed by "ms") rather than on
# the Thai words, so a wording change cannot silently make this NOT MEASURED.
#
# ! TAKE THE BATCH WITH THE MOST IMAGES, NOT THE LAST LINE.
#   Working textures decode through the same pool, so panning can add small
#   batches.  Taking the last line once reported "5 ms" for a board that took
#   591 ms to open -- a beautiful number measuring a few working textures.
$loadItems = 0
foreach ($l in ($run.Stdout -split "`r?`n")) {
    if (($l -match "([0-9]+)\D+([0-9.]+)\s*ms\s*$") -and ($l -notmatch "p50")) {
        $items = [int]$Matches[1]
        if ($items -gt $loadItems) {
            $loadItems = $items
            $loadMs = [double]$Matches[2]
        }
    }
}
if ($loadItems -gt 0) {
    Write-Output ("(open_board row came from the app's own line: {0} images)" -f $loadItems)
}

function Write-Row {
    param([string]$Name, $Value, [double]$Limit, [string]$Unit)
    if ($null -eq $Value) {
        Write-Output ("{0,-24} {1,12} {2,12:N2} {3}" -f $Name, "NOT MEASURED", $Limit, $Unit)
        return
    }
    $verdict = if ($Value -le $Limit) { "under" } else { "OVER" }
    $ratio = if ($Value -gt 0) { $Limit / $Value } else { 0 }
    Write-Output ("{0,-24} {1,12:N2} {2,12:N2} {3,-3} {4,-5} ({5:N1}x headroom)" -f `
        $Name, $Value, $Limit, $Unit, $verdict, $ratio)
}

Write-Output ""
Write-Output ("{0,-24} {1,12} {2,12}" -f "row (docs/08 s2)", "measured", "ceiling")
Write-Output ("{0,-24} {1,12} {2,12}" -f "------------------------", "------------", "------------")
Write-Row "startup_to_window" $run.Startup 400 "ms"
Write-Row "open_board_1000_warm" $loadMs 1500 "ms"
# ! the LABEL must follow what was actually done.  A tool that prints
#   "(redraw)" after it just spent the whole window panning is producing
#   mislabelled evidence, which is the failure mode this whole file is about.
$what = if ($Pan) { "pan" } else { "redraw" }
Write-Row "frame p50 ($what)" $p50 8 "ms"
Write-Row "frame p99 ($what)" $p99 16 "ms"
Write-Row "rss_idle_1000" ([math]::Round($run.RssIdle / 1MB, 2)) 250 "MB"
Write-Row "vram_idle_1000" $vram 200 "MB"
Write-Output ""
Write-Output ("peak RSS during the measured pass: {0:N1} MB" -f ($run.PeakRss / 1MB))
if ($Pan) {
    Write-Output "NOTE: a real middle-button pan ran for the whole bench window,"
    Write-Output "      so the frame rows above ARE frame_pan_1000."
} else {
    Write-Output "NOTE: the frame rows are redraw-with-N-items, NOT while panning."
    Write-Output "      pass -Pan to measure the frame_pan_1000 row of docs/08 s2."
}
