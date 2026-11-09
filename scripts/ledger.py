#!/usr/bin/env python3
"""The requirement ledger's only writer.

## Why this module exists at all

`docs/requirements.csv` is the row-by-row authority for what is built (the specification's
§23.1). It is **CRLF**, and it has been for the whole life of the repo. That is load-bearing:
`scripts/add-m45-57-row.py` carries this warning at the top of the file —

    "That file is CRLF. Rewriting it with Python's `csv` module converts every line ending and
     turns a one-line edit into 698 — a trap this repo has hit before."

A one-line edit became a 698-line diff. The trap is not that the `csv` module is wrong; it is
that the `csv` module's *default* line terminator is `\r\n` on write and its reader is
newline-agnostic, and everybody's editor is not. So this module makes the line ending an
explicit, non-negotiable property of every read and every write, and there is exactly one
place in the repo where the ledger is touched programmatically.

## The rules this module enforces

1. **The line ending is CRLF, always.** Read with `newline=''` so the csv module sees the raw
   bytes; write with `lineterminator='\r\n'`. There is no code path here that can write a lone
   `\n`.
2. **The header is preserved.** A ledger that gains a column gains it in the canonical order,
   and a ledger that has not yet gained a column does not gain one because somebody edited a
   row. The canonical order is
   `id,area,requirement,milestone,status,evidence,notes,verified_at,gates`.
3. **Rows are keyed by `id`,** which is the only column the specification says is unique.
4. **`upsert_row` is idempotent.** Running it twice with the same row leaves the file
   byte-identical. The spec's rule is "the ledger is re-derived, never believed", and a writer
   that cannot be run twice is not re-derivable.

## Usage

    python3 scripts/ledger.py add   < row.json     # append, or replace an existing id
    python3 scripts/ledger.py upsert < row.json    # the same; the verb is a courtesy
    python3 scripts/ledger.py show  <id>           # print one row as JSON
    python3 scripts/ledger.py count                # print the data-row count

A row object is a JSON object with the ledger's column names as keys. Unknown keys are an
error; missing keys are an error for `add`, and are filled with the empty string for `upsert`
of an existing row.
"""

from __future__ import annotations

import argparse
import csv
import io
import json
import os
import sys
from typing import Dict, List, Sequence

#: The canonical column order. A ledger's header is this, in this order, plus nothing.
CANONICAL_FIELDS: List[str] = [
    "id",
    "area",
    "requirement",
    "milestone",
    "status",
    "evidence",
    "notes",
    "verified_at",
    "gates",
]

#: The line terminator. Not a default; a decision.
LINE_TERMINATOR = "\r\n"

#: Where the ledger lives, relative to the repo root.
DEFAULT_LEDGER = os.path.join("docs", "requirements.csv")

# `csv`'s default field limit is 131072 bytes, and `read_rows` is the only reader of a file this module writes — so when one
# row's `notes` grew past it (M70-13, 2026-11-09: 131,739 characters of pass journal), the module could no longer read the
# ledger it had just written, and `gate.sh ledger` failed with `field larger than field limit`. The limit exists to stop a
# malformed file exhausting memory; it is not a bound on what a legitimate row may say, so it is raised to a size a real ledger
# cell cannot reach. `NOTES_LIMIT` below is the bound that *should* hold, and it is enforced on write.
CSV_FIELD_LIMIT = 16 * 1024 * 1024
csv.field_size_limit(CSV_FIELD_LIMIT)

# A `notes` cell is a pointer, not a journal.
#
# M70-13's notes reached 131,739 characters — every pass of a long ratchet appended in place — which is a document wearing a CSV
# cell as a container: unreadable in a spreadsheet, unreadable in `psql \copy`, and it broke the reader above. The journal
# belongs in `docs/plans/roadmap-remaining.md` (which already holds every pass) and in the handoffs; the cell carries a summary
# and a pointer. 32 KiB is generous for that and small enough that a spreadsheet opens it.
NOTES_LIMIT = 32 * 1024


def _read_raw(path: str) -> str:
    """Read the ledger as text with the line endings untouched.

    `newline=''` is the whole point. With the default universal-newline translation, Python
    rewrites `\r\n` to `\n` on the way in and the file's shape is changed before a single row
    is parsed.
    """
    with io.open(path, "r", encoding="utf-8", newline="") as handle:
        return handle.read()


def _write_raw(path: str, text: str) -> None:
    """Write the ledger as bytes, with the line endings exactly as given."""
    with io.open(path, "w", encoding="utf-8", newline="") as handle:
        handle.write(text)


def _header_for(path: str) -> List[str]:
    """The ledger's header, exactly as the file has it.

    An existing header is **preserved, verbatim** — its columns, its order, and its count. The
    canonical order is what a *new* ledger gets; it is not a repair job for an existing one.

    This is the rule the module's own header states: *"a ledger that has not yet gained a
    column does not gain one because somebody edited a row."* Appending the missing canonical
    columns to a ledger that does not have them violates exactly that: the ledger gains
    `verified_at` and `gates` — empty, on every one of 703 rows — because one row was edited.

    A ledger that has *not* yet grown a column therefore keeps not having it, and the column
    arrives when the migration that introduces it arrives, which is a deliberate act with its
    own commit.
    """
    if not os.path.exists(path) or os.path.getsize(path) == 0:
        return list(CANONICAL_FIELDS)
    raw = _read_raw(path)
    if not raw.strip():
        return list(CANONICAL_FIELDS)
    first = raw.split(LINE_TERMINATOR)[0]
    return next(csv.reader([first]))


def read_rows(path: str = DEFAULT_LEDGER) -> List[Dict[str, str]]:
    """Every data row, as a list of dicts. The header row is not a data row."""
    if not os.path.exists(path):
        return []
    raw = _read_raw(path)
    if not raw.strip():
        return []
    reader = csv.DictReader(io.StringIO(raw, newline=""))
    rows = []
    for row in reader:
        # DictReader leaves a None key for a row with more fields than the header, and a None
        # value for a row with fewer. Both are a corrupt ledger, not a row.
        if None in row:
            raise ValueError(
                "%s: a row has more fields than the header (%r)" % (path, row[None])
            )
        rows.append({k: ("" if v is None else v) for k, v in row.items()})
    return rows


def _check_notes_size(rows: Sequence[Dict[str, str]]) -> None:
    """Refuse a `notes` cell that has become a document.

    The failure has to be loud and on write: a row that grows unchecked breaks the reader above, and the reader's failure is
    reported against the *file*, long after the pass that wrote it. Naming the id and the size here puts the failure where the
    author still remembers what they were doing.
    """
    for row in rows:
        notes = row.get("notes") or ""
        if len(notes) > NOTES_LIMIT:
            raise ValueError(
                "%s: notes is %d characters (limit %d) — the pass journal belongs in "
                "docs/plans/roadmap-remaining.md; keep a summary and a pointer here"
                % (row.get("id") or "<no id>", len(notes), NOTES_LIMIT)
            )


def write_rows(path: str, rows: Sequence[Dict[str, str]], header: Sequence[str] = None) -> None:
    """Write the rows, with the header, with CRLF.

    The file is written through a `StringIO` and then committed in one `write`, so a failure
    part-way through does not leave a half-written ledger.
    """
    _check_notes_size(rows)
    if header is None:
        header = _header_for(path)
    buffer = io.StringIO(newline="")
    writer = csv.DictWriter(
        buffer,
        fieldnames=list(header),
        lineterminator=LINE_TERMINATOR,
        extrasaction="raise",
    )
    writer.writeheader()
    for row in rows:
        writer.writerow({field: row.get(field, "") for field in header})
    _write_raw(path, buffer.getvalue())


def upsert_row(path: str, row: Dict[str, str]) -> bool:
    """Insert or replace the row with this `id`. Returns True if a row was replaced.

    Idempotent: running it twice with the same row leaves the file byte-identical.
    """
    if "id" not in row or not row["id"]:
        raise ValueError("a ledger row must have a non-empty 'id'")
    unknown = set(row) - set(CANONICAL_FIELDS)
    if unknown:
        raise ValueError(
            "unknown ledger column(s): %s — the columns are %s"
            % (", ".join(sorted(unknown)), ", ".join(CANONICAL_FIELDS))
        )
    header = _header_for(path)
    rows = read_rows(path)
    for index, existing in enumerate(rows):
        if existing.get("id") == row["id"]:
            merged = dict(existing)
            merged.update(row)
            rows[index] = merged
            write_rows(path, rows, header)
            return True
    rows.append({field: row.get(field, "") for field in header})
    write_rows(path, rows, header)
    return False


def rename_row(path: str, old_id: str, new_id: str) -> Dict[str, str]:
    """Renumber the **last** row carrying `old_id` to `new_id`, in place, and return it.

    Why a verb and not a hand edit: renumbering is how a duplicate id is repaired, and it is the
    one edit that cannot be expressed as an upsert — the old key has to stop existing, and a
    writer with no way to do that invites somebody to `sed` the CSV instead.

    Three rules, each because its absence is the ambiguity being repaired:

    1. **The later row moves, not the earlier one.** A duplicate exists because a sweep appended
       a row onto an id its section had already spent. The first row is the one the section
       number belongs to, so it keeps the id and the later row takes the suffix. This is why the
       verb reads every match and renames the last, rather than refusing on a duplicate.
    2. **The row keeps its position.** A renumber that also reordered the file would turn a
       one-column change into a 900-line diff, which is the trap the whole module exists for.
    3. **A taken `new_id` is refused**, as is a missing `old_id`, and `old_id == new_id`. A
       rename that created the ambiguity it was meant to remove is not a repair.

    The caller writes the sentence saying who moved and why into both rows' `notes`: this verb
    records nothing, and a renumber nobody explained is a mystery the next session re-litigates.
    """
    if not old_id or not new_id:
        raise ValueError("rename needs two non-empty ids")
    if old_id == new_id:
        raise ValueError("renaming %r to itself changes nothing" % old_id)
    header = _header_for(path)
    rows = read_rows(path)
    if any(row.get("id") == new_id for row in rows):
        raise ValueError("id %r is already used by a row; pick an unused suffix" % new_id)
    target = None
    for index, existing in enumerate(rows):
        if existing.get("id") == old_id:
            # Deliberately the last match: see rule 1 above.
            target = index
    if target is None:
        raise ValueError("no row with id %r" % old_id)
    rows[target]["id"] = new_id
    write_rows(path, rows, header)
    return rows[target]


def add_columns(path: str, columns: Sequence[str]) -> bool:
    """Add `columns` to the ledger's header, and to every row, empty.

    This is the **deliberate migration** the module's rules defer to: a ledger that
    has not yet grown a column does not grow one because a row was edited, so the
    column arrives here, in its own command, with its own commit.

    The new columns are appended in the order given, after the existing ones, and
    every existing row gets an empty value for each. **The row count does not
    change** — a migration that added or dropped a row would not be a migration.

    Returns True if the ledger changed.
    """
    if not columns:
        return False
    unknown = [c for c in columns if c not in CANONICAL_FIELDS]
    if unknown:
        raise ValueError(
            "unknown ledger column(s): %s — the columns are %s"
            % (", ".join(unknown), ", ".join(CANONICAL_FIELDS))
        )
    header = _header_for(path)
    added = [c for c in columns if c not in header]
    if not added:
        return False
    new_header = list(header) + added
    rows = read_rows(path)
    for row in rows:
        for column in added:
            row.setdefault(column, "")
    write_rows(path, rows, new_header)
    return True


def show_row(path: str, row_id: str) -> Dict[str, str]:
    """The row with this `id`, or a `KeyError`."""
    for row in read_rows(path):
        if row.get("id") == row_id:
            return row
    raise KeyError(row_id)


def main(argv: Sequence[str] = None) -> int:
    parser = argparse.ArgumentParser(description="The requirement ledger's only writer.")
    parser.add_argument("--ledger", default=DEFAULT_LEDGER, help="path to requirements.csv")
    sub = parser.add_subparsers(dest="command", required=True)

    for verb in ("add", "upsert"):
        p = sub.add_parser(verb, help="insert or replace a row, read as JSON on stdin")
        p.add_argument("--id", default=None, help="override the row's id")

    migrate = sub.add_parser("migrate", help="add a column to the ledger, empty on every row")
    migrate.add_argument(
        "--add-column",
        action="append",
        default=[],
        metavar="COLUMN",
        help="a column to add; may be given more than once",
    )

    p = sub.add_parser("show", help="print one row as JSON")
    p.add_argument("id")

    p = sub.add_parser(
        "rename", help="renumber the later row carrying an id, so the id names one row"
    )
    p.add_argument("old_id")
    p.add_argument("new_id")

    sub.add_parser("count", help="print the data-row count")

    args = parser.parse_args(argv)

    if args.command in ("add", "upsert"):
        payload = json.load(sys.stdin)
        if args.id:
            payload["id"] = args.id
        upsert_row(args.ledger, payload)
        return 0

    if args.command == "migrate":
        changed = add_columns(args.ledger, args.add_column)
        print("migrated" if changed else "already up to date")
        return 0

    if args.command == "show":
        json.dump(show_row(args.ledger, args.id), sys.stdout, indent=2, sort_keys=True)
        sys.stdout.write("\n")
        return 0

    if args.command == "rename":
        moved = rename_row(args.ledger, args.old_id, args.new_id)
        # Re-read it: a rename that misses leaves the id where it was with no error, which is the
        # failure the skill's "re-read a row you updated by script" rule names.
        after = show_row(args.ledger, args.new_id)
        if after["id"] != args.new_id:
            raise SystemExit("rename did not land: %r still absent" % args.new_id)
        print(
            "%s -> %s: %s"
            % (
                args.old_id,
                args.new_id,
                (moved.get("requirement") or "")[:100],
            )
        )
        return 0

    if args.command == "count":
        print(len(read_rows(args.ledger)))
        return 0

    return 2


if __name__ == "__main__":
    sys.exit(main())
