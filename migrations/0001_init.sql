-- Derived index (ADR 0003/0019).
--
-- This schema is a *rebuildable cache* over federated Matrix state, not a source of
-- truth. Every row records the `event_id` that last wrote it, and every write is an
-- idempotent upsert, so replaying events reconstructs the store exactly.
--
-- Permission bitfields are stored as BIGINT. Nuck reserves bits 56..=63, and no role
-- permission set ever sets bit 63, so the value always fits a signed 64-bit integer.

CREATE TABLE guilds (
    guild_id             TEXT PRIMARY KEY,
    home_server          TEXT NOT NULL,
    name                 TEXT,
    join_mode            TEXT,
    directory_visibility TEXT,
    event_id             TEXT NOT NULL,
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE roles (
    guild_id    TEXT NOT NULL,
    role_id     TEXT NOT NULL,
    position    BIGINT NOT NULL,
    permissions BIGINT NOT NULL,
    name        TEXT,
    color       BIGINT,
    hoist       BOOLEAN NOT NULL DEFAULT false,
    mentionable BOOLEAN NOT NULL DEFAULT false,
    event_id    TEXT NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (guild_id, role_id)
);

-- Role assignments per member. Separate from room membership so role changes do not
-- rewrite membership (ADR 0014).
CREATE TABLE members (
    guild_id   TEXT NOT NULL,
    user_id    TEXT NOT NULL,
    roles      TEXT[] NOT NULL DEFAULT '{}',
    event_id   TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (guild_id, user_id)
);

CREATE TABLE channels (
    channel_id TEXT PRIMARY KEY,
    guild_id   TEXT NOT NULL,
    kind       TEXT NOT NULL,
    position   BIGINT NOT NULL DEFAULT 0,
    category   TEXT,
    event_id   TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX channels_guild_idx ON channels (guild_id);

-- Per-channel permission overrides (ADR 0020). `target` is a role id or a user id.
CREATE TABLE overrides (
    channel_id TEXT NOT NULL,
    target     TEXT NOT NULL,
    allow      BIGINT NOT NULL DEFAULT 0,
    deny       BIGINT NOT NULL DEFAULT 0,
    event_id   TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (channel_id, target)
);

CREATE TABLE bans (
    guild_id   TEXT NOT NULL,
    user_id    TEXT NOT NULL,
    reason     TEXT,
    banned_by  TEXT,
    created_at BIGINT,
    event_id   TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (guild_id, user_id)
);

CREATE TABLE invites (
    guild_id   TEXT NOT NULL,
    code_hash  TEXT NOT NULL,
    max_uses   INTEGER,
    uses       INTEGER NOT NULL DEFAULT 0,
    expires_at BIGINT,
    created_by TEXT,
    event_id   TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (guild_id, code_hash)
);

-- Append-only moderation log (ADR 0016). `event_id` is unique so replay is idempotent.
CREATE TABLE audit_log (
    id         BIGSERIAL PRIMARY KEY,
    guild_id   TEXT NOT NULL,
    actor      TEXT NOT NULL,
    action     TEXT NOT NULL,
    target     TEXT,
    event_id   TEXT NOT NULL UNIQUE,
    created_at BIGINT NOT NULL
);

CREATE INDEX audit_log_guild_idx ON audit_log (guild_id, id DESC);

-- Message search (ADR 0029): plaintext guild channels only. E2EE DM content is never
-- indexed (ADR 0008).
CREATE TABLE messages (
    event_id  TEXT PRIMARY KEY,
    room_id   TEXT NOT NULL,
    sender    TEXT NOT NULL,
    origin_ts BIGINT NOT NULL,
    body      TEXT NOT NULL,
    search    tsvector GENERATED ALWAYS AS (to_tsvector('simple'::regconfig, body)) STORED
);

CREATE INDEX messages_room_ts_idx ON messages (room_id, origin_ts DESC);
CREATE INDEX messages_search_idx ON messages USING GIN (search);

-- Idempotency guard: one row per ingested event (ADR 0019).
CREATE TABLE processed_events (
    event_id    TEXT PRIMARY KEY,
    room_id     TEXT NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Projection bookkeeping, e.g. the last reconciled position per room (ADR 0019).
CREATE TABLE projection_state (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
