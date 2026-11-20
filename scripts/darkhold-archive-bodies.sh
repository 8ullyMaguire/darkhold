#!/usr/bin/env bash
# TA2-6: enqueue body imports for private taste matches from stored metadata.
set -euo pipefail

cd /personal/documents/code/rust/darkhold
BIN=/personal/documents/code/rust/darkhold/target/release/dh
DB_URL='postgresql:///darkhold?host=/run/postgresql'
STATE_DIR=/home/alvaro/.local/state/darkhold
mkdir -p "$STATE_DIR"

"$BIN" --database-url "$DB_URL" archive rank --limit 100000 > "$STATE_DIR/archive-rank.latest"
"$BIN" --database-url "$DB_URL" archive bodies --yes
date -u +%FT%TZ > "$STATE_DIR/archive-bodies.latest"
