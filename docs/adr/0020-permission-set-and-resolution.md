# ADR 0020 — Permission set & resolution algorithm (Discord-aligned)

- Status: Accepted
- Date: 2026-10-21

## Context

ADR 0006 fixed the shape of Nuck permissions: ordered roles, a permission bitfield,
`@everyone`, per-channel overrides, mirrored into `m.room.power_levels`. This ADR
fixes the content: which permissions exist and how they resolve.

## Decision

**Discord-aligned (option a), with Nuck extensions.**

- **Permission set:** a `u64` bitfield of named permissions, aligned with Discord's
  set (~50 bits), so users and bot developers already understand it.
- **Nuck-specific bits** for things Discord lacks: voice-channel management, guild
  federation settings, audit-log visibility. Reserved high bits; the bitfield is
  **versioned** (ADR 0014). If we ever exceed `u64`, add a second field rather than
  repurposing bits.
- **Resolution algorithm (Discord's):**
  1. base = `@everyone` ∪ all member roles;
  2. apply channel **role** overrides (allow adds, deny removes);
  3. apply channel **member** overrides;
  4. `ADMINISTRATOR` and guild owner bypass all overrides.
- **Hierarchy:** a member may only modify roles/members **strictly below** their
  highest role. This is what makes ADR 0016's moderation safe.
- **Power-level mapping (versioned protocol commitment):** effective permissions →
  `m.room.power_levels` via a documented mapping (e.g. `ADMINISTRATOR` → 100,
  `MANAGE_ROLES` → 50, `SEND_MESSAGES` → 0), with per-channel overrides folded in.
  This is the bridge that makes permissions substrate-enforceable under federation
  (ADR 0006/0010).

## Consequences

- Permission checks are a fast bitfield operation on the hot path.
- Discord bot developers can port mental models and permission names directly.
- The bitfield layout and the PL mapping are **federated protocol** — both must be
  versioned, and remote servers of different Nuck versions must agree on bit meaning
  (ADR 0010/0014).
- The permission engine (resolution + PL mirroring) is one of the most
  correctness-critical components in nuck-server and needs thorough testing.

## Finding (2026-10-21)

The property test mandated by ADR 0022 was implemented and **fails**: the mirror is
**permissive**, not exact — a scalar power level cannot represent an arbitrary
permission set. It never under-grants, but it over-grants for sets that are not
level-closed, and only six actions (ban, kick, redact, invite, send, set-state) are
substrate-enforceable at all. See **ADR 0040** for the evidence and the pending
decision.

**Resolved by ADR 0041:** the mirror is no longer the enforcement path. Power levels
are used only coarsely (members vs. the action bot); the engine here remains the
authoritative policy, evaluated in nuck-server. `required_level` is an ordering used
for role comparison, not a security-critical substrate mapping.

## Implementation (2026-10-21)

Per-channel overrides are implemented end to end, completing the permission model.

- **Projection** — `space.nuck.override.v1` maps to the `overrides` table
  (`projection::ingest`); reconciliation rebuilds and prunes it from resolved channel
  state (ADR 0019). `Projection::list_overrides` plus `authorization::overrides_from_rows`
  feed the resolver.
- **Endpoints** — `PUT` / `DELETE /_nuck/v1/channels/{channelId}/overrides/{target}`
  through the action-bot path (ADR 0041). The target is a role id (including
  `@everyone`) or a Matrix user id.
- **Resolution** — `GuildPermissions::channel_permissions` applies the `@everyone`
  override, then role overrides by ascending position, then the member override; deny
  wins within an override, and `ADMINISTRATOR` short-circuits.
- **Hierarchy** — an override may only target a role **strictly below** the caller's
  highest, or a member the caller outranks.
- **Escalation guard** — you cannot **grant** a permission you do not hold yourself,
  whether through an override `allow`, a role's `permissions`, or a role assignment.
  The hierarchy rule alone does not close this: without the guard, `MANAGE_ROLES` at a
  high position could mint a lower role or override carrying powers the actor lacks,
  then assign it.
- **Removal** — `DELETE` writes an all-zero override. Matrix has no "delete state";
  an all-zero override resolves to exactly the same permissions as no override.

**Reserved role id:** the everyone role's id is `everyone`, **not** `@everyone` — Matrix
treats a state key beginning with `@` as a user id, and the substrate rejects writing
such a role event. `@everyone` remains the role's display name.

**Ownership:** implemented. `m.room.create` for a `m.space` room is projected into
`guild_owners` (ADR 0019), so the guild owner is identified and the bypass is real:
the owner is exempt from the role hierarchy (they outrank everyone) and bypasses channel
overrides (`channel_permissions` short-circuits to `ALL`, like `ADMINISTRATOR`).
`has()` and `can_grant` remain role-based — the owner still needs the relevant
permission bit (guild creation is expected to grant them a role, ADR 0038). Channel
access is gated by room membership, which is also projected (ADR 0019).
