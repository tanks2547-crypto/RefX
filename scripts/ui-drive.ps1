# Drive RefX with real mouse/keyboard input and capture what is on screen.
#
# docs/08 3.9 item 5 makes a real screenshot mandatory before any user-visible
# work can be called done, because it is the only tool that catches the
# "every piece is right, the assembly is wrong" class of bug.  This is the
# harness that produces those screenshots.
#
# ---------------------------------------------------------------------------
# WHY THE FOCUS CHECK EXISTS  (cost one whole session on 4 Aug 2026)
# ---------------------------------------------------------------------------
# Windows refuses SetForegroundWindow when the calling process is not itself
# in the foreground.  The old version of this script ignored the return value,
# so when the refusal happened every SetCursorPos/mouse_event afterwards was
# delivered to WHATEVER WINDOW HAPPENED TO BE ON TOP, and CopyFromScreen
# photographed that other window's pixels into a file named like evidence.
#
# The symptom was recorded in HANDOFF as "dragging in the app does not move
# the image (659 -> 659)" and was suspected to be a real input bug in RefX.
# It was not.  RefX never saw the clicks.
#
# So: focus is asserted before every step that clicks, types or photographs,
# and the script EXITS NON-ZERO rather than produce a screenshot that lies.
# A harness that fails silently is worse than no harness (docs/08 3.9 item 2).
#
# ---------------------------------------------------------------------------
# Usage
# ---------------------------------------------------------------------------
#   ! call it IN-PROCESS with & - going through "powershell -File" strips the
#     quoting and the child's parser eats every step containing a '|'
#
#   & .\scripts\ui-drive.ps1 -Steps @(
#     "launch|target\release\refx.exe|--open-dir=C:\shots\imgs",
#     "click|452|386",
#     "shot|C:\shots\selected.png")
#
# Steps (coordinates are CLIENT coordinates of the RefX window):
#   launch|<exe>|<args>              start it, size the window, take focus
#   launchlog|<exe>|<args>|<file>    same, with stdout captured (--bench-seconds)
#   attach                           re-focus an already running refx.exe
#   move|<x>|<y>                     move the cursor
#   down / up                        primary button
#   click|<x>|<y>                    move + press + release
#   pan|<x1>|<y1>|<x2>|<y2>          middle-button camera pan (P2-4 moved pan there)
#   wheel|<x>|<y>|<notches>          zoom
#   keydn|<vk> / keyup|<vk> / key|<vk>   virtual key codes (Ctrl = 17, Z = 90)
#   copyimg|<png>                    put a real IMAGE on the clipboard, Ctrl+V it
#                                    (the pasted-image path of P4-5 - 'paste'
#                                     below carries text and never reaches it)
#   paste|<text>                     put text on the clipboard then Ctrl+V it
#                                    (into RefX itself - forces main-window focus)
#   dlgtype|<text>                   type into a NATIVE dialog + press Enter
#                                    (Save As / Open / find-the-file - asserts a
#                                     dialog of ours owns the foreground first)
#   shot|<file>                      PNG of the client area
#   sleep|<ms>
#   kill                             stop refx and WAIT for the single-instance
#                                    lock to be released
#
#   -Validate                        check the step list and run NOTHING
#
# Exit codes: 1 = lost focus (would produce a screenshot of another window)
#             2 = bad step list (checked before anything runs)
#             3 = target hung (would produce a screenshot of a dead app)
#
# ---------------------------------------------------------------------------
# WHY EVERY STEP IS CHECKED BEFORE ANY STEP RUNS  (cost a device-lost run,
# 17 Sep 2026)
# ---------------------------------------------------------------------------
# 'launch' read $parts[1] and $parts[2] and said nothing about a $parts[3].  So
#
#   "launch|refx.exe|--open-dir=E:\imgs|--force-device-lost-after-ms=6000"
#
# started RefX with the folder and WITHOUT the simulation flag.  The app came
# up, sat there, and the run "passed" -- because nothing happened.  Nothing was
# supposed to happen: the experiment was never performed.  It was only caught by
# reading refx.log and seeing generation=0 where the whole point was to see a 1.
#
# An argument that is dropped in silence is an experiment that never ran, and it
# lands in the checklist as a line that was verified.  So the step list is now
# checked against the arity table below BEFORE the first mouse moves: too many
# fields is an error, too few is an error, and the run stops at zero steps
# rather than half way through with real input already delivered.
#
# ASCII only on purpose: Windows PowerShell 5.1 reads a BOM-less file as ANSI,
# so non-ASCII comments come back as mojibake and can break parsing.
param([string[]]$Steps, [switch]$Validate)

# verb -> @(fewest fields, most fields) INCLUDING the verb itself.
# ! keep in step with the switch below - a handler that starts reading one more
#   field must widen its entry here, and that is the point: the table is the
#   place where "how many fields does this step have" is written down once.
$ARITY = @{
  'launch'    = @(2, 3)   # args are optional
  'launchlog' = @(4, 4)
  'attach'    = @(1, 1)
  'move'      = @(3, 3)
  'down'      = @(1, 1)
  'up'        = @(1, 1)
  'click'     = @(3, 3)
  'pan'       = @(5, 5)
  'wheel'     = @(4, 4)
  'keydn'     = @(2, 2)
  'keyup'     = @(2, 2)
  'key'       = @(2, 2)
  'paste'     = @(2, 2)
  'copyimg'   = @(2, 2)
  'dlgtype'   = @(2, 2)
  'dlgenter'  = @(1, 1)
  'shot'      = @(2, 2)
  'sleep'     = @(2, 2)
  'kill'      = @(1, 1)
}

function Check-Steps($steps) {
  $problems = @()
  for ($i = 0; $i -lt $steps.Count; $i++) {
    $parts = $steps[$i] -split '\|'
    $verb = $parts[0]
    if (-not $ARITY.ContainsKey($verb)) {
      $problems += "step $($i + 1) '$($steps[$i])': unknown step '$verb'"
      continue
    }
    $lo = $ARITY[$verb][0]; $hi = $ARITY[$verb][1]; $n = $parts.Count
    if ($n -gt $hi) {
      $extra = ($parts[$hi..($n - 1)]) -join '|'
      $problems += "step $($i + 1) '$($steps[$i])': $n fields but '$verb' reads $hi - '$extra' WOULD BE DROPPED SILENTLY"
    } elseif ($n -lt $lo) {
      $problems += "step $($i + 1) '$($steps[$i])': $n fields but '$verb' needs $lo"
    }
  }
  return ,$problems
}

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, IntPtr e);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int t, bool r);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, IntPtr extra);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
  [DllImport("user32.dll", EntryPoint="GetWindowThreadProcessId")] public static extern uint GetWindowPid(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr res);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
}
"@

$LEFTDOWN = 0x0002; $LEFTUP = 0x0004; $WHEEL = 0x0800; $KEYUP = 0x0002
$MIDDOWN = 0x0020; $MIDUP = 0x0040

$script:hwnd = [IntPtr]::Zero
$script:ox = 0; $script:oy = 0
$script:pid2 = 0

function Get-Origin {
  $p = New-Object W+POINT
  [void][W]::ClientToScreen($script:hwnd, [ref]$p)
  $script:ox = $p.X; $script:oy = $p.Y
}

# ---------------------------------------------------------------------------
# IS THE TARGET STILL ALIVE?
# ---------------------------------------------------------------------------
# A hung window and a window that simply ignores input are INDISTINGUISHABLE
# from a screenshot: Windows keeps presenting the last frame either way, so
# CopyFromScreen produces a perfectly normal-looking picture of a dead app.
#
# That cost several wrong conclusions in one session -- every keypress "did
# nothing", which read as a shortcut bug, when the process had actually
# deadlocked.  Same failure class as the silent SetForegroundWindow refusal
# this script already guards: evidence that looks real and is not.
#
# WM_NULL with SMTO_ABORTIFHUNG returns 0 when the message pump is stuck.
# Process.Responding alone is not enough -- it can lag.  Both are checked.
function Test-Alive {
  $res = [IntPtr]::Zero
  $answered = [W]::SendMessageTimeout($script:hwnd, 0x0000, [IntPtr]::Zero, [IntPtr]::Zero, 0x0002, 3000, [ref]$res)
  if ($answered -eq [IntPtr]::Zero) { return $false }
  $proc = Get-Process -Id $script:pid2 -ErrorAction SilentlyContinue
  if (-not $proc) { return $false }
  $proc.Refresh()
  return $proc.Responding
}

function Assert-Alive($what) {
  if ($script:hwnd -eq [IntPtr]::Zero) { return }
  if (-not (Test-Alive)) {
    Write-Output "TARGET HUNG before '$what' - the window still paints its last frame, so a screenshot here would look normal and be a lie"
    exit 3
  }
}

# ---------------------------------------------------------------------------
# ! AttachThreadInput is a hazard, use it as little as possible
# ---------------------------------------------------------------------------
# It is the only way to lift Windows' foreground lock, but coupling two input
# queues can wedge BOTH threads.  RefX was seen deadlocked (Responding=False,
# pump silent, never recovered) right after a burst of these calls, and a
# deliberate 25-cycle burst reproduced it -- while a single cycle on a freshly
# launched app did not.  So: only call it when focus is genuinely lost, and
# check liveness afterwards instead of assuming it worked.
#
# Switching keyboard layout is NOT the cause -- see scripts/ui-layout-switch.ps1
# Attaching our input queue to the current foreground thread is what lifts
# Windows' foreground lock; SetForegroundWindow alone is simply ignored.
function Force-Foreground {
  for ($try = 0; $try -lt 6; $try++) {
    if ([W]::GetForegroundWindow() -eq $script:hwnd) { return $true }
    $fg = [W]::GetForegroundWindow()
    $fgThread = [W]::GetWindowThreadProcessId($fg, [IntPtr]::Zero)
    $me = [W]::GetCurrentThreadId()
    [void][W]::AttachThreadInput($me, $fgThread, $true)
    [void][W]::BringWindowToTop($script:hwnd)
    [void][W]::ShowWindow($script:hwnd, 5)     # SW_SHOW
    [void][W]::SetForegroundWindow($script:hwnd)
    [void][W]::AttachThreadInput($me, $fgThread, $false)
    Start-Sleep -Milliseconds 250
  }
  return ([W]::GetForegroundWindow() -eq $script:hwnd)
}

function Assert-Focus($what) {
  Assert-Alive $what
  if ([W]::GetForegroundWindow() -ne $script:hwnd) {
    if (-not (Force-Foreground)) {
      Write-Output "FOCUS LOST before '$what' - refusing to produce fake evidence"
      exit 1
    }
  }
}

# The waits are generous on purpose.  egui coalesces input into one frame, so a
# move and the press that follows it too closely arrive together and the press
# lands at the post-move position (first seen in P2-5).
function Move-To($x, $y) {
  Assert-Focus "move"
  [void][W]::SetCursorPos($script:ox + $x, $script:oy + $y)
  Start-Sleep -Milliseconds 120
}
function Btn-Down { Assert-Focus "down"; [W]::mouse_event($LEFTDOWN, 0, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 120 }
function Btn-Up   { [W]::mouse_event($LEFTUP,   0, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 120 }
function Wheel-By($n) { $d = [int64]([int]$n * 120) -band 0xFFFFFFFFL; [W]::mouse_event($WHEEL, 0, 0, [uint32]$d, [IntPtr]::Zero); Start-Sleep -Milliseconds 200 }
function Key-Down($vk) { Assert-Focus "keydn"; [W]::keybd_event([byte]$vk, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 60 }
function Key-Up($vk) { [W]::keybd_event([byte]$vk, 0, $KEYUP, [IntPtr]::Zero); Start-Sleep -Milliseconds 60 }

# ---------------------------------------------------------------------------
# TYPING INTO A NATIVE DIALOG  (added 21 Aug 2026, P4-6)
# ---------------------------------------------------------------------------
# Assert-Focus forces the MAIN window to the foreground.  That is right for
# every step that drives RefX itself and WRONG for a native Save As / Open /
# pick-a-file dialog: forcing the main window steals focus from the dialog that
# is supposed to receive the keystrokes, so the typing lands in the canvas and
# the dialog sits there untouched.  HANDOFF recorded that as "cannot be
# verified automatically" -- it was the check, not the app.
#
# The fix is not to drop the check.  It is to check the RIGHT window: the
# foreground window must belong to the SAME process and must NOT be the main
# window, i.e. a dialog of ours really is on top.  If no dialog is up we exit
# non-zero instead of typing into whatever happens to be focused.
function Assert-Dialog($what) {
  Assert-Alive $what
  $fg = [W]::GetForegroundWindow()
  if ($fg -eq $script:hwnd) {
    Write-Output "NO DIALOG before '$what' - the main window still has focus, typing would land in the canvas"
    exit 1
  }
  $owner = 0
  [void][W]::GetWindowPid($fg, [ref]$owner)
  if ($owner -ne $script:pid2) {
    Write-Output "FOREIGN WINDOW before '$what' - foreground belongs to pid $owner, not RefX ($($script:pid2))"
    exit 1
  }
  Write-Output "dialog is up (pid $owner)"
}

function Shot($path) {
  Start-Sleep -Milliseconds 350
  Assert-Focus "shot $path"
  # check again right next to the capture - the window can die during the 350 ms wait
  Assert-Alive "shot $path"
  $r = New-Object W+RECT
  [void][W]::GetClientRect($script:hwnd, [ref]$r)
  $w = $r.R - $r.L; $h = $r.B - $r.T
  $bmp = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($script:ox, $script:oy, 0, 0, (New-Object System.Drawing.Size $w, $h))
  $g.Dispose()
  # ---------------------------------------------------------------------------
  # CHECK FOCUS AGAIN, AFTER the pixels are copied  (added 12 Aug 2026)
  # ---------------------------------------------------------------------------
  # The check above happens BEFORE the copy, which leaves a window of a few
  # hundred milliseconds where another program can come to the front.  That is
  # not hypothetical: a screenshot taken during this audit came back showing a
  # completely different application, and the pre-check had passed.  Same class
  # of lie the foreground check was added for -- just a smaller window.
  #
  # So: verify after the copy too, and throw the bitmap away rather than save a
  # picture of somebody else's window under an evidence file name.
  if ([W]::GetForegroundWindow() -ne $script:hwnd) {
    $bmp.Dispose()
    Write-Output "FOCUS LOST during capture of '$path' - discarded, no file written"
    exit 1
  }
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  Write-Output "shot $path ($w x $h)"
}

# A big --open-dir delays the first window well past any fixed sleep, so poll.
function Wait-Window($proc) {
  for ($i = 0; $i -lt 60; $i++) {
    Start-Sleep -Seconds 1
    $proc.Refresh()
    if ($proc.MainWindowHandle -ne [IntPtr]::Zero) { return $proc.MainWindowHandle }
  }
  return [IntPtr]::Zero
}

function Setup-Window($proc, $note) {
  $script:pid2 = $proc.Id
  $script:hwnd = Wait-Window $proc
  if ($script:hwnd -eq [IntPtr]::Zero) { Write-Output "NO WINDOW"; exit 1 }
  Start-Sleep -Seconds 3
  [void][W]::MoveWindow($script:hwnd, 0, 0, 1296, 839, $true)
  Start-Sleep -Milliseconds 600
  if (-not (Force-Foreground)) { Write-Output "CANNOT FOCUS"; exit 1 }
  Get-Origin
  Write-Output "pid $($proc.Id) origin $($script:ox),$($script:oy) focused $note"
}

# ---------------------------------------------------------------------------
# WHY THE STEP LIST IS ECHOED  (cost part of a session on 18 Aug 2026)
# ---------------------------------------------------------------------------
# Calling this script through a CHILD powershell (powershell -File ui-drive.ps1
# -Steps @(...)) passes the array elements UNQUOTED, so the child's parser sees
# the '|' inside "sleep|500" as a pipeline separator and keeps only the first
# element.  The run then did one step, skipped the rest, printed nothing about
# it and EXITED 0 - a harness reporting success for work it never did, which is
# the exact failure mode the header of this file warns about.
#
# Two things stop it now: this banner (the count is visibly wrong immediately)
# and the unknown-step guard below.  Callers should invoke the script IN-PROCESS
# (& .\scripts\ui-drive.ps1 -Steps @(...)) which passes the array intact.
Write-Output "steps: $($Steps.Count)"

$problems = Check-Steps $Steps
if ($problems.Count -gt 0) {
  Write-Output "STEP LIST REJECTED - nothing was run"
  foreach ($p in $problems) { Write-Output "  $p" }
  exit 2
}
if ($Validate) {
  Write-Output "step list ok: $($Steps.Count) steps - nothing run (-Validate)"
  exit 0
}

foreach ($step in $Steps) {
  $parts = $step -split '\|'
  switch ($parts[0]) {
    'launch' {
      if ($parts[2]) { $proc = Start-Process -FilePath $parts[1] -ArgumentList $parts[2] -PassThru }
      else           { $proc = Start-Process -FilePath $parts[1] -PassThru }
      Setup-Window $proc ""
    }
    'launchlog' {
      $proc = Start-Process -FilePath $parts[1] -ArgumentList $parts[2] -PassThru -RedirectStandardOutput $parts[3]
      Setup-Window $proc "log=$($parts[3])"
    }
    'attach' {
      $proc = Get-Process refx -ErrorAction Stop
      $script:pid2 = $proc.Id
      $script:hwnd = $proc.MainWindowHandle
      # ! liveness FIRST - MoveWindow/ShowWindow/SetForegroundWindow all send
      #   messages internally, so on a hung target they block forever and the
      #   harness never reaches its own gate.  Ask a question with a timeout
      #   before making any call that waits for an answer.
      Assert-Alive "attach"
      [void][W]::ShowWindow($script:hwnd, 9)   # SW_RESTORE
      Start-Sleep -Milliseconds 400
      [void][W]::MoveWindow($script:hwnd, 0, 0, 1296, 839, $true)
      Start-Sleep -Milliseconds 400
      if (-not (Force-Foreground)) { Write-Output "CANNOT FOCUS"; exit 1 }
      Get-Origin
      Write-Output "attached pid $($proc.Id) origin $($script:ox),$($script:oy) focused"
    }
    'move'   { Move-To ([int]$parts[1]) ([int]$parts[2]) }
    'down'   { Btn-Down }
    'up'     { Btn-Up }
    'click'  { Move-To ([int]$parts[1]) ([int]$parts[2]); Btn-Down; Btn-Up }
    'pan'    {
      Move-To ([int]$parts[1]) ([int]$parts[2])
      [W]::mouse_event($MIDDOWN, 0, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 150
      Move-To ([int]$parts[3]) ([int]$parts[4])
      Move-To ([int]$parts[3]) ([int]$parts[4])
      [W]::mouse_event($MIDUP, 0, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 150
    }
    'wheel'  { Move-To ([int]$parts[1]) ([int]$parts[2]); Wheel-By ([int]$parts[3]) }
    'keydn'  { Key-Down ([int]$parts[1]) }
    'keyup'  { Key-Up ([int]$parts[1]) }
    'key'    { Key-Down ([int]$parts[1]); Key-Up ([int]$parts[1]) }
    # Type text into whatever has keyboard focus, including a NATIVE dialog
    # (Save As / Open), by way of the clipboard.  Sending one keybd_event per
    # character would need VkKeyScan per char and still get the layout wrong on
    # a non-US keyboard - the same class of bug as the Thai-layout shortcut one.
    # The clipboard carries the exact string whatever the layout is.
    'paste'  {
      Set-Clipboard -Value $parts[1]
      Start-Sleep -Milliseconds 250
      Key-Down 17; Key-Down 86; Key-Up 86; Key-Up 17   # Ctrl+V
      Start-Sleep -Milliseconds 250
      Write-Output "paste '$($parts[1])'"
    }
    # Put a real IMAGE on the clipboard, then Ctrl+V it.  'paste' above carries
    # TEXT, which exercises a completely different branch of the app: the text
    # path ends in ClipboardContent::Files or NoImage and never touches the
    # decode/spool path that P4-5 is about.  Proving the pasted-image path needs
    # actual pixels in the clipboard, so this step exists.
    #
    # ! Clipboard.SetImage needs an STA thread.  powershell.exe 5.1 is STA by
    #   default, but say so out loud rather than assume: if the apartment is
    #   MTA the call throws and this step fails loudly instead of pasting
    #   nothing and letting the run "succeed" with no image (docs/08 3.9 item 2).
    'copyimg' {
      if ([Threading.Thread]::CurrentThread.GetApartmentState() -ne 'STA') {
        Write-Output "NOT STA - Clipboard.SetImage cannot run here"; exit 1
      }
      Add-Type -AssemblyName System.Windows.Forms
      $img = [System.Drawing.Image]::FromFile((Resolve-Path $parts[1]))
      # copy into a new bitmap so the file handle is released before Ctrl+V
      $bmp = New-Object System.Drawing.Bitmap $img
      $img.Dispose()
      [System.Windows.Forms.Clipboard]::SetImage($bmp)
      Start-Sleep -Milliseconds 400
      if (-not [System.Windows.Forms.Clipboard]::ContainsImage()) {
        Write-Output "clipboard has no image after SetImage"; exit 1
      }
      Write-Output "copyimg $($parts[1]) $($bmp.Width)x$($bmp.Height)"
      Key-Down 17; Key-Down 86; Key-Up 86; Key-Up 17   # Ctrl+V
      Start-Sleep -Milliseconds 250
    }
    # Type a path into a NATIVE dialog and press Enter.
    #
    # ! deliberately does NOT call Assert-Focus: that helper forces the MAIN
    #   window forward, which is exactly what breaks this case.  Assert-Dialog
    #   checks the right thing instead (a dialog OF OURS owns the foreground).
    'dlgtype' {
      Assert-Dialog "dlgtype"
      Set-Clipboard -Value $parts[1]
      Start-Sleep -Milliseconds 300
      # ! SendKeys, not keybd_event.  A native file dialog runs its own modal
      #   message loop; the synthetic key-down/key-up pairs that RefX itself
      #   receives fine are swallowed there (observed 21 Aug 2026: the path
      #   pasted but Enter never committed, dialog just sat open).  SendKeys
      #   goes through the same path a real keyboard does for that loop.
      # ! retry: the first ^v after the dialog appears is sometimes swallowed
      #   while the dialog is still settling, and then Enter dismisses an EMPTY
      #   name box (seen 21 Aug 2026 - one run wrote the file, the next did not
      #   with identical steps).  Re-sending is safe: if the paste did land the
      #   dialog is already gone and the loop stops.
      #   ★ and wait for the dialog to actually CLOSE before deciding to retry:
      #   focus takes over a second to travel back, so "still up" and "just
      #   slow" look identical if asked too early - a retry there fires a
      #   second Enter into the main window instead (seen 21 Aug 2026).
      #
      # ! keybd_event, NOT SendKeys.  SendKeys' ^a/^v land on whichever child
      #   control the dialog happens to focus (the file LIST, not the name box)
      #   and then Enter dismisses an empty name - measured on 21 Aug 2026:
      #   keybd_event wrote the file, the SendKeys version never did.
      [W]::keybd_event(17, 0, 0, [IntPtr]::Zero)          # Ctrl down
      [W]::keybd_event(86, 0, 0, [IntPtr]::Zero)          # V
      [W]::keybd_event(86, 0, $KEYUP, [IntPtr]::Zero)
      [W]::keybd_event(17, 0, $KEYUP, [IntPtr]::Zero)
      Start-Sleep -Milliseconds 500
      [W]::keybd_event(13, 0, 0, [IntPtr]::Zero)          # Enter
      [W]::keybd_event(13, 0, $KEYUP, [IntPtr]::Zero)
      $back = $false
      for ($i = 0; $i -lt 24; $i++) {
        Start-Sleep -Milliseconds 250
        if ([W]::GetForegroundWindow() -eq $script:hwnd) { $back = $true; break }
      }
      # The dialog must be GONE afterwards - if it is still up the step did
      # nothing and every later assertion would be about a state we never reached
      if (-not $back) {
        Write-Output "DIALOG STILL UP after 'dlgtype' - the path was not accepted"
        exit 1
      }
      Write-Output "dlgtype '$($parts[1])'"
    }
    # Accept whatever a native dialog already has selected (no typing at all).
    # Useful when the point is "does the dialog work", not "which path".
    'dlgenter' {
      Assert-Dialog "dlgenter"
      [W]::keybd_event(13, 0, 0, [IntPtr]::Zero)
      [W]::keybd_event(13, 0, $KEYUP, [IntPtr]::Zero)
      $back = $false
      for ($i = 0; $i -lt 24; $i++) {
        Start-Sleep -Milliseconds 250
        if ([W]::GetForegroundWindow() -eq $script:hwnd) { $back = $true; break }
      }
      if (-not $back) { Write-Output "DIALOG STILL UP after 'dlgenter'"; exit 1 }
      Write-Output "dlgenter"
    }
    'shot'   { Shot $parts[1] }
    'sleep'  { Start-Sleep -Milliseconds ([int]$parts[1]) }
    'kill'   {
      # the single-instance lock lives as long as the process does; launching
      # again too early just prints "another RefX instance already holds the lock"
      Get-Process refx -ErrorAction SilentlyContinue | Stop-Process -Force
      for ($i = 0; $i -lt 20; $i++) {
        if (-not (Get-Process refx -ErrorAction SilentlyContinue)) { break }
        Start-Sleep -Milliseconds 300
      }
      Start-Sleep -Milliseconds 1200
    }
    # unreachable now that Check-Steps runs first, and kept anyway: the arity
    # table and this switch are two lists that have to agree, and this is the
    # arm that says so out loud if somebody adds a verb to only one of them.
    default  { Write-Output "unknown step: $($parts[0])"; exit 1 }
  }
}
