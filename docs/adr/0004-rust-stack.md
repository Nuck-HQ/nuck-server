# ADR 0004 — nuck-server is written in Rust

- Status: Accepted
- Date: 2026-10-21

## Context

nuck-server is a separate process that talks to Tuwunel over HTTP (Matrix C-S API
plus admin/federation endpoints). Nothing about Tuwunel forces a language choice,
so the stack was chosen on its own merits.

Under ADR 0002/0003 nuck-server must define, validate, and federate custom Matrix
events, and project them into a relational store. Correct Matrix event typing is
therefore load-bearing.

## Decision

nuck-server is written in **Rust**, using:

- **`axum`** — HTTP API and reverse-proxy layer.
- **`ruma`** — canonical Matrix types (events, IDs, signatures, canonical JSON).
  The same library Tuwunel uses, so event schemas are spec-exact by construction.
- **`sqlx`** — the derived relational store (ADR 0003).

## Consequences

- Custom Nuck events are defined as `ruma` event types, giving compile-time
  correctness across the federation boundary.
- We can reuse `matrix-rust-sdk` if nuck-server ever needs to act as a Matrix
  client, and can go in-process with Tuwunel later without a language change.
- Deployment symmetry: single static binary, same toolchain family as the substrate.
- Cost: slower iteration than a scripting stack; `ruma` has a learning curve for
  custom event registration.
