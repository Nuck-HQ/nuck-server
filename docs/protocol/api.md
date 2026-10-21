# Protocol: `/_nuck/v1` Endpoint Inventory (draft)

First draft of the Nuck API surface referenced by ADR 0013. **Not final** — request/
response shapes and the error model are not yet specified. Everything here is
**client-facing** unless marked **[S2S]** (server-to-server federation).

## Conventions

- **Base path:** `/_nuck/v1/`. Version in the path (ADR 0013/0026).
- **Auth:** Matrix access token (`Authorization: Bearer …`), validated by nuck-server
  (ADR 0009/0028). Public endpoints are marked.
- **Substrate:** everything not listed here is the proxied Matrix C-S API.
- **Rate limits:** identity-aware, per route (ADR 0036). Tightest on registration,
  invite redeem, search, friend requests, and the gateway.
- **Errors:** a single JSON error shape (to be specified); never silently misbehave
  (ADR 0026).
- **Pagination:** cursor-based (to be specified).

## Server

| Method | Path | Notes |
|---|---|---|
| `GET` | `/server-info` | Public. Nuck protocol versions + capabilities (ADR 0010/0026). |
| `GET` | `/me` | Current user profile + Nuck settings. |

## Registration & sessions (ADR 0037)

| Method | Path | Notes |
|---|---|---|
| `POST` | `/register` | Public. Applies server policy: `open`/`token`/`approval`/`closed`. |
| `GET` | `/register/requests` | Admin: pending approval requests. |
| `POST` | `/register/requests/{id}/approve` | Admin. |
| `POST` | `/register/requests/{id}/deny` | Admin. |

## Guilds (ADR 0005/0006)

| Method | Path | Notes |
|---|---|---|
| `POST` | `/guilds` | Create guild (space + default channels). |
| `GET` | `/guilds/{guildId}` | Aggregated guild view. |
| `PATCH` | `/guilds/{guildId}` | Update guild settings. |
| `DELETE` | `/guilds/{guildId}` | Delete guild. |
| `GET` | `/guilds/{guildId}/members` | Member list (paginated). |
| `GET` | `/guilds/{guildId}/roles` | List roles. |
| `POST` | `/guilds/{guildId}/roles` | Create role. |
| `PATCH` | `/guilds/{guildId}/roles/{roleId}` | Update role (name, color, permissions, position). |
| `DELETE` | `/guilds/{guildId}/roles/{roleId}` | Delete role. |
| `PUT` | `/guilds/{guildId}/members/{userId}/roles` | Assign a member's roles. |
| `GET` | `/guilds/{guildId}/bans` | List guild bans. |
| `PUT` | `/guilds/{guildId}/bans/{userId}` | Ban (ADR 0016). |
| `DELETE` | `/guilds/{guildId}/bans/{userId}` | Unban. |
| `GET` | `/guilds/{guildId}/audit-log` | Paginated audit room view (ADR 0016). |

## Channels (ADR 0005/0020)

| Method | Path | Notes |
|---|---|---|
| `GET` | `/guilds/{guildId}/channels` | Channel list with structure. |
| `POST` | `/guilds/{guildId}/channels` | Create channel (text/voice/category). |
| `PATCH` | `/channels/{channelId}` | Update channel (type, position, topic, category). |
| `DELETE` | `/channels/{channelId}` | Delete channel. |
| `PUT` | `/channels/{channelId}/overrides/{roleOrUserId}` | Set per-channel override. |
| `DELETE` | `/channels/{channelId}/overrides/{roleOrUserId}` | Remove override. |

## Invites & joining (ADR 0011/0025)

| Method | Path | Notes |
|---|---|---|
| `POST` | `/guilds/{guildId}/invites` | Create invite code. |
| `GET` | `/guilds/{guildId}/invites` | List invites. |
| `DELETE` | `/guilds/{guildId}/invites/{code}` | Revoke invite. |
| `GET` | `/invites/{code}` | Preview a code (guild name, etc.). |
| `POST` | `/invites/{code}/redeem` | Redeem from any server; nuck-server calls the host **[S2S]**. |
| `POST` | `/guilds/{guildId}/join-requests` | Request to join (approval mode). |
| `POST` | `/guilds/{guildId}/join-requests/{userId}/approve` | Approve. |
| `POST` | `/guilds/{guildId}/join-requests/{userId}/deny` | Deny. |

## Voice (ADR 0007)

| Method | Path | Notes |
|---|---|---|
| `POST` | `/channels/{channelId}/voice/token` | Mint a scoped LiveKit token. Permission-checked; revoked on permission change. |

Voice *membership* is standard MatrixRTC (`m.call.member` state) — no Nuck endpoint.

## Directory (ADR 0021)

| Method | Path | Notes |
|---|---|---|
| `GET` | `/directory` | Public guilds hosted on **this** server. |
| `GET` | `/directory/{guildId}` | Public guild detail. |

## Friends & blocking (ADR 0030)

| Method | Path | Notes |
|---|---|---|
| `GET` | `/friends` | Friend list. |
| `POST` | `/friends/requests` | Send request (delivered as a to-device event). |
| `POST` | `/friends/requests/{userId}/accept` | Accept. |
| `POST` | `/friends/requests/{userId}/decline` | Decline. |
| `DELETE` | `/friends/{userId}` | Remove friend. |
| `PUT` | `/blocks/{userId}` | Block (also suppresses requests/invites). |
| `DELETE` | `/blocks/{userId}` | Unblock. |

## Bots (ADR 0018)

| Method | Path | Notes |
|---|---|---|
| `POST` | `/apps` | Register a bot (creates an AppService + bot user). |
| `GET` | `/apps` | List the caller's bots. |
| `DELETE` | `/apps/{appId}` | Delete bot. |
| `POST` | `/apps/{appId}/token` | Rotate bot token. |
| `GET` | `/gateway` | **WebSocket.** Bot connects out; receives events; no public endpoint required. |

## Search (ADR 0029)

| Method | Path | Notes |
|---|---|---|
| `GET` | `/search?q=&guildId=&channelId=` | Permission-filtered; plaintext guild channels only. |

## Federation [S2S]

Server-to-server, authenticated by Matrix federation identity (ADR 0010/0026).

| Method | Path | Notes |
|---|---|---|
| `GET` | `/federation/server-info` | Peer capability/version check. |
| `POST` | `/federation/invites/{code}/redeem` | Host-authoritative redemption (ADR 0011). |

## Open issues

- **Content schemas** for every request/response, and the shared error model.
- **Pagination** convention (cursor format, ordering).
- **Impersonation boundary:** exactly which operations nuck-server performs *as* the
  user vs. via AppService impersonation (ADR 0028).
- **Realtime:** confirm nothing here needs the deferred `a2` WebSocket; bot gateway is
  the only WebSocket in v1 (ADR 0017/0018).
- **Idempotency keys** for create/redeem operations.
