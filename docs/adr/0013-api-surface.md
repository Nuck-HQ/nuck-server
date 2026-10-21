# ADR 0013 — API surface: namespaced REST + sync-driven Nuck state

- Status: Accepted
- Date: 2026-10-21

## Context

ADR 0005 made the substrate (rooms, events, membership) and the Nuck layer (guilds,
roles, permissions, invites, voice) genuinely different things. They need different
client surfaces, and the Nuck layer must be versioned for federation (ADR 0010).

## Decision

- **Substrate:** the client uses the **proxied Matrix C-S API** for rooms, messages,
  sync, media, and E2EE (ADR 0002/0005, option i).
- **Nuck features:** a **versioned, namespaced REST surface, `/_nuck/v1/*`**, for
  guilds, channels, roles, permissions, invites, and voice tokens.
- **Real-time: sync-driven (a1).** Anything expressible as room state/events —
  roles, permissions, guild settings, channel membership — rides the Matrix `/sync`
  stream, so it is automatically federated, consistent, and resumable via sync
  tokens.
- **A separate WebSocket (a2) is added only for genuinely ephemeral, non-federated
  data.** It is a consistency liability and is avoided by default.

## Consequences

- One obvious place to version the Nuck contract; no collision with Matrix spec
  changes when Tuwunel is upgraded.
- Clients get an explicit, documented contract for the Discord-shaped parts rather
  than guessing which Matrix primitives are overloaded.
- Nuck state changes are durable and federated by construction, at the cost of
  routing them through the event graph rather than a low-latency side channel.
- Any Nuck feature that cannot be expressed as room state/events needs a deliberate
  exception (and likely the (a2) WebSocket).
