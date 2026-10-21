# Crew Mesh: Tailscale networking with our own account system

Status: design draft, phase 0. Nothing is deployed.

## Goal

Keep the Tailscale data plane (WireGuard, NAT traversal, MagicDNS, the official apps on
Windows, Linux, Android, and iOS) but replace Tailscale's hosted login and coordination
server with one we run, signed in with **Crew accounts** instead of Google, Microsoft, or
GitHub accounts.

## Approach

| Part | Choice | Why |
|---|---|---|
| Coordination server | **Headscale** (open-source Tailscale control server, BSD-3-Clause) | Official Tailscale clients support a custom login server, so no client fork is needed |
| Accounts | **Crew Accounts**: an OpenID Connect provider we run | Headscale signs users in through OIDC. Start with a self-hosted provider (Zitadel or Authentik), branded as Crew Accounts, then decide whether to build our own |
| Relays | Headscale's built-in DERP relay plus STUN (UDP 3478) | Relays traffic when a direct path is blocked |
| Clients | Official Tailscale apps, pointed at our server | `tailscale up --login-server https://mesh.<our-domain>` on desktop; "Use alternate server" on mobile |
| Management | A Crew app page using Headscale's API | List and approve devices, issue pre-auth keys, edit ACLs |

Remote Access Crew already accepts the Tailscale address ranges, and Headscale uses the same
default prefixes (`100.64.0.0/10`, `fd7a:115c:a1e0::/48`), so the bridge needs no changes.

## Phases

1. **Lab:** run Headscale and an OIDC provider on one machine, join two test devices, and
   verify login, MagicDNS, ACLs, and a Remote Access Crew transfer over the mesh.
2. **Hosted:** a small public VPS with a domain and TLS. Estimated about 5 to 12 USD per month,
   depending on provider and relay bandwidth. This is a billable change and needs owner approval.
3. **Migration:** move the owner's devices from the hosted tailnet one at a time, keeping the
   hosted tailnet as a fallback until each device is verified.
4. **Crew integration:** device list, approvals, keys, and ACL editing in Crew; a "Join Crew
   Mesh" flow in the Remote Access Crew page.

## Limits to accept

- Headscale is a community project, not supported by Tailscale Inc. Some newer features
  lag behind or are missing (for example, Tailnet Lock).
- "Tailscale" is a trademark. Our product name is Crew Mesh; we refer to the official clients
  by name only to describe compatibility.
- Our server becomes security-critical: it decides which devices can join. It needs
  backups, updates, and MFA on the account system.

## Decisions needed from the owner

1. The domain for the login server (for example `mesh.<domain>`).
2. VPS provider and monthly budget.
3. Off-the-shelf OIDC provider first, or build Crew Accounts from scratch now.
