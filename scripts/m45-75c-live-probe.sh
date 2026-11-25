#!/usr/bin/env bash
# M45-75 NULL source-clock regression: live proof after the COALESCE fix.
set -u
cd /personal/documents/code/rust/darkhold
systemctl --user restart darkhold.service
sleep 2
echo "HEALTH=$(curl -s -o /dev/null -w '%{http_code}' --max-time 8 localhost:8081/healthz)"
echo "META=$(curl -s --max-time 8 localhost:8081/api/v1/meta | jq -r .build)"

enc() { python3 -c 'import urllib.parse,sys; print(urllib.parse.quote(sys.argv[1]))' "$1"; }
q1=$(enc 'source:ao3')
echo "AO3=$(curl -s --max-time 40 "localhost:8081/api/v1/search?q=$q1&limit=5" | jq -c '{count:(.items|length),error:.error.message,first:(.items[0]|{source_key,source_updated_at})}')"
q2=$(enc 'source:ao3 OR source:pawchive')
echo "BOTH=$(curl -s --max-time 40 "localhost:8081/api/v1/search?q=$q2&limit=50" | jq -c '{count:(.items|length),sources:([.items[].source_key]|unique)}')"
q3=$(enc '(source:ao3 OR source:pawchive) AND source_updated:<=-4m')
echo "STALE_CROSS=$(curl -s --max-time 40 "localhost:8081/api/v1/search?q=$q3&sort=popularity&limit=5" | jq -c '{count:(.items|length),sources:([.items[].source_key]|unique)}')"
