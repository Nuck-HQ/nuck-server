# ADR 0005 — Guild = Matrix Space; Channel = child room; guild-level state model

- Status: Accepted
- Date: 2026-10-21

## Context

Nuck needs Discord-style structure (guilds containing typed channels) on top of
Matrix. Matrix's native grouping construct is the **Space** — a room with
`m.space` type whose children are declared with `m.space.child` state events.
Matrix **power levels are per-room**, whereas Discord roles are **per-guild** and
apply across all channels.

## Decision

**Structure — option (a):**

- **Guild** = a Matrix Space room. It carries guild-level state as custom events:
  roles, role→permission mappings, guild settings, member→role assignments.
- **Channel** = a child room of the guild Space (`m.space.child`), with a channel
  type (text / voice / category) in custom state.

**State model — option (c):**

- **Guild membership** = membership in the guild Space room.
- **Channel access** = nuck-server keeps each member's channel-room membership in
  sync with their guild roles (auto-join / auto-leave).
- **Effective permissions** are computed by nuck-server (guild roles ∩ channel
  overrides) and **mirrored into each channel room's `m.room.power_levels`**, so the
  substrate's own auth still enforces them (required for federation — see ADR 0002).

## Consequences

- Guild and channel structure federates natively: spaces and rooms are just rooms.
- Discord semantics (guild-wide roles) live in our layer, where they are queryable
  (ADR 0003), while the substrate remains authoritative for event acceptance.
- nuck-server owns a **reconciliation loop**: role/permission changes must propagate
  to channel memberships and to every channel room's power levels.
- Per-channel overrides are a first-class concept and must be folded into the
  effective-permission computation.
- Rejected option (b): single-room guilds cannot express per-channel membership,
  per-channel voice, or independent channel history.
