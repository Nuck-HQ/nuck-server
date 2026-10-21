# ADR 0041 — Bot-mediated privileged actions (supersedes ADR 0040's options)

- Status: Accepted
- Date: 2026-10-21
- Supersedes: the options in ADR 0040

## Context

ADR 0040 found that a **scalar** Matrix power level cannot mirror an arbitrary
permission set: the substrate over-grants for non-level-closed sets, and only six
actions are expressible at all. Trying to make the substrate enforce fine-grained
permissions is therefore the wrong approach.

## Decision

**Keep flexible Discord-style roles. Move privileged-action enforcement out of the
substrate's power levels and into nuck-server, with a privileged bot performing the
actions.**

- **Power levels are used only coarsely.** Normal members sit at a low level (they may
  send message-like events); a single **action bot** — an AppService user (ADR 0018) —
  sits at a high level. `state_default`, `redact`, `ban`, and `kick` thresholds are set
  so that **only the bot passes**. A two-tier ladder is **exactly** expressible, so
  ADR 0040's problem does not arise.
- **Channel access is room membership, not power levels.** Who may view or post in a
  channel is who is a member of that channel room, which ADR 0005 already syncs from
  guild roles. No mirroring required.
- **All authorization lives in nuck-server**, using the full permission engine
  (ADR 0020): roles, ordering, per-channel overrides — the whole checklist model.
- **Execution:** the client calls `/_nuck/v1` (ADR 0013); nuck-server authorizes; the
  **action bot** performs the substrate write (state event, ban/kick membership, or
  redaction). A remote member's request is relayed S2S to the guild's home server,
  exactly like invite redemption (ADR 0011). The **request is not a public room
  event**; only the resulting action is.
- **Audit attribution:** the bot records the acting user in the resulting event and the
  audit log, so actions remain attributable even though the bot performed them
  (ADR 0016).
- **Voice already follows this pattern** (nuck-server mints scoped LiveKit tokens,
  ADR 0007).

## Why this is better than mirroring

- **Exact enforcement with full flexibility.** The substrate enforces a coarse,
  trivially-exact tier; nuck-server enforces the fine-grained policy with the complete
  model. Nothing is approximated.
- **More secure under federation.** A malicious or divergent server's user is simply
  low-level and cannot overstep, because the bot is the only writer of privileged
  state. This is stronger than the permissive mirror ADR 0040 exposed.
- **No new approximation surface.** The "which permissions are substrate-enforceable"
  problem disappears; there is no permission we cannot enforce.

## Consequences

- The **action bot is a critical, always-on hot path** and its token is a high-value
  secret (ADR 0028). Its availability bounds moderation availability.
- Privileged actions gain a round-trip through nuck-server (and S2S for remote
  members).
- The **guild's home server is the single point of authority** for its guild's
  privileged actions — already true (ADR 0038), now load-bearing.
- **ADR 0040 is superseded**; its "which option" question is moot.
- **ADR 0006 and ADR 0010 are corrected:** the substrate boundary is now
  *membership + coarse power tiers*, which **is** exactly enforceable, with
  nuck-server authoritative for everything fine-grained.
- **ADR 0022's property test changes:** instead of testing fine-grained mirror
  exactness (impossible), it should test (a) the coarse tiering is exact, and (b)
  the authorization engine itself. The existing `mirror_invariant.rs` tests remain
  useful as documentation of why mirroring is not the enforcement path.
- The permission bitfield and `required_level`s are no longer a security-critical
  substrate mapping; they are an ordering used by the engine and for role comparison.

## Implementation (2026-10-21)

The execution path is implemented for the first privileged actions.

- **Identity** — `api::auth::AuthenticatedUser` resolves the bearer token to a Matrix
  user via `GET /_matrix/client/v3/account/whoami`; there is no second login
  (ADR 0009). A rejected token is `401`; an unreachable substrate is `503` (ADR 0026:
  never silently misbehave).
- **Membership** — `m.room.member` is projected (`memberships`, ADR 0019/0005), so
  guild membership (the guild Space room) and channel access (the channel room) are
  real checks. Privileged actions require guild membership; channel reads are gated by
  channel membership.
- **Authorization** — `authorization::load` reads the guild's roles and the caller's
  role assignments from the projection and computes their standing with the protocol
  engine (`nuck_protocol::authorization`). `GuildAccess` answers the permission and
  hierarchy questions. Only the guild's **home server** acts (ADR 0038); a guild hosted
  elsewhere returns `501` until S2S relay exists.
- **Execution** — `bot::ActionBot` performs the substrate write as the AppService
  **action bot** (ADR 0018/0028): a `space.nuck.*` state event through the C-S state
  endpoint, or a membership ban. The client's request is never a room event — only the
  resulting action is.
- **Attribution** — where the event schema has an actor field it is set (e.g.
  `space.nuck.ban.v1.banned_by`); every action also appends an `audit_log` row keyed by
  the resulting event id, recording the real actor, action, and target (ADR 0016).
- **Endpoints** — `POST /_nuck/v1/guilds/{id}/roles`,
  `PUT /_nuck/v1/guilds/{id}/members/{userId}/roles`,
  `PUT /_nuck/v1/guilds/{id}/bans/{userId}`,
  `GET`/`PUT`/`DELETE /_nuck/v1/channels/{id}/overrides[/{target}]`.

**Open:** cross-server relay for guilds hosted on other servers; and role
update/delete, unban, kick, and redaction. (Guild ownership and membership are now
projected — ADR 0019 — so the owner bypass and membership gating are live.)

Verified: `cargo test --workspace` (80 tests) against a real Postgres and a fake
Tuwunel, plus `cargo clippy --workspace --all-targets` clean. The new `tests/guilds.rs`
drives the full path: token validation, membership gating, the owner bypass, permission
and hierarchy refusals, the can't-grant-what-you-don't-hold guard, the bot's recorded
substrate writes, audit attribution, and channel overrides resolving through the engine.
