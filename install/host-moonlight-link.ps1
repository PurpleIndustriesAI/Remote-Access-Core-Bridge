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
     client learns the real fingerprint and pins it. The Remote Access Core (Client) viewer hides it.
  Step 2 edits Sunshine's apps.json and restarts Sunshine, which needs one UAC approval.
  The elevated step runs %ProgramFiles%\RemoteAccessCore\rac-sunshine-app.ps1, which
  install.ps1 -Firewall places there; this script never elevates itself.
  It refuses to restart while a stream is live unless -Force is given.

  -Remove undoes both steps.
#>
param(
  [string]$SunshineConfig = "$env:ProgramFiles\Sunshine\config",
  [switch]$Remove,
  [switch]$Force
)
$ErrorActionPreference = 'Stop'
$state = Join-Path $SunshineConfig 'sunshine_state.json'
$helper = Join-Path $env:ProgramFiles 'RemoteAccessCore\rac-sunshine-app.ps1'

# The elevated step must be a file that only administrators can change.
function Assert-AdminOnly([string]$path) {
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
    throw "Missing $path. Run install.ps1 -Firewall once from an Administrator PowerShell."
  }
  # Write bits only (Modify/FullControl also carry read bits). 0x50000000 = GENERIC_ALL | GENERIC_WRITE.
  $writeRights = [int64][Security.AccessControl.FileSystemRights]'WriteData, AppendData, WriteExtendedAttributes, WriteAttributes, Delete, ChangePermissions, TakeOwnership' -bor 0x50000000
  $trusted = @('S-1-5-18', 'S-1-5-32-544', 'S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464')
  foreach ($item in @((Get-Item -LiteralPath $path), (Get-Item -LiteralPath (Split-Path -Parent $path)))) {
    $acl = Get-Acl -LiteralPath $item.FullName
    $owner = (New-Object Security.Principal.NTAccount $acl.Owner).Translate([Security.Principal.SecurityIdentifier]).Value
    if ($trusted -notcontains $owner) { throw "Refusing to elevate: $($item.FullName) is not owned by an administrator." }
    foreach ($rule in $acl.Access) {
      if ($rule.AccessControlType -ne 'Allow' -or -not ([int64]$rule.FileSystemRights -band $writeRights)) { continue }
      $sid = $rule.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value
      if ($trusted -notcontains $sid -and $sid -ne 'S-1-3-0') {
        throw "Refusing to elevate: $($rule.IdentityReference) can change $($item.FullName)."
      }
    }
  }
}

$rac = Get-Command rac -ErrorAction SilentlyContinue
if (-not $rac) { $rac = Join-Path $env:LOCALAPPDATA 'Programs\RemoteAccessCrew\rac.exe' } else { $rac = $rac.Source }
if (-not (Test-Path -LiteralPath $rac)) { throw "rac.exe not found. Install Remote Access Core (Bridge) first (install.ps1)." }
Assert-AdminOnly $helper

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

$notice = 'Windows will ask for approval once. Requester: Remote Access Core (Bridge), host-moonlight-link.ps1. ' +
  "Action: run $helper to update Sunshine's app list and restart Sunshine. Purpose: Moonlight link for clipboard and files."
Write-Host $notice
$notify = Join-Path $env:USERPROFILE '.codex-crew\apps\notification-bridge\notify-approval.py'
$python = Join-Path $env:LOCALAPPDATA 'Programs\codexcrew-desktop\resources\backend-dist\codexcrew-backend\python.exe'
if ((Test-Path -LiteralPath $notify) -and (Test-Path -LiteralPath $python)) {
  & $python -I $notify --kind uac --requester 'Remote Access Core (Bridge)' --chat-title 'host-moonlight-link.ps1' `
    --session-id "rac-link-$PID" --action 'Update Sunshine app list and restart Sunshine' --target $helper `
    --purpose 'Moonlight link: clipboard and files for Sunshine-paired clients' --request-id "rac-link-$PID" | Out-Null
}
$argList = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$helper`"", '-Title', "`"$title`"")
$p = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') -ArgumentList $argList -Verb RunAs -WindowStyle Hidden -Wait -PassThru
if ($p.ExitCode -ne 0) { throw "The Sunshine update did not complete (exit $($p.ExitCode))." }
if ($Remove) { 'Moonlight link removed.' } else { "Moonlight link on. Published $title" }
