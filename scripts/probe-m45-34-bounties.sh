#!/usr/bin/env bash
# Live probe for M45-34: fic-finder bounties over a real socket.
#
# The credits are the feature, so this checks the money not just the JSON: a bounty reserves its budget, the
# finder is paid exactly once, an answer from the asker is refused, and cancelling returns the credits.
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

register() {
  curl -sS -c "$DIR/$1.jar" -H 'content-type: application/json' \
    -d "{\"email\":\"$1@example.com\",\"password\":\"a-long-enough-passphrase\",\"handle\":\"$1\",\"display_name\":\"$1\",\"age_band\":\"adult\"}" \
    "$BASE/api/v1/auth/register" >/dev/null
  curl -sS -b "$DIR/$1.jar" "$BASE/api/v1/auth/me" | python3 -c 'import json,sys;d=json.load(sys.stdin);print(d.get("active_pseud_id") or "", d.get("account_id") or d.get("account",{}).get("id") or "")'
}
csrf() { awk '/darkhold_csrf/ {print $7}' "$DIR/$1.jar" | tail -1; }
avail() { curl -sS -b "$DIR/$1.jar" "$BASE/api/v1/finder/me" | python3 -c 'import json,sys;d=json.load(sys.stdin);print(d.get("available_credits"),"credits,",len(d.get("open_bounties_i_asked",[])),"open")'; }

read -r ASKER_PSEUD ASKER_ACCT <<<"$(register asker)"
read -r FINDER_PSEUD FINDER_ACCT <<<"$(register finder)"
echo "asker=${ASKER_ACCT:0:8} finder=${FINDER_ACCT:0:8}"
[ -n "$ASKER_ACCT" ] || { echo "no account id from /auth/me — the probe cannot seed credits"; exit 1; }

# 40 credits, through the same tables the economy writes.
sqlite3 "$DB" "
  INSERT INTO credit_transactions (id, type, idempotency_key, reference, created_at)
  VALUES ('probe-txn','grant','probe-grant:$ASKER_ACCT','probe','2026-01-01T00:00:00Z');
  INSERT INTO credit_entries (transaction_id, account, bucket, amount_bp, created_at)
  VALUES ('probe-txn','$ASKER_ACCT','earned',40,'2026-01-01T00:00:00Z');"
WORK=$(python3 -c 'import uuid;print(uuid.uuid4())')
sqlite3 "$DB" "PRAGMA foreign_keys=ON;
  INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at)
  VALUES ('$WORK','$ASKER_PSEUD','The remembered fic','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
  INSERT INTO work_contributors (work_id, pseud_id, role, public_attribution, created_at)
  VALUES ('$WORK','$ASKER_PSEUD','owner',1,'2026-01-01T00:00:00Z');"

CLOSES=$(python3 -c 'import datetime;print((datetime.datetime.now(datetime.UTC)+datetime.timedelta(days=7)).strftime("%Y-%m-%dT%H:%M:%SZ"))')
DESC='Naruto fic read around 2019: Sakura learns a seal, there is a broken tanto in chapter 3, and the ending is on a rooftop in the rain.'

post() { # handle, path, json
  curl -sS -o "$DIR/out.json" -w '%{http_code}' -b "$DIR/$1.jar" -c "$DIR/$1.jar" -X POST \
    -H 'content-type: application/json' -H "x-csrf-token: $(csrf "$1")" -d "$3" "$BASE/api/v1$2"
}
err() { python3 -c 'import json;print(json.load(open("'"$DIR"'/out.json"))["error"]["message"])'; }

echo
echo "--- the asker has 40 credits ---"
echo "before: $(avail asker)"

echo
echo "--- open a 25-credit bounty ---"
echo "status: $(post asker /finder/bounties "{\"description\":\"$DESC\",\"fandom\":\"Naruto\",\"budget\":25,\"closes_at\":\"$CLOSES\"}")"
BOUNTY=$(python3 -c 'import json;print(json.load(open("'"$DIR"'/out.json"))["bounty"]["id"])')
python3 -c 'import json;b=json.load(open("'"$DIR"'/out.json"))["bounty"];print("  state:",b["state"],"budget:",b["budget"],"fandom:",b["fandom"])'
echo "after reserving: $(avail asker)   ← the 25 is held, not spent"

echo
echo "--- the public list, no session ---"
curl -sS "$BASE/api/v1/finder/bounties?fandom=naruto" | python3 -c 'import json,sys;d=json.load(sys.stdin);print("count:",d["count"],"budgets:",[b["budget"] for b in d["items"]])'

echo
echo "--- too short a description is refused before any credits move ---"
echo "status: $(post asker /finder/bounties "{\"description\":\"a fic pls\",\"budget\":5,\"closes_at\":\"$CLOSES\"}") $(err)"

echo
echo "--- the finder answers; then answers the same work again ---"
echo "answer:   $(post finder /finder/bounties/$BOUNTY/proposals "{\"work_id\":\"$WORK\",\"note\":\"I think it is this one\"}")"
PID=$(python3 -c 'import json;print(json.load(open("'"$DIR"'/out.json"))["proposal"]["id"])')
echo "repeat:   $(post finder /finder/bounties/$BOUNTY/proposals "{\"work_id\":\"$WORK\"}") $(err)"

echo
echo "--- the asker cannot answer their own bounty ---"
echo "status: $(post asker /finder/bounties/$BOUNTY/proposals "{\"work_id\":\"$WORK\"}") $(err)"

echo
echo "--- accept: the finder is paid, once ---"
echo "status: $(post asker /finder/bounties/$BOUNTY/accept "{\"proposal_id\":\"$PID\"}")"
python3 -c 'import json;d=json.load(open("'"$DIR"'/out.json"));print("  paid:",d["paid"],"state:",d["bounty"]["state"],"txn:",d["transaction_id"][:8])'
echo "asker:  $(avail asker)   ← 15 left, nothing still held"
echo "finder: $(avail finder)  ← the 25 arrived"
echo "second accept: $(post asker /finder/bounties/$BOUNTY/accept "{\"proposal_id\":\"$PID\"}") $(err)"
echo "finder still:  $(avail finder)"

echo
echo "--- a bounty that is cancelled returns its credits ---"
CLOSES2=$(python3 -c 'import datetime;print((datetime.datetime.now(datetime.UTC)+datetime.timedelta(days=2)).strftime("%Y-%m-%dT%H:%M:%SZ"))')
post asker /finder/bounties "{\"description\":\"$DESC\",\"budget\":10,\"closes_at\":\"$CLOSES2\"}" >/dev/null
SECOND=$(python3 -c 'import json;print(json.load(open("'"$DIR"'/out.json"))["bounty"]["id"])')
echo "reserved: $(avail asker)"
echo "cancel:   $(post asker /finder/bounties/$SECOND/cancel '{}')"
echo "released: $(avail asker)"

echo
echo "--- an unfunded bounty is refused ---"
echo "status: $(post asker /finder/bounties "{\"description\":\"$DESC\",\"budget\":100,\"closes_at\":\"$CLOSES\"}") $(err)"

echo
echo "--- anonymous ---"
echo "list:  $(curl -sS -o /dev/null -w '%{http_code}' "$BASE/api/v1/finder/bounties")"
echo "post:  $(curl -sS -o /dev/null -w '%{http_code}' -X POST -H 'content-type: application/json' -d '{}' "$BASE/api/v1/finder/bounties")"
echo "mine:  $(curl -sS -o /dev/null -w '%{http_code}' "$BASE/api/v1/finder/me")"
