#!/usr/bin/env bash
# End-to-end smoke test for the deployable MVP (docs/mvp.md).
#
# Exercises the whole path through nuck-server: register/login via the proxied C-S
# API, create a guild and channels, send and read a message (permission-checked), and
# verify a non-member is refused.
set -euo pipefail

BASE="${NUCK_URL:-http://localhost:8008}"
STAMP="$(date +%s)"

jget() {
    python3 -c 'import sys,json;d=json.load(sys.stdin);print(d[sys.argv[1]])' "$1" 2>/dev/null
}

register() {
    local user="$1" pass="$2" first session
    first="$(curl -sS -X POST "$BASE/_matrix/client/v3/register" \
        -H 'content-type: application/json' \
        -d "{\"username\":\"$user\",\"password\":\"$pass\"}")"
    session="$(printf '%s' "$first" | jget session || true)"
    if [ -z "$session" ]; then
        # Already registered — log in instead.
        curl -fsS -X POST "$BASE/_matrix/client/v3/login" \
            -H 'content-type: application/json' \
            -d "{\"type\":\"m.login.password\",\"identifier\":{\"type\":\"m.id.user\",\"user\":\"$user\"},\"password\":\"$pass\"}"
        return
    fi
    curl -fsS -X POST "$BASE/_matrix/client/v3/register" \
        -H 'content-type: application/json' \
        -d "{\"username\":\"$user\",\"password\":\"$pass\",\"auth\":{\"type\":\"m.login.dummy\",\"session\":\"$session\"}}"
}

echo "== register =="
alice_json="$(register "alice$STAMP" "pw-alice-$STAMP")"
alice_token="$(printf '%s' "$alice_json" | jget access_token)"
alice_id="$(printf '%s' "$alice_json" | jget user_id)"
echo "registered $alice_id"

echo "== create guild =="
guild_json="$(curl -fsS -X POST "$BASE/_nuck/v1/guilds" \
    -H "Authorization: Bearer $alice_token" -H 'content-type: application/json' \
    -d '{"name":"Smoke Guild"}')"
guild_id="$(printf '%s' "$guild_json" | jget guild_id)"
channel_id="$(printf '%s' "$guild_json" | jget channel_id)"
echo "guild $guild_id, default channel $channel_id"

echo "== wait for the projection and read the guild =="
view=""
for _ in $(seq 1 40); do
    view="$(curl -sS "$BASE/_nuck/v1/guilds/$guild_id" -H "Authorization: Bearer $alice_token")"
    if printf '%s' "$view" | grep -q "$channel_id"; then break; fi
    sleep 0.5
done
if ! printf '%s' "$view" | grep -q "$channel_id"; then
    echo "FAIL: guild view never included the default channel" >&2
    echo "$view" >&2
    exit 1
fi
echo "guild view ok"

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
if ! printf '%s' "$msgs" | grep -q "hello from the smoke test"; then
    echo "FAIL: message did not round-trip" >&2
    printf '%s' "$msgs" | head -c 600 >&2
    exit 1
fi
echo "message round-trip ok"

echo "== permission checks =="
bob_json="$(register "bob$STAMP" "pw-bob-$STAMP")"
bob_token="$(printf '%s' "$bob_json" | jget access_token)"

code=$(curl -s -o /dev/null -w '%{http_code}' "$BASE/_nuck/v1/guilds/$guild_id" \
    -H "Authorization: Bearer $bob_token")
[ "$code" = "403" ] || { echo "FAIL: non-member guild read returned $code, expected 403" >&2; exit 1; }

code=$(curl -s -o /dev/null -w '%{http_code}' -X PUT \
    "$BASE/_matrix/client/v3/rooms/$chan_id/send/m.room.message/bob-$STAMP" \
    -H "Authorization: Bearer $bob_token" -H 'content-type: application/json' \
    -d '{"msgtype":"m.text","body":"intruder"}')
[ "$code" = "403" ] || { echo "FAIL: non-member send returned $code, expected 403" >&2; exit 1; }
echo "non-member guild read and send both refused (403)"

echo "SMOKE PASS"
