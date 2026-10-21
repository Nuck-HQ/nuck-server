# Protocol: DMs, friends & blocking — concrete design (ADR 0030 concretized)

Design record for the v1 implementation of ADR 0030. **For review before any code.**

ADRs 0030 and 0019 defer the mechanics ("DMs are Matrix direct rooms; nothing to build"
and "friend state is best-effort") but leave open *where the state actually lives*, *how
cross-server delivery works*, and *where blocking is enforced*. This note answers those
for v1, whose scope is a **single server**.

## Scope

- **1:1 DMs** (E2EE). **Group DMs are deferred** (they are just more Matrix rooms; revisit
  with the client).
- **Friends** — a mutual, unordered list with pending requests.
- **Blocking** — suppress friend requests and DM invites from a user.
- **Single server in v1.** Cross-server friend requests ride Matrix **to-device** events
  (ADR 0030) and so will work when federation lands, but v1 only needs the local case.

## Storage — where each thing lives

| Thing | Canonical store | Federated? | Authority |
|---|---|---|---|
| DM room | Matrix room with `m.direct` account data (`is_direct`) | yes (federation) | the room (substrate) |
| DM messages | Matrix events, **E2EE** (`m.room.encrypted`) | yes | the room |
| Friend list | account data `space.nuck.friends.v1` on the user's home server | no | own server |
| Friend request | to-device `space.nuck.friend_request.v1` / `…friend_response.v1`, **plus a local pending row** (see below) | (to-device) | own server |
| Block list | account data `space.nuck.blocks.v1` on the user's home server, mirrored to `m.ignored_user_list` | no | own server |

**Decisions:**

1. **DM messages are never indexed.** E2EE means the server holds no plaintext; the
   projection only ever indexes plaintext guild channels (ADR 0008/0019). DM content is
   invisible to search by construction.
2. **`space.nuck.blocks.v1` is a new account-data type.** It must be added to the
   account-data table in `events.md` and to the `nuck-protocol` crate. Its payload is an
   array of blocked user ids. We **additionally** mirror blocks into the standard
   `m.ignored_user_list`, so the substrate's own `/sync` filtering (ADR 0030 verification)
   is a second line of defence.
3. **A local pending-request row.** Because the substrate does not let nuck-server read a
   user's to-device inbox, `POST /friends/requests` both **sends the to-device event** and
   **records a pending row** (from, to, state, created_at) in the projection. This is what
   `GET /friends/requests` reads. In v1 (single server) the two users share a server, so
   the row is authoritative. When federation lands, the remote side additionally surfaces
   the to-device event to its own client, and a `[S2S]` notification reconciles the row.

## Endpoints & semantics

### Friends

- `GET /friends` → `FriendsResponse { friends: [FriendInfo] }`, `FriendInfo { user_id, display_name?, since }`,
  ordered by `user_id`. Read from `space.nuck.friends.v1`.
- `GET /friends/requests` → `FriendRequestsResponse { requests: [FriendRequestInfo] }`,
  `FriendRequestInfo { user_id, message?, direction: "incoming"|"outgoing", created_at }`,
  ordered oldest-first. **Add this row to `api.md`** (it was omitted from the summary
  table).
- `POST /friends/requests { user_id, message? }`:
  - `400` if `user_id` is self or malformed; `409 M_CONFLICT` if already friends or a
    request is already pending in either direction.
  - `403 M_FORBIDDEN` if **either** side has blocked the other (see enforcement).
  - Sends `space.nuck.friend_request.v1` to-device **and** writes the pending row.
  - Rate-limited (abuse surface, ADR 0036).
- `POST /friends/requests/{userId}/accept`:
  - Writes the friend into `space.nuck.friends.v1` (both directions as two account-data
    writes: the caller's list and the target's list — the target is local in v1),
    sends `space.nuck.friend_response.v1 { accepted: true }`, and clears the pending row.
  - Returns the updated `FriendsResponse`.
- `POST /friends/requests/{userId}/decline`: sends `…friend_response.v1 { accepted: false }`,
  clears the pending row, returns `ActionResponse`.
- `DELETE /friends/{userId}`: removes from both lists; `404` if not a friend.

### Blocking

- `GET /blocks` → `BlocksResponse { blocked: [string] }` (user ids), ordered by id.
- `PUT /blocks/{userId}`: idempotent. Adds to `space.nuck.blocks.v1`, mirrors into
  `m.ignored_user_list`, and **drops any friendship and pending requests** with that user
  (blocking is a superset of unfriend + decline). Returns the new `BlocksResponse`.
- `DELETE /blocks/{userId}`: idempotent. Removes from both stores. Does **not** restore a
  friendship.
- Blocking **never** removes guild membership and **never** gates guild resources — it is
  not a security boundary (ADR 0030).

### DMs

- `POST /dm { user_id }` → `CreateDmResponse { room_id }`.
  - `403 M_FORBIDDEN` if either side has blocked the other. **This is the primary
    server-side block enforcement for DMs.**
  - Otherwise creates (or returns the existing) direct room: `createRoom` with
    `is_direct: true`, `preset: trusted_private_chat`, and the two members; updates the
    caller's `m.direct` account data. Idempotent per `(caller, user_id)`: an existing
    direct room between the two is returned rather than duplicated.
  - E2EE is enabled by the **client** (matrix-rust-sdk); the server does not manage keys.
- **Listing DMs is not a Nuck endpoint** — the client reads its direct rooms from the
  `m.direct` account data and `m.room.member` through the proxy, or filters `/sync`.

## Enforcement points (blocking)

Blocking is best-effort and local, so enforcement is at every boundary nuck-server brokers:

1. **`POST /dm`** refuses when either side is blocked (authoritative for new DMs).
2. **The `/sync` proxy post-filters** the response, dropping `space.nuck.friend_request.v1`
   to-device events and DM invites originating from users the caller has blocked. This
   mirrors the substrate's own ignore-list filtering and is the reason we also mirror into
   `m.ignored_user_list` — but we do **not** rely on the substrate alone (ADR 0030
   verification: it is sync-time filtering, not enforcement).
3. **`POST /friends/requests`** refuses when either side is blocked.

An already-stored invite or message from a now-blocked user is **not** retroactively
removed (that would require redaction and is not a social-blocking concern).

## What has to be built in nuck-server

- **Account-data read/write as the user** (`space.nuck.friends.v1`, `space.nuck.blocks.v1`,
  `m.ignored_user_list`, `m.direct`) — the client-authenticated C-S account-data API,
  forwarded as the user (ADR 0028 "as-user").
- **To-device send** for friend requests/responses.
- **A `friend_requests` projection table** (from, to, state, message, created_at) for the
  pending queue.
- **A `/sync` response post-filter** for blocked-sender events.
- **`POST /dm`** room provisioning with `is_direct` + `m.direct` maintenance.
- `space.nuck.blocks.v1` in `nuck-protocol` + a row in `events.md`.

## Decisions (frozen)

- **Group DMs:** deferred to post-v1. v1 is 1:1 only.
- **Pending requests:** stored in a projection table; `GET /friends/requests` reads it
  (the to-device event is still sent). Not sync-only.
- **Blocking scope:** suppresses **DMs and friend requests** only. Guild invites and guild
  membership are untouched.

## Remaining open questions

- **Federation gates** for these endpoints (deferred with federation, ADR 0026).
- **Block-list storage detail:** account data vs. projection table. v1 reads/writes it as
  account data (`space.nuck.blocks.v1`) and mirrors `m.ignored_user_list`; `/sync`
  post-filtering reads it.
