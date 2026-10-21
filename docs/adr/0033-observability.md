# ADR 0033 — Observability: instrument, don't bundle

- Status: Accepted
- Date: 2026-10-21

## Context

A federated system cannot be debugged from logs alone ("why did guild state diverge
between two servers?"), but bundling a full observability stack by default fights the
one-command self-host promise (ADR 0012).

## Decision

**Instrument, don't bundle (option a).**

- **`tracing`** structured logs with spans across the API, projection, permission
  engine, and federation calls.
- **Prometheus metrics** exposed for scraping.
- **OTLP trace export** so operators with an existing backend point at it.
- **Optional compose profile** for a basic Grafana stack; not installed by default.

**Required metrics (the early-warning system for federation problems):**

- per-peer federation success/error rates;
- **projection lag** (event received → indexed);
- permission-engine latency and **PL-mirroring divergence count**;
- gateway connections and dropped/backpressured events;
- AppService transaction failures.

## Consequences

- Operators can debug federation without running a heavy stack by default.
- The metrics above are effectively part of the product's operability contract and
  must be maintained as the system evolves.
- PL-mirroring divergence being a metric ties directly to the core invariant in
  ADR 0022 — production visibility into the same property CI property-tests.
- Full-stack observability (option c) remains available to those who want it, without
  imposing it on everyone.
