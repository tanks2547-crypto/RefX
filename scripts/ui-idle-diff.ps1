# Prove I-1 for one app state: did the frame counter move while nothing happened?
#
# docs/08 3.9 item 11 says I-1 must be checked in EVERY state the app can sit
# in, not just "launched and untouched".  Item 9 says pixel evidence must be
# diffed as a NUMBER, because eyes read repeated patterns wrong.  This script
# is the number.
#
# ---------------------------------------------------------------------------
# HOW TO USE
# ---------------------------------------------------------------------------
#   & .\scripts\ui-drive.ps1 -Steps @(
#       "attach",
#       "move|620|650",              # ! see PARK THE CURSOR below
#       "sleep|1500",
#       "shot|C:\shots\a.png",
#       "sleep|60000",
#       "shot|C:\shots\b.png")
#   & .\scripts\ui-idle-diff.ps1 -A C:\shots\a.png -B C:\shots\b.png
#
# It compares the bottom strip of the client area - the status bar, where the
# frame counter lives.  Identical strip = the counter did not move = the app
# really slept.  Anything else prints MOVED and names the two hashes.
#
# ---------------------------------------------------------------------------
# ! PARK THE CURSOR ON EMPTY CANVAS FIRST  (cost ~40 minutes on 28 Aug 2026)
# ---------------------------------------------------------------------------
# A measurement taken with the pointer left resting inside the window showed
# the counter climbing while nothing was touched, and it looked exactly like an
# I-1 regression in the code that had just been written.  It was not.  egui
# animates hover highlights and tooltips for whatever widget the pointer sits
# on, and asks for repaints while it does.  The same state, measured again with
# the pointer moved to empty canvas, produced ZERO frames in 60 seconds.
#
# So the pointer position is part of the state being measured.  Move it
# somewhere with no widget under it before the first shot, or the number you
# get is about egui's hover animation instead of about your code.
#
# ASCII only, same reason as ui-drive.ps1: PowerShell 5.1 reads a BOM-less file
# as ANSI and mangles anything else.
param(
  [Parameter(Mandatory = $true)][string]$A,
  [Parameter(Mandatory = $true)][string]$B,
  # Height of the status bar in client pixels.  Only widen this if the bar grows.
  [int]$StripHeight = 22)

Add-Type -AssemblyName System.Drawing

function Strip-Hash([string]$path) {
  if (-not (Test-Path $path)) { Write-Output "MISSING $path"; exit 2 }
  $img = [System.Drawing.Bitmap]::FromFile($path)
  try {
    if ($img.Height -le $StripHeight) {
      Write-Output "TOO SMALL $path ($($img.Width) x $($img.Height)) - a 0x0 shot means ui-drive lost the window; throw that evidence away"
      exit 2
    }
    $rect = New-Object System.Drawing.Rectangle 0, ($img.Height - $StripHeight), $img.Width, $StripHeight
    $crop = $img.Clone($rect, $img.PixelFormat)
    try {
      $ms = New-Object System.IO.MemoryStream
      $crop.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
      $bytes = $ms.ToArray()
      $ms.Dispose()
    }
    finally { $crop.Dispose() }
  }
  finally { $img.Dispose() }
  $sha = [System.Security.Cryptography.SHA256]::Create()
  (($sha.ComputeHash($bytes) | ForEach-Object { $_.ToString("x2") }) -join "").Substring(0, 16)
}

$ha = Strip-Hash $A
$hb = Strip-Hash $B
"{0}  {1}" -f $ha, (Split-Path $A -Leaf)
"{0}  {1}" -f $hb, (Split-Path $B -Leaf)
if ($ha -eq $hb) {
  "IDLE OK - the status bar is identical, so the frame counter did not move"
  exit 0
}
"MOVED - the status bar changed while nothing was touched."
"        Check the pointer was parked on empty canvas (see the header), then"
"        crop both strips and read the Frames number to see how far it ran."
exit 1
