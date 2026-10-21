# Open Questions

Everything raised during design is either **decided** (see the ADRs), **deferred**
(planned, intentionally not v1), or **to verify** against the substrate. This file
tracks the latter two.

## Verified against Tuwunel

All six original assumptions were checked against Tuwunel's docs and source — see
[`verification/tuwunel.md`](./verification/tuwunel.md). Outcomes: backup/checkpoint,
AppService delivery, and push **confirmed**; admin API, MatrixRTC, and blocklists
**confirmed with caveats**; ignore-list enforcement **assumption was wrong**.

**New open items raised by verification:**

- **Unredacted retention — decided.** ADR 0039: keep a copy for the **operator** only
  (`allow_room_admins_to_request_unredacted_events = false`), 60-day window,
  operator-configurable. No longer open.
- **Blocklist compilation.** Tuwunel's blocklists are static config regex; subscribable
  signed lists (ADR 0024) must be compiled into config and reloaded by nuck-server —
  needs its own design note.
- **No CAPTCHA.** Tuwunel has no recaptcha equivalent; the optional CAPTCHA for `open`
  registration (ADR 0037) must be built in nuck-server or dropped.
- **Reports are not in Tuwunel.** `event_reports`/`user_reports` are unimplemented, so
  the moderation reports flow (ADR 0016) is nuck-server work.
- **RTC JWT service.** nuck-server must act as (or front) the MatrixRTC JWT service to
  enforce Nuck voice permissions (ADR 0007); the stock lk-jwt-service only checks
  Matrix membership.
- **LAN push gateway.** `ip_range_denylist` blocks private-range push destinations by
  default; the private-network gateway needs an explicit allow. (ADR 0031)

## Deferred features (roadmap, not v1)

- **Bots: interactions** — slash commands, buttons, modals; and **webhooks**.
  (ADR 0018)
- **Anti-raid** — join throttling, verification gates, challenges. (ADR 0025)
- **Rich presence** — online/idle/dnd, custom status, activity, over the `a2`
  WebSocket. (ADR 0017)
- **Automod** — keyword filters, raid protection, spam heuristics. (ADR 0016)
- **Retention policies** — auto-expire messages after N days. (ADR 0035)
- **Call E2EE for DMs** — LiveKit insertable streams, key distribution via Matrix.
  (ADR 0032)
- **SFU cascade** — media topology across servers. (ADR 0007)
- **Mobile client + push gateway** — APNs/FCM. (ADR 0031)
- **Cross-server directory aggregation** — beyond per-server directory + seed list.
  (ADR 0021)
- **Continuous replication / PITR** for backups. (ADR 0034)
- **Self-contained signed invite codes** — offline validation. (ADR 0011)
- **OIDC login** — as an additional auth method. (ADR 0009)
- **Dedicated search engine** — Meilisearch/Typesense behind the search interface.
  (ADR 0029)

## Not yet designed (new work, no ADR)

- **`/_nuck/v1` endpoint inventory.** *Drafted* in
  [`protocol/api.md`](./protocol/api.md) — needs request/response schemas, error
  model, and pagination convention.
- **`space.nuck.*` event registry.** *Drafted* in
  [`protocol/events.md`](./protocol/events.md) — needs concrete content schemas, a
  role-ID scheme, and an owner for the registry mechanism.
- **Permission bitfield layout.** The concrete bit assignments and the exact
  power-level mapping table. (ADR 0020 fixes the approach, not the table.)
- **Federation feature-gate matrix.** Which capabilities are gated behind which
  protocol versions. (ADR 0026)
- **DM-call UX** when text is E2EE but media is not (ADR 0032).
- **Hosting/upgrade runbook** for the pinned version matrix. (ADR 0026)

## Rejected

- Nuck-owned accounts separate from Matrix identity. (ADR 0009)
- Central guild directory or central blocklist authority. (ADR 0021/0024)
- E2EE with a server-side device in every room. (ADR 0008)
- Bundling a full observability stack by default. (ADR 0033)
- Wails client (Go backend cannot reuse `nuck-protocol` or `matrix-rust-sdk`).
  (ADR 0027)
