# ADR 0016 — Moderation model: Discord-style primitives (v1), automod deferred

- Status: Accepted
- Date: 2026-10-21

## Context

Permissions are a stated Nuck differentiator (ADR 0006), and moderation is where
permissions earn their keep. Nuck is federated, so moderation actions have to
distinguish between guild-scoped and server-scoped authority.

## Decision

**v1: Discord-style primitives (option a).** Ban, kick, timeout (mute), warn,
message purge, per-guild audit log, and reports. All actions are permission-gated by
roles (ADR 0006) and mirrored into `m.room.power_levels`.

**Automod (option c) is deferred** — a large ongoing investment, added once the core
is solid.

**Guild ban vs server ban (both needed, and distinct):**

- **Guild ban** — federated **guild state** (`space.nuck.*` event). Removes the user
  from the guild Space and channels, blocks rejoin. Enforced by the host server via
  kick + power-level mirror + rejecting future joins. Discord's model.
- **Server ban** — a Nuck server refusing a user entirely, **local and not
  federated**, via Tuwunel's own ban API. An operator action, not a guild action.

**Audit log** — append-only, therefore **message events** in a dedicated per-guild
audit room, permission-gated for viewing. Durable and federated without a new
mechanism.

## Consequences

- Moderation is expressed in substrate-enforceable terms (power levels), so it holds
  under federation (ADR 0006/0010).
- Guild bans are visible to and enforced by all participating servers; server bans
  are invisible to the federation by design.
- The audit room is a per-guild room and thus part of the guild's room set.
- Reports need a destination and a review flow — an open detail (see open questions).

## Verification update (2026-10-21)

Checked against Tuwunel
([verification](../verification/tuwunel.md#moderation-adr-0016)): the substrate
provides room bans (`rooms moderation ban-room` / `unban-room` / `ban-list-of-rooms`),
`rooms delete`, `federation disable-room`, `users deactivate`, `users redact-event`,
identifier blocklists, and `block_non_admin_invites`.

**Resolves the open detail:** **reports are not implemented in Tuwunel**
(`event_reports` / `user_reports` are deferred), so the reports flow is
**nuck-server work**, not a substrate feature. Guild **purge** maps to
`POST /_synapse/admin/v1/user/{user_id}/redact` (bulk) plus
`GET /_synapse/admin/v1/rooms/{room_id}/messages` for scoping.

