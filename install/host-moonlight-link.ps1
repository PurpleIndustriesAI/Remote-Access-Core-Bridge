<#
.SYNOPSIS
  Host setup for Moonlight link: clients that Sunshine already paired get clipboard and
  file sharing with no pairing code.

.DESCRIPTION
  Run in a normal (not Administrator) PowerShell as the user who runs the bridge.
  1. Turns on Moonlight link in the bridge (reads only public client certificates from
     sunshine_state.json).
  2. Publishes this bridge's certificate fingerprint as a Sunshine app titled "RAC:<fp>".
     Moonlight fetches the app list over HTTPS pinned to Sunshine's certificate, so the
     client learns the real fingerprint and pins it. The Remote Access Crew client hides it.
  Step 2 edits Sunshine's apps.json and restarts Sunshine, which needs one UAC approval.
  It refuses to restart while a stream is live unless -Force is given.

  -Remove undoes both steps.
#>
param(
  [string]$SunshineConfig = "$env:ProgramFiles\Sunshine\config",
  [switch]$Remove,
  [switch]$Force,
  # Internal: the elevated half receives the title to publish ("" removes it).
  [string]$ElevatedTitle,
  [switch]$Elevated
)
$ErrorActionPreference = 'Stop'
$apps = Join-Path $SunshineConfig 'apps.json'
$state = Join-Path $SunshineConfig 'sunshine_state.json'

function Set-SunshineApp([string]$title) {
  $json = Get-Content -Raw -LiteralPath $apps | ConvertFrom-Json
  $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
  Copy-Item -LiteralPath $apps -Destination "$apps.before-rac-link-$stamp"
  $kept = @($json.apps | Where-Object { -not ($_.name -like 'RAC:*') })
  if ($title) {
    if ($title -notmatch '^RAC:[0-9a-f]{64}$') { throw "Refusing to publish an unexpected title: $title" }
    $kept += [pscustomobject]@{ name = $title; 'image-path' = 'desktop.png' }
  }
  $json.apps = $kept
  $tmp = "$apps.tmp"
  $json | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $tmp -Encoding UTF8
  Move-Item -LiteralPath $tmp -Destination $apps -Force
  Restart-Service -Name SunshineService -ErrorAction Stop
  "Sunshine app list updated and Sunshine restarted (backup: apps.json.before-rac-link-$stamp)."
}

if ($Elevated) {
  Set-SunshineApp $ElevatedTitle
  return
}

$rac = Get-Command rac -ErrorAction SilentlyContinue
if (-not $rac) { $rac = Join-Path $env:LOCALAPPDATA 'Programs\RemoteAccessCrew\rac.exe' } else { $rac = $rac.Source }
if (-not (Test-Path -LiteralPath $rac)) { throw "rac.exe not found. Install Remote Access Crew first (install.ps1)." }

$log = Join-Path $SunshineConfig 'sunshine.log'
if (-not $Force -and (Test-Path -LiteralPath $log)) {
  $last = Select-String -LiteralPath $log -Pattern 'CLIENT CONNECTED|CLIENT DISCONNECTED' | Select-Object -Last 1
  if ($last -and $last.Line -match 'CLIENT CONNECTED') {
    throw 'A stream is live. Run this again after it ends, or pass -Force (the stream will drop).'
  }
}

if ($Remove) {
  & $rac moonlight-link off
  $title = ''
} else {
  & $rac moonlight-link on $state
  if ($LASTEXITCODE -ne 0) { throw 'The bridge refused to turn on Moonlight link.' }
  $title = (& $rac host-app-title).Trim()
  if ($title -notmatch '^RAC:[0-9a-f]{64}$') { throw "Unexpected host identity: $title" }
}

Write-Host 'Windows will ask for approval once: Remote Access Crew (Host) needs to update Sunshine''s app list and restart Sunshine.'
$argList = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"", '-Elevated', '-SunshineConfig', "`"$SunshineConfig`"", '-ElevatedTitle', "`"$title`"")
$p = Start-Process -FilePath 'powershell.exe' -ArgumentList $argList -Verb RunAs -WindowStyle Hidden -Wait -PassThru
if ($p.ExitCode -ne 0) { throw "The Sunshine update did not complete (exit $($p.ExitCode))." }
if ($Remove) { 'Moonlight link removed.' } else { "Moonlight link on. Published $title" }
