-- Approval state (ADR 0037/0046): a `status` so a denied request leaves the pending
-- queue while still blocking login for its (created) account.
ALTER TABLE registration_requests ADD COLUMN status TEXT NOT NULL DEFAULT 'pending';
