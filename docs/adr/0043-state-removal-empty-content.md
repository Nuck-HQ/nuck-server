# ADR 0043 — State removal via empty content

- Status: Accepted
- Date: 2026-10-22

## Context

Implementing delete for roles and channels (and unban / kick) surfaced a Matrix
constraint: **there is no state deletion.** A state event's content can only be replaced
by a newer event; a redaction erases the content but the event remains in state. The
projection's reconciliation (`reconcile_room`) prunes rows whose writing event is no
longer current state — which never happens for a redacted event — so redaction alone
cannot express "this role/channel/ban is gone."

## Decision

**Removal is encoded as a state event with empty content (`{}`).**

- It reuses the shape a redaction produces, and the convention the projection already
  applies to `m.room.member` (a redacted member event is *ignored*, not rejected).
- `projection::ingest` maps an empty content object for `guild`, `role`, `member`,
  `channel`, `invite`, `ban`, and `join_request` to a `Remove*` operation, which deletes
  the corresponding row(s). A non-empty but malformed content is still an error
  (ADR 0014 tolerant parsing is preserved).
- Because the empty event becomes the room's current state for that key, the outcome is
  **rebuildable**: reconciliation replays it and re-applies the removal.
- **Overrides are the exception**: they keep the all-zero convention (ADR 0020), which
  already resolves identically to "no override".

## Consequences

- Deletion needs no new event type and no substrate feature, and stays consistent under
  reconciliation and federation.
- The protocol gains a documented removal encoding (`docs/protocol/events.md`);
  clients must treat an empty `space.nuck.*` state event as "removed".
- The underlying Matrix room is not destroyed (a deleted channel room, or a deleted
  guild's Space, still exists on the substrate); only Nuck state is removed. Full room
  destruction is deferred.
