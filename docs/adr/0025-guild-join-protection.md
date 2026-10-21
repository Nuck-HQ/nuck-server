# ADR 0025 — Guild join protection: invite limits, rate limits, join gates

- Status: Accepted
- Date: 2026-10-21

## Context

The invite redeem endpoint (ADR 0011) is a cross-server attack surface, and guilds
need to control how people join.

## Decision

**v1: invite limits + rate limits + join gates (option a).**

- Invites have **max-uses and expiry** (already guild state, ADR 0011).
- The redeem endpoint is **rate-limited per IP, per user, and per origin server**;
  codes are unguessable.
- **Join modes (guild state, federated):**
  - `open` — anyone in the directory may join;
  - `invite-only` — requires a valid invite code (default);
  - `approval` — a join request enters a queue; a member with `MANAGE_MEMBERS`
    approves.
- The **approval queue is stored as guild state** and surfaced in the client; no new
  mechanism.

**Anti-raid (option c) is deferred** — join-rate throttling, verification gates, and
challenge infrastructure. It is easier to add on top of join gates than to bolt on
later.

## Consequences

- Covers the realistic threats: leaked codes (max-uses/expiry), code brute-forcing
  (rate limits + entropy), unwanted joins (join gates).
- Approval requests are durable and federated, so they survive host restarts.
- A host outage blocks redemption (accepted, ADR 0011).
- Deferring anti-raid means a determined raid can still overwhelm a guild; the
  escape hatch is switching to `invite-only`/`approval` and using moderation
  (ADR 0016).
