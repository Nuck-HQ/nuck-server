# ADR 0008 — E2EE stance: plaintext guild channels, E2EE DMs

- Status: Accepted
- Date: 2026-10-21

## Context

E2EE determines whether the server can see message content, which in turn decides
whether server-side search, moderation, content indexing (ADR 0003), and bots are
possible. Nuck's differentiators — permissions, moderation, voice — need the server
to act on content or metadata.

## Decision

**Hybrid model (option c):**

- **Guild channels are plaintext by default.** The homeserver can read them, so
  server-side search, moderation, content indexes, and bots work.
- **DMs are E2EE** (Matrix Olm/Megolm), since there is no moderation use case for
  them and users expect it.
- **E2EE is never used for guild state.** Roles and permissions (ADR 0006) must be
  readable by every participating server to be enforced; they are always plaintext.
- **Rejected (d):** a server-side device holding keys in every room gives E2EE's
  complexity without its guarantees. If we want at-rest/in-transit encryption only,
  say so explicitly — but do not call it E2EE.

## Consequences

- Server-side content search and moderation are available where the product needs
  them (guild channels) and absent where they aren't (DMs).
- Clients must implement Olm/Megolm for DMs (via `matrix-rust-sdk`), and device
  verification is required for DM E2EE to be meaningful.
- Content indexes (ADR 0003) cover guild channels only; DM content is never indexed.
- A per-channel E2EE toggle remains a possible future extension, not v1.
