#!/usr/bin/env bash
# Live probe for M69-01: the taste fingerprint over a real socket.
set -euo pipefail
DIR="$(mktemp -d)"; PORT="${PORT:-8096}"; BASE="http://127.0.0.1:${PORT}"
BIN="${BIN:-/home/alvaro/.cargo-target-darkhold-dev/debug/darkhold}"; DB="$DIR/probe.sqlite"; SERVER=""
cleanup() { [ -n "${SERVER:-}" ] && kill "$SERVER" 2>/dev/null || true; wait "${SERVER:-}" 2>/dev/null || true; rm -rf "$DIR"; }
trap cleanup EXIT
export DARKHOLD_DATABASE_URL="sqlite://$DB?mode=rwc" DARKHOLD_STORAGE_ROOT="$DIR/storage" DARKHOLD_ENV=development \
       DARKHOLD_BASE_URL="$BASE" DARKHOLD_LOG=warn
mkdir -p "$DIR/storage"; "$BIN" migrate >/dev/null 2>&1
"$BIN" serve --port "$PORT" >"$DIR/server.log" 2>&1 & SERVER=$!
for _ in $(seq 1 80); do curl -sf "$BASE/health" >/dev/null 2>&1 && break; sleep 0.25; done

echo "--- no theme yet: not offered, not an empty document ---"
echo "status: $(curl -sS -o "$DIR/a.json" -w '%{http_code}' "$BASE/.well-known/taste-fingerprint.json") $(python3 -c 'import json;print(json.load(open("'"$DIR"'/a.json")).get("error",{}).get("message",""))')"

echo "--- a theme the instance has NOT published ---"
sqlite3 "$DB" "INSERT INTO instance_themes (instance_id, theme_vector, public, computed_at, updated_at)
  VALUES ('$BASE', '{\"slow burn\": 3, \"hurt/comfort\": 5, \"angst\": 2}', 0, '2026-11-01T00:00:00Z', '2026-11-01T00:00:00Z');"
echo "status: $(curl -sS -o /dev/null -w '%{http_code}' "$BASE/.well-known/taste-fingerprint.json")"

echo "--- published ---"
sqlite3 "$DB" "UPDATE instance_themes SET public = 1 WHERE instance_id = '$BASE';"
curl -sS -D "$DIR/headers" "$BASE/.well-known/taste-fingerprint.json" | tee "$DIR/b.json" | python3 -c '
import json,sys
d=json.load(sys.stdin)
print("version:", d["version"], "instance:", d["instance_id"], "generated_at:", d["generated_at"])
print("tags:", [(t["tag"], t["weight"]) for t in d["tags"]])
print("sum:", round(sum(t["weight"] for t in d["tags"]), 6))'
grep -i "^content-type" "$DIR/headers" | head -1

echo "--- same bytes on a second request (deterministic) ---"
curl -sS "$BASE/.well-known/taste-fingerprint.json" > "$DIR/c.json"
cmp -s "$DIR/b.json" "$DIR/c.json" && echo "identical" || echo "DIFFERS"
