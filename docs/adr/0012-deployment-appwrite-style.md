# ADR 0012 — Deployment: multi-container stack with an Appwrite-style installer

- Status: Accepted
- Date: 2026-10-21

## Context

ADR 0003 keeps Tuwunel an unmodified, independently upgradable binary, and ADR 0007
adds LiveKit as a co-deployed dependency. The deployment is therefore inherently
multi-process, but the operator experience must be "everyone can run their own Nuck
server" — ideally one command.

## Decision

Deploy as a **multi-container Docker stack with a one-command installer**, modeled
on **Appwrite's self-hosting approach**:

- **`docker compose`** orchestrates the services: `tuwunel`, `nuck-server`,
  `livekit`, and the supporting data stores. A reverse proxy (e.g. Caddy) handles
  TLS.
- **One-command install**: a single `docker run … --entrypoint="install" …` (or
  equivalent script) performs an interactive install, generating `.env` with the
  server name, secrets, and ports.
- **Pinned, tested versions** of every image, with an override mechanism and a
  warning on untested combinations.
- **A `nuck` CLI** for lifecycle: `up`, `down`, `upgrade`, `backup`, `logs`.

## Consequences

- Operators get a single, familiar, Docker-native install path.
- Tuwunel, nuck-server, and LiveKit remain independently upgradable artifacts, so
  ADR 0003's "unmodified upstream substrate" holds.
- We inherit the operational realities of a multi-container stack (image pulls,
  volumes, networking) and must keep the compose file and installer in sync.
- **Backups are first-class**: one command must consistently back up the Tuwunel
  database + media and the nuck-server store.
- The stack includes a **Postgres** container for the nuck-server store
  (ADR 0015).
