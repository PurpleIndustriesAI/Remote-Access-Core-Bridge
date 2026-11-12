<#
.SYNOPSIS
  Make Windows name Sunshine's tray toasts "RAC Host" instead of "Sunshine".

.DESCRIPTION
  Sunshine sets no AppUserModelID, so Windows names its toasts after the Start Menu
  shortcut that targets sunshine.exe. Two shortcuts target it today: the installer's
  LizardByte\Sunshine\Sunshine.lnk and our host shortcut. This script:
  1. Moves LizardByte\Sunshine\Sunshine.lnk into config\rebrand-backup (kept, not deleted).
  2. Renames the host shortcut to "RAC Host.lnk", creating it if missing.
  3. Sets the SunshineService display name to "RAC Host".
  It never restarts Sunshine, so a live stream is not dropped. The new name shows after
  Sunshine's next start. Run in an Administrator PowerShell. -Remove restores the backup.
#>
param(
  [string]$Name = 'RAC Host',
  [string]$SunshineDir = "$env:ProgramFiles\Sunshine",
  [switch]$Remove
)
$ErrorActionPreference = 'Stop'
$programs = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs'
$vendorLnk = Join-Path $programs 'LizardByte\Sunshine\Sunshine.lnk'
$backupDir = Join-Path $SunshineDir 'config\rebrand-backup'
$backupLnk = Join-Path $backupDir 'Sunshine.lnk.rac-host'
$hostLnk = Join-Path $programs "$Name.lnk"
$oldHostNames = 'Remote Access Crew (Host)', 'Remote Access Core (Host)'
$exe = Join-Path $SunshineDir 'sunshine.exe'

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
  [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) { throw 'Run this in an Administrator PowerShell.' }
if (-not (Test-Path $exe)) { throw "Sunshine not found at $exe" }

if ($Remove) {
  if (Test-Path $backupLnk) { Move-Item -Force $backupLnk $vendorLnk; "Restored $vendorLnk" }
  if (Test-Path $hostLnk) { Rename-Item $hostLnk "$($oldHostNames[0]).lnk"; "Renamed host shortcut back" }
  sc.exe config SunshineService DisplayName= $oldHostNames[0] | Out-Null
  return
}

New-Item -ItemType Directory -Force $backupDir | Out-Null
if (Test-Path $vendorLnk) { Move-Item -Force $vendorLnk $backupLnk; "Moved vendor shortcut to $backupLnk" }

if (-not (Test-Path $hostLnk)) {
  $old = $oldHostNames | ForEach-Object { Join-Path $programs "$_.lnk" } | Where-Object { Test-Path $_ } | Select-Object -First 1
  if ($old) { Rename-Item $old "$Name.lnk" }
  else {
    $s = (New-Object -ComObject WScript.Shell).CreateShortcut($hostLnk)
    $s.TargetPath = $exe; $s.Arguments = '--shortcut'; $s.IconLocation = "$exe,0"; $s.Save()
  }
}
"Host shortcut: $hostLnk"

sc.exe config SunshineService DisplayName= $Name | Out-Null
"Service display name: $((Get-Service SunshineService).DisplayName)"

# Self-check: exactly one Start Menu shortcut must target sunshine.exe, and it must carry the new name.
$shell = New-Object -ComObject WScript.Shell
$targets = Get-ChildItem $programs, (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs') -Recurse -Filter *.lnk |
  Where-Object { $shell.CreateShortcut($_.FullName).TargetPath -ieq $exe }
if (@($targets).Count -ne 1 -or $targets[0].BaseName -ne $Name) {
  throw "Expected one shortcut named '$Name' targeting sunshine.exe, found: $($targets.FullName -join ', ')"
}
"OK: toasts will read '$Name' after Sunshine next starts."
