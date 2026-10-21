# nuck-server

Nuck is a Discord alternative built on Matrix. This repository is **nuck-server**,
the backend for a federated network of self-hosted Nuck servers serving the
first-party Nuck client.

Start with the [glossary](./docs/glossary.md), then read the ADRs in order.

## Architecture in one paragraph

Each Nuck server runs an unmodified **Tuwunel** Matrix homeserver as its substrate
(rooms, events, membership, E2EE, media) plus **nuck-server**, a Rust BFF that the
Nuck client talks to. The client speaks a superset of the Matrix Client-Server API
for the substrate and a versioned `/_nuck/v1/*` REST surface for Discord-style
features. Guilds are Matrix Spaces, channels are child rooms, and Nuck-specific state
(roles, permissions, guild settings) federates as custom `space.nuck.*` room events.
nuck-server projects that federated state into a **Postgres** index for fast queries
and mirrors effective permissions into standard room power levels, which are the real
security boundary under federation. Voice/video use MatrixRTC signalling and a
LiveKit SFU. The whole stack ships as a multi-container, one-command install.

## Decisions

| ADR | Decision |
|---|---|
| [0001](./docs/adr/0001-nuck-only-network-federated.md) | Nuck-only network (client + server); federated among Nuck servers |
| [0002](./docs/adr/0002-federation-via-matrix.md) | Federation rides on Matrix federation; Nuck state as custom events |
| [0003](./docs/adr/0003-tuwunel-substrate-hybrid-state.md) | Tuwunel substrate; hybrid state (substrate + Postgres index) |
| [0004](./docs/adr/0004-rust-stack.md) | nuck-server is written in Rust (`axum` + `ruma` + `sqlx`) |
| [0005](./docs/adr/0005-guild-channel-mapping.md) | Guild = Matrix Space; Channel = child room; guild-level state model |
| [0006](./docs/adr/0006-discord-style-roles.md) | Discord-style roles, mirrored into Matrix auth |
| [0007](./docs/adr/0007-voice-video-matrixrtc-livekit.md) | Voice/video via MatrixRTC + LiveKit; single SFU per call |
| [0008](./docs/adr/0008-e2ee-stance.md) | E2EE stance: plaintext guild channels, E2EE DMs |
| [0009](./docs/adr/0009-matrix-identity.md) | Matrix user IDs are canonical Nuck identity |
| [0010](./docs/adr/0010-federation-trust-and-versioning.md) | Open federation + capability negotiation; power levels are the boundary |
| [0011](./docs/adr/0011-guild-discovery-joining.md) | Guild discovery/joining: invite codes + optional directory |
| [0012](./docs/adr/0012-deployment-appwrite-style.md) | Deployment: multi-container stack, Appwrite-style installer |
| [0013](./docs/adr/0013-api-surface.md) | API surface: namespaced REST + sync-driven Nuck state |
| [0014](./docs/adr/0014-event-namespace-versioning.md) | Custom event namespace `space.nuck.*`; version in type name |
| [0015](./docs/adr/0015-postgres-datastore.md) | Postgres is the nuck-server datastore |
| [0016](./docs/adr/0016-moderation-model.md) | Moderation: Discord-style primitives (v1); automod deferred |
| [0017](./docs/adr/0017-presence-matrix-native.md) | Presence: Matrix-native for v1; rich presence deferred |
| [0018](./docs/adr/0018-bot-platform.md) | Nuck-native bot platform on AppService identities |
| [0019](./docs/adr/0019-projection-pipeline.md) | Projection pipeline: AppService push + reconciliation |
| [0020](./docs/adr/0020-permission-set-and-resolution.md) | Permission set & resolution (Discord-aligned) |
| [0021](./docs/adr/0021-guild-directory.md) | Guild directory: per-server, no central service |
| [0022](./docs/adr/0022-testing-ci.md) | Testing & CI: property tests, nightly federation harness |
| [0023](./docs/adr/0023-message-features.md) | Message features: inherit Matrix; custom only where needed |
| [0024](./docs/adr/0024-federation-blocklists.md) | Federation blocklists: local + subscribable lists |
| [0025](./docs/adr/0025-guild-join-protection.md) | Guild join protection: invites, rate limits, join gates |
| [0026](./docs/adr/0026-versioning-policy.md) | Versioning: pinned deployment, semver'd protocol |
| [0027](./docs/adr/0027-client-protocol-crate-tauri.md) | Client: shared `nuck-protocol` crate; Tauri |
| [0028](./docs/adr/0028-trust-and-privilege-model.md) | Trust & privilege: private network, scoped credentials |
| [0029](./docs/adr/0029-message-search.md) | Message search: Postgres FTS behind an interface |
| [0030](./docs/adr/0030-dms-friends-blocking.md) | DMs, friends, blocking |
| [0031](./docs/adr/0031-notifications.md) | Notifications: Matrix push rules; mobile deferred |
| [0032](./docs/adr/0032-call-e2ee.md) | Call E2EE: none in v1; DM-call E2EE deferred |
| [0033](./docs/adr/0033-observability.md) | Observability: instrument, don't bundle |
| [0034](./docs/adr/0034-backup-restore.md) | Backup & restore: one-command consistent snapshot |
| [0035](./docs/adr/0035-retention-export-deletion.md) | Data retention, export, and deletion |
| [0036](./docs/adr/0036-rate-limiting.md) | Rate limiting: identity-aware per-route limits |
| [0037](./docs/adr/0037-home-server-registration.md) | Home server registration policy |
| [0038](./docs/adr/0038-guild-creation-and-home-server.md) | Guild creation and guild home server |
| [0039](./docs/adr/0039-unredacted-retention-policy.md) | Unredacted event retention: operator-only |
| [0040](./docs/adr/0040-scalar-power-level-limitation.md) | **Finding:** a scalar power level cannot mirror an arbitrary permission set (superseded by 0041) |
| [0041](./docs/adr/0041-bot-mediated-privileged-actions.md) | Bot-mediated privileged actions; power levels used only coarsely |
| [0042](./docs/adr/0042-protocol-specification-artifacts.md) | Protocol spec finalized: permission table, event registry, role IDs |
| [0043](./docs/adr/0043-state-removal-empty-content.md) | State removal: empty-content encoding (no Matrix state deletion) |
| [0044](./docs/adr/0044-v1-friends-storage.md) | v1 friends & blocking held in the projection (single server) |
| [0045](./docs/adr/0045-user-state-key-encoding.md) | User-id state keys encoded (Tuwunel auth constraint) |

## Status

The deployable MVP (below) is met, and the **client-ready v1 surface** is implemented:
guilds, channels, roles, members, bans, per-channel overrides, invites & joining,
registration policy, DMs, friends, blocking, and the audit log — all under `/_nuck/v1`
plus the proxied Matrix C-S API. Deferred: federation, voice, bots, search, directory,
and reports.

- [`docs/protocol/api.md`](./docs/protocol/api.md) — the v1 endpoint inventory (final).

## Building it

- [`mvp.md`](./docs/mvp.md) — the deployable-MVP definition, milestones, and runbook.

## Protocol specification

The concrete protocol. `permissions.md` and `events.md` are **final** (ADR 0042);
`api.md` is still a draft.

- [`protocol/permissions.md`](./docs/protocol/permissions.md) — the permission bitfield and the coarse power-level mapping.
- [`protocol/events.md`](./docs/protocol/events.md) — the `space.nuck.*` event registry.
- [`protocol/api.md`](./docs/protocol/api.md) — the `/_nuck/v1` endpoint inventory (draft).

## Verification

- [`verification/tuwunel.md`](./docs/verification/tuwunel.md) — checks of our substrate
  assumptions against Tuwunel's docs and source.

## Open questions

See [open-questions.md](./docs/open-questions.md).
