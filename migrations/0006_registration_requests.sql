-- Pending registration requests for `approval` mode (ADR 0037).
--
-- No password is stored: the account is created immediately via the admin API in a
-- **deactivated** state, and approval only activates it. This keeps plaintext
-- passwords out of rest.
CREATE TABLE registration_requests (
    id         TEXT PRIMARY KEY,
    username   TEXT NOT NULL,
    user_id    TEXT NOT NULL,
    created_at BIGINT NOT NULL
);
