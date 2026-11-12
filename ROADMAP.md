# Remote Access Core roadmap

One place for every workstream: the streaming host, the rebranded client, the clipboard and
file bridge, network quality, and the self-hosted mesh. Status words mean:

- **Done (verified):** working, with the evidence named.
- **Done (unverified):** code merged or installed, not yet proven end to end.
- **Blocked:** waiting on the named person or system.
- **Planned:** not started.

Last updated: 2026-10-22.

## 1. Streaming host (Windows laptop running Sunshine)

| Item | Status | Evidence / next step |
|---|---|---|
| Automatic portrait and landscape | Done (verified) | Host helper v5: rotation within about 0.6 s of the client's request; landscape restored on disconnect. Live Miku sessions 14:57 to 14:58 on 2026-10-21. |
| Secure elevated logging | Done (verified) | v4 security fix: admin-owned log in the protected install folder. |
| Wi-Fi prefers 5 GHz | Done (verified) | Adapter "Preferred Band" set to 5 GHz; host on Wi-Fi 6 channel 100 at 85% signal. |
| NVIDIA encoding (NVENC/HEVC) | Done (verified) | Root cause: Optimus panel on Intel GPU, no software MUX on AORUS 15G YB. Fix: virtual monitor on the NVIDIA GPU. Live sessions on 2026-10-21 (20:40 to 21:25) used `hevc_nvenc` at 1920x1080. See Moonlight-Crew `windows-host/nvidia-virtual-display/`. |
| Host toast and service name "RAC Host" | Applied 2026-10-29, toast pending Sunshine restart | `install/rac-host-rebrand.ps1` (admin) moved the vendor `Sunshine.lnk` to `config\rebrand-backup`, renamed the host shortcut to `RAC Host.lnk`, and set the service display name. Windows names Sunshine toasts after the only shortcut targeting `sunshine.exe`. `-Remove` reverts. |
| Host AI view (time-boxed, owner-approved) | In review | Moonlight-Crew PR #7 is the single host adapter (PR #10 closed). Owner approves 3, 6 or 12 hours via UAC; admin-only grant file; no cursor. 38 checks pass. |
| Intel driver update for HEVC on QuickSync | Planned, fallback only | Only needed if the NVIDIA virtual display fails. |
| Virtual display driver pipe hardening | Planned | Upstream driver exposes a local control pipe to all local processes. Track upstream; low risk on a single-user host. |

## 2. Viewer client (Moonlight fork, rebranded)

| Item | Status | Evidence / next step |
|---|---|---|
| Rebrand to "Remote Access Crew" (name, logo, taskbar, Task Manager, Linux launcher) | Done (verified by Miku's agent) | Merged in PR #3 and PR #8. Miku runs client 6.1.3 with the title "Remote Access Crew (Client)". Settings and pairing locations deliberately unchanged. |
| Rename to "Remote Access Core", always with a role: (Client), (Host), (Bridge) | In review | 2026-10-24. Shown names only; binaries, `RAC_HOME` (`~/.remote-access-crew`), the received-files folder `Downloads/Remote Access Crew`, certificate names and firewall program paths keep the old spelling so pairing and existing installs keep working. |
| Automatic reconnect with a Settings toggle | Done (verified by Miku's agent) | Merged in PR #4: on by default, 5 attempts with backoff, "Stop reconnecting" button, never after a user disconnect. Miku's client 6.1.1 reconnected on its own after a host restart. |
| Follow display resolution and free cursor | Done (verified by Miku's agent) | Client reports native 1050x1680 portrait and a free cursor. |
| Recommended stream settings on Miku | Planned | 60 fps, about 20 Mbps, automatic bitrate adjustment on (currently 30 fps, 29.5 Mbps, adjustment off). Apply after the new client is installed; refine with `rac speedtest`. |
| Pairing parser security fix | Done (verified) | Independent local test of every truncated challenge. |
| Disable or repoint the upstream update check | Planned | So the fork never offers to replace itself with upstream Moonlight. |
| Linux client build | Planned | Not built or tested yet. |

## 3. Clipboard and file bridge (this repository)

The streaming client's own clipboard is text only and one way (client to host). Image and file
copy, and host-to-client copy, come only from this bridge.

| Item | Status | Evidence / next step |
|---|---|---|
| Two-way text, image, and file clipboard | Done (verified on one machine) | Live Windows clipboard test in every direction, echo suppression, password-manager exclusion. |
| Security: mutual TLS 1.3, pinned certificates, 80-bit pairing code, LAN/Tailscale only | Done (verified) | Three-node end-to-end test including intruder, wrong fingerprint, wrong code lockout, unpair. |
| Network helper: live Wi-Fi signal finder, advice, speed test | Done (verified) | `rac net` on the host read the 2.4/5 GHz choice correctly; probe and speed test covered by tests. |
| Host install with Start Menu and Startup shortcuts and a scoped firewall rule | Done (verified) | Installed under `%LOCALAPPDATA%\Programs\RemoteAccessCrew`, firewall rule limited to LocalSubnet and Tailscale. |
| Release v0.2.0 (Windows) | Done (verified) | Pre-release with SHA256SUMS; download re-verified. |
| Android app (`android/`): pairing, text, image and file copy both ways, opens the stream app | Done (unverified) | Built and signed 0.3.0; pairing proof, path and address checks match the desktop (`android/test.ps1`, `proof_known_vector`). Next: install on the Fold and test both directions over LAN and Tailscale. |
| Install and pair on Miku | Done (verified) | Paired; text, images and files went both ways (logs on both machines). Install v0.2.0 (Windows zip built from commit 30b90d0). Pairing is live only: show the code on one machine and type it on the other within 5 minutes; never post codes in GitHub threads. |
| Cross-machine copy and paste over LAN | Blocked: depends on pairing | Then repeat over Tailscale once Miku joins the tailnet. |
| Linux build and live test | Blocked: CI | GitHub Actions does not start on private BarnsL repos (account billing or limit). Local container build is the fallback. |
| Crew app page | Blocked: owner trust | Crew requires the owner to trust the app in Settings. |
| CI release pipeline | Blocked: CI | Workflow is written with pinned actions; runs once Actions is available. |

## 4. Coordination with Claude on Miku

| Item | Status | Next step |
|---|---|---|
| Shared channel | Working both ways | Moonlight-Crew PR #1. Latest: Miku MIKU_REPLY_11 (client 6.1.3); host HOST_20261022_09 (AI view merge). |
| Direct agent-to-agent link | Planned | Remote Access Core (Bridge)'s paired link can carry signed task messages once Miku is paired. |

## 5. Crew Mesh: Tailscale with our own accounts

| Item | Status | Next step |
|---|---|---|
| Design | Done | `docs/crew-mesh.md`: Headscale control server, OIDC "Crew Accounts", official clients. |
| Lab deployment | Planned | Needs owner decisions: domain, VPS budget, and the identity provider choice. |
