# Remote Access Crew roadmap

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
| NVIDIA encoding (NVENC/HEVC) | In progress | Root cause: Optimus panel on Intel GPU, no software MUX on AORUS 15G YB. Virtual monitor driver audited, installed, and shown on the NVIDIA GPU. Next: owner approves `Enable-VddNvenc.ps1`, then a live stream confirms `hevc_nvenc`. See Moonlight-Crew `windows-host/nvidia-virtual-display/`. |
| Intel driver update for HEVC on QuickSync | Planned, fallback only | Only needed if the NVIDIA virtual display fails. |
| Virtual display driver pipe hardening | Planned | Upstream driver exposes a local control pipe to all local processes. Track upstream; low risk on a single-user host. |

## 2. Viewer client (Moonlight fork, rebranded)

| Item | Status | Evidence / next step |
|---|---|---|
| Rebrand to "Remote Access Crew" (name, logo, taskbar, Task Manager, Linux launcher) | Done (unverified) | Merged in Moonlight-Crew PR #3. Needs a Windows build on Miku and a visual check. Settings and pairing locations deliberately unchanged. |
| Automatic reconnect with a Settings toggle | Done (unverified) | Merged in PR #4: on by default, 5 attempts with backoff, "Stop reconnecting" button, never after a user disconnect. Needs a build and a Wi-Fi-off test on Miku. |
| Follow display resolution and free cursor | Done (verified by Miku's agent) | Client reports native 1050x1680 portrait and a free cursor. |
| Pairing parser security fix | Done (verified) | Independent local test of every truncated challenge. |
| Disable or repoint the upstream update check | Planned | So the fork never offers to replace itself with upstream Moonlight. |
| Linux client build | Planned | Not built or tested yet. |

## 3. Clipboard and file bridge (this repository)

| Item | Status | Evidence / next step |
|---|---|---|
| Two-way text, image, and file clipboard | Done (verified on one machine) | Live Windows clipboard test in every direction, echo suppression, password-manager exclusion. |
| Security: mutual TLS 1.3, pinned certificates, 80-bit pairing code, LAN/Tailscale only | Done (verified) | Three-node end-to-end test including intruder, wrong fingerprint, wrong code lockout, unpair. |
| Network helper: live Wi-Fi signal finder, advice, speed test | Done (verified) | `rac net` on the host read the 2.4/5 GHz choice correctly; probe and speed test covered by tests. |
| Host install with Start Menu and Startup shortcuts and a scoped firewall rule | Done (verified) | Installed under `%LOCALAPPDATA%\Programs\RemoteAccessCrew`, firewall rule limited to LocalSubnet and Tailscale. |
| Release v0.2.0 (Windows) | Done (verified) | Pre-release with SHA256SUMS; download re-verified. |
| Install and pair on Miku | Blocked: Miku's Claude | Instructions in Moonlight-Crew PR #1 (HOST_20261021_02). Miku's bridge port was closed at 01:35 UTC. |
| Cross-machine copy and paste over LAN | Blocked: depends on pairing | Then repeat over Tailscale once Miku joins the tailnet. |
| Linux build and live test | Blocked: CI | GitHub Actions does not start on private BarnsL repos (account billing or limit). Local container build is the fallback. |
| Crew app page | Blocked: owner trust | Crew requires the owner to trust the app in Settings. |
| CI release pipeline | Blocked: CI | Workflow is written with pinned actions; runs once Actions is available. |

## 4. Coordination with Claude on Miku

| Item | Status | Next step |
|---|---|---|
| Shared channel | Working one way | Moonlight-Crew PR #1. The host posts tasks; Miku's agent has not replied since 2026-10-21 20:54 UTC. The owner can prompt it locally. |
| Direct agent-to-agent link | Planned | Remote Access Crew's paired link can carry signed task messages once Miku is paired. |

## 5. Crew Mesh: Tailscale with our own accounts

| Item | Status | Next step |
|---|---|---|
| Design | Done | `docs/crew-mesh.md`: Headscale control server, OIDC "Crew Accounts", official clients. |
| Lab deployment | Planned | Needs owner decisions: domain, VPS budget, and the identity provider choice. |
