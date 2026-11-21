# Changelog

All notable changes to Remote Access Core (Bridge). Entries cite the commit
subjects they summarize.

## Unreleased (branch: feat/rac-linux-windows-branded-installers)

### Added
* Client microphone on Linux hosts (0.7.1): the host plays a paired client's microphone
  into a PipeWire loopback that apps record as "RAC Client Microphone" (`rac-client-mic`
  user service, enabled by install-rac-linux.sh and started by the bridge on demand). A
  Linux client streams its default PipeWire microphone. Raw 48 kHz mono over the existing
  paired TLS stream; no new protocol. Verified with a 440 Hz tone recorded back from the
  source at full level.
* Setup wizard step "Client microphone" and a per-device Microphone switch in settings; the
  host checks report whether the virtual microphone exists.
* Visible GUI setup wizard on the Crew page: a step-by-step flow (bridge
  running, pair a device, approve clipboard and files, open crew-link and
  crew-sessions channels, read-only host verification, live link test,
  one-click no-secrets handoff notes) so the full machine checklist runs
  from the GUI. Two new owner-gated routes, `GET /rac/wizard-check` and
  `POST /rac/wizard-notes`, plus a stdlib-only `crew-app/backend/wizard.py`;
  every wizard action reuses the existing audited endpoints. 22 new unit
  tests in `crew-app/tests/test_wizard.py`. Crew app version 0.6.2.
  (a97f646)
* Complete Linux installer `install/install-rac-linux.sh` plus Windows
  installer and hardware driver setup: one script that installs and brands
  the RAC Host, Bridge v0.7.0, and Client exclusively as Remote Access
  Core, with strict RFC1918 LAN + Tailscale UFW rules. (8f15afa)
* Linux RAC Host `apps.json` normalization: exact duplicate Desktop entries
  removed, and Low Res Desktop kept visible with a clear label instead of
  duplicate box art. (1b112da, cd257ee)
* This changelog.

### Changed
* User-facing copy says "RAC Host" instead of "Sunshine": the Crew app
  streaming card, the host-link error strings in `crew-app/backend/bridge.py`,
  and the README microphone section. Underlying technical identifiers and
  architecture references (`sunshine_state.json`, `~/.config/sunshine`,
  ROADMAP rebrand history) are unchanged by the project's rename policy:
  shown names are RAC, interfaces stay upstream.

## 0.7.0 — 2026-11-20

### Added
* Stream a client microphone into the host Steam Streaming Microphone
  (`rac mic on/off`, 440 Hz test tone option). (1a00ed5)
* Host tooling tracked in-repo: the RAC Host rebrand script and the
  display wake helper. (7ec5071)

## 0.6.1 — 2026-11-20

### Fixed
* Clipboard placement now retries while another application holds the
  clipboard instead of failing the transfer. (8023f1c)

## 0.6.0

### Added
* Crew session routing over the `crew-sessions` channel. (3e37389)

## 0.5.0

### Added
* Access requests and per-device access controls (clipboard, files,
  device inventory, microphone) with local per-peer consent required for
  every transfer. (3bb1d39, a0aadce)
* Bounded, client-approved USB device inventory. (8b65ed3)
* MIKU setup documentation. (3a08517)

### Security
* Install can no longer create a second bridge identity or write
  user-writable elevation. (8f2c33e)
* Linked-peer consent enforced on connection admission; transport consent
  enforced on payload connections. (e45c73b, 9a6d432)

## Earlier releases (tags v0.1.0 – v0.4.0)

Highlights, oldest first:

* v0.1.0 — secure two-way clipboard over LAN and Tailscale with mutual
  TLS and certificate pinning. (1dd216f)
* v0.2.0 — network helper: live Wi-Fi signal finder, advice, and a
  two-way speed test with a suggested Moonlight bitrate. (30b90d0)
* Android phone app (Remote Access Crew). (e64b554)
* Product name shown as Remote Access Core (Bridge). (fc09e09)
* Moonlight link and app channels, sender recording for channel files,
  and Crew app channel controls. (1e12e5b, a78f1b8, 7ac8764)
* Linked-peer consent test fixes: channel revocation coverage and
  loopback-only test bridges. (16ea87d, d3463c1)
