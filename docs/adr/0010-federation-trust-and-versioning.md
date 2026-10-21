# ADR 0010 — Open federation with capability negotiation; power levels are the security boundary

- Status: Accepted
- Date: 2026-10-21

## Context

Matrix federation is open by default: any homeserver can send events for a room it
participates in. ADR 0001 requires "Nuck servers only," but that cannot be enforced
by an airtight protocol lock — a generic Matrix homeserver can still have a user
join a public room.

## Decision

**Trust model (a + c):**

- **Open federation among Nuck servers** by default. Any server advertising Nuck
  capability may federate.
- **Operator-controlled blocklists** for abuse control.

**Capability & version negotiation:**

- Each Nuck server advertises Nuck protocol versions and supported capabilities via
  a stable endpoint (`/_nuck/v1/server-info`) and/or a `.well-known` entry.
- Before exercising Nuck semantics, a server checks the peer advertises Nuck support
  and a compatible version range. Incompatible peers are **feature-gated**, not
  broken.

**Security boundary:**

- Guild state and permissions are mirrored into standard `m.room.power_levels`
  (ADR 0006), which **every** Matrix server enforces. This is the hard security
  boundary and works even against a non-Nuck server.
- A non-Nuck server in a Nuck room therefore lacks guild/role semantics but cannot
  exceed its power level. Degradation is graceful, not a bypass.

## Consequences

- Federation is open and matches the product goal (anyone self-hosts and
  interconnects), with a blocklist as the operational escape hatch.
- Custom event schemas (permission bitfields, guild state) **must be versioned**;
  peers must tolerate unknown fields and gate features by advertised version.
- "Nuck-only" is enforced by capability advertisement plus policy, with power levels
  as the real enforcement. We must not rely on the protocol lock for security.

## Correction (2026-10-21)

"Power levels as the real enforcement" is **too strong**: the mirror is permissive and
covers only six actions (ADR 0040). The security boundary is therefore **nuck-server's
own enforcement, backed by capability-gated Nuck-only federation**, with the mirror as
a permissive backstop. This makes the capability gate (peers must be Nuck servers) more
load-bearing than originally stated — a peer that is not a Nuck server, or one running
a divergent engine, is not reliably constrained by the substrate alone.

**Resolved by ADR 0041:** the substrate boundary is now **membership + coarse power
tiers** (members vs. the privileged action bot), which *is* exactly enforceable.
Members are low-level and cannot overstep; privileged writes come only from the bot,
authorized by nuck-server.
