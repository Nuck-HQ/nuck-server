# Glossary

Terms are added as they are pinned down during design.

- **Nuck** — A Discord alternative built on Matrix. Product name for the whole
  system (server + client).
- **Nuck client** — The first-party client application. The only supported client.
  Consumes Nuck's custom APIs; is not a generic Matrix client.
- **nuck-server** — The backend service described by this repository. Owns the
  Discord-like layer (permissions, voice/video, guild structure) and mediates access
  to the underlying Matrix homeserver.
- **Homeserver** — The Matrix server implementation used as the substrate for rooms,
  events, and identity. An implementation detail of Nuck, not a compatibility
  surface. (Exact choice: TBD.)
- **Matrix** — The open protocol/data model used underneath. Used as transport and
  storage substrate; not treated as an interop contract (see ADR 0001).
- **Guild** — Nuck's top-level community, analogous to a Discord server.
  Implemented as a Matrix **Space** room carrying guild-level state (roles,
  settings, member→role assignments). Owns channels and members.
- **Channel** — A room that is a child of a Guild space (`m.space.child`). Has a
  type: text, voice, or category.
- **Space** — A Matrix construct: a room with `m.space` type whose children are
  other rooms. Nuck uses it to express Guild structure so it federates natively.
- **Role** — A named, ordered set of permissions within a Guild, assigned to
  members. Definition of ordering/permissions: TBD (see ADR 0006).
- **Permission** — A discrete capability within a Guild (e.g. send message, manage
  channel, connect to voice). Definition: see ADR 0006. Enforced by **nuck-server**
  (ADR 0041), not by Matrix power levels.
- **Action bot** — A privileged AppService user holding a high room power level; the
  only writer of privileged guild state (roles, bans, redactions). It performs actions
  authorized by nuck-server on behalf of members. Power levels are otherwise used only
  coarsely (members vs. the bot), which is exactly enforceable (ADR 0041).
- **Home server** — The Nuck server named in a user's Matrix ID (`@user:server`).
  Owns that user's account, sessions, and devices.
- **Guild home server** — The server on which a guild is hosted: the creator's home
  server at creation time (ADR 0038). Authoritative for the guild's space, channels,
  invites, bans, and directory listing. Permanent in v1.
- **Invite code** — A shareable, host-authoritative token that grants guild
  membership when redeemed, from any Nuck server (ADR 0011).
- **Guild ban** — A ban expressed as federated guild state; removes a user from a
  guild and blocks rejoin, enforced by all participating servers (ADR 0016).
- **Server ban** — A local, non-federated operator ban of a user from a Nuck server
  (ADR 0016).
- **Timeout** — A temporary mute of a guild member (ADR 0016).
- **Audit log** — An append-only, permission-gated per-guild record of moderation
  actions, stored as message events in a dedicated audit room (ADR 0016).
- **SFU** — Selective Forwarding Unit. LiveKit carries voice/video media for Nuck
  calls (ADR 0007).
- **MatrixRTC** — Matrix's federated real-time (call) mechanism. Nuck uses it for
  voice/video signalling (ADR 0007).
- **Custom event** — A Matrix event with a `space.nuck.*` type, carrying Nuck
  semantics that federate via the substrate (ADR 0014).
