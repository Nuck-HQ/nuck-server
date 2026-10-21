# ADR 0003 — Tuwunel as substrate; hybrid state model

- Status: Accepted
- Date: 2026-10-21

## Context

ADR 0002 requires a real Matrix homeserver underneath each Nuck server. Candidate
substrates: Synapse (mature, heavy), Conduit (Rust, single binary, now effectively
superseded), Tuwunel (Rust, single binary, actively maintained).

Conduit-family storage is a **key-value engine**, not a relational database: ~80+
named byte-keyed trees behind a `KvTree` trait, composite keys separated by `0xff`,
IDs compressed to integers, room state stored as chained diffs, and migrations that
are imperative Rust code. There is no query language and no joins, and the database
is bound to `server_name`.

That makes Nuck-specific state (guilds, roles, permissions, voice metadata)
awkward to store and query inside the homeserver, even though storing it there would
federate for free.

## Decision

**Substrate: Tuwunel.** The official successor to conduwuit/Conduit: Rust,
single-binary, RocksDB-backed, actively maintained, with native MatrixRTC and OIDC,
and in-place migration from Conduit/conduwuit. "Conduit" as a choice is now
effectively "Tuwunel."

**Hybrid state model (option c):**

- **Homeserver (Tuwunel)** owns the substrate: rooms, events, membership, E2EE,
  media, and the federated transport.
- **Nuck state federates as custom Matrix room events/state** (per ADR 0002), so it
  is replicated to every participating Nuck server.
- **nuck-server maintains its own relational store** (SQL) holding *derived* indexes
  over that federated state — guild/role/permission lookups, membership indexes,
  search — rebuilt from the event stream. This store is a **cache/index, not the
  source of truth**; it must be reconstructible by replaying federated events.

## Consequences

- Product features get real relational queries without patching the homeserver's KV
  internals.
- The homeserver stays an unmodified, upgradable upstream Tuwunel.
- We own a projection/derivation pipeline from federated events → SQL indexes, and
  must handle out-of-order/backfilled events idempotently.
- Two stores must stay consistent; the SQL store may lag or be wiped and rebuilt.
- Identity is bound to the homeserver's `server_name`; the SQL store must key on
  fully-qualified Matrix IDs (`@user:server`, `!room:server`).
