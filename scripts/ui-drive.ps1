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
#   scripts/ui-drive.ps1 -Steps @(
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
#   shot|<file>                      PNG of the client area
#   sleep|<ms>
#   kill                             stop refx and WAIT for the single-instance
#                                    lock to be released
#
# Exit codes: 1 = lost focus (would produce a screenshot of another window)
#             3 = target hung (would produce a screenshot of a dead app)
#
# ASCII only on purpose: Windows PowerShell 5.1 reads a BOM-less file as ANSI,
# so non-ASCII comments come back as mojibake and can break parsing.
param([string[]]$Steps)

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
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
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
    default  { Write-Output "unknown step: $($parts[0])"; exit 1 }
  }
}
