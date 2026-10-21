# ADR 0002 — Federation rides on Matrix federation

- Status: Accepted
- Date: 2026-10-21

## Context

Nuck servers must federate with each other (ADR 0001), but only with other Nuck
servers. Federation across servers is one of the hardest parts of a chat system:
server discovery, key signing, state resolution, backfill, membership transitions,
and E2EE key distribution.

## Decision

Each Nuck server runs a real Matrix homeserver underneath and **reuses Matrix
federation as the backbone**:

- Nuck-specific state (permissions, guilds, voice-channel metadata, etc.) is encoded
  as **custom Matrix room events and state**, so it federates with the substrate's
  own machinery and requires no separate protocol.
- A Nuck-specific side-channel is added **only where forced** — real-time,
  high-frequency data where riding room events is wasteful. **MatrixRTC** is the
  default for federated voice/video, so a bespoke channel may not be needed at all.

## Consequences

- Zero new work for server discovery, signing, state resolution, backfill,
  membership, or federated E2EE.
- Nuck semantics must be expressible as Matrix room events/state. Any feature that
  cannot be is a red flag — reconsider it or route it through MatrixRTC/media.
- Federation bugs surface as Matrix federation bugs; we inherit the substrate's
  behaviour rather than controlling it.
- We must keep Nuck event schemas versioned and forward-compatible, since remote
  Nuck servers of different versions will exchange them.
