# ADR 0038 — Guild creation and guild home server

- Status: Accepted
- Date: 2026-10-21

## Context

ADR 0037 covers who may join a **home server**; ADR 0025 covers who may join a
**guild**. Neither covers who may **create** a guild, nor where a guild lives — but
a guild consumes a server's resources and needs an authority for host-authoritative
operations (invites, ADR 0011; bans, ADR 0016; directory listing, ADR 0021).

A more flexible model (guild hosted on any chosen server, including one you are not a
user on) was considered and **rejected as too complex**: it turns guild creation into
a cross-server operation, makes guild ownership a federated role the host cannot fully
moderate, and introduces stranded-guild failure modes.

## Decision

- **Creation (a1): any registered user** may create a guild. Operators may restrict
  it — disable, require approval, or impose a per-user quota — because guilds consume
  storage, rooms, and federation resources. Enforced by nuck-server, **local and never
  federated** (same philosophy as ADR 0037).
- **Hosting (b1): the creator's home server.** A guild is created on the creator's
  home server, which becomes the **guild's home server** — the authority for invites
  (ADR 0011), the space and its channels, and directory listing. Members join from any
  server via federation.
- **Guild creation is a local operation.** The client calls `POST /_nuck/v1/guilds` on
  its own server; no S2S step is involved.
- **No guild migration in v1.** A guild's home server is permanent, for the same
  reason a user's home server is (ADR 0009).

## Consequences

- Guild creation stays simple and local; no cross-server trust or hosting policy.
- The guild's home server is a **single point of authority and availability** for that
  guild: if it is down, invites and join requests cannot be processed (accepted,
  ADR 0011), though members elsewhere can still read history.
- Operators retain control over their own server's resource consumption.
- Guild ownership is a normal, local role — the host can fully moderate the guilds it
  hosts (unlike the rejected model).
- Users who want their community on a different server must be a user there — which is
  consistent with ADR 0009.
