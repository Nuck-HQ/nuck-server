-- Room membership, derived from `m.room.member` (ADR 0005/0019).
--
-- This is the substrate-authoritative "who is in what room": guild membership is
-- membership in the guild Space room, and channel access is membership in the channel
-- room (ADR 0005/0041). It is distinct from `members`, which holds the
-- `space.nuck.member.v1` **role assignments** (`@everyone` + roles).
--
-- Like every projection table this is a rebuildable cache: one row per (room, user),
-- recording the `m.room.member` event that last wrote it, so reconciliation can prune
-- memberships whose event is gone from resolved state.

CREATE TABLE memberships (
    room_id    TEXT NOT NULL,
    user_id    TEXT NOT NULL,
    -- `join`, `invite`, `leave`, `ban`, or `knock`.
    membership TEXT NOT NULL,
    event_id   TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (room_id, user_id)
);

-- "Which rooms is this user in" — the hot lookup for per-user authorization.
CREATE INDEX memberships_user_idx ON memberships (user_id);
