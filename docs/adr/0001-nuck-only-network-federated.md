# ADR 0001 — Nuck-only network (client + server), federated among Nuck servers

- Status: Accepted (supersedes the earlier "no federation" draft)
- Date: 2026-10-21

## Context

Nuck is a Discord alternative built on Matrix. Matrix is the substrate (rooms,
events, identity), but Nuck's value is in features generic Matrix clients cannot
express: rich permissions, voice & video channels, guild-like structure.

Two independent axes were conflated:

1. **Clients** — who may talk to a Nuck server.
2. **Servers** — who may federate with a Nuck server.

## Decision

- **Clients: closed.** The **Nuck client** is the only supported client. We make no
  compatibility guarantees toward generic Matrix clients. Custom client APIs and
  custom event types are fair game.
- **Servers: federated, Nuck-only.** Anyone may run their own Nuck server, and Nuck
  servers federate with each other. Interop is with **Nuck servers only** — generic
  Matrix homeservers are not a target.
- The Matrix homeserver is an implementation detail, not a compatibility surface.

## Consequences

- We get a federated network (self-hostable, like Matrix/Discord hybrid) without
  owing interop to the wider Matrix ecosystem.
- Nuck-specific state (permissions, guilds, voice) must itself federate — either by
  encoding it as custom Matrix events that ride the substrate's federation, or via a
  Nuck-specific federation protocol. **Mechanism: see ADR 0002 (TBD).**
- Revisit only if Nuck ever needs to talk to the wider Matrix network.
