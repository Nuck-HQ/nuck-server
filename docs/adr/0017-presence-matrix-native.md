# ADR 0017 — Presence: Matrix-native for v1; rich presence deferred

- Status: Accepted
- Date: 2026-10-21

## Context

ADR 0013 established that the `a2` WebSocket exists only for genuinely ephemeral,
non-federated data. Presence is the candidate. Discord's presence model
(online/idle/dnd, custom status, activity) has no full Matrix equivalent, and Matrix
presence over federation is unreliable.

## Decision

**v1: Matrix-native only (option a).**

- Typing indicators and presence use Matrix's own mechanisms. No custom presence
  service, no `a2` WebSocket in v1.
- Voice-channel occupancy ("who is in voice channel X") is derived from
  `m.call.member` state (ADR 0007) — already free and federated.

**Rich presence is deferred (option c).** When online/idle/dnd, custom status, and
activity are introduced, they will be served from nuck-server over the **`a2`
WebSocket**, scoped to the guilds the client cares about. Federation of rich presence
is optional and best-effort, because presence is not a correctness-bearing fact.

## Consequences

- v1 ships no second real-time channel; all state remains on `/sync` (ADR 0013).
- We accept Matrix presence's limitations in v1, including its federated reliability.
- Rich presence, when added, is explicitly **not** durable and **not** authoritative —
  it must never be load-bearing for permissions, membership, or moderation.
- The `a2` WebSocket remains designed-for but unbuilt; adding it later must not
  become a backdoor for durable state.
