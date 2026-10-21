#!/usr/bin/env bash
# End-to-end smoke test for the v1 client surface (docs/protocol/api.md).
#
# Exercises the whole path through nuck-server: register via the Nuck policy endpoint,
# create a guild and channels, send/read a message (permission-checked), invite a second
# user and redeem, moderation (role + ban/unban), and a DM.
set -euo pipefail

BASE="${NUCK_URL:-http://localhost:8008}"
STAMP="$(date +%s)"

jget() {
    python3 -c 'import sys,json;d=json.load(sys.stdin);print(d[sys.argv[1]])' "$1" 2>/dev/null
}

# Register through the Nuck policy endpoint (ADR 0037); fall back to login if the user
# already exists.
register() {
    local user="$1" pass="$2" json
    json="$(curl -sS -X POST "$BASE/_nuck/v1/register" \
        -H 'content-type: application/json' \
        -d "{\"username\":\"$user\",\"password\":\"$pass\"}")"
    if printf '%s' "$json" | grep -q access_token; then
        printf '%s' "$json"
        return
    fi
    curl -fsS -X POST "$BASE/_matrix/client/v3/login" \
        -H 'content-type: application/json' \
        -d "{\"type\":\"m.login.password\",\"identifier\":{\"type\":\"m.id.user\",\"user\":\"$user\"},\"password\":\"$pass\"}"
}

echo "== policy =="
curl -fsS "$BASE/_nuck/v1/register/policy" && echo

echo "== register =="
alice_json="$(register "alice$STAMP" "pw-alice-$STAMP")"
alice_token="$(printf '%s' "$alice_json" | jget access_token)"
alice_id="$(printf '%s' "$alice_json" | jget user_id)"
bob_json="$(register "bob$STAMP" "pw-bob-$STAMP")"
bob_token="$(printf '%s' "$bob_json" | jget access_token)"
bob_id="$(printf '%s' "$bob_json" | jget user_id)"
echo "registered $alice_id and $bob_id"

echo "== create guild =="
guild_json="$(curl -fsS -X POST "$BASE/_nuck/v1/guilds" \
    -H "Authorization: Bearer $alice_token" -H 'content-type: application/json' \
    -d '{"name":"Smoke Guild"}')"
guild_id="$(printf '%s' "$guild_json" | jget guild_id)"
channel_id="$(printf '%s' "$guild_json" | jget channel_id)"
echo "guild $guild_id, default channel $channel_id"

echo "== guild appears in the caller's list =="
for _ in $(seq 1 40); do
    list="$(curl -sS "$BASE/_nuck/v1/guilds" -H "Authorization: Bearer $alice_token")"
    if printf '%s' "$list" | grep -q "$guild_id"; then break; fi
    sleep 0.5
done
printf '%s' "$list" | grep -q "$guild_id" || { echo "FAIL: guild not listed" >&2; exit 1; }
echo "guild listed"

echo "== create a channel =="
chan_json="$(curl -fsS -X POST "$BASE/_nuck/v1/guilds/$guild_id/channels" \
    -H "Authorization: Bearer $alice_token" -H 'content-type: application/json' \
    -d '{"name":"random","kind":"text"}')"
chan_id="$(printf '%s' "$chan_json" | jget channel_id)"
[ -n "$chan_id" ] || { echo "FAIL: no channel id" >&2; exit 1; }
echo "channel $chan_id"

echo "== send + read a message (permission-checked) =="
curl -fsS -X PUT "$BASE/_matrix/client/v3/rooms/$chan_id/send/m.room.message/smoke-$STAMP" \
    -H "Authorization: Bearer $alice_token" -H 'content-type: application/json' \
    -d '{"msgtype":"m.text","body":"hello from the smoke test"}' >/dev/null
msgs="$(curl -fsS "$BASE/_matrix/client/v3/rooms/$chan_id/messages?dir=b&limit=20" \
    -H "Authorization: Bearer $alice_token")"
printf '%s' "$msgs" | grep -q "hello from the smoke test" || {
    echo "FAIL: message did not round-trip" >&2
    exit 1
}
echo "message round-trip ok"

echo "== non-member is refused =="
code=$(curl -s -o /dev/null -w '%{http_code}' "$BASE/_nuck/v1/guilds/$guild_id" \
    -H "Authorization: Bearer $bob_token")
[ "$code" = "403" ] || { echo "FAIL: non-member guild read returned $code" >&2; exit 1; }
echo "non-member refused (403)"

echo "== invite + redeem joins the second user =="
invite_json="$(curl -fsS -X POST "$BASE/_nuck/v1/guilds/$guild_id/invites" \
    -H "Authorization: Bearer $alice_token" -H 'content-type: application/json' -d '{}')"
code="$(printf '%s' "$invite_json" | jget code)"
preview="$(curl -fsS "$BASE/_nuck/v1/invites/$code")"
printf '%s' "$preview" | grep -q "$guild_id" || { echo "FAIL: invite preview" >&2; exit 1; }
curl -fsS -X POST "$BASE/_nuck/v1/invites/$code/redeem" -H "Authorization: Bearer $bob_token" >/dev/null
for _ in $(seq 1 40); do
    code=$(curl -s -o /dev/null -w '%{http_code}' "$BASE/_nuck/v1/guilds/$guild_id" \
        -H "Authorization: Bearer $bob_token")
    [ "$code" = "200" ] && break
    sleep 0.5
done
[ "$code" = "200" ] || { echo "FAIL: redeemed user cannot read the guild ($code)" >&2; exit 1; }
echo "invite redeemed; $bob_id is now a member"

echo "== moderation: role + ban/unban =="
role_json="$(curl -fsS -X POST "$BASE/_nuck/v1/guilds/$guild_id/roles" \
    -H "Authorization: Bearer $alice_token" -H 'content-type: application/json' \
    -d '{"name":"Mod","position":10,"permissions":0}')"
role_id="$(printf '%s' "$role_json" | jget role_id)"
[ -n "$role_id" ] || { echo "FAIL: no role id" >&2; exit 1; }
curl -fsS -X PUT "$BASE/_nuck/v1/guilds/$guild_id/bans/$bob_id" \
    -H "Authorization: Bearer $alice_token" -H 'content-type: application/json' \
    -d '{"reason":"smoke"}' >/dev/null
bans="$(curl -fsS "$BASE/_nuck/v1/guilds/$guild_id/bans" -H "Authorization: Bearer $alice_token")"
printf '%s' "$bans" | grep -q "$bob_id" || { echo "FAIL: ban not listed" >&2; exit 1; }
curl -fsS -X DELETE "$BASE/_nuck/v1/guilds/$guild_id/bans/$bob_id" \
    -H "Authorization: Bearer $alice_token" >/dev/null
echo "role $role_id created; ban + unban ok"

echo "== DM =="
dm_json="$(curl -fsS -X POST "$BASE/_nuck/v1/dm" \
    -H "Authorization: Bearer $alice_token" -H 'content-type: application/json' \
    -d "{\"user_id\":\"$bob_id\"}")"
printf '%s' "$dm_json" | grep -q room_id || { echo "FAIL: no DM room" >&2; exit 1; }
echo "dm room $(printf '%s' "$dm_json" | jget room_id)"

echo "SMOKE PASS"
