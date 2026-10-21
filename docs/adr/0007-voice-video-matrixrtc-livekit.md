# ADR 0007 — Voice & video via MatrixRTC + LiveKit

- Status: Accepted
- Date: 2026-10-21

## Context

Nuck needs Discord-style voice and video channels that work across federated Nuck
servers. Matrix has a standard answer: **MatrixRTC**, where call membership is
ordinary room state (`m.call.member`) and media is carried by an SFU.

## Decision

- **Signalling: MatrixRTC.** Joining a call publishes `m.call.member` state in the
  voice channel's room, which federates natively (ADR 0002/0005). Tuwunel already
  supports MatrixRTC, so the substrate side is done.
- **Media: LiveKit SFU.** Production-grade, open-source, self-hostable — matches the
  self-host story.
- **SFU topology: single SFU per call (a1).** The SFU is hosted by the server that
  owns the guild/voice room; remote participants connect to it directly. SFU
  cascade (a2) is a deferred optimisation.
- **A voice channel is a room** with the `voice` channel type. Media and signalling
  are scoped to that room.
- **Tokens: nuck-server mints LiveKit access tokens.** It is the only component
  that knows effective permissions (ADR 0006); it issues short-lived, scoped tokens
  on join and can revoke them when permissions change.

## Consequences

- Group voice/video works across federation with one deployable per server.
- Media for a federated call transits the host server — an accepted v1 trade-off.
- nuck-server depends on LiveKit being co-deployed (or reachable) alongside it.
- Token minting ties voice access to the permission engine; permission changes must
  invalidate live tokens.
- If E2EE for calls is later required, LiveKit's insertable-streams E2EE is the path
  (see ADR 0008, TBD).

## Verification update (2026-10-21)

Checked against Tuwunel ([verification](../verification/tuwunel.md#4-matrixrtc--confirmed-one-design-refinement)):

- Tuwunel supports MatrixRTC via **MSC4143** (`org.matrix.msc4143.rtc_foci`) and
  exposes `livekit_url` into `.well-known`, but **does not embed an SFU** — LiveKit is
  external, as assumed.
- **Refinement:** "nuck-server mints LiveKit tokens" means **nuck-server acts as the
  MatrixRTC JWT service** (the role Element's `lk-jwt-service` plays). The stock
  service only checks Matrix room membership and knows nothing of Nuck permissions
  (ADR 0006), so voice-permission enforcement requires nuck-server to validate the
  caller and mint the JWT itself. Point `rtc_foci.livekit_service_url` at nuck-server.

