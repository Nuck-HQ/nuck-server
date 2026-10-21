# The deployable MVP

**Status: complete and verified end to end.** `./install.sh` on a clean checkout brings
up the stack, and `./scripts/smoke.sh` exercises the full path.

## Definition of done (met)

From a clean checkout, `./install.sh` brings up Tuwunel + Postgres + nuck-server. Then,
talking **only to nuck-server**, a user can:

1. register and log in, ✅
2. create a guild (a Space with a default text channel), ✅
3. create an additional channel, ✅
4. send a message in a channel and read it back, ✅
5. have permissions enforced (a non-member is refused; the owner/admin can manage
   roles and channel overrides). ✅

## Milestones

| # | Milestone | Status |
|---|---|---|
| M1 | **Deployment foundation** — `./install.sh` boots the stack, `/healthz` + `/_nuck/v1/server-info`, migrations, AppService registration. | ✅ |
| M2 | **Client surface & auth** — transparent `/_matrix/client/*` and `/_matrix/media/*` proxy; register/login/sync through nuck-server. | ✅ |
| M3 | **Guilds & channels** — `POST /_nuck/v1/guilds` (Space + `@everyone`/admin roles + default channel), `GET /_nuck/v1/guilds/{id}`, `POST /_nuck/v1/guilds/{id}/channels`. | ✅ |
| M4 | **Messages** — send/read through the proxy; guild-channel sends are permission-checked. | ✅ |

## Out of scope (explicit)

Voice/video, DMs and friends, invites and join requests, the bot gateway/WebSocket,
federation (single server only), notifications/push, a client UI, and search UI.

## Runbook

```sh
# Fresh install: generates .env + data/tuwunel.toml, builds the image, starts.
./install.sh

# Prove the whole path end to end (register → guild → channel → message → 403s).
./scripts/smoke.sh

# Logs / stop / wipe.
docker compose logs -f nuck-server
docker compose down -v
rm -f .env && rm -rf data   # then re-run ./install.sh
```

Services: `postgres` (index), `tuwunel` (substrate, internal only), `nuck-server`
(exposed on `NUCK_PORT`, default 8008). Only nuck-server is reachable from outside.

## Design notes discovered while deploying

- **Registration is open on the substrate** for the MVP (`allow_registration` + the
  "very sure" flag). Fine on localhost; lock down with a registration token before
  exposing it (ADR 0037 policy is still deferred).
- **The reserved everyone role id is `everyone`, not `@everyone`.** Matrix treats a
  state key beginning with `@` as a user id and the substrate rejects writing such a
  role event; `@everyone` remains the display name.
- **Pre-join state is backfilled.** AppService push only starts when the projector
  joins, so state written earlier (the `m.room.create` that establishes ownership, the
  creator's membership) is read from the C-S API **as the projector** and reconciled
  into the projection. This complements the admin-token reconciliation loop, which is
  still off without `TUWUNEL_ADMIN_TOKEN`.
- **Local room ids omit `:server`** in Tuwunel, so the guild home server is empty for
  local guilds and is treated as "this server".
- **Channel creation requires `MANAGE_CHANNELS`**; guilds are created by the user (who
  becomes the owner) and the creator gets an `admin` role (ADR 0038).

## Not in the MVP but next

The full roadmap remains in `docs/open-questions.md` and the ADRs. The permission
model (roles, overrides, bans, ownership, membership) is implemented beyond what the MVP
exercises.
