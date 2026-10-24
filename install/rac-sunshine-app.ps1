<#
.SYNOPSIS
  Elevated half of host-moonlight-link.ps1: publishes or removes the "RAC:<fp>" Sunshine app.

.DESCRIPTION
  install.ps1 -Firewall (run by the owner as administrator) copies this file to
  %ProgramFiles%\RemoteAccessCore, which only administrators can change. host-moonlight-link.ps1
  elevates this installed copy, never its own user-writable copy, so a local process that edits
  user files cannot get its code run as administrator on the next approval.
  The Sunshine config path is fixed here, not taken from the caller.
#>
param([string]$Title = '')
$ErrorActionPreference = 'Stop'
if ($Title -and $Title -notmatch '^RAC:[0-9a-f]{64}$') { throw "Refusing to publish an unexpected title: $Title" }
$config = Join-Path $env:ProgramFiles 'Sunshine\config'
$apps = Join-Path $config 'apps.json'
$json = Get-Content -Raw -LiteralPath $apps | ConvertFrom-Json
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
Copy-Item -LiteralPath $apps -Destination "$apps.before-rac-link-$stamp"
$kept = @($json.apps | Where-Object { -not ($_.name -like 'RAC:*') })
if ($Title) { $kept += [pscustomobject]@{ name = $Title; 'image-path' = 'desktop.png' } }
$json.apps = $kept
$tmp = "$apps.tmp"
$json | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $tmp -Encoding UTF8
Move-Item -LiteralPath $tmp -Destination $apps -Force
Restart-Service -Name SunshineService -ErrorAction Stop
"Sunshine app list updated and Sunshine restarted (backup: apps.json.before-rac-link-$stamp)."
