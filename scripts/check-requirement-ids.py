#!/usr/bin/env python3
"""Check that every requirement id in `docs/requirements.csv` names exactly one row.

Why: `M63-35` recorded that the catalogue sweep appended rows without checking the id space, so
**eight ids were assigned to twelve extra rows** — `M46-05` six times, `M63-03/04/05` twice each. A
citation of `M63-04` then landed on whichever of two unrelated requirements sorted first, and no
count of "open requirements" could be trusted. The rows were renumbered on 2026-11-02 (each moved row
says so in its own `notes`); this is the gate that keeps it fixed.

**The first version of this gate was wrong, and the file said so.** It required every id to match
`M<n>-<two digits>` and produced 435 failures. The tracker is also a test ledger:
`M09.a_work_round_trips`, `M15.search.a_rebuild_indexes_every_token`, `M57A-01`, `DM3-04`, `GS-1`,
`PG-01a`, `M45-45g1`. Those are ids on purpose — a test row cites the test by name — so demanding one
shape would have meant renaming 435 real rows to satisfy a rule nobody asked for. The gate now
validates the canonical form where it is used and leaves the ledger ids alone.

The rules, each because its absence is a silent miscount:

1. **Every id is unique.** A duplicate makes `grep M63-04 docs/` ambiguous, and every later count
   (open items, closed items, milestone progress) inherits the ambiguity.
2. **The row count does not collapse.** A parser that matches nothing satisfies rule 1 trivially, so
   the count is asserted against a floor. Raise the floor when rows are added; never lower it.

**And a rule this gate must NOT have: the id's prefix need not equal the `milestone` column.** The
second draft required it and failed five true rows — `M11-17`, `M11-17a`…`M11-17d`, all filed under
`M57`. Those ids are correct: the id names the **section of origin** (§15.17's metadata exchange) and
the milestone names when it is scheduled, which `docs/goal.md` says outright — *"Finish the M11-17
block (M57)"* — and ten files cite those ids, including `crates/app/tests/m57_metadata_exchange.rs`.
Renaming them to `M57-0x` to satisfy the gate would have broken every citation to make a rule true. The
divergence is reported as information, not as a failure.

Usage:
    python3 scripts/check-requirement-ids.py
    python3 scripts/check-requirement-ids.py --check   # same check; the flag is for symmetry
"""

from __future__ import annotations

import collections
import csv
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CSV = ROOT / "docs/requirements.csv"

#: The number of rows the file had when this gate was written (2026-11-02, after the duplicate ids
#: were renumbered). A floor, not an equality: adding requirements is normal, losing them is not.
MIN_ROWS = 834

#: A requirement id proper: `M63-36`, `M45-45g`, `M6-16`. Dotted (`M09.a_test_name`) and
#: letter-prefixed (`DM3-04`, `GS-1`, `M57A-01`) ids are the test ledger and are exempt from rule 2.
CANONICAL = re.compile(r"^M(\d+)-\d+[a-z]?$")


def main() -> int:
    with open(CSV, encoding="utf-8") as handle:
        rows = list(csv.DictReader(handle))

    problems: list[str] = []

    if len(rows) < MIN_ROWS:
        problems.append(
            f"only {len(rows)} rows parsed, floor is {MIN_ROWS} — a parser that matches nothing "
            f"passes every other rule in this file"
        )

    # 1. unique
    counts = collections.Counter(row["id"] for row in rows)
    for rid, n in sorted(counts.items()):
        if n > 1:
            problems.append(
                f"id {rid} is used by {n} rows; renumber the later ones and say so in `notes`"
            )

    # Rule 2 is informational only: an id whose prefix differs from the milestone names the section
    # of origin rather than the schedule (`M11-17` → `M57`), and ten files cite those ids.
    diverged = []
    canonical = 0
    for row in rows:
        match = CANONICAL.match(row["id"])
        if not match:
            continue
        canonical += 1
        declared = row["milestone"].strip()
        if declared != f"M{match.group(1)}":
            diverged.append(f"{row['id']}→{declared}")

    by_milestone = collections.Counter(row["milestone"].strip() for row in rows)

    if problems:
        for problem in problems:
            print(f"FAIL: {problem}")
        print(f"\n{len(problems)} problem(s) in {CSV.relative_to(ROOT)} ({len(rows)} rows read)")
        return 1

    ledger = len(rows) - canonical
    print(
        f"OK: {len(rows)} rows, {len(counts)} unique ids, {len(by_milestone)} milestones; "
        f"{canonical} canonical requirement ids and {ledger} ledger ids (test names and the "
        f"DM/GS/PG prefixes), which this gate does not govern"
    )
    if diverged:
        print(
            f"note: {len(diverged)} id(s) name their section of origin rather than their milestone "
            f"({', '.join(sorted(diverged))}) — by design, not a fault"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
