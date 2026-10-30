# MIKU setup for Remote Access Core 0.5.0

This guide is for Codex or Kimi on MIKU. The iPhone is physically connected to MIKU by USB. PURPDINO-LAPTOP is the requesting computer.

## Current verified state

The host runs RAC 0.5.0. Its identity starts `50E7-F036-84B9`. MIKU is paired with clipboard and file access.

A host ping to MIKU succeeded on October 29, 2026. The device inventory request failed because MIKU runs bridge 0.4.3. That version does not support `device_inventory`.

The host delivered `RAC050.zip` through RAC. A delivery receipt does not prove the destination file state. Verify the actual file on MIKU before installation.

A fresh owner requested inventory check on October 30, 2026 returned the same unsupported request error from bridge 0.4.3. The suite layout request remains unanswered. Kimi on MIKU should now complete sections 1 and 2, report the actual package evidence and active paths, and continue only through the verified suite update route. No new package or duplicate host probe is needed.

## Security boundaries

Run commands as the signed in Windows user. Do not use an Administrator terminal for the update.

Keep the existing identity, certificate, pairing records, clipboard permission and file permission. Do not create another identity.

Do not use `install.ps1`. MIKU uses the Remote Access Suite. The standalone installer can create a second bridge with a second identity.

Do not enable USB peripheral sharing. Do not install drivers. Do not change firewall or pairing state. Do not resume paused remote tasks. Do not repeat the previously canceled USB elevation.

Inventory returns bounded USB metadata only. Inventory does not redirect a USB device and does not control the iPhone.

## 1. Verify the delivered package

Verify the normal RAC receive folder:

```powershell
$pkg = Join-Path $env:USERPROFILE 'Downloads\Remote Access Crew\RAC050.zip'
Get-Item -LiteralPath $pkg
(Get-FileHash -LiteralPath $pkg -Algorithm SHA256).Hash.ToLower()
```

Expected values:

```text
Size: 5886936 bytes
SHA256: 65c0956afdb472df3fb30951c14e233b33936e090ba213cfdab6d8411add1540
Source revision: 8b65ed342c8c15e34ae8572ec60b15064a926a8e
```

Stop if the file is absent or either value differs. Report the actual path, size and hash. Do not rename another archive as a substitute.

## 2. Identify the active suite binaries

Read the active process path before replacing anything:

```powershell
Get-CimInstance Win32_Process -Filter "Name='remote-access-crew.exe'" |
  Select-Object ProcessId, ExecutablePath, CommandLine
```

Read the Crew app binaries when that path manages the active bridge:

```powershell
$app = Join-Path $env:USERPROFILE '.codex-crew\apps\remote-access-crew'
Get-Item -LiteralPath (Join-Path $app 'bin\remote-access-crew.exe')
Get-Item -LiteralPath (Join-Path $app 'bin\rac.exe')
```

Preserve `%USERPROFILE%\.remote-access-crew`. That directory contains `state.json`, `identity.cert.der`, `identity.key.der` and `control.token`. Do not publish or copy those files into Git or a release archive.

Stop if the active suite update route is unclear. Report the process path and Crew app path. Do not guess a destination.

## 3. Stage and verify the update

Use a new local staging directory after the package hash matches:

```powershell
$stage = Join-Path $env:TEMP 'RAC050-verified'
Expand-Archive -LiteralPath $pkg -DestinationPath $stage -Force
Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $stage 'remote-access-crew.exe')
Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $stage 'rac.exe')
```

Compare both executable hashes with `SHA256SUMS.txt` in the archive. Stop on any mismatch.

## 4. Preserve rollback and update the suite

Use the existing Remote Access Suite update route. Back up the two active executables before replacement. Preserve all state and identity files in place.

Stop only the identified RAC bridge process during replacement. Do not stop Crew, other chats, remote desktop sessions or unrelated processes. Replace only `remote-access-crew.exe` and `rac.exe` in the verified active suite locations. Update the Crew app files through the existing app update route when that route manages them.

Start the same suite bridge after replacement. Do not create new shortcuts or a second startup entry.

## 5. Verify the running update

Run these tests with the same Windows user:

```powershell
rac status
rac ping PURPDINO-LAPTOP
```

Verify that `rac status` reports version 0.5.0 and retains PURPDINO-LAPTOP. Compare both running executable hashes with the staged release hashes.

If the update fails, stop the identified bridge, restore only its two backed up executables, and start the same suite bridge. Report the error and rollback result.

## 6. Approve bounded inventory on MIKU

Approval is local to MIKU. A remote request cannot grant it.

Use RAC Settings, Devices and hardware. Approve only `device_inventory` for host fingerprint `50E7-F036-84B9-6821-4F2F-5242`.

The equivalent local command is:

```powershell
rac allow PURPDINO-LAPTOP device_inventory on
```

Leave clipboard and file permissions unchanged. Do not grant reciprocal hardware access automatically.

## 7. Test and report

Ask chat 178 to run this command once on the host:

```powershell
rac devices MIKU
```

Return only the Apple USB count and device type. Do not publish serial numbers, personal device names, device paths or device contents.

Record the responding agent name and actual UTC observation times in the existing Remote Access Crew coordination discussion. Report every failure exactly.

## Remaining acceptance

A successful inventory does not establish iPhone control. Apple USB trust, a reviewed iPhone control route and live acceptance remain separate jobs. Gallery, calls, iMessage behavior, screen control and AirDrop require their own verified implementations and tests.
