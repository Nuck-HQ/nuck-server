# ADR 0045 — User-id state keys are encoded (Tuwunel auth constraint)

- Status: Accepted
- Date: 2026-10-22

## Context

The guild-level `space.nuck.member.v1`, `space.nuck.ban.v1`, and
`space.nuck.join_request.v1` events (ADR 0005/0016/0025) are state events keyed by the
target **user id**, and per-user channel overrides (`space.nuck.override.v1`) are keyed
by a user id too. Under ADR 0041 the privileged **action bot** writes them.

Against the real substrate this fails: Tuwunel's room auth rejects a state event whose
`state_key` is another user's id —
`sender cannot send event with state_key matching another user's ID` — even for an
AppService bot. Verified live for `member`, `ban`, and user-target `override` (the
fake substrate used in tests does not enforce it, which is why it went unnoticed; the
MVP never exercised these paths).

## Decision

**Encode the user id in the state key by dropping its leading `@`.**

- `@bob:example.org` → state key `bob:example.org` — no longer a valid user id, so the
  auth check passes, and the mapping is trivially reversible.
- Applies to the `member`, `ban`, and `join_request` state keys, and to the
  **user** form of an `override` target.
- **Role ids and `everyone` are unchanged** (they are never user-id-shaped). An override
  target is a user iff it contains `:` (a user id is `localpart:server`; role ids never
  contain `:`).
- Decoding happens in the projection ingest, so the projection and every API response
  carry **real user ids**. The encoding is a substrate detail and never reaches the
  client (v1's surface is REST, not raw Nuck state).

Implementation: `src/keys.rs` (`encode_user_key` / `decode_user_key` /
`encode_override_target` / `decode_override_target`).

## Consequences

- Member/ban/join-request/override writes succeed on unmodified Tuwunel, and the
  projection is identical to before (real user ids).
- `docs/protocol/events.md` records the encoding; a client or peer reading raw
  `space.nuck.*` state must decode it. (The v1 client does not.)
- If Tuwunel ever relaxes the rule, the encoding can be dropped by a tolerant
  decode that already accepts both forms.
