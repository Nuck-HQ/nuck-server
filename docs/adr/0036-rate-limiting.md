# ADR 0036 — Rate limiting: identity-aware per-route limits

- Status: Accepted
- Date: 2026-10-21

## Context

Nuck is internet-facing and federated. Several endpoints are expensive or abusable
(invite redeem, registration, search, friend requests, bot gateway connections). A
reverse-proxy limiter alone cannot distinguish identities or endpoints.

## Decision

**Identity-aware, per-route limits in nuck-server (option a), with a coarse outer
layer (b).**

- **nuck-server** applies limits keyed by **identity** (user, IP, origin server) and
  **route** on `/_nuck/v1`. It knows who is calling and which endpoint.
- **The reverse proxy** applies a **coarse volumetric** limit as a first line of
  defence; it cannot distinguish identity or endpoint, so it is not the primary
  mechanism.
- **The substrate's own limits** cover the C-S and federation paths; we do not
  duplicate them.

**Tightest limits on the abuse surface identified across earlier ADRs:**

- **invite redeem** (ADR 0011/0025) — cross-server, brute-forceable;
- **registration** (ADR 0009) — account farming;
- **search** (ADR 0029) — expensive, per-user permission filtering;
- **friend requests** (ADR 0030) — to-device spam;
- **bot gateway connections** (ADR 0018) — connection/backpressure abuse.

## Consequences

- Limits are enforced where identity is known, which is where abuse can be
  meaningfully distinguished from legitimate load.
- Rate-limit state is per-server; in a federated system, a distributed abuse
  campaign across many origin servers must be handled by the blocklist mechanism
  (ADR 0024), not by per-identity limits alone.
- Limits must be configurable per operator (self-hosters have very different scales)
  and observable (ADR 0033) so operators can tune rather than guess.
- Tight limits on invite redeem and registration interact with the deferred
  anti-raid work (ADR 0025).
