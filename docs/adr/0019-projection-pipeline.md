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

- **Schema** — `migrations/0001_init.sql` (guilds, roles, members,
  channels, overrides, bans, invites, audit_log, messages + `tsvector`, processed_events,
  projection_state).
- **Write path** — `projection::Projection` with idempotent upserts and `apply(op)`;
  `record_event` is the idempotency guard.
- **Mapping** — `projection::ingest::project` maps a raw `space.nuck.*` event to a
  `ProjectionOp`. Pure and unit-tested; unknown types are ignored (ADR 0014), malformed
  known types are rejected. Plaintext `m.room.message` is indexed; `m.room.encrypted`
  is not (ADR 0008/0029). `m.room.member` maps to the `memberships` table (room, user →
  membership state), so guild and channel membership are queryable and reconciled like
  the rest of current state (ADR 0005); redacted member events (empty content) are
  ignored rather than rejected. `m.room.create` for a `m.space` room maps to the
  `guild_owners` table (`creator`, or the event's sender on room v11+), so the owner
  bypass (ADR 0020) is real.
- **Input** — the AppService endpoints under `/_matrix/app/v1/*`, with `hs_token`
  authorization, verified end-to-end (HTTP transaction → Postgres) against a real
  database.

**Still to build:** the **reconciliation loop** that re-reads room state on startup and
periodically, and the AppService registration itself (currently configured, not
auto-registered).

### Reconciliation & registration (2026-10-21)

The pipeline is now complete end-to-end.

- **Reconciliation loop** — `src/projection/reconcile.rs`. On startup and
  every `NUCK_RECONCILE_INTERVAL_SECS` it re-reads each guild room's resolved state
  (`GET /_synapse/admin/v1/rooms/{id}/state`, admin bypass), applies the resulting ops,
  and **prunes** rows whose writing event is no longer in current state
  (`Projection::reconcile_room`) — repairing drift after a missed or out-of-order push
  and making the store genuinely rebuildable (ADR 0003). Guilds are reconciled before
  channels, so channels created since the last pass are picked up in the same pass. A
  dead substrate is a per-room failure, never a fatal error.
- **Projector auto-join** — when a reconciled room's state carries
  `space.nuck.guild.v1`, the projector user is joined via
  `POST /_synapse/admin/v1/join/{room}`, which is what makes federated delivery work
  (see the verification note on auto-join).
- **Message backfill** — the startup pass *only* indexes the most recent timeline
  events of each channel room (`GET .../rooms/{id}/messages`) into `messages`, bounded
  by `NUCK_MESSAGE_BACKFILL_LIMIT`. Current state is never reconstructed by replaying
  events; only append-only search is backfilled.
- **AppService registration** — `src/matrix/registration.rs` generates the
  projector's registration document (id, push URL, tokens, exclusive projector
  namespace) and writes it as YAML to `APPSERVICE_REGISTRATION_PATH`. Tuwunel loads
  `appservice_dir` files on startup; the same YAML applies live via
  `!admin appservices register`. There is no HTTP registration endpoint (verified in
  Tuwunel's `docs/appservices.md`).
- **Substrate client** — `src/matrix/` speaks the admin API for real
  (room discovery, state, messages, join) with bounded timeouts, so a wedged substrate
  cannot stall the loop.

Verified: `cargo test --workspace` (45 tests) with and without `TEST_DATABASE_URL`, and
`cargo clippy --workspace --all-targets` clean. New integration tests cover drift repair
and pruning (updates, removals, append-only rows untouched, idempotency) and
substrate-outage resilience.

## Update (2026-10-22) — reconciliation reads as the projector

Superseded by [ADR 0046](./0046-no-stored-admin-token.md): the reconciliation loop no
longer uses the admin API. It reads resolved room state and history **as the projector**
— a normal member of every guild room — over the C-S API (`GET /rooms/{id}/state`,
`GET /rooms/{id}/messages`), discovers rooms from the projector's `GET /joined_rooms`,
and joins with the AppService token. No admin token is held, so reconciliation is **on by
default** rather than disabled without one. (History is bounded by the room's history
visibility; guild rooms use `shared`.)


