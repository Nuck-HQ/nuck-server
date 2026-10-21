# Protocol: `space.nuck.*` Event Registry (draft)

First draft of the custom event registry referenced by ADR 0014. **Not final** — every
type below needs a concrete content schema, and the registry mechanism itself needs
an owner (see [open-questions](../open-questions.md)).

Conventions (ADR 0014):

- Namespace `space.nuck.*`; **global**, identical on every Nuck server.
- **Version in the type name**; breaking change → new type (`...v2`); additive change
  → same type, receivers **ignore unknown fields**.
- The meaning of an existing type is **never mutated**.

## Room types

| Type | Used for | Notes |
|---|---|---|
| `m.space` | **Guild** room | Keeps native Matrix space semantics (`m.space.child`). |
| `space.nuck.channel` | **Channel** room | Room `type` on creation. Type/position live in the guild (below). |

## Guild room state

Authoritative for guild-wide structure. One state event per role / member / channel /
code, so a large guild has many state events — comparable to Matrix's own one
`m.room.member` per member.

| Type | State key | Purpose |
|---|---|---|
| `space.nuck.guild.v1` | `""` | Guild settings: join mode, directory visibility, default notification level, federation config. (Name/icon/topic use `m.room.name`/`m.room.avatar`/`m.room.topic`.) |
| `space.nuck.role.v1` | role ID | Role definition: name, color, position, `permissions` (`u64`), hoist, mentionable. |
| `space.nuck.member.v1` | user ID | Role assignments for a member. Kept separate from `m.room.member` so role changes don't rewrite membership. **Must be cleaned up on leave.** |
| `space.nuck.channel.v1` | channel room ID | Channel structure: type (`text`/`voice`/`category`), position, category, parent guild. |
| `space.nuck.invite.v1` | code hash | Invite: max uses, uses, expiry, inviter, created. |
| `space.nuck.ban.v1` | user ID | Guild ban: reason, moderator, timestamp (ADR 0016). |
| `space.nuck.join_request.v1` | user ID | Pending/decided join request (approval mode, ADR 0025). |
| `space.nuck.emoji.v1` | name | Custom emoji: image MXC URI, uploader. |
| `space.nuck.sticker.v1` | ID | Custom sticker: image MXC URI, description, uploader. |

## Channel room state

Channel-specific, so permission changes spread across rooms instead of hot-spotting
the guild room.

| Type | State key | Purpose |
|---|---|---|
| `space.nuck.override.v1` | role ID or user ID | Per-channel allow/deny permission overrides (ADR 0020). |
| `m.room.power_levels` | `""` | **Mirrored** effective permissions (ADR 0006). Standard type, not custom. |

## Message events

| Type | Purpose |
|---|---|
| `space.nuck.embed.v1` | Rich embed (ADR 0023). |
| `space.nuck.interaction.v1` | Slash-command invocation / component interaction (ADR 0018/0023). |
| `m.room.message`, `m.reaction`, `m.replace`, `m.thread` | Standard messaging — **not** Nuck types (ADR 0023). |

## To-device events

Federate natively, point-to-point (ADR 0030).

| Type | Purpose |
|---|---|
| `space.nuck.friend_request.v1` | Friend request. |
| `space.nuck.friend_response.v1` | Accept/decline. |

## Account data

Per-user, **local to the user's home server** (does not federate to others). Fits
ADR 0030's "own server is authoritative" model.

| Type | Purpose |
|---|---|
| `space.nuck.friends.v1` | The user's friend list. |
| `space.nuck.guild_folders.v1` | Client-side guild organization. |
| `space.nuck.settings.v1` | Client settings. |
| `m.ignored_user_list` | Blocking — standard type (ADR 0030). |
| `m.push_rules` | Notification settings — standard type (ADR 0031). |

## Open issues

- **Member/role scale.** One `space.nuck.member.v1` per member is a lot of state for
  very large guilds; consider folding role IDs into `m.room.member` content instead.
- **Guild room hot-spotting.** All guild-wide writes (roles, channels, invites) land
  in one room. Acceptable for now; revisit if it becomes a bottleneck.
- **Role IDs.** Decide the ID scheme (opaque random vs. derived) and whether role
  order is stored in the role event (`position`) or a separate ordering event.
- **Content schemas.** Each type needs a concrete, versioned JSON schema.
- **Registry mechanism.** Where the canonical registry lives and how `nuck-protocol`
  is generated from it.
