#!/usr/bin/env bash
# Public-only M45-77 Step 12 probe. Run after H3 deploys the trope doors.
#
# Usage:
#   scripts/trope-live-probe.sh [base-url] [group-slug] [member-tag ...]
#
# With no group, prints the vocabulary and its sources. With a group and its
# member tags, compares the trope search with the OR of the member tag searches.
# No auth header, cookie, token or private surface is used; if a caller had
# exported credentials, they are dropped before the first request.

set -euo pipefail
unset DARKHOLD_BASE_URL
unset DARKHOLD_TOKEN DARKHOLD_COOKIE

BASE="${1:-http://127.0.0.1:8081}"

echo "GET $BASE/api/v1/tropes"
curl -fsS "$BASE/api/v1/tropes" | python3 -c '
import json, sys
data = json.load(sys.stdin)
for item in sorted(data.get("items", []), key=lambda row: row.get("slug", "")):
    print("{}\t{}".format(item.get("slug"), item.get("source")))
'

if [[ $# -lt 2 ]]; then
    echo "No group given; vocabulary probe complete."
    exit 0
fi

GROUP="$2"
shift 2
if [[ $# -lt 1 ]]; then
    echo "A group needs at least one member tag for the parity check." >&2
    exit 2
fi

TROPE_QUERY="q=trope%3A${GROUP}&limit=20"
TAG_QUERY="q=$(printf 'tag%%3A%s' "$1")"
for tag in "${@:2}"; do
    TAG_QUERY="${TAG_QUERY}+OR+tag%3A${tag}"
done
TAG_QUERY="${TAG_QUERY}&limit=20"

trope_total=$(curl -fsS "$BASE/api/v1/search?$TROPE_QUERY" | python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("items", [])))')
tag_total=$(curl -fsS "$BASE/api/v1/search?$TAG_QUERY" | python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("items", [])))')

echo "trope:${GROUP} -> ${trope_total}"
echo "member tags -> ${tag_total}"
if [[ "$trope_total" != "$tag_total" ]]; then
    echo "FAIL: trope and member tags disagree" >&2
    exit 1
fi
echo "PASS: trope and member tags agree"
