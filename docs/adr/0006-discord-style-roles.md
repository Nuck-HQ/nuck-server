# ADR 0006 — Discord-style roles, mirrored into Matrix auth

- Status: Accepted
- Date: 2026-10-21

## Context

Nuck needs expressive, guild-wide permissions. Matrix power levels are per-room and
flat (a single number per user). Federation (ADR 0002) means remote Nuck servers
write events into their own Tuwunel and those events federate to ours — we do **not**
get to intercept them. The substrate's room auth is what accepts or rejects an
event, so permissions must be expressible as substrate-enforceable rules or a
malicious/outdated remote server can bypass them.

## Decision

**Model (option a) — Discord-style:**

- A Guild has **roles**: named, ordered by position, each carrying a **permission
  bitfield**. The highest-positioned role wins on conflict.
- A base **`@everyone`** role applies to all guild members.
- **Per-channel allow/deny overrides** are first-class.
- Roles are **assigned to members** (member → role set).
- All of the above is stored as **custom guild state events** in the Guild Space
  room (ADR 0005), so it federates.

**Enforcement — two layers:**

1. **nuck-server** computes effective permissions (guild roles ∩ channel overrides)
   and checks them at the API boundary.
2. Those effective permissions are **mirrored into each channel room's
   `m.room.power_levels`**, so the substrate enforces them too. This is the layer
   that is authoritative under federation.

API-boundary checks are necessary but **not sufficient**; the mirrored Matrix auth
rules are the real boundary against remote servers.

## Consequences

- Users get a familiar mental model; moderators can define reusable, ordered roles.
- nuck-server owns a permission-computation engine and a mirroring loop that must
  stay consistent with the substrate (shared with ADR 0005's reconciliation loop).
- The permission bitfield and its semantics must be **versioned and federated**;
  remote servers of different Nuck versions must agree on what a bit means.
- Any permission that cannot be reduced to Matrix auth rules is a red flag: it will
  not be enforced against remote servers.

## Correction (2026-10-21)

The claim that mirrored power levels are "the real boundary against remote servers" is
**too strong**. A scalar power level cannot mirror an arbitrary permission set: the
mirror over-grants for non-level-closed sets and only covers six actions. See
**ADR 0040**. Until its decision lands, treat the mirror as a *permissive backstop*
and nuck-server as authoritative.

**Resolved by ADR 0041:** power levels are used only **coarsely** (members vs. the
action bot) and all fine-grained authorization lives in nuck-server. Roles keep full
Discord flexibility, and the substrate boundary — *membership + coarse tiers* — is
exactly enforceable.
