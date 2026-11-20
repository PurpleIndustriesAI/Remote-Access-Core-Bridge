---
name: remote-access-crew
description: Operate Remote Access Core (Bridge) to pair machines and send text, images, or files between them over LAN or Tailscale. Use when the owner wants clipboard sync, to send a file to another of their machines, or to check whether a peer is reachable.
---

# Remote Access Core (Bridge)

A native bridge (`remote-access-crew`) runs on each machine. Paired machines sync the
clipboard (text, images, files) automatically and accept on-demand sends. The `rac` CLI
and the Crew page at `/apps/remote-access-crew` control the bridge on this machine.

## Setup wizard (GUI)

The Crew page opens with a visible step-by-step **Setup wizard**: bridge running, pair a
device, approve clipboard and files, open the `crew-link` and `crew-sessions` channels,
read-only host verification (RAC Host service, `/dev/uinput` permissions, `apps.json`
desktop entries, LAN-only firewall), a live link test, and one-click no-secrets handoff
notes to a paired machine. Every wizard action reuses the same audited, owner-gated
endpoints as the rest of the page (`GET /rac/wizard-check`, `POST /rac/wizard-notes`).
Use it before saying a machine is fully set up; a green `7/7 complete` chip means every
step passed live.

## Commands

- `rac status`: this machine, peers, recent transfers (type and size only).
- `rac pair-code`: open pairing for 5 minutes. Give the code to the owner; never post it
  in a public channel.
- `rac pair <address> <code>`: pair with a machine showing a code.
- `rac send [--to PEER] [--text TEXT | FILE...]`: send text or files; no input sends the
  current clipboard.
- `rac ping PEER`, `rac sync on|off [--peer PEER]`, `rac unpair PEER`.
- `rac moonlight-link on|off` (host): let clients Sunshine already paired link without a
  code. Publishing the identity to Sunshine needs `host-moonlight-link.ps1`, which asks
  Windows for approval once and restarts Sunshine; never run it while a stream is live.
- `rac channel on|off NAME`, `rac channel-send --to PEER --channel NAME FILE`: verified
  single-file delivery for apps such as Crew Link, into `<home>/channels/NAME/inbox/<sender fingerprint>`.
- `python -m backend.session_client list --peer HOST --agent kimi --all`: show every Crew
  session page available to the local router chat. Run from the Crew app directory. Use
  `codex` for Codex. Select the exact `key`, never a title.
- `python -m backend.session_client send --peer HOST --agent kimi --target CHAT_KEY --text "Hello"`:
  send a bounded message through the signed `crew-sessions` channel.
- `python -m backend.session_client read --peer HOST --agent kimi --target CHAT_KEY --since CURSOR`:
  read bounded replies. Continue with the returned `next_since` value.

## Rules

- Pairing pins certificates. Never pair with an address the owner did not name.
- A successful `send` result means the peer verified the bytes and placed them on its
  clipboard. Report failures with the peer's stated reason.
- Received files land in `Downloads/Remote Access Crew`, are never executed, and carry the
  Windows downloaded-file mark.
- Do not read, print, or store clipboard content when diagnosing; status shows only metadata.
- Channel files are data for the receiving app. They never reach the clipboard, and the
  receiving app must verify them itself (Crew Link checks its own signatures).
- Session routing needs the `crew-sessions` channel, file approval, session routing approval,
  a pinned Crew Link peer key, and a selected eligible router chat. It grants no execution
  trust and cannot bypass ordinary Crew session authorization.
- A session send result means started or queued, not completed work. A response timeout is
  uncertain. Never retry the same request automatically.
