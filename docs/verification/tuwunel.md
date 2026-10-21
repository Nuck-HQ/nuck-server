# Verification: Tuwunel Assumptions

Checks against Tuwunel's documentation and source
(`github.com/matrix-construct/tuwunel`, `main`) for the assumptions listed in
[open-questions](../open-questions.md). Date: 2026-10-21.

Legend: **✅ confirmed** · **⚠️ confirmed with a caveat** · **❌ assumption wrong**

## Summary

| # | Assumption | Status | Impact |
|---|---|---|---|
| 1 | RocksDB consistent checkpoint / backup exists | ✅ (better) | ADR 0034 simplified |
| 2 | Admin API covers user lifecycle, room ops, AppService mgmt | ⚠️ | ADR 0028/0037 refined; no backup HTTP endpoint |
| 3 | AppService receives all room events incl. federated | ✅ | ADR 0019 confirmed |
| 4 | Tuwunel supports MatrixRTC + external SFU | ⚠️ | ADR 0007 refined: nuck-server is the RTC JWT service |
| 5 | Push-rule / push-gateway support | ✅ | ADR 0031 confirmed; LAN gateway caveat |
| 6 | `m.ignored_user_list` enforced server-side | ❌ | ADR 0030 confirmed as *not* enforcement; nuck-server must enforce |
| 7 | Federation blocklists | ⚠️ | ADR 0024: config-regex, not subscribable lists |

Plus two findings that affect ADRs not on the list: **reports are not implemented
in Tuwunel** (ADR 0016) and **unredacted events are retained by default** (ADR 0035).

---

## 1. Backup & checkpoint — ✅ confirmed (better than assumed)

Tuwunel offers **three** online/offline methods (`docs/backups.md`):

- **Managed online backups** — `database_backup_path` +
  `database_backups_to_keep`; admin-room commands `server backup-database`,
  `list-backups`, `verify-backup`, `delete-backups`; restore via
  `tuwunel --restore-backup[=ID]` (CLI only, deliberately refused from config).
- **Checkpoints** — `server checkpoint-database [--path …]`, a consistent,
  directly usable RocksDB directory, created **online** (hard-links SST files).
  Schedulable via `admin_signal_execute` + `SIGUSR2`.
- **Offline copy** — stop and copy `database_path`.

**Caveats:** online backups/checkpoints **exclude media** (back up `media/` and any
storage providers separately). Admin commands are **admin-room commands**, not HTTP
endpoints; a CLI can run them via `tuwunel --maintenance --execute "server
checkpoint-database --path …"`.

**Impact (ADR 0034):** no coordinated quiesce is needed — use
`server backup-database` or `checkpoint-database` online. `nuck backup` invokes
Tuwunel via `--execute`, then snapshots media + Postgres. The version-manifest idea
still stands and is ours to add.

## 2. Admin API — ⚠️ confirmed with caveats

Tuwunel serves the **Synapse Admin API** under `/_synapse/admin`
(`docs/development/compliance/synapse-admin.md`; 69/100 endpoints supported).

Relevant, all ✅:

- **Users:** `PUT /v2/users/{user_id}` (create/modify), `POST /v1/deactivate/{id}`,
  `POST /v1/reset_password/{id}`, `PUT /v1/suspend/{id}`,
  `POST /v1/users/{id}/login` (**impersonation**: mint a token to act as a user),
  `POST /v1/user/{id}/redact` (bulk redact a user's events).
- **Rooms:** `GET /v1/rooms/{id}/state` (full state, admin bypass),
  `GET /v1/rooms/{id}/messages` (timeline, admin bypass), `POST /v1/join/{room}`,
  `POST /v1/rooms/{id}/make_room_admin`, `DELETE /v1/rooms/{id}`,
  `POST /v1/purge_history/{id}`.
- **Registration tokens:** full CRUD at `/v1/registration_tokens`.
- **Federation:** `GET /v1/federation/destinations` (+ retry state),
  `POST .../reset_connection`.
- **Misc:** `GET /v1/fetch_event/{event_id}` (raw, unredacted),
  `POST /v1/send_server_notice`.

**Caveats:**

- **No HTTP endpoint for backup/checkpoint** — those are admin-room/CLI only (§1).
- **Impersonation and some user endpoints are "not served when MAS is active"** —
  we are not using MAS, so this is fine, but it forecloses MAS later without a
  rethink.
- **Event/user reports are 🟥 not implemented** (`event_reports`, `user_reports`
  deferred) → the moderation **reports** flow (ADR 0016) must be built in
  nuck-server.
- No "create room as user" endpoint; use impersonation + C-S API, or an AppService.

**Impact (ADR 0028):** the scoped admin token is validated — the surface exists and
is broad. **Impersonation via `users/{id}/login`** is a cleaner mechanism than
AppService masquerading for acting as a user; prefer it for the narrow cases that
need it. The projector's reconciliation/backfill (ADR 0019) can use
`rooms/{id}/state` and `rooms/{id}/messages` directly.

## 3. AppService event delivery — ✅ confirmed

`src/service/appservice/append.rs`: `append_pdu` runs **after accepting each new
PDU** (including federated ones, which flow through `timeline::append`) and delivers
to an appservice when:

- `appservice_in_room(room_id, appservice)` — the appservice has a user in the room,
  **or**
- the PDU sender matches the appservice's user namespace, **or**
- it is a room-member event matching the appservice.

Delivery goes through the `sending` service (with retry/backoff; there is a
`backoff_metrics` admin command). Registration supports admin-room, config, and
`appservice_dir` (YAML); `url: null` = receive-only; `receive_ephemeral` and
`msc3202_transaction_extensions` exist for typing/receipts and E2E bookkeeping.

**Impact (ADR 0019):** confirmed. The projector AppService **must have its user
joined to every guild room** to receive remote events — the auto-join plan is
correct and required.

**Registration (`docs/appservices.md`):** three methods exist — the admin-room command
`!admin appservices register` (persisted in the database, applied **live**, no restart),
inline `tuwunel.toml` config, and an `appservice_dir` of YAML registration files (loaded
on **startup**; new/removed files need a restart). There is **no HTTP registration
endpoint**. nuck-server therefore *generates* the registration document and, when
`APPSERVICE_REGISTRATION_PATH` is set, writes it into `appservice_dir`; the admin-room
form of the same YAML applies it live.

## 4. MatrixRTC — ⚠️ confirmed, one design refinement

`docs/calls/matrix_rtc.md`: MatrixRTC (Element Call) is supported, using
**MSC4143** (`org.matrix.msc4143.rtc_foci`). **Tuwunel does not embed an SFU** — an
external one (LiveKit) is required, discovered via `.well-known` `rtc_foci` with
`type: livekit` and `livekit_service_url`. Tuwunel exposes `livekit_url` config that
it serves into `.well-known`.

The reference deployment runs a **`matrix-rtc-jwt` service** (Element's
lk-jwt-service) exposing `/sfu/get`, `/get_token`, `/healthz`: the client sends a
Matrix **OpenID token**, and the JWT service returns the LiveKit URL + a JWT.

**Refinement (ADR 0007):** the assumption "nuck-server mints LiveKit tokens" is
correct in spirit but must be **nuck-server acting as the RTC JWT service** (or
fronting it). The stock lk-jwt-service only checks Matrix room membership — it knows
nothing of Nuck permissions (ADR 0006), so voice-permission enforcement requires
nuck-server to validate the caller and mint the JWT itself. Point `.well-known`
`rtc_foci.livekit_service_url` at nuck-server's RTC endpoint.

## 5. Push — ✅ confirmed

`docs/pushers.md`: Tuwunel implements the **Push Gateway API**; clients create
pushers naming a gateway URL and Tuwunel POSTs to `/_matrix/push/v1/notify`.
UnifiedPush works out of the box.

**Caveat:** `ip_range_denylist` refuses **loopback and private-range** destinations
by default — a self-hosted gateway on a LAN address is rejected at pusher creation
unless the range is admitted. Relevant to our private container network.

**Impact (ADR 0031):** confirmed; the deferred mobile push gateway is a real,
self-hostable component.

## 6. `m.ignored_user_list` — ❌ not server-side enforcement

`src/main/tests/invite_ignored_user.rs` states it explicitly:

> "The ignore list decides what sync serves, never what the server stores, so the row
> has to be written and withheld rather than dropped."

An ignored user's invite is **stored and federated**; it is merely **withheld from
`/sync`** and surfaces on un-ignore.

**Impact (ADR 0030):** confirmed and sharpened. Blocking is **sync-time filtering,
not enforcement**. nuck-server **must** enforce blocking at the API level
(suppress friend requests and DM invites) — the substrate will not.

## 7. Federation blocklists — ⚠️ config-regex, not subscribable lists

`docs/moderation.md`: `forbidden_remote_server_names` (regex, matched against room
IDs/aliases, sender server names, inbound `X-Matrix` origins, outbound handlers),
`allowed_remote_server_names_experimental` (allow-list), and
`forbidden_remote_room_directory_server_names`. These are **static config values**
(the config is reloadable).

**Impact (ADR 0024):** the mechanism is a **config-file regex ACL**, not a
subscribable signed list. To implement subscribable lists, nuck-server must fetch
them and **compile them into Tuwunel's config regex, then trigger a config reload**.
That is a real component: fetch → verify signature → render config → reload → verify.
Worth its own design note.

---

## Additional findings

### Moderation (ADR 0016)

- Available: `!admin rooms moderation ban-room` / `unban-room` / `ban-list-of-rooms`,
  `rooms delete`, `federation disable-room`, `users deactivate`, `users redact-event`,
  `media delete-list`, identifier blocklists, `block_non_admin_invites`,
  auto-deactivation triggers.
- **Reports are not implemented** (`event_reports`/`user_reports` deferred) → build
  in nuck-server.
- Guild **purge** maps well to `POST /v1/user/{id}/redact` (bulk) and
  `rooms/{id}/messages` for scoping; or per-event redaction.

### Retention & deletion (ADR 0035) — important nuance

`docs/moderation.md`: **`save_unredacted_events` defaults to `true`**, with
**`redaction_retention_seconds` default 60 days**, and
**`allow_room_admins_to_request_unredacted_events` defaults to `true`** (MSC2815) —
users with the `redact` power level can retrieve unredacted copies. `disable_local_redactions`
also exists.

**Impact:** "deletion is a request, not a guarantee" is even truer than stated:
redacted content is **retained for 60 days by default and retrievable by anyone with
redact power** — which, under our mirrored power levels (ADR 0006), means guild
moderators. Decide explicitly whether Nuck sets these knobs differently (e.g.
`save_unredacted_events = false`, or a shorter window), because it is a privacy
policy decision, not just an implementation detail.

### Registration (ADR 0037) — ✅ confirmed, one gap

All four modes map to real primitives: registration disabled by default;
`allow_registration` + **registration tokens** (static, file-based, or admin-issued
with `--once`/`--max-uses`/`--max-age`, plus revoke/list) = `token`; the explicit
`yes_i_am_very_very_sure…` flag = `open`; admin user creation = `closed`/`approval`.
**Email verification** exists (`require_email_for_registration`,
`require_email_for_token_registration`, SMTP relay; email only). **Guest
registration** is separate and off by default.

**Gap:** **no CAPTCHA** (Synapse's recaptcha has no Tuwunel equivalent) → the
optional CAPTCHA for `open` mode must be implemented in nuck-server's registration
proxy, or dropped from the plan.

### Custom events & room types

No obstacle found: Tuwunel is a full Matrix implementation with the standard state
resolution and power-level machinery, which is what ADR 0005/0006/0014 depend on.
Custom room `type` and custom `space.nuck.*` state events are ordinary Matrix
features. (Worth a targeted test once `nuck-protocol` exists.)

## User-id state keys — a real constraint (2026-10-22)

Found while running the v1 surface against a live substrate: Tuwunel's room auth
rejects a state event whose `state_key` is **another user's id**, even when sent by the
privileged action bot —

```
PUT .../state/space.nuck.ban.v1/%40bob%3Alocalhost  ->  403
{"errcode":"M_FORBIDDEN","error":"Auth check failed: sender cannot send event with
 `state_key` matching another user's ID"}
```

Confirmed for `space.nuck.member.v1`, `space.nuck.ban.v1`, and a user-keyed
`space.nuck.override.v1`. This is a Tuwunel-specific auth check (it is **not** a general
Matrix auth rule); it does not apply to the standard `m.room.member`, which those events
are not.

**Resolution:** user-id state keys are **encoded without the leading `@`** (ADR 0045),
which the projection decodes back to the real user id. Role ids and `everyone` are
unaffected. The fake substrate used by the unit tests does not enforce the rule, so this
was only visible end to end — `scripts/smoke.sh` now exercises it.
