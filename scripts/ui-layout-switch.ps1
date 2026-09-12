# Does switching the keyboard layout while RefX is focused hang it?
#
# WHY THIS EXISTS
# ---------------
# docs/03 section 0 says Thai is the program's second language, and Thai and
# Japanese users switch keyboard layout dozens of times a day -- between typing
# a note and using a shortcut.  If the app wedged on every switch it would be
# unusable for exactly the audience we claim to support, and nobody developing
# on an English-only layout would ever see it.
#
# That is the same shape as the bug this check was written after: every letter
# shortcut was dead on a Thai layout from P2-4 onward, unnoticed for months,
# because the development machine was one environment and not all of them.
#
# HOW TO RUN
#   1. start RefX (any board)
#   2. make sure at least two keyboard layouts are installed:
#        Get-WinUserLanguageList
#      (this machine: 0409 English + 041E Thai)
#   3. scripts/ui-layout-switch.ps1 -Rounds 6
#
# PASS = "SURVIVED n switches".  Anything else is a release blocker.
#
# Two paths are checked because they are NOT the same thing:
#   winspace    - what a user actually does; Windows posts the message itself
#   postmessage - synthetic, posts WM_INPUTLANGCHANGEREQUEST straight at the
#                 window.  Kept because that is what a debugging script did when
#                 the question first came up, and the difference matters when
#                 reading old notes.
param([string]$Mode = "winspace", [int]$Rounds = 6)

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class LP {
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, IntPtr extra);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
  [DllImport("user32.dll")] public static extern IntPtr GetKeyboardLayout(uint thread);
  [DllImport("user32.dll")] public static extern IntPtr LoadKeyboardLayout(string id, uint flags);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr res);
}
"@

$KEYUP = 0x0002
$VK_LWIN = 0x5B
$VK_SPACE = 0x20

$proc = Get-Process refx -ErrorAction Stop
$hwnd = $proc.MainWindowHandle

function Layout-Now {
  $t = [LP]::GetWindowThreadProcessId($hwnd, [IntPtr]::Zero)
  return "0x{0:X}" -f ([LP]::GetKeyboardLayout($t)).ToInt64()
}

# A hung window and a window ignoring input look identical in a screenshot --
# Windows keeps presenting the last frame.  WM_NULL with SMTO_ABORTIFHUNG is
# the thing that actually tells them apart.
# ! Write-Host, NOT Write-Output  (fixed 14 Sep 2026 - HANDOFF 2.42k, queue 3nj)
#
# This function reported SURVIVED on a window that was already hung, and the
# cause was never the timing of the WM_NULL probe.  A PowerShell function
# returns EVERYTHING it writes to the pipeline, so
#
#     Write-Output "..." ; return $ok
#
# hands the caller an ARRAY of (string, bool).  `if (-not (Alive ...))` then
# negates an array, which is truthy whenever it has elements -- so the test
# could never fail, for any app, ever.  The same bug also swallowed the log
# lines, which is why the per-round detail never appeared in the output.
#
# Write-Host goes to the host instead of the pipeline, so the return value is
# the bool alone.  Anything in this repo that prints from inside a function
# that also returns a value has to do the same.
function Alive($label) {
  $proc.Refresh()
  $res = [IntPtr]::Zero
  $answered = [LP]::SendMessageTimeout($hwnd, 0x0000, [IntPtr]::Zero, [IntPtr]::Zero, 0x0002, 3000, [ref]$res)
  $ok = $proc.Responding -and ($answered -ne [IntPtr]::Zero)
  Write-Host ("  {0,-20} responding={1,-5} pump_answers={2}" -f $label, $proc.Responding, ($answered -ne [IntPtr]::Zero))
  return $ok
}

Write-Output "pid $($proc.Id)  mode=$Mode  layout at start = $(Layout-Now)"
if (-not (Alive "before")) { Write-Output "ALREADY HUNG - nothing to measure"; exit 1 }

for ($i = 1; $i -le $Rounds; $i++) {
  $before = Layout-Now
  if ($Mode -eq "winspace") {
    [LP]::keybd_event($VK_LWIN, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 60
    [LP]::keybd_event($VK_SPACE, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 60
    [LP]::keybd_event($VK_SPACE, 0, $KEYUP, [IntPtr]::Zero); Start-Sleep -Milliseconds 60
    [LP]::keybd_event($VK_LWIN, 0, $KEYUP, [IntPtr]::Zero)
  } else {
    $target = if ($before -like "*41E*") { "00000409" } else { "0000041E" }
    $hkl = [LP]::LoadKeyboardLayout($target, 1)
    [void][LP]::PostMessage($hwnd, 0x0050, [IntPtr]::Zero, $hkl)
  }
  Start-Sleep -Milliseconds 1200
  Write-Output "round $i : $before -> $(Layout-Now)"
  if (-not (Alive "after switch $i")) {
    Write-Output "HUNG after round $i - this is a release blocker, see HANDOFF 2.12"
    exit 2
  }
}
Write-Output "SURVIVED $Rounds switches"
