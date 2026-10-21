# ADR 0030 — DMs, friends, and blocking

- Status: Accepted
- Date: 2026-10-21

## Context

Discord has DMs (1:1 and group), a friends list, and blocking. Matrix has direct
rooms (`is_direct`), ignore lists (`m.ignored_user_list`), and to-device events, but
no friend graph.

## Decision

**Matrix-native DMs + a custom friend layer (option a).**

- **DMs:** Matrix direct rooms. 1:1 and group, **E2EE by default** (ADR 0008),
  federated, handled by `matrix-rust-sdk`. Nothing to build.
- **Friends:** a Nuck social graph stored in **account data**. **Requests** are
  delivered cross-server as a **custom to-device event** (`space.nuck.friend_request`),
  which federates natively and is point-to-point. Accepting establishes mutual entries
  in each side's account data.
- **Blocking:** Matrix's `m.ignored_user_list` (local, best-effort federated) plus a
  Nuck-level block that also suppresses friend requests and DM invites from that user.

## Consequences

- DMs come with E2EE, sync, and federation for free; no custom DM system.
- **Friend state is inherently non-consensus:** friendship is mutual state held
  separately by two servers, so it is **eventually consistent and best-effort**. A
  user's own server is authoritative for their own list. Same for blocks.
- This is acceptable because **friendship is not a security boundary** — the security
  boundaries remain room membership and permissions (ADR 0006/0010).
- Blocking must be enforced in nuck-server (suppress requests/invites) since
  `m.ignored_user_list` alone only affects the local client's rendering.

## Verification update (2026-10-21)

Checked in Tuwunel source (`src/main/tests/invite_ignored_user.rs`)
([verification](../verification/tuwunel.md#6-mignored_user_list--not-server-side-enforcement)):
the ignore list is **sync-time filtering, not enforcement** — an ignored user's invite
is **stored and federated**, merely withheld from `/sync`, and surfaces on un-ignore.

**Sharpens the Decision:** the substrate will *not* enforce blocking; nuck-server
**must** suppress friend requests and DM invites at the API level. (The earlier wording
"only affects the local client's rendering" was slightly off — it is server-side
`/sync` filtering — but the conclusion is unchanged and now certain.)

## Implementation note (2026-10-22) — superseded by [ADR 0044](./0044-v1-friends-storage.md)

For the single-server v1, friend and block state is held in the **projection** rather
than per-user account data: writing the *counterpart's* account data would require
admin login-as, and v1 already commits to a server-side index. The account-data mirror
(and the `m.ignored_user_list` mirror) is deferred with federation. DM rooms remain
plain Matrix direct rooms, as decided above. See ADR 0044.


