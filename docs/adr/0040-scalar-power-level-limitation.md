# ADR 0040 — Finding: a scalar power level cannot mirror an arbitrary permission set

- Status: **Superseded by ADR 0041** (finding retained; options withdrawn)
- Date: 2026-10-21
- Relates to: ADR 0006, 0010, 0020, 0022

> The finding below stands and is why the mirror is not the enforcement path.
> **ADR 0041** resolves it: keep flexible roles, use power levels only coarsely, and
> enforce privileged actions through a bot. The options in this ADR are withdrawn.

## Context

ADR 0020 defined the mirror from effective permissions to `m.room.power_levels`, and
ADR 0022 mandated a property test asserting that the engine and the substrate agree.
That test was implemented and **fails**.

## Finding

**A single scalar power level cannot faithfully represent an arbitrary permission
set.** The substrate compares one number per member against per-action thresholds, so
the set of actions it permits is always a *prefix* of the action order. Discord-style
permission sets are arbitrary, not prefixes.

Empirically ([nuck-protocol's `mirror_invariant` test](https://github.com/nuck-hq/nuck-protocol/blob/v0.2.0/tests/mirror_invariant.rs), 200k random sets):

- The substrate **never under-grants** — it never denies something the engine grants.
  Functionality is preserved.
- The substrate **over-grants** — it permits actions the engine denies — for sets that
  are **not level-closed**. Example: a member holding only `MANAGE_CHANNELS` (level 50)
  is permitted `SEND_MESSAGES`, `INVITE`, and `SET_STATE` by the substrate, none of
  which the engine grants.
- For **level-closed** sets (the set implied by some level), the engine and substrate
  agree **exactly**.

Consequences:

- ADR 0006's and ADR 0010's claim that the mirrored power levels are "the real security
  boundary" is **too strong** for fine-grained permissions. It is a *permissive
  backstop*, not an exact enforcement.
- Only six actions are substrate-enforceable at all (`Action::ALL`): ban, kick, redact,
  invite, send message, set state. Everything else (reactions, attachments, viewing,
  voice connect/speak, embeds, threads) has no Matrix power-level representation and is
  enforced by nuck-server alone.

## Options

1. **Restrict roles to level-closed sets.** A role may only hold the permissions
   implied by a single level. The mirror becomes exact. Cost: no arbitrary permission
   combinations — a significant loss of Discord-like flexibility.
2. **Accept a permissive mirror.** Keep flexible roles; the mirror is a backstop and
   nuck-server is authoritative. Cost: a remote or malicious Nuck server's user can
   exceed the engine's intent for the six mapped actions. This weakens ADR 0010's
   security story.
3. **Hybrid: exact for the mapped actions, flexible elsewhere.** Assign the mapped
   (privilege-bearing) permissions **distinct** levels and require role permission sets
   to be closed **with respect to them**; the substrate then mirrors those six actions
   exactly. Unmapped permissions are flexible and are enforced by nuck-server, which is
   where they are enforced regardless.

## Recommendation

**Option 3.** The over-grant risk is confined to exactly the six substrate-enforceable
actions, and those are the privilege-bearing ones (ban, kick, redact, invite, send,
state) — precisely the ones where exactness matters. The cosmetic permissions cannot
be substrate-enforced no matter what, so flexibility there costs nothing in security.
This keeps most of Discord's expressiveness while restoring an exact mirror where it
counts.

## Consequences (if option 3)

- Each mapped permission gets a **distinct** `required_level`; role validation rejects
  sets that are not closed with respect to the mapped permissions.
- The property test is strengthened to assert **exact** agreement for the mapped
  actions under the closure rule, and the "permissive" characterization is removed.
- ADR 0006 and ADR 0010 are corrected: the mirror is exact **for the mapped actions**
  and nuck-server is authoritative for everything else.
- The permission bitfield and the level assignment become a tighter protocol
  commitment (ADR 0014/0020) — a wrong level now silently weakens security.

## Open

This is a **product decision** (how much permission flexibility to trade for an exact
substrate mirror). Until it is made, the permissive mirror stands and the property test
documents the limit rather than asserting exactness.
