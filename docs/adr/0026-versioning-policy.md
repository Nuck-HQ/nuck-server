# ADR 0026 — Versioning: pinned deployment, semver'd protocol with a compatibility window

- Status: Accepted
- Date: 2026-10-21

## Context

A federated network requires version compatibility across independently upgraded
servers. ADR 0010 established capability negotiation; ADR 0012 pins a tested
deployment matrix. The support policy was undefined.

## Decision

**Two distinct version numbers, deliberately kept separate:**

- **Product version** (e.g. nuck-server 1.4.2) — moves freely.
- **Nuck protocol version** — moves rarely; negotiated via `/_nuck/v1/server-info`.

**Deployment (a):**

- The installer ships a **pinned, tested version matrix** of Tuwunel / nuck-server /
  LiveKit / Postgres (ADR 0012). Upgrades are **explicit operator actions** with a
  tested path. No auto-update.

**Protocol (c):**

- `space.nuck.*` semantics are **semver'd** with a documented **compatibility
  window**: a server must speak the current and previous **major** protocol version.
- **Additive** changes are compatible by default (tolerant parsing, ADR 0014); only
  **breaking** changes bump the major version.
- Newer features are **feature-gated** for older peers (ADR 0010).

**Hard requirement:** the client and server must **never silently misbehave** with an
incompatible peer — they degrade with a clear signal, never half-work.

## Consequences

- No surprise breaking changes mid-federation; operators upgrade deliberately.
- Old servers keep working for at least one major protocol cycle.
- We must maintain the protocol version, the compatibility window, and the
  feature-gate matrix as first-class artifacts.
- The `server-info` endpoint becomes load-bearing for interoperability and must be
  stable and cheap.
