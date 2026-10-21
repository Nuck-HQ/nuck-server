# ADR 0011 — Guild discovery & joining: invite codes + optional directory

- Status: Accepted
- Date: 2026-10-21

## Context

Nuck needs Discord-style guild growth across federated servers. Guild membership is
ultimately a Matrix room invite underneath (ADR 0005/0009), but that is not a
shareable growth mechanism on its own.

## Decision

- **Invite codes/links are the primary mechanism.** A guild generates a code; a user
  on **any** Nuck server redeems it and is added to the guild Space and its initial
  channels.
- **An optional public directory** lets guilds opt into searchable discovery.
- **Matrix-native user invites** work implicitly (guild membership is a room invite),
  but are not a growth mechanism.
- **Redemption is host-authoritative (a1).** The invite lives on the guild's home
  server. A remote user's nuck-server calls the host's
  `/_nuck/v1/invites/{code}/redeem`, which validates the code and issues the Matrix
  room invites (guild + initial channels). The host is the source of truth for
  invites, because max-uses, expiry, and revocation all require it.

## Consequences

- Growth works across servers with no central service.
- Invites are guild state on the host, so they federate and survive host restarts.
- Remote redemption requires the host to be reachable; a host outage blocks new
  joins (accepted — joining is inherently a host-authoritative operation).
- Invite codes must be unguessable and rate-limited; the redeem endpoint is a
  cross-server attack surface.
- Self-contained signed codes (a2) are a possible later optimisation, not v1.
