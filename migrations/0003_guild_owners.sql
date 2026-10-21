-- Guild ownership, derived from `m.room.create` (ADR 0020/0038).
--
-- The guild owner is the room creator. Room version 11 dropped the `creator` content
-- field, so the create event's `sender` is the fallback. Only Matrix Spaces can be
-- guilds (ADR 0005), so only `m.space` create events are projected — rows for other
-- rooms are never read.
--
-- Kept separate from `guilds` because `m.room.create` and `space.nuck.guild.v1` are
-- different events: a single row cannot carry both without one overwriting the other.
-- Like every projection table it is rebuildable and pruned by event id when the create
-- event leaves resolved state.

CREATE TABLE guild_owners (
    guild_id   TEXT PRIMARY KEY,
    owner      TEXT NOT NULL,
    event_id   TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
