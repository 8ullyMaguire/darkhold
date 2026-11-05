#!/usr/bin/env bash
# Live probe for M45-32: the trend radar over a real socket, on an instance with seeded arrivals.
#
# The store test pins the window split; this proves the route's own arithmetic (the two RFC 3339 bounds it
# computes from `recent_days`/`baseline_days`), the JSON shape, and the ordering. Arrivals are seeded
# *relative to now*, because the route computes its bounds from the clock.
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

recent() { date -u -d "$1 days ago" +%Y-%m-%dT%H:%M:%SZ; }
OWNER=$(python3 -c 'import uuid;print(uuid.uuid4())')
sql() { sqlite3 "$DB" "PRAGMA foreign_keys=ON; $1"; }

sql "INSERT INTO accounts (id, email, created_at, updated_at) VALUES ('$OWNER','probe@x.test','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');"
sql "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) VALUES ('$OWNER','$OWNER','probe','Probe','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');"

# Two fandoms: one hot (6 arrivals in the last 10 days, 1 in the baseline), one flat (2 and 2).
seed() { # fandom, [days_ago...]
  local fandom="$1"; shift
  local node
  node=$(python3 -c 'import uuid;print(uuid.uuid4())')
  sql "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at, review_status, signal_count, node_state, node_count) VALUES ('$node','fandom','$fandom','$fandom','2026-01-01T00:00:00Z','curated',0,'browsable',10);"
  for days in "$@"; do
    local work
    work=$(python3 -c 'import uuid;print(uuid.uuid4())')
    sql "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES ('$work','$OWNER','$fandom work','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');"
    sql "INSERT INTO work_tags (work_id, node_id, prominence, source, role, added_at) VALUES ('$work','$node',10,'imported','primary','$(recent "$days")');"
  done
}
seed "surge-fandom" 3 4 5 6 8 9 40
seed "flat-fandom" 5 9 30 45

"$BIN" serve --port "$PORT" >"$DIR/server.log" 2>&1 &
SERVER=$!
for _ in $(seq 1 80); do
  curl -sf "$BASE/health" >/dev/null 2>&1 && break
  sleep 0.25
done

echo "--- GET /api/v1/catalogue/trends (default windows: 14 recent, 56 baseline) ---"
curl -sS -D "$DIR/h" -o "$DIR/b" "$BASE/api/v1/catalogue/trends"
grep -iE '^(HTTP/|cache-control)' "$DIR/h" | tr -d '\r'
python3 - "$DIR/b" <<'PY'
import json, sys
doc = json.load(open(sys.argv[1]))
print("window:", doc["recent_days"], "recent /", doc["baseline_days"], "baseline; floor", doc["min_recent"],
      "works and", doc["ratio_threshold"], "x")
for row in doc["items"]:
    print(f'  {row["fandom"]:14} recent={row["recent"]} baseline={row["baseline"]} '
          f'ratio={row["ratio"]} verdict={row["verdict"]}')
    print(f'    {row["reason"]}')
PY

echo
echo "--- a 3-day window: below the floor, so the list is empty (the store will not offer what the rule cannot judge) ---"
curl -sS "$BASE/api/v1/catalogue/trends?recent_days=3&baseline_days=7" | python3 -c 'import json,sys;d=json.load(sys.stdin);print([(r["fandom"], r["verdict"]) for r in d["items"]])'

echo
echo "--- a window the route refuses ---"
curl -sS -w '\nstatus=%{http_code}\n' "$BASE/api/v1/catalogue/trends?recent_days=0" | tail -2
