# Who owns the WM_INPUTLANGCHANGEREQUEST hang: us, or winit?
#
# ---------------------------------------------------------------------------
# WHAT THIS ANSWERS
# ---------------------------------------------------------------------------
# RefX stops responding every time this message is posted at its window
# (HANDOFF 2.42k, re-confirmed 13 Sep 2026).  All 32 threads sit in Wait, one
# of them inside a cross-process call, so it is neither a spin nor a deadlock
# on a lock of ours.  That leaves two possibilities, and they are opposites:
#
#   bare winit window hangs too  -> not our code; it lives under winit/the OS
#   bare winit window survives   -> ours; something above winit causes it
#
# So this script drives BOTH targets through the identical sequence and prints
# the two answers side by side.  Anything that differs between the two runs
# other than the target is a reason to distrust the result.
#
# ---------------------------------------------------------------------------
# HOW TO USE
# ---------------------------------------------------------------------------
#   cargo build -p refx-platform --example bare_winit
#   Start-Process target\debug\examples\bare_winit.exe
#   & .\scripts\lang-switch-bare.ps1 -ProcessName bare_winit
#
#   ... and for the comparison run, with RefX already open:
#   & .\scripts\lang-switch-bare.ps1 -ProcessName refx
#
# ---------------------------------------------------------------------------
# ! WHY THIS DOES NOT REUSE ui-layout-switch.ps1
# ---------------------------------------------------------------------------
# That script's Alive check reported SURVIVED on a window that was already
# hung (HANDOFF 2.42k) and the fix is still queued.  Evidence that decides who
# owns a bug must not come from a gate with a known false-negative, so this
# script asks the question its own way and prints BOTH signals every round
# instead of collapsing them into one verdict:
#
#   Responding    - what Task Manager shows the user
#   pump_answers  - whether the window actually drained a message (WM_NULL
#                   with SMTO_ABORTIFHUNG); this is the one that cannot be
#                   faked by a window that merely keeps presenting its last
#                   frame
#
# ASCII only: PowerShell 5.1 reads a BOM-less file as ANSI.
param([string]$ProcessName = "bare_winit", [int]$Rounds = 4, [int]$SettleMs = 1500)

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class BL {
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
  [DllImport("user32.dll")] public static extern IntPtr GetKeyboardLayout(uint thread);
  [DllImport("user32.dll")] public static extern IntPtr LoadKeyboardLayout(string id, uint flags);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr res);
}
"@

$WM_INPUTLANGCHANGEREQUEST = 0x0050
$WM_NULL                   = 0x0000
$SMTO_ABORTIFHUNG          = 0x0002

$proc = Get-Process $ProcessName -ErrorAction Stop
if ($proc.Count -gt 1) { Write-Output "more than one $ProcessName is running - close the extras"; exit 1 }
$hwnd = $proc.MainWindowHandle
if ($hwnd -eq [IntPtr]::Zero) { Write-Output "$ProcessName has no main window yet"; exit 1 }

function Layout-Now {
  $t = [BL]::GetWindowThreadProcessId($hwnd, [IntPtr]::Zero)
  return "0x{0:X}" -f ([BL]::GetKeyboardLayout($t)).ToInt64()
}

# Both signals, always printed.  A window that keeps painting its last frame
# looks alive in a screenshot and in Responding for a while; the pump is the
# part that tells the truth.
# ! Write-Host, NOT Write-Output.  A PowerShell function returns EVERYTHING it
#   writes to the pipeline, so "Write-Output ...; return $ok" hands the caller
#   an ARRAY of (string, bool).  `if (-not (Probe ...))` then tests an array,
#   which is truthy whenever it has elements -- so the check can never fail and
#   the script always prints SURVIVED.  That is exactly the false negative
#   ui-layout-switch.ps1 has had since 6 Sep 2026 (HANDOFF 2.42k / queue 3nj),
#   and the reason its log lines never appeared either.
function Probe($label) {
  $proc.Refresh()
  $res = [IntPtr]::Zero
  $answered = [BL]::SendMessageTimeout($hwnd, $WM_NULL, [IntPtr]::Zero, [IntPtr]::Zero, $SMTO_ABORTIFHUNG, 3000, [ref]$res)
  $pump = ($answered -ne [IntPtr]::Zero)
  Write-Host ("  {0,-16} responding={1,-5} pump_answers={2,-5} layout={3}" -f $label, $proc.Responding, $pump, (Layout-Now))
  return ($proc.Responding -and $pump)
}

Write-Output "target=$ProcessName pid=$($proc.Id) hwnd=$hwnd rounds=$Rounds settle=${SettleMs}ms"
if (-not (Probe "before")) { Write-Output "RESULT: ALREADY HUNG before any message - nothing to measure"; exit 1 }

for ($i = 1; $i -le $Rounds; $i++) {
  $before = Layout-Now
  $target = if ($before -like "*41E*") { "00000409" } else { "0000041E" }
  $hkl = [BL]::LoadKeyboardLayout($target, 1)
  [void][BL]::PostMessage($hwnd, $WM_INPUTLANGCHANGEREQUEST, [IntPtr]::Zero, $hkl)
  Start-Sleep -Milliseconds $SettleMs
  if (-not (Probe "after round $i")) {
    Write-Output "RESULT: HUNG after round $i (posted $i message(s), layout was $before)"
    exit 2
  }
}
Write-Output "RESULT: SURVIVED $Rounds posted messages"
exit 0
