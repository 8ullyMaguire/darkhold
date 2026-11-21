#!/usr/bin/env bash
# Prove the deployed darkhold taste declaration agrees with the tracked fragment (ADR 0062).
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FRAGMENT="${TASTE_FRAGMENT:-$ROOT/config/taste.toml}"
CONFIG="${TASTE_CONFIG:-$ROOT/darkhold.toml}"

python3 - "$FRAGMENT" "$CONFIG" <<'PY'
import hashlib
import sys
import tomllib
from pathlib import Path

fragment_path, config_path = (Path(p) for p in sys.argv[1:])
if not fragment_path.is_file():
    raise SystemExit('FAIL  taste fragment missing: ' + str(fragment_path))
if not config_path.is_file():
    raise SystemExit('FAIL  deployed config missing: ' + str(config_path))

fragment = tomllib.loads(fragment_path.read_text()).get('taste')
deployed = tomllib.loads(config_path.read_text()).get('taste')
if fragment is None:
    raise SystemExit('FAIL  fragment has no [taste] declaration')
if deployed is None:
    raise SystemExit('FAIL  deployed config has no [taste] declaration')

problems = []

def compare(left, right, path):
    if isinstance(left, dict) and isinstance(right, dict):
        for key in sorted(set(left) | set(right)):
            if key not in left:
                problems.append(path + '.' + key + ' exists only in deployed')
            elif key not in right:
                problems.append(path + '.' + key + ' exists only in fragment')
            else:
                compare(left[key], right[key], path + '.' + key)
    elif isinstance(left, list) and isinstance(right, list):
        if len(left) != len(right):
            problems.append(path + ' length ' + str(len(left)) + ' != ' + str(len(right)))
        for i, (a, b) in enumerate(zip(left, right)):
            compare(a, b, path + '[' + str(i) + ']')
    elif left != right:
        problems.append(path + ' ' + repr(left) + ' != ' + repr(right))

compare(fragment, deployed, 'taste')
digest = lambda p: hashlib.sha256(p.read_text().encode()).hexdigest()[:16]
print('fragment sha256[:16] ' + digest(fragment_path))
print('deployed sha256[:16] ' + digest(config_path))
if problems:
    for p in problems:
        print('FAIL  ' + p)
    print('-> 1 checks, 1 failed')
    raise SystemExit(1)
print('PASS  taste declaration agrees: ' + str(len(fragment.get('source', []))) + ' source(s), ' + str(len(fragment)) + ' section key(s)')
print('-> 1 checks, 0 failed')
PY
