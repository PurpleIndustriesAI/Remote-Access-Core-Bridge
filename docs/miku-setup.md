# MIKU setup for Remote Access Core (Bridge) 0.4.0

For the Claude agent on MIKU. Goal: text, screenshots and other images, and files copy and paste
between MIKU and the host PURPDINO-LAPTOP in both directions.

## Current state (2026-10-24)

- Host PURPDINO-LAPTOP runs bridge 0.4.0, port 48765, firewall rule on. LAN 192.168.0.159.
  Host identity starts `50E7-F036-84B9`.
- The host has MIKU paired (`2620-04AF-71D3`) with clipboard and files allowed.
- From the host, MIKU (192.168.0.161) answers ping, but TCP 48765 is closed. Every host send
  to MIKU times out. Either MIKU's bridge is not running or MIKU's firewall blocks the port.

## Steps

Run in a normal (not Administrator) terminal unless a step says otherwise.

1. Pull this branch (`feat/rac-access-requests`) and build:
   `cargo build --release`. Check `target/release/rac version` prints `rac 0.4.0`.
2. Install. Windows: `powershell -ExecutionPolicy Bypass -File .\install\install.ps1`.
   Linux: `./install/install.sh`. This replaces the binaries and keeps the pairing in
   `%APPDATA%\RemoteAccessCrew` or `~/.config/remote-access-crew`.
3. Open the firewall once. Windows, elevated PowerShell:
   `powershell -ExecutionPolicy Bypass -File .\install\install.ps1 -Firewall`.
   Linux with ufw: run the `ufw` commands `install.sh` prints. Windows asks for UAC
   approval. Tell the owner first which chat is asking and why.
4. Confirm the bridge is up: `rac status` shows `(v0.4.0)` and the peer PURPDINO-LAPTOP.
   If no peer is listed, pair again: the owner shows a code on the host Crew page
   (Remote Access Core, Show a pairing code), then run `rac pair 192.168.0.159 <code>`.
   Pairing codes have no letters I or O; read `1` and `0` as digits.
5. Ask for access: `rac request-access PURPDINO-LAPTOP`. This allows clipboard and files on
   MIKU's side and sends a request to the host. The host is already approved for MIKU, so it
   replies "already approved". If the host owner had not approved, they would get a toast and
   approve on the host page or with `rac approve MIKU`.
6. Check both directions: `rac ping PURPDINO-LAPTOP`, then
   `rac send --text "MIKU test"`. Ask the owner to copy text on the host and paste on MIKU.

## Acceptance checks

Report each one as passed or failed in the Moonlight-Crew coordination PR. Do not post
pairing codes there.

- Text copied on MIKU pastes on the host, and the reverse.
- A screenshot (Print Screen or Win+Shift+S on Windows, Spectacle on Linux) copied on one
  machine pastes as an image on the other.
- Files copied in Explorer or Dolphin arrive in `Downloads/Remote Access Crew` on the other
  machine, and Paste in the file manager works.
- `rac status` on both machines shows `clipboard on  files on` for the other machine.

## If it fails

- `timed out`: the other bridge is not running, or a firewall blocks 48765.
- `local client approval required`: this machine has not allowed that device. Run
  `rac allow <peer> clipboard on` (and `files on`).
- `not accepting access requests`: the other owner turned requests off. They can allow
  the device with its switches, or run `rac access-requests on`.
- `bad control token`: run `rac` as the same user that runs the bridge, not as Administrator.
