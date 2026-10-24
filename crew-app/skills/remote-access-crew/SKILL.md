---
name: remote-access-crew
description: Operate Remote Access Core (Bridge) to pair machines and send text, images, or files between them over LAN or Tailscale. Use when the owner wants clipboard sync, to send a file to another of their machines, or to check whether a peer is reachable.
---

# Remote Access Core (Bridge)

A native bridge (`remote-access-crew`) runs on each machine. Paired machines sync the
clipboard (text, images, files) automatically and accept on-demand sends. The `rac` CLI
and the Crew page at `/apps/remote-access-crew` control the bridge on this machine.

## Commands

- `rac status`: this machine, peers, recent transfers (type and size only).
- `rac pair-code`: open pairing for 5 minutes. Give the code to the owner; never post it
  in a public channel.
- `rac pair <address> <code>`: pair with a machine showing a code.
- `rac send [--to PEER] [--text TEXT | FILE...]`: send text or files; no input sends the
  current clipboard.
- `rac ping PEER`, `rac sync on|off [--peer PEER]`, `rac unpair PEER`.

## Rules

- Pairing pins certificates. Never pair with an address the owner did not name.
- A successful `send` result means the peer verified the bytes and placed them on its
  clipboard. Report failures with the peer's stated reason.
- Received files land in `Downloads/Remote Access Crew`, are never executed, and carry the
  Windows downloaded-file mark.
- Do not read, print, or store clipboard content when diagnosing; status shows only metadata.
