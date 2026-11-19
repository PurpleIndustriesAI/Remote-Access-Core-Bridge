# Remote Access Core (RAC) Windows Installer Helper
# Installs Remote Access Core (Host) from the verified USB MSI, applies full
# Remote Access Core branding and logo, and configures Private LAN firewall rules.
$ErrorActionPreference = 'Stop'

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$HostDir   = Join-Path $ScriptDir 'host'
$BrandDir  = Join-Path $ScriptDir 'branding'
$Msi       = Join-Path $HostDir 'rac-host-windows-amd64.msi'
$Sums      = Join-Path $HostDir 'SHA256SUMS'
$HostIco   = Join-Path $BrandDir 'rac-host.ico'

Write-Output "=== Remote Access Core (RAC) Windows Setup ==="

if (-not (Test-Path $Msi)) {
    throw "RAC Host MSI not found at $Msi"
}

$expected = ((Get-Content $Sums | Select-String 'rac-host-windows-amd64.msi').Line -split '\s+')[0].ToLower()
$actual   = (Get-FileHash $Msi -Algorithm SHA256).Hash.ToLower()
if ($actual -ne $expected) {
    throw "SHA256 mismatch for RAC Host MSI! Expected $expected got $actual"
}
Write-Output "[OK] Verified RAC Host MSI SHA256: $actual"

Write-Output "Installing Remote Access Core (Host) silently..."
$proc = Start-Process -FilePath 'msiexec.exe' -ArgumentList "/i `"$Msi`" /qn /norestart" -Wait -PassThru -WindowStyle Hidden
if ($proc.ExitCode -ne 0 -and $proc.ExitCode -ne 3010) {
    throw "RAC Host MSI install failed with exit code $($proc.ExitCode)"
}
Write-Output "[OK] Remote Access Core (Host) installed."

$installRoot = Join-Path $env:ProgramFiles 'Sunshine'
$hostExe     = Join-Path $installRoot 'sunshine.exe'
$programs    = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs'
$vendorLnk   = Join-Path $programs 'LizardByte\Sunshine\Sunshine.lnk'
$backupDir   = Join-Path $installRoot 'config\rebrand-backup'
$racLnk      = Join-Path $programs 'Remote Access Core (Host).lnk'
$iconDst     = Join-Path $installRoot 'assets\rac-host.ico'

if (Test-Path $hostExe) {
    New-Item -ItemType Directory -Force -Path $backupDir | Out-Null
    if (Test-Path $vendorLnk) {
        Move-Item -Force $vendorLnk (Join-Path $backupDir 'vendor-host.lnk.bak')
    }
    if (Test-Path $HostIco) {
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $iconDst) | Out-Null
        Copy-Item -Force $HostIco $iconDst
    }
    $shell = New-Object -ComObject WScript.Shell
    $sc = $shell.CreateShortcut($racLnk)
    $sc.TargetPath = $hostExe
    $sc.Arguments  = '--shortcut'
    if (Test-Path $iconDst) { $sc.IconLocation = "$iconDst,0" } else { $sc.IconLocation = "$hostExe,0" }
    $sc.Save()

    sc.exe config SunshineService DisplayName= "Remote Access Core (Host)" | Out-Null
    Write-Output "[OK] Rebranded Start Menu shortcut and service to Remote Access Core (Host)."
}

Write-Output "Ensuring Windows Firewall allows Remote Access Core ports on Private (LAN) profile only..."
New-NetFirewallRule -DisplayName 'Remote Access Core Host TCP (LAN)' -Direction Inbound -Action Allow -Protocol TCP -LocalPort 47984,47989,47990,48010 -Profile Private -ErrorAction SilentlyContinue | Out-Null
New-NetFirewallRule -DisplayName 'Remote Access Core Host UDP (LAN)' -Direction Inbound -Action Allow -Protocol UDP -LocalPort 47998,47999,48000,48002,48010 -Profile Private -ErrorAction SilentlyContinue | Out-Null
New-NetFirewallRule -DisplayName 'Remote Access Core Bridge TCP (LAN)' -Direction Inbound -Action Allow -Protocol TCP -LocalPort 48765 -Profile Private -ErrorAction SilentlyContinue | Out-Null

Write-Output "Done. Open https://localhost:47990 to finish Remote Access Core (Host) pairing."
