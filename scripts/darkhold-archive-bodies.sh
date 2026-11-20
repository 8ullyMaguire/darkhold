#!/usr/bin/env bash
# TA2-6: enqueue body imports for private taste matches from stored metadata.
set -euo pipefail

cd /personal/documents/code/rust/darkhold
BIN=/personal/documents/code/rust/darkhold/target/release/dh
DB_URL='postgresql:///darkhold?host=/run/postgresql'
STATE_DIR=/home/alvaro/.local/state/darkhold
mkdir -p "$STATE_DIR"

"$BIN" archive --db-url "$DB_URL" --account 9aa50758-bf73-44d3-bf65-9fddb4135925 --pseud 999167a9-cc06-45d0-8bd2-12243a148a0c rank --limit 100000 > "$STATE_DIR/archive-rank.latest"
"$BIN" archive --db-url "$DB_URL" --account 9aa50758-bf73-44d3-bf65-9fddb4135925 --pseud 999167a9-cc06-45d0-8bd2-12243a148a0c bodies --yes
date -u +%FT%TZ > "$STATE_DIR/archive-bodies.latest"
