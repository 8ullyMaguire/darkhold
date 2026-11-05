#!/usr/bin/env bash
# M70-01 — orphaning, live against a running server.
#
# Proves four things a reader of the code cannot: the route is mounted where it says it is, the session cookie is
# what authorises it, a stranger's refusal is indistinguishable from a missing work, and the work survives the
# orphaning as a row with a frozen author line.
#
# Usage: scripts/probe-m70-01-orphan.sh [base-url] [sqlite-path]
set -euo pipefail
BASE="${1:-http://127.0.0.1:8098}"
DBPATH="${2:-}"

say() { printf '%-28s %s\n' "$1" "$2"; }

AUTHOR_COOKIE=$(mktemp); STRANGER_COOKIE=$(mktemp)
trap 'rm -f "$AUTHOR_COOKIE" "$STRANGER_COOKIE"' EXIT

register() { # email handle cookiejar
  curl -sS -o /dev/null -w '%{http_code}' -c "$3" \
    -H 'content-type: application/json' \
    -d "{\"email\":\"$1\",\"password\":\"probe-pass-1234\",\"handle\":\"$2\",\"display_name\":\"$2\",\"age_band\":\"adult\"}" \
    "$BASE/api/v1/auth/register"
}

say "register author" "$(register "author-$$@probe.invalid" "hollowgast$$" "$AUTHOR_COOKIE")"
say "register stranger" "$(register "stranger-$$@probe.invalid" "bystander$$" "$STRANGER_COOKIE")"

ACCOUNT=$(curl -sS -b "$AUTHOR_COOKIE" "$BASE/api/v1/auth/me" | python3 -c 'import json,sys; print(json.load(sys.stdin)["account"]["id"])')
say "author account" "$ACCOUNT"

# Seed a published work through the database, because authoring one over HTTP needs the whole posting form and this
# probe is about the orphan door, not the posting one.
WORK=$(python3 - "$DBPATH" "$ACCOUNT" <<'PY'
import sqlite3, sys, uuid
path, account = sys.argv[1], sys.argv[2]
db = sqlite3.connect(path)
pseud = db.execute("select id from pseuds where account_id = ?", (account,)).fetchone()[0]
work = str(uuid.uuid4())
db.execute(
    "insert into works (id, owner_pseud_id, title, summary, language, rating, visibility, lifecycle, completion,"
    " created_at, updated_at, version) values (?,?,'The Long Way Round','','en','general','public','published',"
    " 'complete','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)",
    (work, pseud),
)
db.commit()
print(work)
PY
)
say "seeded work" "$WORK"

echo
say "stranger, signed in" "$(curl -sS -o /tmp/m70-stranger.json -w '%{http_code}' -b "$STRANGER_COOKIE" -X POST -H 'content-type: application/json' -d '{}' "$BASE/api/v1/works/$WORK/orphan")"
say "  says" "$(python3 -c 'import json;print(json.load(open("/tmp/m70-stranger.json"))["error"]["message"])')"
say "anonymous" "$(curl -sS -o /dev/null -w '%{http_code}' -X POST -H 'content-type: application/json' -d '{}' "$BASE/api/v1/works/$WORK/orphan")"
say "owner" "$(curl -sS -o /tmp/m70-owner.json -w '%{http_code}' -b "$AUTHOR_COOKIE" -X POST -H 'content-type: application/json' -d '{}' "$BASE/api/v1/works/$WORK/orphan")"
say "  says" "$(cat /tmp/m70-owner.json)"
say "owner, twice" "$(curl -sS -o /tmp/m70-twice.json -w '%{http_code}' -b "$AUTHOR_COOKIE" -X POST -H 'content-type: application/json' -d '{}' "$BASE/api/v1/works/$WORK/orphan")"
say "  says" "$(python3 -c 'import json;print(json.load(open("/tmp/m70-twice.json"))["error"]["message"])')"
echo
say "work still present" "$(python3 - "$DBPATH" "$WORK" <<'PY'
import sqlite3, sys
db = sqlite3.connect(sys.argv[1])
row = db.execute(
    "select w.title, p.display_name, p.account_id from works w join pseuds p on p.id = w.owner_pseud_id where w.id = ?",
    (sys.argv[2],),
).fetchone()
print(f"title={row[0]!r} author={row[1]!r} owner={row[2]}")
PY
)"
