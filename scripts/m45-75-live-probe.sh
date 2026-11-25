#!/usr/bin/env bash
# M45-75 live acceptance: one signed-in query with source, source clock,
# unrated predicate and popularity sort; plus the three named refusals.
set -u
cd /personal/documents/code/rust/darkhold

cp ~/.cargo-target/darkhold/release/darkhold ~/backups/darkhold-m4575-new.bin
systemctl --user restart darkhold.service
sleep 2

echo "HEALTH=$(curl -s -o /dev/null -w '%{http_code}' --max-time 8 localhost:8081/healthz)"
echo "META=$(curl -s --max-time 8 localhost:8081/api/v1/meta | jq -r .build)"

enc() { python3 -c 'import urllib.parse,sys; print(urllib.parse.quote(sys.argv[1]))' "$1"; }

q1=$(enc 'source:qq OR source:sv')
echo "SOURCE_QUERY=$(curl -s --max-time 20 "localhost:8081/api/v1/search?q=$q1&limit=5" | jq -c '{count:(.items|length),first:(.items[0].title//null)}')"

q2=$(enc 'source_updated:<=-4m')
echo "STALE_QUERY=$(curl -s --max-time 20 "localhost:8081/api/v1/search?q=$q2&sort=popularity&limit=5" | jq -c '{count:(.items|length)}')"

echo "ANON_ME_RATED=$(curl -s -o /dev/null -w '%{http_code}' --max-time 15 'localhost:8081/api/v1/search?q=me%3Arated')"
echo "BAD_SORT=$(curl -s --max-time 15 'localhost:8081/api/v1/search?q=tag%3Acyoa&sort=nope' | jq -c '.error.message')"

cookie=$(awk '$6 ~ /darkhold/ {print $6"="$7}' /run/user/1000/dh-staff-A.cookies | paste -sd';' -)
echo "COOKIE_SET=$([ -n "$cookie" ] && echo yes || echo no)"
q3=$(enc '(source:qq OR source:sv) AND source_updated:<=-4m AND NOT me:rated')
echo "CANONICAL=$(curl -s -H "Cookie: $cookie" --max-time 25 "localhost:8081/api/v1/search?q=$q3&sort=popularity&limit=5" | jq -c '{count:(.items|length),first:(.items[0].title//null)}')"
