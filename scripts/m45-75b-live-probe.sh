#!/usr/bin/env bash
# M45-75 follow-up live proof: 0220 applied, hit carries source facts, CLI prints them,
# and the planner uses the new composite index.
set -u
cd /personal/documents/code/rust/darkhold
DH=~/.cargo-target/darkhold/release/darkhold
DH_CLI=~/.cargo-target/darkhold/release/dh

"$DH" migrate >/tmp/m4575b-migrate.log 2>&1
echo "MIGRATE_EXIT=$?"
systemctl --user restart darkhold.service
sleep 2

echo "HEALTH=$(curl -s -o /dev/null -w '%{http_code}' --max-time 8 localhost:8081/healthz)"
echo "META=$(curl -s --max-time 8 localhost:8081/api/v1/meta | jq -r .build)"
echo "LEDGER=$(sudo -u postgres psql -tA -d darkhold -c 'SELECT count(*) || chr(124) || max(version) FROM _migrations')"
echo "INDEX=$(sudo -u postgres psql -tA -d darkhold -c \"SELECT count(*) FROM pg_indexes WHERE indexname = 'idx_library_items_source_work'\")"

enc() { python3 -c 'import urllib.parse,sys; print(urllib.parse.quote(sys.argv[1]))' "$1"; }
q=$(enc 'source_updated:<=-4m')
echo "HIT=$(curl -s --max-time 20 "localhost:8081/api/v1/search?q=$q&sort=popularity&limit=3" | jq -c '.items[0] | {title,source_key,source_updated_at,likes}')"

q1=$(enc 'source:qq OR source:sv')
echo "PLAN_INDEX=$(curl -s --max-time 25 "localhost:8081/api/v1/search?q=$q1&explain=1" | jq -r '.explain.plan[]' | grep -c idx_library_items_source_work)"

echo "CLI_FIELDS=$("$DH_CLI" q 'source_updated:<=-4m' --sort popularity --limit 2 --jsonl --fields title,source,updated 2>&1 | head -2)"
