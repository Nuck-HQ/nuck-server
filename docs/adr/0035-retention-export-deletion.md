# ADR 0035 — Data retention, export, and deletion

- Status: Accepted
- Date: 2026-10-21

## Context

Operators and users need account lifecycle, deletion, and export. The substrate
already provides several primitives; the hard boundary is E2EE content.

## Decision

**Substrate-native + minimal custom (option a).**

- **Account deactivation/deletion:** Tuwunel admin API, via nuck-server's scoped
  admin token (ADR 0028).
- **Message deletion:** Matrix **redaction** (federates). Guild "purge" (ADR 0016) is
  bulk redaction.
- **Export:** server-side for **plaintext** data (profile, guilds, plaintext channel
  history). **E2EE DM content is never exportable by the server** (ADR 0008) — only
  the client, holding the keys, can export it; the server can export DM *metadata*
  only.
- **Retention policies** (auto-expire after N days) are **deferred**. When added, they
  are guild state enforced by bulk redaction.

## Consequences

- Account and message lifecycle reuse substrate semantics; no parallel system.
- **The export boundary must be stated plainly to users:** the server cannot export
  E2EE DM content, and no amount of server-side work changes that.
- Redaction-based deletion is federated and best-effort across servers — a
  misbehaving remote server could retain copies. Deletion is a request, not a
  guarantee, in a federated system.
- Retention remains unimplemented; guilds cannot yet auto-expire content.

## Verification update (2026-10-21) — decision required

Checked against Tuwunel
([verification](../verification/tuwunel.md#retention--deletion-adr-0035--important-nuance)):
Tuwunel defaults to **`save_unredacted_events = true`**,
**`redaction_retention_seconds = 60 days`**, and
**`allow_room_admins_to_request_unredacted_events = true`** (MSC2815).

So redacted content is **retained for 60 days and retrievable by anyone holding the
`redact` power level** — which, under our mirrored power levels (ADR 0006), means
**guild moderators**. "Deletion is a request, not a guarantee" is truer than this ADR
originally stated.

**Open decision (not yet made):** Nuck's stance on these knobs — e.g.
`save_unredacted_events = false`, or a shorter window, or accepting the default as a
moderation tool. This is a **privacy policy choice**, not an implementation detail, and
should be settled before launch.

**Resolved:** see **ADR 0039** — keep a copy for the **operator** only
(`allow_room_admins_to_request_unredacted_events = false`), 60-day window,
operator-configurable.

