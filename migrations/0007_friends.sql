-- Friends, blocks, and pending friend requests (ADR 0030).
--
-- v1 is single-server, so these are held in the projection rather than per-user
-- account data (which would require acting as the counterpart with admin login-as to
-- write the other side of a friendship). The to-device `space.nuck.friend_request.v1` /
-- `space.nuck.friend_response.v1` events are still sent, so the mechanism federates
-- when federation lands; the account-data mirror is a later optimisation.
--
-- `friendships` is normalised so `user_a < user_b` (one row per pair).
CREATE TABLE friendships (
    user_a TEXT NOT NULL,
    user_b TEXT NOT NULL,
    since  BIGINT NOT NULL,
    PRIMARY KEY (user_a, user_b)
);

-- A directional block: `blocker` has blocked `blocked`.
CREATE TABLE blocks (
    blocker TEXT NOT NULL,
    blocked TEXT NOT NULL,
    PRIMARY KEY (blocker, blocked)
);

-- Pending friend requests (cleared on accept/decline).
CREATE TABLE friend_requests (
    from_user  TEXT NOT NULL,
    to_user    TEXT NOT NULL,
    message    TEXT,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (from_user, to_user)
);
