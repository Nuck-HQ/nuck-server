# ADR 0044 — v1 friends & blocking storage (single server)

- Status: Accepted
- Date: 2026-10-22
- Supersedes: the storage mechanism of [ADR 0030](./0030-dms-friends-blocking.md) for v1

## Context

ADR 0030 chose account data for the friend graph and blocks. Implementing it for the
**single-server v1** exposed a cost: accepting a friend request must write *both* users'
lists, and deleting a friend or blocking must touch the counterpart's list — but a
client token can only write its own account data. Doing it as the counterpart requires
admin login-as (`POST /_synapse/admin/v1/users/{id}/login`), which needs the Tuwunel
admin token and does not fit the account-data API cleanly. v1 also already commits to a
server-side Postgres index (ADR 0015/0019).

## Decision

**Hold friendship, block, and pending-friend-request state in the projection** for v1
(`friendships`, `blocks`, `friend_requests`), not in account data.

- The API is exactly as specified (`docs/protocol/api.md`); only the storage moves.
- Friend requests are **still delivered as to-device** `space.nuck.friend_request.v1` /
  `…friend_response.v1` events, so the mechanism federates when federation lands; the
  local table backs `GET /friends/requests`.
- Blocking is enforced at the API boundary (`POST /dm`, `POST /friends/requests`);
  the `/sync` post-filter and the `m.ignored_user_list` mirror are deferred.
- DMs remain plain Matrix direct rooms, created as the caller with `is_direct`
  (unchanged from ADR 0030); E2EE is the client's (ADR 0008).

## Consequences

- Friends/blocks work on a single server with no admin token and are fully testable.
- When federation lands, the account-data/to-device path returns: a remote friendship
  must be reconciled, and blocking becomes best-effort across servers (already the
  ADR 0030 stance — friendship is not a security boundary).
- `GET /friends` reads the projection, not sync, for v1.
