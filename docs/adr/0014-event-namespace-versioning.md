# ADR 0014 — Custom event namespace & versioning

- Status: Accepted
- Date: 2026-10-21

## Context

Nuck defines many custom Matrix events (guild, roles, permissions, channel type,
invite, voice metadata). They must federate across Nuck servers of differing
versions (ADR 0010). Matrix custom events use reverse-DNS type names.

## Decision

- **Namespace: `space.nuck.*`.** This is a **global protocol namespace** shared by
  every Nuck server — defined once by the Nuck protocol, not per server. Per-server
  namespaces would make federation impossible, since a remote server must recognize
  the same event types.
- **Server identity is a separate axis.** A server is identified by its Matrix
  `server_name` and signing keys; an event *type* is identified by `space.nuck.*`.
- **Versioning: version in the type name.**
  - Additive changes (new optional fields): same type; receivers **ignore unknown
    fields** (tolerant parsing).
  - Breaking changes: **new type** (`...v2`); the old type keeps working during
    transition.
  - The meaning of an existing type is **never mutated**.
- **Guild configuration is state events.** Roles, permissions, channel type, guild
  settings, and invite lists are **state events** (current value matters; part of
  room state; deterministic federation). Append-only or transient data stays message
  events.

## Consequences

- Event types are globally recognizable and stable; the namespace is a long-lived
  protocol commitment tied to the `nuck.space` domain.
- Versioning is visible in the event graph, in federation logs, and in state, and
  distinct versions naturally occupy distinct state keys.
- Tolerant parsing is mandatory on every receiver; unknown fields must never cause
  rejection.
- We must maintain a registry of `space.nuck.*` event types and their versions as
  part of the protocol.
