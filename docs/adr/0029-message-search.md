# ADR 0029 — Message search: Postgres FTS behind a swappable interface

- Status: Accepted
- Date: 2026-10-21

## Context

Server-side search is wanted for plaintext guild channels (ADR 0008). Options were
Postgres full-text search, a dedicated search engine, or client-side only.

## Decision

- **v1: Postgres full-text search (option a).** `tsvector`/`tsquery` over the
  projection (ADR 0019). We already run Postgres (ADR 0015) and already project
  message events, so this adds no service.
- **Behind a search interface** so a dedicated engine (option b — Meilisearch /
  Typesense / Elasticsearch) can replace it later without touching callers.
- **Permission-filtered results.** A user may only see results from channels they can
  read; the search layer filters by per-user channel access, not a global query.
- **E2EE DMs are never indexed** (ADR 0008); search has no DM results, and the client
  must not imply otherwise.

## Consequences

- No additional operational surface for v1; search ships with the existing stack.
- Relevance and typo tolerance are adequate but not Discord-grade; that is the
  accepted trade-off for a non-differentiating feature.
- Permission filtering makes search queries per-user and thus more complex and
  slower than a naive global index; this must be designed carefully from the start.
- If (b) is adopted, a sync pipeline from Postgres/events into the engine becomes
  required infrastructure.
