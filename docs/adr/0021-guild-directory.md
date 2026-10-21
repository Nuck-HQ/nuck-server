# ADR 0021 — Guild directory: per-server, federated discovery, no central service

- Status: Accepted
- Date: 2026-10-21

## Context

ADR 0011 allows an optional public directory but did not decide its shape. A
directory must solve the bootstrap problem: how a user finds a guild they weren't
invited to, without introducing a central point of control.

## Decision

- **Per-server directory (b).** Each server exposes a `/_nuck/v1/directory` listing
  the public guilds it hosts, derived from guild state.
- **Third-party listing sites (d) are encouraged.** Anyone may crawl public
  directories and publish listings.
- **No central directory service (c).** A single hosted index would contradict
  ADR 0001.
- **Bootstrap via a curated seed list** bundled with client and server, the way
  Matrix bootstraps server discovery. Cross-server aggregation is **deferred** and
  best-effort when added.
- **v1 ships** per-server directory + seed list. Invite codes (ADR 0011) remain the
  primary growth path; the directory complements them.

## Consequences

- No single point of control or failure for discovery.
- The bootstrap problem is solved by an optional seed list, not an authority.
- A guild's visibility (public/private) is guild state and federates.
- Cross-server search is out of v1; users find remote guilds via invite codes, seed
  list, or third-party listings.
