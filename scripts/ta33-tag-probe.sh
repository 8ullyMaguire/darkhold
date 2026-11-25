#!/usr/bin/env bash
# TA3-3: prove one live AO3 work has source-backed tags after the refetch pass.
set -u
cd /personal/documents/code/rust/darkhold
q=$(python3 -c 'import urllib.parse; print(urllib.parse.quote("source:ao3"))')
id=$(curl -s --max-time 30 "localhost:8081/api/v1/search?q=$q&limit=1" | jq -r '.items[0].work_id')
echo "WORK=$id"
sudo -u postgres psql -tA -d darkhold -c "SELECT count(*) FROM work_tags WHERE work_id = '$id'"
