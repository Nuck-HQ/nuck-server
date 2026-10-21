# Protocol: `/_nuck/v1` Endpoint Inventory (v1 — **final**)

The client-facing Nuck REST surface (ADR 0013), versioned in the path and negotiated via
`server-info` (ADR 0026). This document finalizes the draft that ADR 0042 left open:
conventions, the shared error model, pagination, idempotency, the impersonation boundary,
and the endpoint inventory.

**Version.** This is **v1**. A breaking change to any shape here bumps
`PROTOCOL_VERSION` and adds a new path version (`/_nuck/v2`) — never mutating v1
(ADR 0026).

**Where the shapes live.** Like [`events.md`](./events.md) and
[`permissions.md`](./permissions.md), **the code is the registry**: the request/response
DTOs live in `nuck-protocol/src/api.rs` and are the single source of truth. This document
is derived from them; `nuck-protocol/tests/api_registry.rs` round-trips every DTO and
checks the endpoint table (the `ENDPOINTS` const in the crate) for structural sanity
(unique, well-formed paths). Field-level schemas are therefore normative **in the crate**,
not duplicated here.

## Scope

**In scope for v1** (single server): guilds, channels, roles, members, bans, per-channel
overrides, invites & joining, registration policy, DMs, friends, blocking, and the audit
log.

**Explicitly deferred** (not advertised, do not build a client against them): voice/video
(ADR 0007), the bot platform + gateway WebSocket (ADR 0018), search (ADR 0029),
the public directory (ADR 0021), federation as a whole (ADR 0010/0011 S2S — v1 is a
single server), reports (ADR 0016), automod, rich presence (ADR 0017), push (ADR 0031),
retention (ADR 0035), OIDC (ADR 0009).

## Conventions

- **Base path:** `/_nuck/v1/`.
- **Content type:** `application/json`; field names are `snake_case` (Matrix/MSC style).
- **Auth:** `Authorization: Bearer <matrix access token>`, validated against the
  substrate (`GET /_matrix/client/v3/account/whoami`). Public endpoints are marked
  `public`. A missing/invalid token is `401 M_MISSING_TOKEN` / `M_UNKNOWN_TOKEN`.
- **Identifiers:** `guild_id` and `channel_id` are Matrix room ids (`!…`); `user_id` is a
  Matrix user id (`@…`); `role_id` is the opaque `r` + 12 hex (or `everyone`) from
  `events.md`; invite codes are opaque, unguessable strings (`nuck_` + 22 base62).
- **Timestamps:** milliseconds since the Unix epoch (matches `events.md`).
- **Substrate:** everything not listed here is the proxied Matrix C-S / media API.

### Error model

A single JSON body on every error:

```json
{ "errcode": "M_FORBIDDEN", "error": "human readable", "retry_after_ms": 5000 }
```

`ApiError { errcode: string, error: string, retry_after_ms?: number }` (the field is
optional and additive — tolerant parsers ignore it). The HTTP status is derived from
`errcode`:

| `errcode` | HTTP | Meaning |
|---|---|---|
| `M_FORBIDDEN` | 403 | Authenticated but not permitted |
| `M_UNAUTHORIZED` | 401 | Action requires a session |
| `M_MISSING_TOKEN` | 401 | No `Authorization` header |
| `M_UNKNOWN_TOKEN` | 401 | Token rejected by the substrate |
| `M_BAD_JSON` / `M_NOT_JSON` | 400 | Malformed body |
| `M_MISSING_PARAM` / `M_INVALID_PARAM` | 400 | Missing/invalid field |
| `M_NOT_FOUND` | 404 | Unknown guild/channel/role/user/invite |
| `M_CONFLICT` | 409 | State contradicts the request (e.g. already banned) |
| `M_LIMIT_EXCEEDED` | 429 | Rate limited; `retry_after_ms` is set |
| `M_UNRECOGNIZED` | 501 | Well-formed but this server cannot serve it |
| `M_UNKNOWN` | 500 | Unexpected failure |

A client must treat an unknown `errcode` by its HTTP status (ADR 0026: never silently
misbehave). Errors never carry a 2xx status.

### Pagination

Cursor-based, uniform across every list endpoint.

- **Request:** `?limit=<1..200>&cursor=<opaque>` (`limit` default `50`).
- **Response:** `{ "items": [ … ], "next_cursor": "…" }` — `next_cursor` is present only
  when more items exist, and is opaque (never parsed by the client).
- **Ordering** is documented per endpoint (it is stable for a given cursor).

### Idempotency

Create/redeem/register accept an optional `Idempotency-Key` header. The server caches the
response per `(user, method, path, key)` for 24h; a replay returns the cached response.
Reusing a key with a different body is `400 M_INVALID_PARAM`.

### Rate limits (ADR 0036)

Identity-aware, per route. Tightest on `POST /register`, `POST /invites/{code}/redeem`,
`POST /friends/requests`, and `POST /dm`. Over the limit → `429 M_LIMIT_EXCEEDED` with
`retry_after_ms`.

## Impersonation boundary (ADR 0028)

Every row below carries an **auth class**:

- **public** — no token.
- **as-user** — nuck-server validates the token and forwards the request to the substrate
  acting **as that user** (the C-S proxy, or a C-S call on their behalf).
- **authorized+bot** — nuck-server authorizes with the permission engine against projected
  state, then the **action bot** performs the substrate write (ADR 0041); the actor is
  recorded in the event and audit log.
- **admin** — an **operator admin**: a user id configured in `NUCK_ADMIN_USERS`. Server
  admins are an operator concept, **not** a guild role (ADR 0037); they do not gain guild
  permissions.

nuck-server never performs an operation as a *different* user except through the action
bot (attributed) or the narrow admin login-as (ADR 0028, `…/users/{id}/login`).

## Capabilities

`GET /server-info` returns `capabilities: [string]`. **v1 advertises exactly:**
`guilds`, `channels`, `roles`, `members`, `bans`, `audit_log`, `invites`,
`join_requests`, `registration`, `friends`, `blocking`, `direct_messages`.

A capability is a **promise**: a client may rely on it and must ignore names it does not
know. A feature that is deferred is **absent** — the current build's aspirational list
(`voice`, `bots`, `directory`, `search`) is removed. When those land, they add their name
(additive; same protocol version).

## Server

| Method | Path | Auth | Perm | Request | Response |
|---|---|---|---|---|---|
| `GET` | `/server-info` | public | — | — | `ServerInfo` |
| `GET` | `/me` | as-user | — | — | `Me` |

`Me` — current profile + Nuck settings. Nuck settings are the user's account data
(`space.nuck.settings.v1`), read/written as the user via the proxy.

## Registration & sessions (ADR 0037)

| Method | Path | Auth | Perm | Request | Response |
|---|---|---|---|---|---|
| `GET` | `/register/policy` | public | — | — | `RegisterPolicy` |
| `POST` | `/register` | public | — | `RegisterRequest` | `RegisterResponse` |
| `GET` | `/register/requests` | admin | — | — | `RegisterRequestsResponse` (paginated) |
| `POST` | `/register/requests/{id}/approve` | admin | — | — | `RegisterRequestOutcome` |
| `POST` | `/register/requests/{id}/deny` | admin | — | — | `RegisterRequestOutcome` |

- `RegisterPolicy { mode: "open"|"token"|"approval"|"closed", token_required: bool, email_required: bool, captcha: bool }`. `captcha` is always `false` (Tuwunel has no
  CAPTCHA; ADR 0037 verification). The client uses this to render the form.
- `POST /register` applies the policy. `open`/`token` create the user immediately
  (via the substrate's C-S registration) and return a session; `token` requires the
  request's `token`. `approval` returns **202** with a pending `request_id` and **holds
  the account**: it is created, but its `POST /login` is refused until an admin approves
  (ADR 0046). `closed` → `403 M_FORBIDDEN`. Honors email verification when the substrate
  requires it.
- **The proxied substrate `POST /_matrix/client/*/register` is disabled** (`403
  M_FORBIDDEN`) so registration cannot bypass policy. Login and all other C-S verbs pass
  through (except a pending `approval` user's login, which is refused).

## Guilds (ADR 0005/0006)

| Method | Path | Auth | Perm | Request | Response |
|---|---|---|---|---|---|
| `POST` | `/guilds` | authorized+bot | — (any user) | `CreateGuildRequest` | `GuildResponse` |
| `GET` | `/guilds` | as-user | — | — | `GuildsResponse` |
| `GET` | `/guilds/{guildId}` | as-user | member | — | `GuildView` |
| `PATCH` | `/guilds/{guildId}` | authorized+bot | `MANAGE_GUILD` | `UpdateGuildRequest` | `ActionResponse` |
| `DELETE` | `/guilds/{guildId}` | authorized+bot | owner | — | `ActionResponse` |
| `GET` | `/guilds/{guildId}/members` | as-user | member | — | `MembersResponse` (paginated) |
| `GET` | `/guilds/{guildId}/roles` | as-user | member | — | `RolesResponse` |
| `POST` | `/guilds/{guildId}/roles` | authorized+bot | `MANAGE_ROLES` + hierarchy | `CreateRoleRequest` | `CreateRoleResponse` |
| `PATCH` | `/guilds/{guildId}/roles/{roleId}` | authorized+bot | `MANAGE_ROLES` + hierarchy | `UpdateRoleRequest` | `ActionResponse` |
| `DELETE` | `/guilds/{guildId}/roles/{roleId}` | authorized+bot | `MANAGE_ROLES` + hierarchy | — | `ActionResponse` |
| `PUT` | `/guilds/{guildId}/members/{userId}/roles` | authorized+bot | `MANAGE_ROLES` + hierarchy | `SetMemberRolesRequest` | `ActionResponse` |
| `DELETE` | `/guilds/{guildId}/members/{userId}` | authorized+bot | `KICK_MEMBERS` + hierarchy | `KickRequest` | `ActionResponse` |

- **`GET /guilds`** lists the guilds the caller is a member of (from the projection),
  ordered by guild id. **This did not exist before and is required to render a client at
  all.**
- `GuildView` keeps its current shape (`guild_id`, `name`, `home_server`, `owner`,
  `channels`, `roles`). It is the aggregate a client renders a guild from.
- `MembersResponse` items are `MemberInfo { user_id, membership, roles, display_name? }`.
  `membership` is the projected `m.room.member` state.
- `PATCH` changes name/icon/topic/`join_mode`/`directory_visibility`
  (the `space.nuck.guild.v1` fields), each optional.
- Hierarchical permissions: a caller may only act on roles/members **strictly below** their
  highest role, and may only grant permissions they themselves hold (ADR 0020; the
  enforcement already exists).

## Channels (ADR 0005/0020)

| Method | Path | Auth | Perm | Request | Response |
|---|---|---|---|---|---|
| `GET` | `/guilds/{guildId}/channels` | as-user | member | — | `ChannelsResponse` |
| `POST` | `/guilds/{guildId}/channels` | authorized+bot | `MANAGE_CHANNELS` | `CreateChannelRequest` | `ChannelInfo` |
| `PATCH` | `/channels/{channelId}` | authorized+bot | `MANAGE_CHANNELS` | `UpdateChannelRequest` | `ActionResponse` |
| `DELETE` | `/channels/{channelId}` | authorized+bot | `MANAGE_CHANNELS` | — | `ActionResponse` |
| `GET` | `/channels/{channelId}/overrides` | as-user | channel member | — | `ChannelOverridesResponse` |
| `PUT` | `/channels/{channelId}/overrides/{target}` | authorized+bot | `MANAGE_CHANNELS` | `SetChannelOverrideRequest` | `ActionResponse` |
| `DELETE` | `/channels/{channelId}/overrides/{target}` | authorized+bot | `MANAGE_CHANNELS` | — | `ActionResponse` |

- `UpdateChannelRequest` covers `name`, `topic`, `kind`, `position`, `category` — all
  optional; only `space.nuck.channel.v1` + `m.room.*` fields change.
- `ChannelsResponse` is ordered by `position`, then channel id.

## Invites & joining (ADR 0011/0025)

| Method | Path | Auth | Perm | Request | Response |
|---|---|---|---|---|---|
| `POST` | `/guilds/{guildId}/invites` | authorized+bot | `CREATE_INSTANT_INVITE` | `CreateInviteRequest` | `CreateInviteResponse` |
| `GET` | `/guilds/{guildId}/invites` | authorized+bot | `MANAGE_GUILD` | — | `InvitesResponse` |
| `DELETE` | `/guilds/{guildId}/invites/{code}` | authorized+bot | `MANAGE_GUILD` | — | `ActionResponse` |
| `GET` | `/invites/{code}` | public | — | — | `InvitePreview` |
| `POST` | `/invites/{code}/redeem` | as-user | — | — | `RedeemResponse` |
| `POST` | `/guilds/{guildId}/join-requests` | as-user | non-member | `JoinRequestCreate` | `JoinRequestResponse` |
| `GET` | `/guilds/{guildId}/join-requests` | as-user | `MANAGE_GUILD` | — | `JoinRequestsResponse` (paginated) |
| `POST` | `/guilds/{guildId}/join-requests/{userId}/approve` | authorized+bot | `MANAGE_GUILD` | — | `ActionResponse` |
| `POST` | `/guilds/{guildId}/join-requests/{userId}/deny` | authorized+bot | `MANAGE_GUILD` | — | `ActionResponse` |

- `CreateInviteRequest { max_uses?, expires_in_ms? }`; the response returns the `code` and
  a shareable `url`. The invite is written as `space.nuck.invite.v1` (keyed by code hash).
- `InvitePreview` is the **public** pre-redeem view: `{ guild_id, guild_name, guild_icon?, member_count, join_mode }`.
- `POST /invites/{code}/redeem` validates the code (uses/expiry/revocation), rejects banned
  users, and joins the caller to the guild Space + initial channels via the bot. Idempotent
  per caller (re-redeeming is a no-op success).
- Join requests are for `join_mode = "approval"`; `GET` returns the queue ordered
  oldest-first.

## Friends, blocking & DMs (ADR 0030)

Concrete design: [`friends-dms.md`](./friends-dms.md). Summary of the surface:

| Method | Path | Auth | Perm | Request | Response |
|---|---|---|---|---|---|
| `GET` | `/friends` | as-user | — | — | `FriendsResponse` |
| `GET` | `/friends/requests` | as-user | — | — | `FriendRequestsResponse` |
| `POST` | `/friends/requests` | as-user | — | `FriendRequestCreate` | `ActionResponse` |
| `POST` | `/friends/requests/{userId}/accept` | as-user | — | — | `FriendsResponse` |
| `POST` | `/friends/requests/{userId}/decline` | as-user | — | — | `ActionResponse` |
| `DELETE` | `/friends/{userId}` | as-user | — | — | `ActionResponse` |
| `GET` | `/blocks` | as-user | — | — | `BlocksResponse` |
| `PUT` | `/blocks/{userId}` | as-user | — | — | `BlocksResponse` |
| `DELETE` | `/blocks/{userId}` | as-user | — | — | `BlocksResponse` |
| `POST` | `/dm` | as-user | not blocked | `CreateDmRequest` | `CreateDmResponse` |

Friendship and blocking are **per-user, non-consensus, best-effort** state held on the
user's own server (ADR 0030) — never a security boundary.

## Moderation & audit log (ADR 0016)

Bans (above, in Guilds) plus:

| Method | Path | Auth | Perm | Request | Response |
|---|---|---|---|---|---|
| `GET` | `/guilds/{guildId}/bans` | as-user | `BAN_MEMBERS` | — | `BansResponse` (paginated) |
| `PUT` | `/guilds/{guildId}/bans/{userId}` | authorized+bot | `BAN_MEMBERS` + hierarchy | `BanRequest` | `ActionResponse` |
| `DELETE` | `/guilds/{guildId}/bans/{userId}` | authorized+bot | `BAN_MEMBERS` + hierarchy | — | `ActionResponse` |
| `GET` | `/guilds/{guildId}/audit-log` | as-user | `VIEW_AUDIT_LOG` | — | `AuditLogResponse` (paginated) |

- `AuditLogResponse` items are `AuditEntry { id, actor, action, target?, reason?, event_id, created_at }`,
  ordered newest-first, projected from the per-guild audit room (ADR 0016/0019).
- Kicking is `DELETE /guilds/{guildId}/members/{userId}` (above).
- **Deferred within moderation:** timeout/mute, warn, message purge, and reports
  (ADR 0016 reports are net-new Nuck work; not v1).

## Resolved open issues (were listed in the draft)

- **Content schemas** → normative in `nuck-protocol/src/api.rs`, round-tripped by
  `api_registry.rs`.
- **Shared error model** → the `ApiError` table above.
- **Pagination** → the cursor convention above.
- **Impersonation boundary** → the auth classes above.
- **Realtime** → the human client uses `/sync` through the proxy; the bot gateway is
  deferred, so v1 has **no** Nuck WebSocket.
- **Idempotency** → the `Idempotency-Key` convention above.

## Remaining open issues

- **Federation gates** (ADR 0026): which of these endpoints become `[S2S]` and behind
  which protocol version. Deferred with federation.
- **Server-admin model**: the exact config surface for `NUCK_ADMIN_USERS` (and whether
  admins can be granted via any server-side role). v1: operator-configured user-id list.
