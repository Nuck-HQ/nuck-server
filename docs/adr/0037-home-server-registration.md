# ADR 0037 — Home server registration policy

- Status: Accepted
- Date: 2026-10-21

## Context

Guild joining (ADR 0025) is distinct from **creating an account on a Nuck home
server**. The home server holds a user's identity (ADR 0009), so this is the more
security-relevant axis. Self-hosting means the operator decides.

## Decision

**Configurable registration modes with abuse controls (option a). Default: `token`.**

- **Modes:** `open` (anyone), `token` (registration token required — **default**),
  `approval` (admin approves each request), `closed` (admin creates accounts only).
- **Optional abuse controls** for `open`: email verification and CAPTCHA.
- **Registration can be disabled at any time.**
- **`approval` is the one custom mode:** nuck-server holds pending requests and an
  admin approves, after which nuck-server creates the user via the admin API
  (ADR 0028).
- **Enforcement lives in nuck-server**, because the client only talks to it
  (ADR 0001/0013). Tuwunel provides primitives (registration tokens, disabling
  registration, admin user creation); nuck-server applies policy, rate-limits
  (ADR 0036), and proxies.
- **Registration policy is local and never federated.** A remote server has no say in
  who joins yours.

## Consequences

- Operators control their own server's membership without a central authority.
- `token` as default prevents accidental open instances (matching Conduit/Tuwunel's
  own default).
- The `approval` queue is new nuck-server state and UI surface.
- Registration endpoints are a primary abuse surface and take the tightest rate
  limits (ADR 0036).
- Email verification/CAPTCHA depend on substrate capabilities; verify before
  advertising them.

## Verification update (2026-10-21)

Checked against Tuwunel
([verification](../verification/tuwunel.md#registration-adr-0037--confirmed-one-gap)):
all four modes map to real primitives — registration is **disabled by default**;
`allow_registration` + **registration tokens** (static, file-based, or **admin-issued**
with `--once` / `--max-uses` / `--max-age`, plus revoke/list) = `token`; the explicit
`yes_i_am_very_very_sure…` flag = `open`; admin user creation
(`PUT /_synapse/admin/v2/users/{user_id}`) = `closed` and the `approval` fulfilment
path. **Email verification** exists (`require_email_for_registration`,
`require_email_for_token_registration`, SMTP relay; email only). **Guest registration**
is separate and off by default.

**Gap:** there is **no CAPTCHA** in Tuwunel (no recaptcha equivalent) — the optional
CAPTCHA for `open` mode must be **built in nuck-server's registration proxy, or
dropped** from the plan. Admin-issued tokens also give a ready-made mechanism for the
`token` mode without config edits.

## Implementation (2026-10-22)

Implemented; see [ADR 0046](./0046-no-stored-admin-token.md) for the no-admin-token
mechanics.

- All non-`closed` modes create the account through the substrate's ordinary C-S
  `/register`; nuck-server applies the policy, and the C-S proxy closes the substrate's
  own `/register` to clients so the policy cannot be bypassed.
- **`approval` creates the account but gates its login:** nuck-server records the request
  and refuses that user's `/login` until an admin approves. No admin API, no
  deactivation, and no plaintext password is stored. Denying marks the request denied —
  it leaves the queue but keeps blocking login.
- `closed` returns `403`; the operator creates accounts out of band.
- The approval queue is gated by `NUCK_ADMIN_USERS` (operator admins, not a guild role).


