# ADR 0046 — No stored Tuwunel admin token

- Status: Accepted
- Date: 2026-10-22
- Relates to: [0028](./0028-trust-and-privilege-model.md), [0019](./0019-projection-pipeline.md), [0037](./0037-home-server-registration.md)

## Context

The MVP held a `TUWUNEL_ADMIN_TOKEN` — a homeserver-admin **user's session token** — for
two jobs: reconciliation (admin-bypass room-state/message reads and force-join) and
privileged registration (`approval`/`closed`).

That credential is the wrong shape: it is a person's live session (revocable,
person-bound, and a full-admin leak if disclosed), and the installer **cannot mint** one
because a Tuwunel admin token is not a config value — it is an ordinary access token
belonging to a user who has the admin badge (Tuwunel grants it to the first user,
`grant_admin_to_first_user`). Storing it made a fresh install confusing: `approval`
silently needed a secret that did not exist yet, and reconciliation was silently
disabled without it.

## Decision

**nuck-server holds no Tuwunel admin token.**

- **Reconciliation reads as the projector.** The projector is a normal **member** of
  every guild room (it must be, for AppService delivery, ADR 0019), so it reads resolved
  room state and history over the plain C-S API (`GET /rooms/{id}/state`,
  `GET /rooms/{id}/messages`). Room discovery uses the projector's own
  `GET /joined_rooms`. The projector joins via the AppService token. No admin bypass.
- **Registration creates accounts over C-S too.** `open`/`token` (and, at request time,
  `approval`) call the substrate's ordinary `/register`; nuck-server applies the policy
  and closes the substrate's registration to clients through the proxy (ADR 0037).
- **`approval` gates login instead of deactivating.** The account is created but held:
  nuck-server records the request and refuses that user's `POST /login` until an admin
  approves. No admin API, no deactivation, no stored password. On deny the request is
  marked denied — it leaves the queue and keeps blocking login.
- **Force-join is bot-invite only** (ADR 0041): redeem/approve invite the user, who then
  accepts. The optional admin force-join is gone.

`TUWUNEL_ADMIN_TOKEN` is removed from the config, `.env.example`, the compose file, and
`install.sh`.

## Consequences

- A default install needs **only generatable secrets** (Postgres password, AppService
  tokens, an optional registration token). No user session is ever persisted, and
  `approval` works with zero manual setup.
- Reconciliation always runs (with the projector identity), so the self-healing index is
  on by default instead of silently off.
- Reconciliation sees what a member sees: state is fully visible, but history is bounded
  by the room's history visibility. Guild rooms use `shared` visibility, so backfill is
  unaffected; a future `world_readable: false` history policy would need revisiting.
- Admin-only substrate features nuck-server may want later (unredacted reads, retention,
  bulk redaction, ADR 0035) will need a credential again — that would be a deliberate,
  feature-scoped decision, not a default.
