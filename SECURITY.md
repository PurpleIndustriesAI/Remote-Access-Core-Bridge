# Security model

Remote Access Core (Bridge) moves clipboard content between machines you own. Clipboard content
often contains secrets, so the design assumes a hostile local network.

## Who can connect

- **Network:** the bridge drops connections before TLS unless the source is loopback,
  RFC 1918 private IPv4, link-local, IPv6 unique-local, or Tailscale (`100.64.0.0/10`,
  `fd7a:115c:a1e0::/48`). The Windows installer adds a firewall rule with the same scope.
  Public addresses are never accepted.
- **Identity:** each machine generates its own P-256 certificate. Connections use **TLS 1.3
  with mutual authentication**. There is no certificate authority; a peer is trusted only
  if its certificate SHA-256 fingerprint was pinned during pairing. A connection from an
  unknown certificate is rejected during the handshake unless pairing or Moonlight linking is open.
  Those temporary admission paths accept only their identity proof, never clipboard data.
- **Pairing:** a one-time 80-bit code, open for 5 minutes, single use, closed after 5 wrong
  attempts. Both sides prove knowledge of the code with an HMAC that covers **both**
  certificate fingerprints. A relay that substitutes its own certificate changes a
  fingerprint and cannot produce a valid proof. The code never crosses the network.
- **Unpairing** removes the pin immediately; that machine can no longer connect.
- **Moonlight link:** Sunshine pairing proves identity, but each linked peer still needs local
  clipboard and file approval. Relinking cannot restore a revoked grant. Data checks also
  verify the original Sunshine certificate remains paired and linking is on. Older automatic
  grants reset once on upgrade because old labels did not reliably identify their source.
  Identities and addresses remain intact. Approve ordinary peers again locally. Recognized old
  Moonlight links also need a fresh link before approval can allow data.
- **Connection limits:** TLS and the first message share a five second deadline. Link attempts
  allow ten per source address per minute, including concurrent attempts. The source table holds
  at most 256 entries. IPv4 and its IPv6 mapped form share one allowance.
- **Sunshine state:** linking reads an absolute local sunshine_state.json path with no links,
  a paired-device list, and a four MiB limit. Custom local installations remain supported.
- **App channels:** a channel must be enabled, and both machines must grant the peer file access.

## What is sent and stored

Transport consent must be verified on the same pinned TLS connection that carries the payload.
The receiver requires the verification frame before content, returns a receipt bound to that
connection, then checks its current policy again before accepting the payload. A sender refuses
older replies that lack this receipt. There is no automatic compatibility downgrade.

Windows classification uses the kernel interface type and the adapter hardware identity rather
than the editable friendly name. Linux uses its device subsystem and wireless metadata. Android
uses network capabilities, with explicit USB and Bluetooth interface hints for downstream tether
links. Unidentified links are blocked. Tailscale and loopback are explicitly grouped with LAN.
These controls apply to network links. They do not provide direct USB device redirection or
Bluetooth peripheral passthrough. Physical USB and Bluetooth acceptance still needs real devices.

Device inventory has separate client approval. Existing clipboard, file and transport grants
never imply permission to inspect hardware. The client returns bounded USB class, product ID
and status data only, with no serial numbers or personal device names. The permission is
checked before and after the query. Requests do not automatically grant reciprocal hardware
access. See [Devices and hardware](docs/HardwareAccess.md) for the supported scope and limits.

The earlier 0.4.2 implementation checked consent on a separate connection and permitted content
without a verification frame. It also treated unrecognized interface names as WiFi. Regression
tests reproduced these defects. The source repair rejects unverified payloads, enforces revocation,
and refuses legacy replies before sending content. Deployment and peer acceptance are separate
from these source checks. TLS handshake failures require identity and connection evidence; an EOF
alone does not establish a protocol version mismatch.

- Clipboard content is sent only to paired peers and is **never logged**. Status and logs
  record direction, peer name, type, size, and result only.
- Windows: content marked private by password managers
  (`ExcludeClipboardContentFromMonitorProcessing`, `Clipboard Viewer Ignore`,
  `CanIncludeInClipboardHistory`, `CanUploadToCloudClipboard`) is never sent.
- Files are written to `Downloads/Remote Access Crew` under sanitized names (no traversal,
  no absolute paths, Windows reserved names escaped), never overwrite existing files, are
  verified against the sender's SHA-256 before they appear, and are **never executed**.
  On Windows they carry the downloaded-file mark (Zone.Identifier, ZoneId=3), so
  SmartScreen and Office Protected View apply.
- Size limits are enforced by the receiver before data is accepted (text 8 MiB, images
  128 MiB and 120 megapixels, file batches 8 GiB and 20,000 files). Automatic sync sends
  file batches up to 512 MiB; larger batches need an explicit send.
- Private key, peers, and the control token live in `%APPDATA%\RemoteAccessCrew` or
  `~/.config/remote-access-crew` (directory mode 0700, files 0600 on Linux).

## Local control

The `rac` CLI and the Crew app talk to the bridge on `127.0.0.1` only, with a random
256-bit token readable only by the signed-in user. Crew routes are owner-only and refuse
restricted sessions.

## Releases

Release binaries are built by GitHub Actions from the tagged commit with actions pinned by
commit SHA and a read-only default token. Verify downloads against `SHA256SUMS.txt`.

## Reporting

Report suspected vulnerabilities privately to the repository owner. Do not open a public issue.
