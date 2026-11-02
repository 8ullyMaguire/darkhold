#!/usr/bin/env python3
"""Pseudonymise a reader id in an evaluation set.

The exporter emits raw account ids because SQLite has no hash function and doing the mapping in two
dialects is how the two come to disagree. This is the one place a reader id becomes a name.

**A stable salt, not a per-run one.** Re-hashing with a fresh salt each run would make every run's
reader ids different, and two runs over the same history would then look like two different
populations. The salt is `eval/salt`, created once, mode 0600, gitignored — the diagram of who read
what never leaves the host (spec §0.3), and a salted hash keeps even the local file from being a
plain list of account ids.

    python3 scripts/eval/pseudonymise.py eval/salt < set.json > set.json.pseud

Reads JSON on stdin, writes JSON on stdout. Any `reader` string that is not already a hex digest is
replaced by `sha256(salt + id)` truncated to 16 hex characters.
"""

from __future__ import annotations

import hashlib
import json
import sys

HEX = set("0123456789abcdef")


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: pseudonymise.py SALT_FILE < in.json > out.json", file=sys.stderr)
        return 2
    with open(argv[1], "rb") as handle:
        salt = handle.read().strip()
    if not salt:
        print(f"{argv[1]}: the salt is empty; refusing to hash with nothing", file=sys.stderr)
        return 2

    document = json.load(sys.stdin)
    readers = document.get("readers", [])
    for entry in readers:
        current = str(entry.get("reader", ""))
        if len(current) == 16 and set(current) <= HEX:
            continue  # already pseudonymised: a re-run must not hash a hash
        digest = hashlib.sha256(salt + current.encode("utf-8")).hexdigest()[:16]
        entry["reader"] = digest

    json.dump(document, sys.stdout, ensure_ascii=False)
    sys.stdout.write("\n")
    # The count goes to stderr so the JSON on stdout stays machine-readable.
    print(f"pseudonymised {len(readers)} reader(s)", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
