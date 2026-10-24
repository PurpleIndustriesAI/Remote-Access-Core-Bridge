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
  unknown certificate is rejected during the handshake.
- **Pairing:** a one-time 80-bit code, open for 5 minutes, single use, closed after 5 wrong
  attempts. Both sides prove knowledge of the code with an HMAC that covers **both**
  certificate fingerprints. A relay that substitutes its own certificate changes a
  fingerprint and cannot produce a valid proof. The code never crosses the network.
- **Unpairing** removes the pin immediately; that machine can no longer connect.

## What is sent and stored

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
