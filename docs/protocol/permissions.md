# Protocol: Permissions — bitfield & power-level mapping

The concrete permission set and how it relates to the Matrix substrate. Versioned
protocol (ADR 0014/0020/0041); the canonical definition lives in
`nuck-protocol/src/permissions.rs`.

## The bitfield

- A `u64`, serialized on the wire as a plain JSON number.
- Bit meaning is a **permanent protocol commitment**: never repurpose a bit. A
  breaking change adds a *new* bit (and bumps the protocol version, ADR 0026).
- **Reserved range `56..=63`** for Nuck-specific extensions. Discord-aligned
  permissions never occupy it. `56..=59` are assigned; `60..=63` are reserved and
  unassigned. If the set ever exceeds `u64`, add a second field rather than reusing
  bits (ADR 0020).

`ADMINISTRATOR` implies **every** bit (it is not merely another bit): a member holding
it is treated as holding `u64::MAX`.

## The table

`level` is the ordering value (see [below](#ordering-level)); it is **not** an
enforcement threshold. Unassigned bit indices in `0..56` (`19`, `25`, `32`, `39`,
`41..=45`, `47..=55`) are intentionally free for future Discord alignment.

### Discord-aligned

| Bit | Name | Meaning | Level |
|---|---|---|---|
| 0 | `CREATE_INSTANT_INVITE` | Create invites to the guild | 0 |
| 1 | `KICK_MEMBERS` | Remove members (rejoinable) | 50 |
| 2 | `BAN_MEMBERS` | Ban members (blocks rejoin) | 50 |
| 3 | `ADMINISTRATOR` | All permissions; bypasses channel overrides | 100 |
| 4 | `MANAGE_CHANNELS` | Create/edit/delete channels and their structure | 50 |
| 5 | `MANAGE_GUILD` | Edit guild-wide settings (name, icon, join mode) | 50 |
| 6 | `ADD_REACTIONS` | Add reactions | 0 |
| 7 | `VIEW_AUDIT_LOG` | Read the guild audit log (ADR 0016) | 50 |
| 8 | `PRIORITY_SPEAKER` | Priority speaker in voice | 0 |
| 9 | `STREAM` | Stream video in voice | 0 |
| 10 | `VIEW_CHANNEL` | See a channel and its history | 0 |
| 11 | `SEND_MESSAGES` | Post messages in text channels | 0 |
| 12 | `SEND_TTS_MESSAGES` | Post text-to-speech messages | 0 |
| 13 | `MANAGE_MESSAGES` | Delete/pin others' messages | 50 |
| 14 | `EMBED_LINKS` | Link previews render | 0 |
| 15 | `ATTACH_FILES` | Upload attachments | 0 |
| 16 | `READ_MESSAGE_HISTORY` | Read history before joining | 0 |
| 17 | `MENTION_EVERYONE` | Use `@everyone` / `@here` | 50 |
| 18 | `USE_EXTERNAL_EMOJIS` | Use emojis from other guilds | 0 |
| 20 | `CONNECT` | Join a voice channel | 0 |
| 21 | `SPEAK` | Speak in a voice channel | 0 |
| 22 | `MUTE_MEMBERS` | Server-mute members in voice | 50 |
| 23 | `DEAFEN_MEMBERS` | Server-deafen members in voice | 50 |
| 24 | `MOVE_MEMBERS` | Move members between voice channels | 50 |
| 26 | `CHANGE_NICKNAME` | Change own nickname | 0 |
| 27 | `MANAGE_NICKNAMES` | Change others' nicknames | 50 |
| 28 | `MANAGE_ROLES` | Create/edit/assign roles below your highest | 50 |
| 29 | `MANAGE_WEBHOOKS` | Manage webhooks | 50 |
| 30 | `MANAGE_GUILD_EXPRESSIONS` | Manage emojis and stickers | 50 |
| 31 | `USE_APPLICATION_COMMANDS` | Use app commands | 0 |
| 33 | `MANAGE_EVENTS` | Manage scheduled events | 50 |
| 34 | `MANAGE_THREADS` | Manage threads | 50 |
| 35 | `CREATE_PUBLIC_THREADS` | Create public threads | 0 |
| 36 | `CREATE_PRIVATE_THREADS` | Create private threads | 0 |
| 37 | `USE_EXTERNAL_STICKERS` | Use stickers from other guilds | 0 |
| 38 | `SEND_MESSAGES_IN_THREADS` | Post in threads | 0 |
| 40 | `MODERATE_MEMBERS` | Time out members (ADR 0016) | 50 |
| 46 | `SEND_VOICE_MESSAGES` | Post voice messages | 0 |

### Nuck extensions (reserved range)

| Bit | Name | Meaning | Level |
|---|---|---|---|
| 56 | `MANAGE_FEDERATION` | Edit the guild's federation settings (blocklists, hosting policy, ADR 0024) | 50 |
| 57 | `MANAGE_BOTS` | Register/manage/revoke guild bots (ADR 0018) | 50 |
| 58 | `MANAGE_DIRECTORY_LISTING` | Toggle the guild's public directory listing (ADR 0021) | 50 |
| 59 | `MANAGE_VOICE_CHANNELS` | Manage voice channels specifically (limit, move, mute) | 50 |

## Ordering level

Each table entry carries a `level`. Since **ADR 0041** it is *not* an enforcement
threshold: all fine-grained authorization is computed by the engine
(`nuck-protocol/src/authorization.rs`) from role membership, and the substrate sees
only the coarse tiering below. `level` survives as:

1. the ordering fed to `effective_level` (the maximum level over the bits a member
   holds), and
2. documented metadata for the **legacy fine-grained mirror** (ADR 0040), which is
   retained only to explain why mirroring is not the enforcement path.

Role hierarchy (who outranks whom) uses role **position**, not this level.

## Power-level mapping (coarse, ADR 0041)

A room is written with exactly these `m.room.power_levels` thresholds — the two-tier
ladder of *members* vs. the **action bot** (ADR 0041). This tiering is **exact**: it is
the substrate-enforceable security boundary under federation.

| Field | Value | Effect |
|---|---|---|
| `users_default` | 0 | Ordinary members |
| `users` | bot user → 100 | The single privileged writer |
| `events_default` | 0 | Members may send message-like events |
| `state_default` | 100 | Only the bot writes state |
| `ban` | 100 | Only the bot bans |
| `kick` | 100 | Only the bot kicks |
| `invite` | 100 | Only the bot invites (ADR 0011) |
| `redact` | 100 | Only the bot redacts (ADR 0016) |

`MEMBER_LEVEL = 0`, `BOT_LEVEL = 100`. The bot performs every privileged action
(state event, ban/kick, redaction) on nuck-server's authorization, and records the
real actor in the resulting event and the audit log (ADR 0016/0041).

**The fine-grained `mirror()` is not used** — a scalar power level cannot represent an
arbitrary permission set (ADR 0040). It stays in the crate as documentation and to
drive the `mirror_invariant` tests.

## Channel access is membership

Who may view or post in a channel is **who is a member of that channel room**, which
nuck-server syncs from guild roles (ADR 0005). Channel access is not expressed in
power levels; per-channel allow/deny overrides (ADR 0020) are evaluated by the engine
in nuck-server, not by the substrate.

## Versioning

- Adding a permission bit is **additive** (same protocol version): peers that do not
  know the bit ignore it.
- Repurposing or removing a bit is **breaking**: it bumps the protocol version
  (ADR 0026), and the bit must never be reused for a different meaning.
- Changing the coarse thresholds above is **breaking** (it changes the substrate
  boundary).
