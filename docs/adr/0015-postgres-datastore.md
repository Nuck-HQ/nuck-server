# ADR 0015 — Postgres is the nuck-server datastore

- Status: Accepted
- Date: 2026-10-21

## Context

ADR 0003 gives nuck-server its own relational store holding *derived indexes* over
federated state. ADR 0012 flagged the engine as open. Candidates were SQLite
(zero-ops, single-node) and Postgres (concurrency, scale).

## Decision

**Postgres, always.** No SQLite path, no dual-engine support.

- nuck-server uses **Postgres** via `sqlx`.
- The stack (ADR 0012) includes a Postgres container alongside `tuwunel`,
  `nuck-server`, and `livekit`.
- Migrations are managed as SQL migrations (e.g. `sqlx migrate`), versioned and
  forward-only in production.

## Consequences

- One engine to develop, test, and operate against — no dialect branching, no
  doubled test matrix.
- Real concurrency, real indexing, and real query planning for the projection and
  permission lookups.
- Operators run a database container; the install must create the role/database and
  manage the Postgres volume and backups (ADR 0012's backup command must include it).
- Consistent with the Appwrite-style multi-container deployment model.
- SQLite's "rebuildable cache, durability doesn't matter" argument no longer applies
  to the engine choice — but the store is still a **derived index** (ADR 0003) and
  must remain reconstructible from federated events.
