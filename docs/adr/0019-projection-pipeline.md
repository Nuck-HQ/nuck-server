# ADR 0019 — Projection pipeline: AppService push + reconciliation

- Status: Accepted
- Date: 2026-10-21

## Context

ADR 0003 makes Postgres a derived index over federated state. It must ingest events
originating on **remote** Nuck servers, not just local ones, and must be
reconstructible. Options were AppService push, a `/sync` projector client, admin-API
polling, or a hybrid.

## Decision

**AppService push with a reconciliation loop (option a).**

- nuck-server registers an AppService with a trusted **projector user**, auto-joined
  to every guild room on creation/join. Tuwunel pushes events to nuck-server's
  transaction endpoint; push is retried by the substrate, so gaps are unlikely.
- The AppService registration machinery is **shared with the bot platform**
  (ADR 0018).
- **Reconciliation:** on startup and periodically, re-fetch current room state and
  rebuild the affected projections, repairing drift. This keeps ADR 0003's
  "rebuildable index" property honest and makes the pipeline self-healing.
- **Idempotency:** every projection write is keyed by event ID and is an idempotent
  upsert; redelivery is harmless.
- **Two projection kinds:**
  - *current-state* (guild settings, roles, channel list, memberships) — derived from
    the substrate's **resolved room state**, refreshed on change;
  - *append-only* (audit log, message search) — keyed by event ID, ordered by origin
    server timestamp with a tie-break.
- **No optimistic client writes in v1.** Client actions write to the substrate; the
  projection updates from the AppService echo. Simpler, and avoids divergence between
  an optimistic write and the resolved state.

## Consequences

- A single ingestion path for both local and federated events.
- Postgres can be wiped and rebuilt by replaying state + events — the escape hatch
  from ADR 0003 is real.
- Current state is never reconstructed by replaying events ourselves; we trust the
  substrate's state resolution, which is what federates correctly.
- Client-visible writes have the substrate echo's latency, not instant feedback —
  accepted for correctness.
- The projector AppService is a trusted, privileged component: it sees all guild
  room events. Its registration and token are security-critical.
- Message search covers guild channels only (ADR 0008); DM content is never indexed.

## Verification update (2026-10-21)

Checked in Tuwunel source (`src/service/appservice/append.rs`)
([verification](../verification/tuwunel.md#3-appservice-event-delivery--confirmed)):
`append_pdu` runs after every accepted PDU, **including federated ones**, and delivers
to an appservice when it has a user in the room (or the sender matches its namespace).
**Confirms** the auto-join requirement — the projector user must be in every guild
room.

**Addition:** reconciliation and backfill need not replay events — Tuwunel exposes
`GET /_synapse/admin/v1/rooms/{room_id}/state` (full state, admin bypass) and
`GET /_synapse/admin/v1/rooms/{room_id}/messages` (timeline paging, admin bypass).
Use these for the startup/periodic reconciliation pass.

## Implementation (2026-10-21)

Landed and verified:

- **Schema** — `nuck-server/migrations/0001_init.sql` (guilds, roles, members,
  channels, overrides, bans, invites, audit_log, messages + `tsvector`, processed_events,
  projection_state).
- **Write path** — `projection::Projection` with idempotent upserts and `apply(op)`;
  `record_event` is the idempotency guard.
- **Mapping** — `projection::ingest::project` maps a raw `space.nuck.*` event to a
  `ProjectionOp`. Pure and unit-tested; unknown types are ignored (ADR 0014), malformed
  known types are rejected. Plaintext `m.room.message` is indexed; `m.room.encrypted`
  is not (ADR 0008/0029).
- **Input** — the AppService endpoints under `/_matrix/app/v1/*`, with `hs_token`
  authorization, verified end-to-end (HTTP transaction → Postgres) against a real
  database.

**Still to build:** the **reconciliation loop** that re-reads room state on startup and
periodically, and the AppService registration itself (currently configured, not
auto-registered).

