# Protocol: `space.nuck.*` event registry

The custom Matrix events Nuck defines. Versioned protocol (ADR 0014); the canonical
definition lives in `nuck-protocol/src/events.rs`.

## Registry mechanism & ownership

**The code is the registry.** `nuck_protocol::events::event_types` plus the
`#[ruma_event]` content structs are the single source of truth; this document is
derived from them and kept honest by `nuck-protocol/tests/registry.rs`, which asserts:

- every `event_types` constant matches its content type's `StaticEventContent::TYPE`;
- every type is in the `space.nuck.` namespace, is uniquely registered, and carries a
  well-formed `.v<digits>` version suffix;
- parsing is **tolerant** (ADR 0014): unknown fields are ignored, and added optional
  fields parse when absent.

To add a type: add the `event_types` constant, the `EventContent` struct, an entry in
the registry test, and a row here.

## Conventions (ADR 0014)

- Namespace `space.nuck.*`; **global** — identical on every Nuck server.
- **Version in the type name**; a breaking change adds a new type (`…v2`) and never
  mutates an existing one. Additive changes keep the type and rely on tolerant parsing.
- Unknown fields are ignored; a field added later must be optional.

## Removal (no state deletion)

Matrix has no state deletion, so Nuck encodes **removal** as a state event whose content
is an **empty object** — the shape a redaction produces, and the same convention
`m.room.member` already relies on (a redacted member event is ignored, not rejected;
see `projection::ingest`). A non-empty but malformed content is still an error.

This applies to the guild-level types `guild`, `role`, `member`, `channel`, `invite`,
`ban`, and `join_request`. **Overrides keep their own convention** — removal writes an
all-zero override (ADR 0020), which resolves identically to no override.

## ID schemes

- **Role ID** — the reserved base role is `everyone` (its *display name* is
  `@everyone`; the id is not `@everyone` because Matrix treats a state key beginning
  with `@` as a user id). Every other role id is **opaque**: `r` + 12 lowercase hex
  characters, generated randomly. Opaque ids avoid leaking names or positions, stay
  stable across renames, and cannot be mistaken for a Matrix user id.
- **Role order** — carried by `position` (an `i64`) on the role event; higher wins on
  conflict. Ties are broken by role id, so every server computes the same order.
- **Channel / emoji / sticker / invite keys** — see each type below.

## Room types

| Type | Used for | Notes |
|---|---|---|
| `m.space` | **Guild** room | Native Matrix space semantics (`m.space.child`). |
| `space.nuck.channel` | **Channel** room | Room `type` set at creation. Kind/position live in the guild (below). |

## Guild room state

Authoritative for guild-wide structure. One state event per role / member / channel /
code, comparable to Matrix's one `m.room.member` per member.

| Type | State key | Kind |
|---|---|---|
| `space.nuck.guild.v1` | `""` | State |
| `space.nuck.role.v1` | role ID | State |
| `space.nuck.member.v1` | user ID | State |
| `space.nuck.channel.v1` | channel room ID | State |
| `space.nuck.invite.v1` | code hash | State |
| `space.nuck.ban.v1` | user ID | State |
| `space.nuck.join_request.v1` | user ID | State |
| `space.nuck.emoji.v1` | emoji name | State |
| `space.nuck.sticker.v1` | sticker ID | State |

### `space.nuck.guild.v1` — guild settings (singleton)

Name, icon, and topic use `m.room.name` / `m.room.avatar` / `m.room.topic`.

| Field | Type | Optional | Meaning |
|---|---|---|---|
| `join_mode` | `JoinMode` | no | `open` \| `invite_only` \| `approval` (ADR 0025) |
| `directory_visibility` | `DirectoryVisibility` | no | `hidden` \| `listed` (ADR 0021) |
| `default_notification_level` | `NotificationLevel` | no | `all_messages` \| `mentions_only` \| `nothing` (ADR 0031) |

### `space.nuck.role.v1` — role definition (ADR 0006/0020)

| Field | Type | Optional | Meaning |
|---|---|---|---|
| `name` | string | no | Display name |
| `color` | number | yes | RGB, `0xRRGGBB` |
| `position` | integer (`i64`) | no | Higher wins on conflict |
| `permissions` | number (`u64` bitfield) | no | See [`permissions.md`](./permissions.md) |
| `hoist` | boolean | no | Show separately in the member list |
| `mentionable` | boolean | no | May be mentioned by anyone |

### `space.nuck.member.v1` — role assignments (keyed by user ID)

Kept separate from `m.room.member` so role changes don't rewrite membership. **Must be
cleaned up when a member leaves** (an all-zero / absent assignment is no roles).

| Field | Type | Optional | Meaning |
|---|---|---|---|
| `roles` | array of role ID | no | The member's role ids (excluding the implicit `everyone`) |

### `space.nuck.channel.v1` — channel structure (keyed by channel room ID)

| Field | Type | Optional | Meaning |
|---|---|---|---|
| `kind` | `ChannelKind` | no | `text` \| `voice` \| `category` |
| `position` | integer (`i64`) | no | Sort order within the guild |
| `category` | string | yes | Parent category channel id |

### `space.nuck.invite.v1` — invite code (keyed by code hash, ADR 0011)

| Field | Type | Optional | Meaning |
|---|---|---|---|
| `max_uses` | integer | yes | Use cap; absent = unlimited |
| `uses` | integer | no | Redemptions so far |
| `expires_at` | integer | yes | Milliseconds since the Unix epoch |
| `created_by` | user ID | no | Inviter |

### `space.nuck.ban.v1` — guild ban (keyed by user ID, ADR 0016)

| Field | Type | Optional | Meaning |
|---|---|---|---|
| `reason` | string | yes | Moderator's reason |
| `banned_by` | user ID | no | The real actor (attribution, ADR 0041) |
| `created_at` | integer | no | Milliseconds since the Unix epoch |

### `space.nuck.join_request.v1` — join request (keyed by user ID, ADR 0025)

| Field | Type | Optional | Meaning |
|---|---|---|---|
| `message` | string | yes | Applicant's message |
| `status` | `JoinRequestStatus` | no | `pending` \| `approved` \| `denied` |
| `requested_at` | integer | no | Milliseconds since the Unix epoch |

### `space.nuck.emoji.v1` — custom emoji (keyed by emoji name)

| Field | Type | Optional | Meaning |
|---|---|---|---|
| `image` | string | no | `mxc://` URI |
| `uploaded_by` | user ID | no | Uploader |

### `space.nuck.sticker.v1` — custom sticker (keyed by sticker ID)

| Field | Type | Optional | Meaning |
|---|---|---|---|
| `image` | string | no | `mxc://` URI |
| `description` | string | yes | Alt text |
| `uploaded_by` | user ID | no | Uploader |

## Channel room state

Channel-specific, so permission changes spread across rooms instead of hot-spotting the
guild room.

| Type | State key | Kind |
|---|---|---|
| `space.nuck.override.v1` | role ID or user ID | State |
| `m.room.power_levels` | `""` | State (standard; **coarse** tiers only — ADR 0041) |

### `space.nuck.override.v1` — per-channel permission override (ADR 0020)

The state key is a role id (including `everyone`) or a Matrix user id. Removal writes
an all-zero override — Matrix has no "delete state", and an all-zero override resolves
identically to no override.

| Field | Type | Optional | Meaning |
|---|---|---|---|
| `allow` | number (`u64` bitfield) | no | Bits granted |
| `deny` | number (`u64` bitfield) | no | Bits removed (deny wins over allow) |

## Message events

| Type | Kind | Purpose |
|---|---|---|
| `space.nuck.embed.v1` | MessageLike | Rich embed (ADR 0023) |
| `space.nuck.interaction.v1` | MessageLike | Slash-command / component / modal interaction (ADR 0018) |
| `m.room.message`, `m.reaction`, `m.replace`, `m.thread` | — | Standard messaging, **not** Nuck types (ADR 0023) |

### `space.nuck.embed.v1`

| Field | Type | Optional | Meaning |
|---|---|---|---|
| `title` | string | yes | Embed title |
| `description` | string | yes | Embed body |
| `url` | string | yes | Target URL |
| `color` | number | yes | RGB, `0xRRGGBB` |

### `space.nuck.interaction.v1`

| Field | Type | Optional | Meaning |
|---|---|---|---|
| `app_id` | string | no | The app (bot) being invoked |
| `kind` | `InteractionKind` | no | `command` \| `component` \| `modal_submit` |
| `command` | string | yes | Command name (`command`) |
| `args` | object | yes | Parsed arguments (`command`) |
| `custom_id` | string | yes | Component id (`component` / `modal_submit`) |
| `values` | array of string | yes | Selected option values (`component`) |

## To-device events

Federate natively, point-to-point (ADR 0030).

| Type | Kind | Fields |
|---|---|---|
| `space.nuck.friend_request.v1` | ToDevice | `message` (string, optional) |
| `space.nuck.friend_response.v1` | ToDevice | `accepted` (boolean) |

## Account data (client-local, not federated)

Per-user, **local to the user's home server** — these do **not** federate and are
**not** yet part of the `nuck-protocol` crate. Their content types will be added with
the features that need them (friends, ADR 0030; client organization).

| Type | Purpose |
|---|---|
| `space.nuck.friends.v1` | The user's friend list |
| `space.nuck.guild_folders.v1` | Client-side guild organization |
| `space.nuck.settings.v1` | Client settings |
| `m.ignored_user_list` | Blocking — standard type (ADR 0030) |
| `m.push_rules` | Notification settings — standard type (ADR 0031) |

## Open issues

- **Member/role scale.** One `space.nuck.member.v1` per member is a lot of state for
  very large guilds. Kept as-is for v1; folding role ids into `m.room.member` content
  is the deferred alternative.
- **Guild room hot-spotting.** All guild-wide writes (roles, channels, invites) land in
  one room. Accepted for now; revisit if it becomes a bottleneck.
