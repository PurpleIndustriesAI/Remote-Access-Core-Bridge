<#
.SYNOPSIS
  Install Remote Access Core (Bridge) for the current Windows user.

.DESCRIPTION
  Copies rac.exe and remote-access-crew.exe to %LOCALAPPDATA%\Programs\RemoteAccessCrew,
  adds Start Menu and Startup shortcuts with the app icon, installs the Crew app when Crew is
  present, and starts the bridge hidden. Run with -Firewall from an elevated PowerShell to add
  an inbound rule limited to your local subnet and Tailscale (100.64.0.0/10).

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File .\install.ps1
  powershell -ExecutionPolicy Bypass -File .\install.ps1 -Firewall   # as administrator
#>
param(
  [string]$Source = $PSScriptRoot,
  [switch]$Firewall,
  [switch]$NoCrewApp,
  [switch]$NoStart
)
$ErrorActionPreference = 'Stop'
$Name = 'Remote Access Core (Bridge)'
# Name used before the 2026-10-24 rename; its rule and shortcuts are replaced, not duplicated.
$LegacyName = 'Remote Access Crew'
$Dest = Join-Path $env:LOCALAPPDATA 'Programs\RemoteAccessCrew'
$Bridge = Join-Path $Dest 'remote-access-crew.exe'

function Find-Binary([string]$file) {
  foreach ($p in @((Join-Path $Source $file), (Join-Path $Source "bin\$file"), (Join-Path $Source "..\target\release\$file"))) {
    if (Test-Path $p) { return (Resolve-Path $p).Path }
  }
  throw "Cannot find $file next to this installer."
}

if ($Firewall) {
  $admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
  if (-not $admin) { throw 'The -Firewall step needs an elevated PowerShell.' }
  if (-not (Test-Path $Bridge)) { throw "Install first (without -Firewall), then add the rule." }
  Get-NetFirewallRule -DisplayName $Name, $LegacyName -ErrorAction SilentlyContinue | Remove-NetFirewallRule
  # Windows may have auto-created broad rules for this program on first listen; replace them.
  Get-NetFirewallApplicationFilter -Program $Bridge -ErrorAction SilentlyContinue | Get-NetFirewallRule | Remove-NetFirewallRule
  New-NetFirewallRule -DisplayName $Name -Description 'Remote Access Core (Bridge) peers on the local subnet and Tailscale only.' `
    -Direction Inbound -Action Allow -Program $Bridge -Protocol TCP -LocalPort 48765 `
    -RemoteAddress @('LocalSubnet', '100.64.0.0/10', 'fd7a:115c:a1e0::/48') -Profile Any | Out-Null
  Write-Host "Firewall rule added: TCP 48765 from LocalSubnet and Tailscale only."
  return
}

$rac = Find-Binary 'rac.exe'
$bridgeSrc = Find-Binary 'remote-access-crew.exe'

# Stop a running bridge so its file can be replaced. Only this app's process is touched.
Get-Process -Name 'remote-access-crew' -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $Bridge } | Stop-Process -Force
Start-Sleep -Milliseconds 300
New-Item -ItemType Directory -Force -Path $Dest | Out-Null
Copy-Item $rac, $bridgeSrc -Destination $Dest -Force
$icon = @((Join-Path $Source 'remote-access-crew.ico'), (Join-Path $Source '..\assets\remote-access-crew.ico')) | Where-Object { Test-Path $_ } | Select-Object -First 1
if ($icon) { Copy-Item $icon -Destination $Dest -Force }
$hostLink = @((Join-Path $Source 'host-moonlight-link.ps1'), (Join-Path $PSScriptRoot 'host-moonlight-link.ps1')) | Where-Object { Test-Path $_ } | Select-Object -First 1
if ($hostLink) { Copy-Item $hostLink -Destination $Dest -Force }

# Put rac on the user PATH (once).
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $Dest) {
  [Environment]::SetEnvironmentVariable('Path', (($userPath.TrimEnd(';'), $Dest) -join ';').TrimStart(';'), 'User')
}

# Start Menu and Startup shortcuts with the app name and icon.
$shell = New-Object -ComObject WScript.Shell
$programs = [Environment]::GetFolderPath('Programs')
$startup = [Environment]::GetFolderPath('Startup')
foreach ($dir in @($programs, $startup)) {
  # Drop a legacy-named shortcut only when it still launches this bridge; another app may own the name.
  $legacy = Join-Path $dir "$LegacyName.lnk"
  if ((Test-Path $legacy) -and $shell.CreateShortcut($legacy).TargetPath -eq $Bridge) { Remove-Item $legacy }
  $lnk = $shell.CreateShortcut((Join-Path $dir "$Name.lnk"))
  $lnk.TargetPath = $Bridge
  $lnk.WorkingDirectory = $Dest
  $lnk.Description = 'Copy on one machine, paste on the other (LAN and Tailscale).'
  $lnk.IconLocation = "$Bridge,0"
  $lnk.WindowStyle = 7
  $lnk.Save()
}

# Crew app, when Crew is installed on this machine.
$crewApps = Join-Path $env:USERPROFILE '.codex-crew\apps'
$appSrc = @((Join-Path $Source 'crew-app'), (Join-Path $Source '..\crew-app')) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $NoCrewApp -and (Test-Path $crewApps) -and $appSrc) {
  $stage = Join-Path $env:TEMP "remote-access-crew-app-$PID"
  Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
  Copy-Item $appSrc -Destination $stage -Recurse
  New-Item -ItemType Directory -Force -Path (Join-Path $stage 'bin') | Out-Null
  Copy-Item (Join-Path $Dest 'remote-access-crew.exe'), (Join-Path $Dest 'rac.exe') -Destination (Join-Path $stage 'bin') -Force
  if ($hostLink) { Copy-Item $hostLink -Destination (Join-Path $stage 'bin') -Force }
  $cc = Get-Command codexcrew -ErrorAction SilentlyContinue
  if ($cc) {
    & $cc.Source app install $stage
    if ($LASTEXITCODE -eq 0) {
      Write-Host 'Crew app installed. Enable it from Crew > Apps (or: codexcrew app enable remote-access-crew --live).'
    } else {
      Write-Host "Crew did not install the app (see the message above). The bridge works without it. App files: $stage"
    }
  } else {
    Write-Host "Crew CLI not found; install the app from $stage in Crew > Apps."
  }
}

if (-not $NoStart) {
  Start-Process -FilePath $Bridge -WorkingDirectory $Dest -WindowStyle Hidden
  Start-Sleep -Seconds 1
  & (Join-Path $Dest 'rac.exe') status
}
Write-Host ''
Write-Host "Installed to $Dest. Pair with: rac pair-code (on one machine) and rac pair <address> <code> (on the other)."
Write-Host 'For other machines to connect, run once as administrator: install.ps1 -Firewall'
