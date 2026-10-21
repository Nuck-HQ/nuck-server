# ADR 0031 — Notifications: Matrix-native push rules; mobile push deferred

- Status: Accepted
- Date: 2026-10-21

## Context

Discord has per-channel notification settings, mentions, and push across
devices. Matrix has a complete notification model: push rules with mentions,
keywords, and per-room overrides, evaluated **server-side** to mark events as
notify/highlight.

## Decision

- **Notification model: Matrix push rules (option a).** Settings live in
  `m.push_rules` account data; the substrate evaluates them and flags events. The
  client renders what the server flags.
- **Desktop (v1): OS notifications from the live sync connection.** The Tauri client
  (ADR 0027) is running and holds a sync connection, so **no push gateway is needed**.
- **Mobile push is deferred.** When added, it uses a self-hostable **push gateway**
  in the stack, and clients register a Matrix pusher.
- **Discord's per-channel modes** ("all messages" / "mentions only" / "nothing") map
  onto Matrix **per-room push rules**; the client writes push rules rather than a
  custom settings store.

## Consequences

- Notification semantics are standard; nuck-server owns nothing new for v1 beyond
  relaying push rules and the `notify`/`highlight` counts from Tuwunel.
- Desktop-only v1 avoids running and securing a push gateway entirely.
- Mobile support later requires the push gateway as new infrastructure (APNs/FCM
  credentials, per-device pushers).
- The client must translate Discord-style notification preferences into push rules,
  which is a mapping to maintain.

## Verification update (2026-10-21)

Checked against Tuwunel
([verification](../verification/tuwunel.md#5-push--confirmed)): Tuwunel implements the
**Push Gateway API** (`/_matrix/push/v1/notify`); clients register pushers; UnifiedPush
works out of the box. **Confirms** the deferred mobile push gateway as real,
self-hostable infrastructure.

**Caveat:** `ip_range_denylist` refuses **loopback and private-range** push
destinations by default, so a gateway on the private container network (ADR 0012/0028)
needs an explicit allow, or the pusher is rejected at creation.

