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
# ASCII ONLY, ON PURPOSE (docs/08 section 3.9 rule 9, 4th row): a Windows
# console defaults to cp1252 and dies when a script prints Thai.
#
# USAGE
#   powershell -File scripts/measure-app.ps1 -Images E:/refx-bench-1000
#   powershell -File scripts/measure-app.ps1 -Images ... -Seconds 15 -SkipWarm
#
# WHAT IT REPORTS (and what it deliberately does NOT claim)
#   startup_to_window  process start -> window handle exists
#   frame p50 / p99    printed by the app itself (--bench-seconds)
#   vram_idle          printed by the app itself (texture budget in use)
#   rss_idle           WorkingSet64 measured from outside, after the report
#
#   * The app draws continuously while --bench-seconds runs, so the frame
#     numbers are "redraw with N items on the board", NOT "while panning".
#     The docs/08 row is called frame_pan_1000 -- driving the mouse is a
#     separate job (scripts/ui-drive.ps1) and is not done here.  Reporting
#     this as frame_pan_1000 would claim something the tool did not measure.

param(
    [Parameter(Mandatory = $true)][string]$Images,
    [int]$Seconds = 15,
    [switch]$SkipWarm,
    [string]$Exe = "target/release/refx.exe",
    # deadline per pass.  cold decode of 1000 files is ~2 min on the dev box;
    # 12 min is generous enough that hitting it means something is really wrong.
    [int]$TimeoutSeconds = 720
)

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
    param([string]$Label, [int]$BenchSeconds)

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

        Start-Sleep -Milliseconds 100
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
    $warm = Invoke-Pass -Label "cold pass" -BenchSeconds $Seconds
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
$run = Invoke-Pass -Label "measured pass" -BenchSeconds $Seconds
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
Write-Row "frame p50 (redraw)" $p50 8 "ms"
Write-Row "frame p99 (redraw)" $p99 16 "ms"
Write-Row "rss_idle_1000" ([math]::Round($run.RssIdle / 1MB, 2)) 250 "MB"
Write-Row "vram_idle_1000" $vram 200 "MB"
Write-Output ""
Write-Output ("peak RSS during the measured pass: {0:N1} MB" -f ($run.PeakRss / 1MB))
Write-Output "NOTE: the frame rows are redraw-with-N-items, NOT while panning."
Write-Output "      docs/08 calls that row frame_pan_1000 -- see this script's header."
