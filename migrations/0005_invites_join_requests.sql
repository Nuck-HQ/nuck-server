-- Invites (ADR 0011) and join requests (ADR 0025).
--
-- The federated `space.nuck.invite.v1` state key is the **code hash** (the plaintext
-- code must not federate). The plaintext `code` is kept locally in the projection so a
-- host can list and share its own invites; it is null for an invite learned only from
-- federated state (post-rebuild).
ALTER TABLE invites ADD COLUMN code TEXT;
-- Redeem/preview look an invite up by code alone (the caller does not know the guild),
-- so the hash must be indexed on its own.
CREATE UNIQUE INDEX invites_code_hash_idx ON invites (code_hash);

-- Join requests (ADR 0025), projected from `space.nuck.join_request.v1`.
CREATE TABLE join_requests (
    guild_id     TEXT NOT NULL,
    user_id      TEXT NOT NULL,
    message      TEXT,
    status       TEXT NOT NULL,
    requested_at BIGINT NOT NULL,
    event_id     TEXT NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (guild_id, user_id)
);
