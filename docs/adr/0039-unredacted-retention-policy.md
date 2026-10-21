# ADR 0039 — Unredacted event retention: keep for the operator, not for moderators

- Status: Accepted
- Date: 2026-10-21

## Context

Tuwunel defaults to `save_unredacted_events = true`,
`redaction_retention_seconds = 60 days`, and
`allow_room_admins_to_request_unredacted_events = true`. Redacted content therefore
survives 60 days and is retrievable by **anyone holding the `redact` power level** —
which, under our mirrored power levels (ADR 0006), means **guild moderators**, who are
ordinary users, not trusted platform staff. Meanwhile ADR 0035 promises deletion is
best-effort; the two together are a privacy footgun.

## Decision

**Option (c), shipped as the default, with operator configurability (d).**

- **Keep a copy:** `save_unredacted_events = true`, so abuse deleted after it was
  reported, legal holds, and raid post-mortems remain reviewable.
- **Lock it to the operator:** `allow_room_admins_to_request_unredacted_events = false`.
  Guild moderators **cannot** retrieve deleted content; only the server operator can.
- **Defined retention window:** keep the 60-day default unless the operator changes it.
- **Operator-configurable, local, never federated** — same philosophy as ADR 0037.
- **State it honestly:** the client and privacy policy must say that redaction removes
  content from the room **but the server operator may briefly retain a copy for abuse
  handling**.

## Consequences

- Guild moderators lose an undelete capability they were never intended to have.
- Operators retain the forensic ability that accountability for a server implies.
- Redaction is no longer "instant and permanent" (option b) — but the copy is
  unreachable by guild members, which is the property users actually expect.
- The default is a policy statement, not just a config value; changing it is a
  user-visible privacy change.
- This setting is per-server, so guilds hosted on different servers may have different
  retention semantics — acceptable, since the hosting server is the accountable party.
