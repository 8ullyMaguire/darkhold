#!/usr/bin/env bash
# Live probe for M45-33: specialist matchmaking over a real socket.
#
# The router tests cover the handlers; this covers what only a socket shows — that the ten routes exist in the
# mounted tree, that a session is enough (no extra scope), and that the matching list, the accept race and the
# 404-on-someone-else's-work all behave with real cookies and real JSON.
#
# Works are seeded through sqlite3 because there is no import fixture here: the probe is about the specialist
# routes, not the authoring flow. The pseud ids come from the registration response.
set -euo pipefail

DIR="$(mktemp -d)"
PORT="${PORT:-8096}"
BASE="http://127.0.0.1:${PORT}"
BIN="${BIN:-/home/alvaro/.cargo-target-darkhold-dev/debug/darkhold}"
DB="$DIR/probe.sqlite"
SERVER=""
cleanup() { [ -n "${SERVER:-}" ] && kill "$SERVER" 2>/dev/null || true; wait "${SERVER:-}" 2>/dev/null || true; rm -rf "$DIR"; }
trap cleanup EXIT

export DARKHOLD_DATABASE_URL="sqlite://$DB?mode=rwc"
export DARKHOLD_STORAGE_ROOT="$DIR/storage"
export DARKHOLD_ENV=development
export DARKHOLD_BASE_URL="$BASE"
export DARKHOLD_LOG=warn
mkdir -p "$DIR/storage"
"$BIN" migrate >/dev/null 2>&1

"$BIN" serve --port "$PORT" >"$DIR/server.log" 2>&1 &
SERVER=$!
for _ in $(seq 1 80); do curl -sf "$BASE/health" >/dev/null 2>&1 && break; sleep 0.25; done

register() { # handle -> prints "<pseud_id>"
  local jar="$DIR/$1.jar"
  curl -sS -c "$jar" -H 'content-type: application/json' \
    -d "{\"email\":\"$1@example.com\",\"password\":\"a-long-enough-passphrase\",\"handle\":\"$1\",\"display_name\":\"$1\",\"age_band\":\"adult\"}" \
    "$BASE/api/v1/auth/register" >/dev/null
  # The registration response names the account, not the pseud; `/me` is where the acting pseud lives.
  curl -sS -b "$jar" "$BASE/api/v1/auth/me" | python3 -c 'import json,sys;d=json.load(sys.stdin);print(d.get("active_pseud_id") or "")'
}
csrf() { awk '/darkhold_csrf/ {print $7}' "$DIR/$1.jar" | tail -1; }

WRITER_PSEUD=$(register writer)
BETA1=$(register betaone)
BETA2=$(register betatwo)
echo "pseuds: writer=$WRITER_PSEUD beta1=$BETA1 beta2=$BETA2"

# A work the writer owns, with the contributor row the request route checks.
WORK=$(python3 -c 'import uuid;print(uuid.uuid4())')
sqlite3 "$DB" "PRAGMA foreign_keys=ON;
  INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at)
  VALUES ('$WORK','$WRITER_PSEUD','A probe work','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
  INSERT INTO work_contributors (work_id, pseud_id, role, public_attribution, created_at)
  VALUES ('$WORK','$WRITER_PSEUD','owner',1,'2026-01-01T00:00:00Z');"

put_profile() { # handle, json
  curl -sS -o /dev/null -w '%{http_code}' -b "$DIR/$1.jar" -c "$DIR/$1.jar" \
    -X PUT -H 'content-type: application/json' -H "x-csrf-token: $(csrf "$1")" \
    -d "$2" "$BASE/api/v1/specialists/me"
}

echo
echo "--- profiles: one names the fandom, one offers any ---"
echo "betaone: $(put_profile betaone '{"skills":["beta","britpick"],"fandoms":["Naruto"],"note":"British English"}')"
echo "betatwo: $(put_profile betatwo '{"skills":["beta"],"fandoms":[],"capacity":"open"}')"
echo "refused (no skills): $(put_profile betatwo '{"skills":[],"fandoms":[]}')"
echo "refused (invented skill): $(put_profile betatwo '{"skills":["betamax"]}')"

echo
echo "--- request on the work: matching list travels with it ---"
curl -sS -b "$DIR/writer.jar" -c "$DIR/writer.jar" -X POST \
  -H 'content-type: application/json' -H "x-csrf-token: $(csrf writer)" \
  -d '{"skill":"beta","fandom":"naruto","brief":"line edits please"}' \
  "$BASE/api/v1/works/$WORK/specialist-requests" | tee "$DIR/created.json" | python3 -c '
import json,sys
d=json.load(sys.stdin)
r=d["request"]; m=d["matches"]
print("request:", r["state"], r["skill"], "fandom=", r["fandom"])
print("summary:", m["summary"])
print("matches:", [(i["pseud_id"][:8], i["fit"], i["live_engagements"]) for i in m["items"]])'
REQ=$(python3 -c 'import json;print(json.load(open("'"$DIR"'/created.json"))["request"]["id"])')

echo
echo "--- a stranger cannot open a request on someone else's work ---"
echo "betatwo asks on writer's work: $(curl -sS -o /dev/null -w '%{http_code}' -b "$DIR/betatwo.jar" -c "$DIR/betatwo.jar" -X POST -H 'content-type: application/json' -H "x-csrf-token: $(csrf betatwo)" -d '{"skill":"beta"}' "$BASE/api/v1/works/$WORK/specialist-requests")"

echo
echo "--- browse, then the accept race ---"
curl -sS -b "$DIR/betaone.jar" "$BASE/api/v1/specialist-requests?skill=beta" | python3 -c 'import json,sys;d=json.load(sys.stdin);print("open for beta:", [(r["id"][:8], r["fandom"]) for r in d["items"]])'
echo "betaone accepts: $(curl -sS -o "$DIR/a1.json" -w '%{http_code}' -b "$DIR/betaone.jar" -c "$DIR/betaone.jar" -X POST -H "x-csrf-token: $(csrf betaone)" "$BASE/api/v1/specialist-requests/$REQ/accept")"
echo "betatwo accepts: $(curl -sS -o "$DIR/a2.json" -w '%{http_code}' -b "$DIR/betatwo.jar" -c "$DIR/betatwo.jar" -X POST -H "x-csrf-token: $(csrf betatwo)" "$BASE/api/v1/specialist-requests/$REQ/accept")"
python3 -c 'import json;d=json.load(open("'"$DIR"'/a2.json"));print("  loser says:", d.get("error",{}).get("message", d))'

echo
echo "--- complete, and a closed profile disappears from the candidate list ---"
echo "complete: $(curl -sS -o /dev/null -w '%{http_code}' -b "$DIR/betaone.jar" -c "$DIR/betaone.jar" -X POST -H "x-csrf-token: $(csrf betaone)" "$BASE/api/v1/specialist-requests/$REQ/complete")"
echo "betaone closes: $(put_profile betaone '{"skills":["beta","britpick"],"fandoms":["Naruto"],"capacity":"closed"}')"
curl -sS -b "$DIR/writer.jar" -X POST -H 'content-type: application/json' -H "x-csrf-token: $(csrf writer)" \
  -d '{"skill":"beta","fandom":"naruto"}' "$BASE/api/v1/works/$WORK/specialist-requests" | python3 -c '
import json,sys
d=json.load(sys.stdin)
print("after closing:", d["matches"]["summary"], [(i["pseud_id"][:8], i["fit"]) for i in d["matches"]["items"]])'

echo
echo "--- no session ---"
echo "anonymous profile read: $(curl -sS -o /dev/null -w '%{http_code}' "$BASE/api/v1/specialists/me")"
