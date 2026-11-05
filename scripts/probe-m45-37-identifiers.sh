#!/usr/bin/env bash
# Live probe for M45-37: stable identifiers over a real socket.
#
# Proves what only a socket shows: the public reads answer without a session, a contributor can claim and drop,
# a second work claiming the same ISBN is told who holds it, an invalid checksum is refused by name, and the
# lookup route finds the work from a different spelling of the same identifier.
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
  curl -sS -b "$DIR/$1.jar" "$BASE/api/v1/auth/me" | python3 -c 'import json,sys;print(json.load(sys.stdin).get("active_pseud_id") or "")'
}
csrf() { awk '/darkhold_csrf/ {print $7}' "$DIR/$1.jar" | tail -1; }

WRITER=$(register writer)
OTHER=$(register other)
echo "pseuds: writer=${WRITER:0:8} other=${OTHER:0:8}"

mkwork() { # pseud -> id
  local work
  work=$(python3 -c 'import uuid;print(uuid.uuid4())')
  sqlite3 "$DB" "PRAGMA foreign_keys=ON;
    INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at)
    VALUES ('$work','$1','Probe work','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
    INSERT INTO work_contributors (work_id, pseud_id, role, public_attribution, created_at)
    VALUES ('$work','$1','owner',1,'2026-01-01T00:00:00Z');"
  echo "$work"
}
W1=$(mkwork "$WRITER")
W2=$(mkwork "$OTHER")
echo "works: ${W1:0:8} (writer) ${W2:0:8} (other)"

claim() { # handle, work, scheme, value
  curl -sS -o "$DIR/claim.json" -w '%{http_code}' -b "$DIR/$1.jar" -c "$DIR/$1.jar" -X PUT \
    -H 'content-type: application/json' -H "x-csrf-token: $(csrf "$1")" \
    -d "{\"value\":\"$4\"}" "$BASE/api/v1/works/$2/identifiers/$3"
}

echo
echo "--- claim an ISBN on the writer's work ---"
echo "status: $(claim writer "$W1" isbn '978-0-306-40615-7')"
python3 -c 'import json;d=json.load(open("'"$DIR"'/claim.json"));i=d["identifier"];print("stored:", i["scheme"], i["value"], "source=", i["source"])'

echo
echo "--- the public read, with no session at all ---"
curl -sS "$BASE/api/v1/works/$W1/identifiers" | python3 -c 'import json,sys;d=json.load(sys.stdin);print("claims:", [(i["scheme"], i["value"]) for i in d["items"]])'

echo
echo "--- lookup from a different spelling, no session ---"
curl -sS "$BASE/api/v1/identifiers/lookup?scheme=isbn&value=978%200%20306%2040615%207" | python3 -c 'import json,sys;d=json.load(sys.stdin);print("found:", d["found"], "work:", (d.get("identifier") or {}).get("work_id","")[:8])'

echo
echo "--- the second work claims it too: refused, and the refusal names the holder ---"
echo "status: $(claim other "$W2" isbn '9780306406157')"
python3 -c 'import json;d=json.load(open("'"$DIR"'/claim.json"));print(" ", d["error"]["message"])'

echo
echo "--- a bad checksum, and a bare TMDB number ---"
echo "isbn off-by-one: $(claim writer "$W1" isbn '9780306406158') $(python3 -c 'import json;print(json.load(open("'"$DIR"'/claim.json"))["error"]["message"])')"
echo "tmdb bare:      $(claim writer "$W1" tmdb '603') $(python3 -c 'import json;print(json.load(open("'"$DIR"'/claim.json"))["error"]["message"])')"

echo
echo "--- someone with no contributor row cannot claim ---"
echo "other claims on W1: $(claim other "$W1" wikidata 'Q42')"

echo
echo "--- withdraw an identifier the work does not hold, then the one it does ---"
echo "wrong scheme row: $(curl -sS -o /dev/null -w '%{http_code}' -b "$DIR/other.jar" -c "$DIR/other.jar" -X DELETE -H "x-csrf-token: $(csrf other)" "$BASE/api/v1/works/$W2/identifiers/isbn/9780306406157")"
echo "holder deletes:   $(curl -sS -o /dev/null -w '%{http_code}' -b "$DIR/writer.jar" -c "$DIR/writer.jar" -X DELETE -H "x-csrf-token: $(csrf writer)" "$BASE/api/v1/works/$W1/identifiers/isbn/9780306406157")"
curl -sS "$BASE/api/v1/works/$W1/identifiers" | python3 -c 'import json,sys;print("claims after delete:", json.load(sys.stdin)["items"])'

echo
echo "--- anonymous claim ---"
echo "status: $(curl -sS -o /dev/null -w '%{http_code}' -X PUT -H 'content-type: application/json' -d '{"value":"Q42"}' "$BASE/api/v1/works/$W1/identifiers/wikidata")"
