# ADR 0024 — Federation blocklists: local by default, subscribable lists

- Status: Accepted
- Date: 2026-10-21

## Context

ADR 0010 gives operators blocklists but not how they are shared, if at all.

## Decision

- **Local blocklists (a) are the default** and always available.
- **Subscribable published lists (b) are optional.** Operators may subscribe to
  externally published, **signed, versioned** blocklists (server name + reason +
  timestamp) fetched over HTTPS — like email RBLs or adblock lists. Multiple
  independent providers.
- **No central authoritative list (c)** — same reasoning as ADR 0021.
- **Operator entries always take precedence**; individual peers can be overridden or
  exempted.
- **Signing** lets operators trust a provider without trusting the transport.

## Consequences

- No central authority; operators retain final say over their federation.
- Subscribable lists create an ecosystem of blocklist providers, with the usual
  risks (over-blocking, provider capture) that signing and override mitigate but do
  not eliminate.
- A blocklisted peer must be handled gracefully: existing shared rooms degrade
  (no new events), and the client should surface the state rather than failing
  silently.
- Format and signing scheme are a small, versioned protocol surface to maintain.

## Verification update (2026-10-21)

Checked against Tuwunel ([verification](../verification/tuwunel.md#7-federation-blocklists--config-regex-not-subscribable-lists)):
Tuwunel's blocklists are **static config regex** —
`forbidden_remote_server_names`, `allowed_remote_server_names_experimental`,
`forbidden_remote_room_directory_server_names` — not subscribable lists.

**Refinement:** subscribable signed lists (b) are a **nuck-server component**: fetch
the list, verify the signature, **render it into Tuwunel's config regex, trigger a
config reload, and verify the result**. Operator entries still take precedence. This
pipeline needs its own design note.

