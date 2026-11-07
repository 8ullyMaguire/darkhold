#!/usr/bin/env python3
"""Migration numbers are unique, per engine, and the two engines agree.

WHY THIS EXISTS

Two migrations can share a number without anything failing at author time. On 2026-11-07 two did: this tree's
`0178_source_engagement` (M63-37) and another session's `0178_session_roles` (W7 task 2 step 3), written in parallel
checkouts from the same base. Both loaded, both ran, and the only reason it surfaced was a propagation merge looking
odd — a collision that reaches a deploy is a schema whose order depends on file-name sort within one number, which is
alphabetical, which is not a decision anybody made.

The gate refuses three things, each a different failure:

* a file with **no** numeric prefix — the runner sorts by name, so `add_roles.sql` runs wherever alphabet puts it,
  probably before the tables it alters;
* a prefix that **repeats within an engine** — two orders with one number;
* a number present in **one engine and not the other** — the two arms of every statement are then not the same schema,
  and the divergence shows up as a test that passes on SQLite and a column that does not exist on PostgreSQL.

Gaps are allowed on purpose: a number is spent once it has been committed anywhere, including a branch that never
merged. Reusing it is the error, not skipping it. So the check reports the highest number and the count, which is what
the next author needs, and does not demand a contiguous range.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ENGINES = ("sqlite", "postgres")
NAME = re.compile(r"^(?P<number>\d{4})_(?P<slug>[a-z0-9]+(?:_[a-z0-9]+)*)\.sql$")


def scan(engine: str) -> tuple[dict[int, list[str]], list[str]]:
    """Numbers → file names, and the problems found in this engine's directory."""
    directory = ROOT / "migrations" / engine
    if not directory.is_dir():
        return {}, [f"migrations/{engine} does not exist"]
    numbers: dict[int, list[str]] = {}
    problems: list[str] = []
    for path in sorted(directory.glob("*.sql")):
        match = NAME.match(path.name)
        if not match:
            problems.append(
                f"migrations/{engine}/{path.name}: no `NNNN_snake_case.sql` prefix, so its order is alphabetical"
            )
            continue
        numbers.setdefault(int(match.group("number")), []).append(path.name)
    return numbers, problems


def main() -> int:
    problems: list[str] = []
    per_engine: dict[str, dict[int, list[str]]] = {}
    for engine in ENGINES:
        numbers, engine_problems = scan(engine)
        per_engine[engine] = numbers
        problems.extend(engine_problems)
        for number, names in sorted(numbers.items()):
            if len(names) > 1:
                problems.append(
                    f"migrations/{engine}: {number:04d} is used {len(names)} times — {', '.join(names)}"
                )

    # The two arms must be the same schema: every number in one engine must exist in the other.
    for engine, other in (("sqlite", "postgres"), ("postgres", "sqlite")):
        only_here = sorted(set(per_engine[engine]) - set(per_engine[other]))
        for number in only_here:
            names = ", ".join(per_engine[engine][number])
            problems.append(f"{number:04d} exists in migrations/{engine} ({names}) but not in migrations/{other}")

    if problems:
        print("FAIL: the migration numbers are not a single, unambiguous sequence.", file=sys.stderr)
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        print(
            "\nRenumbering is the fix, and the newcomer moves: a number already committed anywhere is spent.",
            file=sys.stderr,
        )
        return 1

    counts = {engine: len(numbers) for engine, numbers in per_engine.items()}
    highest = max(per_engine["sqlite"], default=0)
    print(
        f"OK: {counts['sqlite']} migrations per engine, numbers unique and identical across both; "
        f"highest is {highest:04d}, next is {highest + 1:04d}."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
