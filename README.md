<p align="center"><img src="assets/remote-access-crew.png" width="96" alt="Remote Access Core (Bridge) logo"></p>

# Remote Access Core (Bridge)

Copy on one machine, paste on the other. Remote Access Core (Bridge) syncs **text, images, and files**
in both directions between your own Windows and Linux machines over your **LAN or Tailscale**,
with mutual TLS and certificate pinning. It also ships as a Crew app.

This app was called Remote Access Crew until 2026-10-24. Only shown names changed:
binaries, `~/.remote-access-crew`, and the `Downloads/Remote Access Crew` folder keep their names,
so existing pairings and received files stay where they are.

## What it does

| You copy... | The other machine gets... |
|---|---|
| Text | The same text on its clipboard |
| An image (screenshot, "Copy image") | A real image on its clipboard (CF_DIB on Windows, `image/png` on Linux) |
| Files or folders (Explorer/Dolphin "Copy") | The files saved to `Downloads/Remote Access Crew`, verified by SHA-256, and placed on its clipboard as files so Paste works in the file manager |

Sync is automatic for every paired device with **Sync** on. You can also send on demand from
the Crew page or with `rac send`.

## Install

Download the release for your platform and check it against `SHA256SUMS.txt`.

**Windows**

```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
# Once, as administrator, so other machines can connect (LAN subnet and Tailscale only):
powershell -ExecutionPolicy Bypass -File .\install.ps1 -Firewall
```

This adds **Remote Access Core (Bridge)** to the Start Menu and to Startup, with its icon.

**Linux**

```sh
./install.sh
```

This installs to `~/.local/bin`, adds a launcher and an autostart entry, and prints `ufw`
commands if a firewall is active. Clipboard support: X11, KDE Plasma and wlroots Wayland
(data-control), and GNOME Wayland through XWayland.

## Pair two machines

1. On machine A: Crew page, **Show a pairing code**, or `rac pair-code`.
2. On machine B: **Enter a code** with A's LAN or Tailscale address, or
   `rac pair 192.168.1.20 ABCD-EFGH-JKLM-NPQR`.

The code is 80 random bits, valid for 5 minutes, single use, and locked after 5 wrong tries.

## Commands

```
rac status                      this machine, peers, recent transfers
rac pair-code / rac pair ADDR CODE
rac send [--to PEER] [--text TEXT | FILE...]   no input sends the current clipboard
rac ping PEER
rac sync on|off [--peer PEER]
rac unpair PEER
```

## Network helper

Streaming quality depends on the network more than anything else. The **Network** panel
(and the CLI) shows:

- **Live Wi-Fi:** signal, band, channel, standard, and link rate, refreshed every second
  with the **Live signal finder** on. A trend arrow and "best so far" help you find the
  strongest spot for a laptop or router.
- **Your network on each band:** every access point broadcasting your network, with signal
  and how busy its channel is, so you can see when 5 GHz is the better choice.
- **Advice:** plain fixes from the real readings (switch to 5 GHz, weak signal, busy
  channel, slow link, older Wi-Fi standard, use a cable).
- **Speed test:** two-way throughput, latency, and jitter to a paired device over the same
  encrypted link, with a suggested Moonlight video bitrate.

```
rac net              # readings and advice (works without the bridge)
rac net --watch      # live signal finder in the terminal
rac probe PEER       # latency and jitter
rac speedtest PEER   # throughput and suggested Moonlight bitrate
```

Readings come from `netsh` on Windows and `nmcli` on Linux. Nothing leaves the machine.

## Crew app

`crew-app/` is the Crew app (Control Room UI). The installers register it when Crew is
present. Crew requires the owner to trust third-party apps in Settings before they run.

## Build

```sh
cargo test                      # unit + three-node end-to-end test
cargo test --test clipboard_live -- --ignored --test-threads=1   # overwrites your clipboard
cargo build --release
```

## Status and limits (v0.1)

- Verified: Windows live clipboard in every direction (text, image, files), echo
  suppression, and password-manager exclusion. Linux: the same live test runs in CI under Xvfb.
- Not yet verified on two physical machines over Tailscale; see the release notes for the
  current field-test state.
- Linux cannot see password-manager "private" hints through the clipboard library, so on
  Linux turn Sync off for a peer if you copy secrets often.
- Empty folders and symbolic links are not sent.

See [SECURITY.md](SECURITY.md) for the threat model and
[docs/crew-mesh.md](docs/crew-mesh.md) for the self-hosted Tailscale plan.

Copyright (c) 2026 Purple Industries. All rights reserved.
