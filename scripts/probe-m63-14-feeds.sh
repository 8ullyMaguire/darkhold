#!/usr/bin/env bash
# Live probe for M63-14: a saved view fetched as a feed by a client with no cookies.
#
# What this proves that the router test cannot: the document leaves over a real socket, with the headers a
# feed reader keys on (`Content-Type`, `ETag`, `Cache-Control`), and `If-None-Match` answers `304` with an
# empty body on the wire.
#
# Scratch DB and scratch storage root; port from $PORT so it cannot collide with a running instance.
set -euo pipefail

DIR="$(mktemp -d)"
PORT="${PORT:-8096}"
BASE="http://127.0.0.1:${PORT}"
BIN="${BIN:-/home/alvaro/.cargo-target-darkhold-dev/debug/darkhold}"

SERVER=""; cleanup() { [ -n "${SERVER:-}" ] && kill "$SERVER" 2>/dev/null || true; wait "$SERVER" 2>/dev/null || true; rm -rf "$DIR"; }
trap cleanup EXIT

export DARKHOLD_DATABASE_URL="sqlite://$DIR/probe.sqlite?mode=rwc"
export DARKHOLD_STORAGE_ROOT="$DIR/storage"
export DARKHOLD_ENV=development
export DARKHOLD_BASE_URL="$BASE"
export DARKHOLD_LOG=warn
mkdir -p "$DIR/storage"

"$BIN" migrate >/dev/null 2>&1
"$BIN" serve --port "$PORT" >"$DIR/server.log" 2>&1 &
SERVER=$!
for _ in $(seq 1 60); do
  curl -sf "$BASE/health" >/dev/null 2>&1 && break
  sleep 0.25
done

JAR="$DIR/cookies.txt"
curl -sS -c "$JAR" -H 'content-type: application/json' \
  -d '{"email":"probe@example.com","password":"a-long-enough-passphrase","handle":"probe","display_name":"Probe","age_band":"adult"}' \
  "$BASE/api/v1/auth/register" >/dev/null
CSRF=$(awk '/darkhold_csrf/ {print $7}' "$JAR" | tail -1)

VIEW=$(curl -sS -b "$JAR" -c "$JAR" -H 'content-type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{"name":"Grimdark & long","kind":"catalogue","params":"sort=kudos&limit=2","scope":"public"}' \
  "$BASE/api/v1/saved-views" | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
echo "saved view: $VIEW"

FEED="$BASE/api/v1/saved-views/$VIEW/feed"
echo
echo "--- GET $FEED (atom) ---"
curl -sS -D "$DIR/h1" -o "$DIR/b1" -b "$JAR" "$FEED"
grep -iE '^(HTTP/|content-type|etag|cache-control)' "$DIR/h1" | tr -d '\r'
head -c 300 "$DIR/b1"; echo

TAG=$(grep -i '^etag:' "$DIR/h1" | sed 's/^[Ee][Tt][Aa][Gg]: //' | tr -d '\r')
echo
echo "--- GET again with If-None-Match: $TAG ---"
curl -sS -D "$DIR/h2" -o "$DIR/b2" -b "$JAR" -H "if-none-match: $TAG" "$FEED"
grep -iE '^(HTTP/|etag)' "$DIR/h2" | tr -d '\r'
echo "body bytes: $(wc -c < "$DIR/b2")"

echo
echo "--- json feed, no cookie, no header (what a feed reader sends) ---"
curl -sS -o /dev/null -w 'status=%{http_code}\n' "$FEED?format=json"
echo "--- json feed with ?token= nonsense ---"
curl -sS -o /dev/null -w 'status=%{http_code}\n' "$FEED?format=json&token=nonsense"

echo
echo "--- rss feed over the socket, content type and first line ---"
curl -sS -D "$DIR/h3" -o "$DIR/b3" -b "$JAR" "$FEED?format=rss"
grep -iE '^(content-type|etag)' "$DIR/h3" | tr -d '\r'
head -2 "$DIR/b3"

echo
echo "--- unknown format ---"
curl -sS -b "$JAR" -w '\nstatus=%{http_code}\n' "$FEED?format=yaml" | tail -3
