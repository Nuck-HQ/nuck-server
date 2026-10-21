# ADR 0018 — Nuck-native bot platform, built on AppService identities

- Status: Accepted
- Date: 2026-10-21

## Context

Discord's largest ecosystem feature is bots. Matrix's native mechanism is
**AppServices**: a service registers with the homeserver, gets exclusive namespaces,
can create users, and receives events via the **transaction API** — which requires
the bot to expose a public HTTP endpoint (hostile to self-hosting and firewalls).

## Decision

Build a **Nuck-native bot platform (option c)**, staged, on top of Matrix
AppService identities.

- **Identity:** bots are AppService-managed Matrix users (`@bot:server`), so they get
  real identity, room membership, and federation for free (ADR 0009).
- **Events: a gateway WebSocket.** The bot connects *out* to nuck-server; no public
  endpoint required. The gateway is fed from nuck-server's event pipeline.
- **Actions:** bots call the *same* `/_nuck/v1/*` and C-S APIs as clients, subject to
  permissions (ADR 0006). A bot's authority is its role set — no separate permission
  model.
- **Interactions:** slash commands, buttons, and modals are `space.nuck.*` custom
  events delivered over the gateway.
- **E2EE:** bots cannot read E2EE DMs (ADR 0008); guild channels are plaintext, so
  bots work there.

**Staging:** v1 = bot users + REST actions + gateway events. Later = interactions
(slash commands, components), then webhooks as a thin convenience.

**The bot gateway is a distinct WebSocket from the deferred presence channel
(ADR 0017)** — different audience, different lifecycle, kept separate.

## Consequences

- Bots are first-class Matrix participants: they can be invited, mentioned, and
  moderated like users.
- No separate bot permission system to maintain — permissions are roles.
- nuck-server owns a gateway service: authentication, subscription/filtering,
  fan-out, and backpressure. This is real infrastructure.
- AppService registration must be managed by nuck-server as part of setup.
- E2EE DMs are permanently out of reach for bots; this is a documented limitation,
  not a bug.
