#!/usr/bin/env python3
"""Split the Darkhold specification into `docs/spec/`, one file per section.

WHY THIS IS A SCRIPT AND NOT A ONE-OFF

The split is a **copy**, and a copy that is done by hand is a copy that is done
once. The masters stay the masters until the split is complete, so every run of
this script re-derives `docs/spec/` from them — and a section that changed in a
master shows up as a diff in the split rather than as a divergence nobody
notices.

TWO MASTERS, ONE NUMBER SPACE (decided 2026-11-02)

  * **§1–27** come from `~/secondbrain/10-Projects/ultimate-fanfiction-site-spec.md`,
    whose sections are `## N. Title`.
  * **§28–61** come from `docs/spec.md`, whose sections are `# N. Title` (§28–49)
    and `## N Title` (§50–61) — two shapes in one document, both accepted here.

The numbers stay contiguous because a citation is a number: §53.5 has to resolve
to `docs/spec/53-*.md` whether it was written against one master or the other.
Slots **62–70** are reserved and unallocated; a section number is never reused
and never renumbered (spec §24.2).

USAGE

    python3 scripts/split-spec.py                    # write docs/spec/
    python3 scripts/split-spec.py --check            # refuse to write; report the drift
    python3 scripts/split-spec.py --source <path>    # a different first master
    python3 scripts/split-spec.py --extended <path>  # a different second master
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys

#: Where the first master lives. The vault file stays the master for §1–27 until
#: the split is complete; after that this file becomes the index.
DEFAULT_SOURCE = pathlib.Path.home() / "secondbrain/10-Projects/ultimate-fanfiction-site-spec.md"

#: Where the second master lives — the source of §28–61.
DEFAULT_EXTENDED = pathlib.Path("docs/spec.md")

#: Where the split lands.
DEFAULT_TARGET = pathlib.Path("docs/spec")

#: The slot every section of the first master occupies: the number, the slug, the
#: title, and the milestone that owns it.
#:
#: The title is the specification's own section title, and the milestone is the
#: one that ships it. A section with no milestone is a register rather than a
#: slice, and says so.
SLOTS: list[tuple[int, str, str, str]] = [
    (1, "thesis", "The thesis", "M0"),
    (2, "evidence", "Evidence base", "M0"),
    (3, "inheritance-and-decisions", "Inheritance", "M0"),
    (4, "non-goals", "Non-goals — the hard refusals", "M0"),
    (5, "vocabulary", "The canonical vocabulary", "M1"),
    (6, "domain-model", "Domain model", "M1"),
    (7, "data-model", "Data model", "M1"),
    (8, "architecture", "Architecture", "M1"),
    (9, "ingestion", "Ingestion", "M8"),
    (10, "reading", "Reading", "M5"),
    (11, "search-and-discovery", "Search and discovery", "M7"),
    (12, "taste-themes-moods-channels", "Taste, themes, moods and channels", "M13, M14, M17, M18"),
    (13, "community", "Community", "M12"),
    (14, "governance", "Governance — guests, trust, jury", "M3, M20, M21"),
    (15, "privacy-and-safety", "Privacy and safety", "M22"),
    (16, "extensions", "Extensions and the marketplace", "M23"),
    (17, "federation", "Federation and interoperability", "M25"),
    (18, "the-supporter-tier", "The Supporter tier — compute, not rank", "M24"),
    (19, "configuration", "The configuration contract", "M1"),
    (20, "api-conventions", "The API conventions", "M19"),
    (21, "frontend-and-design", "Frontend and design", "M26"),
    (22, "operations-and-budgets", "Operations, deployment and budgets", "M32"),
    (23, "verification", "The verification discipline", "M1"),
    (24, "the-ledger-and-the-milestones", "The ledger and the milestone plan", "M0"),
    (25, "risks", "The risk register", "—"),
    (26, "open-questions", "Open questions", "—"),
    (27, "appendix", "Appendix — the measured numbers this spec inherits", "M0"),
]

#: The range the second master owns. Contiguous with the first master's range,
#: because the two masters share one number space.
EXTENDED_FROM = 28
EXTENDED_TO = 61

#: The slots beyond both masters: reserved, and unallocated.
RESERVED_FROM = 62
RESERVED_TO = 70

#: The source line slots 1–27 have always carried. Kept verbatim so a run of this
#: script does not rewrite 27 files whose content is unchanged.
FIRST_MASTER_SOURCE = "the corresponding section of the Darkhold specification"

#: The four rules of spec §24.2, which the index states.
RULES = [
    "**A section number is a citation target, and it is never renumbered.** The number space is "
    "reserved in blocks of ten, the block titles are fixed when the block opens, and the "
    "directory numbers and the document's section numbers are the same numbers — so the split does "
    "not renumber anything.",
    "**A retired section keeps its number** and gets a withdrawn line. A number is never reused, "
    "so a citation to a retired section still resolves to the thing it cited.",
    "**A new section takes the next free number,** and the index is updated in the same commit as "
    "the section.",
    "**Each master remains the master for its own range until the split is complete.** §1–27 come "
    "from the vault file and §28–61 from `docs/spec.md`; after the split each becomes an index, and "
    "this directory is the specification.",
]

#: A section start in the first master: `## N. Title`.
FIRST_MASTER_HEADING = re.compile(r"^## (\d+)\. (.*)$")

#: A section start in the second master. **Two shapes, both real** (§28–49 are `# N. Title`,
#: §50–61 are `## N Title`), and a parser that accepts one silently drops half the document.
SECOND_MASTER_HEADINGS = (
    re.compile(r"^# (\d+)\. (.*)$"),
    re.compile(r"^## (\d+) (.*)$"),
)


def read_source(path: pathlib.Path) -> str:
    if not path.exists():
        raise SystemExit(f"the master specification is not there: {path}")
    return path.read_text(encoding="utf-8")


def split_sections(text: str) -> dict[int, tuple[str, str]]:
    """The first master's sections: number -> (title, body).

    A body runs from its `## N. Title` heading to the next `## ` heading, and the
    heading itself is *not* part of the body — the split file writes its own.
    """
    lines = text.split("\n")
    starts: list[tuple[int, int, str]] = []
    for index, line in enumerate(lines):
        match = FIRST_MASTER_HEADING.match(line)
        if match:
            starts.append((int(match.group(1)), index, match.group(2).strip()))
    sections: dict[int, tuple[str, str]] = {}
    for position, (number, index, title) in enumerate(starts):
        end = starts[position + 1][1] if position + 1 < len(starts) else len(lines)
        body = "\n".join(lines[index + 1 : end]).strip("\n")
        sections[number] = (title, body)
    return sections


def split_extended(text: str) -> dict[int, tuple[str, str, int]]:
    """The second master's sections: number -> (title, body, heading level).

    The level is carried because the document uses two: a `# 28.` section nests its
    subsections at `##`, and a `## 50` section nests them at `###`. The split needs the
    difference to put every subsection at the same depth in the output.

    A section ends at the next section start — and *only* at a section start. `## 28.1`
    is a subsection, not a section: its number takes a dot, and neither start pattern
    matches a dotted number.
    """
    lines = text.split("\n")
    starts: list[tuple[int, int, str, int]] = []
    for index, line in enumerate(lines):
        for pattern in SECOND_MASTER_HEADINGS:
            match = pattern.match(line)
            if not match:
                continue
            number = int(match.group(1))
            if number >= EXTENDED_FROM:
                level = len(line) - len(line.lstrip("#"))
                starts.append((number, index, match.group(2).strip(), level))
            break
    sections: dict[int, tuple[str, str, int]] = {}
    for position, (number, index, title, level) in enumerate(starts):
        end = starts[position + 1][1] if position + 1 < len(starts) else len(lines)
        body = "\n".join(lines[index + 1 : end]).strip("\n")
        sections[number] = (title, body, level)
    return sections


def promote(body: str) -> str:
    """Push the first master's headings down one level, so the file reads as a document.

    The master's `### N.M` becomes the split file's `## N.M`, and `#### N.M.K`
    becomes `### N.M.K`. A heading that is already at the floor stays where it
    is — pushing `##` to `###` and `###` to `####` is the whole transform, and
    a heading deeper than that is left alone.
    """
    out = []
    for line in body.split("\n"):
        match = re.match(r"^(#{2,5}) (.*)$", line)
        if match:
            hashes, rest = match.groups()
            out.append(f"{hashes[:-1] if len(hashes) > 2 else '#'} {rest}")
        else:
            out.append(line)
    return "\n".join(out)


def promote_extended(body: str, parent_level: int) -> str:
    """Put the second master's subsections at `##` under the split file's own `#` title.

    A `# 28.` section nests its subsections at `##` already, so nothing moves. A `## 50`
    section nests them at `###`, so they move up one. Nothing is ever pushed to `#`: a
    second level-one heading inside a section file would read as a second document.
    """
    delta = 1 - parent_level
    if delta == 0:
        return body
    out = []
    for line in body.split("\n"):
        match = re.match(r"^(#{1,5}) (.*)$", line)
        if match:
            hashes, rest = match.groups()
            out.append(f"{'#' * max(2, len(hashes) + delta)} {rest}")
        else:
            out.append(line)
    return "\n".join(out)


def slugify(title: str) -> str:
    """A filename slug from a section title: ascii, kebab, and short enough to read."""
    ascii_only = title.replace("—", " ").replace("&", " and ")
    ascii_only = ascii_only.encode("ascii", "ignore").decode("ascii").lower()
    slug = re.sub(r"[^a-z0-9]+", "-", ascii_only).strip("-")
    if len(slug) > 48:
        # Cut on a word boundary: `...-and-changelo` reads as a typo, `...-and-changelog` as a title
        # somebody truncated. If the first word alone is too long, cut there and accept it.
        cut = slug[:48]
        slug = cut.rsplit("-", 1)[0] if "-" in cut else cut
    return slug or "section"


def render(number: int, title: str, milestone: str, body: str, source: str) -> str:
    return (
        f"# {number}. {title}\n"
        "\n"
        "**Status:** reserved\n"
        f"**Milestone:** {milestone}\n"
        f"**Source:** {source}\n"
        "\n"
        f"{body}\n"
    )


def render_reserved(number: int) -> str:
    return (
        f"# {number}. Reserved\n"
        "\n"
        "**Status:** reserved, unallocated\n"
        "**Milestone:** —\n"
        "**Source:** —\n"
        "\n"
        "**This slot is reserved and holds nothing.** The number space is reserved before it is\n"
        "used, so that a section added later takes a number nobody has already cited.\n"
        "\n"
        "A new section takes the next free number, and the index is updated in the same commit\n"
        "as the section (spec §24.2).\n"
    )


def render_index(
    sections: dict[int, tuple[str, str]],
    preamble: str,
    extended: list[tuple[int, str, str]],
) -> str:
    rows = []
    for number, slug, title, milestone in SLOTS:
        rows.append(f"| **{number:02d}** | `{slug}.md` | {title} | {milestone} |")
    extended_rows = [
        f"| **{number:02d}** | `{number:02d}-{slug}.md` | {title} | `docs/spec.md` §{number} |"
        for number, slug, title in extended
    ]
    reserved = ", ".join(f"**{n:02d}**" for n in range(RESERVED_FROM, RESERVED_TO + 1))
    return f"""# 0. The Darkhold specification — the index

**This directory is the specification.** It is split from **two masters**, and the
numbers form one space because a citation is a number:

| range | master | what it is |
|---|---|---|
| **§1–27** | `~/secondbrain/10-Projects/ultimate-fanfiction-site-spec.md` | the vault file — remains the master for this range until the split is complete |
| **§28–61** | `../spec.md` | the implementation specification, whose §28–61 are cited by ADRs, amendments and the audit |

**The four rules** (spec §24.2):

{chr(10).join(f"{index}. {rule}" for index, rule in enumerate(RULES, start=1))}

## The section map — §1–27, from the vault master

| slot | file | title | milestone |
|---|---|---|---|
{chr(10).join(rows)}

## The section map — §28–61, from `../spec.md`

| slot | file | title | source |
|---|---|---|---|
{chr(10).join(extended_rows)}

**Slots {RESERVED_FROM:02d} to {RESERVED_TO:02d} are reserved and unallocated:** {reserved}.

## Which document governs

| | | |
|---|---|---|
| **this directory** | the Darkhold specification, split into sections — **the governing document** |
| **the first master** | `~/secondbrain/10-Projects/ultimate-fanfiction-site-spec.md` — §1–27, the master until the split is complete |
| **the second master** | `../spec.md` — §28–61, still cited directly by ADRs and amendments, so it is a master rather than history |
| **the implementation plan** | `~/secondbrain/10-Projects/darkhold-implementation-plan.md` — 255 task cards, M0–M32, the build order |
| **the authority for what is built** | `../requirements.csv` — the requirement ledger, and ADR 0028 for how it is re-triaged |

## How this directory was made

`python3 scripts/split-spec.py` re-derives every file from the two masters, and
`python3 scripts/check-split-spec.py` asserts that each file's heading number
matches its filename and that the split is in step with **both** masters. **A split
that is done by hand is a split that is done once**, and a section that changed
in a master would show up as a divergence nobody notices.

## How to read this

{preamble}
"""


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=pathlib.Path, default=DEFAULT_SOURCE)
    parser.add_argument("--extended", type=pathlib.Path, default=DEFAULT_EXTENDED)
    parser.add_argument("--target", type=pathlib.Path, default=DEFAULT_TARGET)
    parser.add_argument(
        "--check",
        action="store_true",
        help="do not write; report whether the split is in step with the masters",
    )
    args = parser.parse_args(argv)

    text = read_source(args.source)
    sections = split_sections(text)

    missing = [number for number, _, _, _ in SLOTS if number not in sections]
    if missing:
        raise SystemExit(f"the master has no section(s) {missing}")

    extended_sections = split_extended(read_source(args.extended))
    missing_extended = [
        number for number in range(EXTENDED_FROM, EXTENDED_TO + 1) if number not in extended_sections
    ]
    if missing_extended:
        raise SystemExit(
            f"{args.extended} has no section(s) {missing_extended}; the number space is contiguous, "
            "so a gap is a missing section rather than an empty slot"
        )

    # Slugs come from titles here, and a collision would have one section overwrite another — so it
    # is an error rather than a silently shared filename.
    extended: list[tuple[int, str, str]] = []
    seen: dict[str, int] = {}
    for number in range(EXTENDED_FROM, EXTENDED_TO + 1):
        title = extended_sections[number][0]
        slug = slugify(title)
        if slug in seen:
            raise SystemExit(
                f"§{number} and §{seen[slug]} both slugify to `{slug}`; one needs an explicit slug"
            )
        seen[slug] = number
        extended.append((number, slug, title))

    # §0 is the master's own reading guide, and the index is the split's.
    preamble = sections.get(0, ("", ""))[1]
    if not preamble:
        preamble = "See the master specification."

    args.target.mkdir(parents=True, exist_ok=True)
    written: list[pathlib.Path] = []
    stale: list[str] = []
    drift: list[str] = []

    def emit(path: pathlib.Path, content: str) -> None:
        written.append(path)
        if args.check:
            if not path.exists():
                drift.append(f"{path.name} is missing")
            elif path.read_text(encoding="utf-8") != content:
                drift.append(f"{path.name} differs from its master")
        else:
            path.write_text(content, encoding="utf-8")

    for number, slug, _, milestone in SLOTS:
        title, body = sections[number]
        emit(
            args.target / f"{number:02d}-{slug}.md",
            render(number, title, milestone, promote(body), FIRST_MASTER_SOURCE),
        )

    for number, slug, title in extended:
        _, body, level = extended_sections[number]
        emit(
            args.target / f"{number:02d}-{slug}.md",
            render(
                number,
                title,
                "—",
                promote_extended(body, level),
                f"§{number} of `docs/spec.md`",
            ),
        )

    for number in range(RESERVED_FROM, RESERVED_TO + 1):
        emit(args.target / f"{number:02d}-reserved.md", render_reserved(number))

    emit(args.target / "00-index.md", render_index(sections, preamble, extended))

    # A slot that became a real section leaves its `NN-reserved.md` behind, and the two files claim
    # the same number — so a write removes every numbered file it did not just produce. Check mode
    # reports them instead of deleting.
    expected = {path.name for path in written}
    for path in sorted(args.target.glob("[0-9][0-9]-*.md")):
        if path.name in expected:
            continue
        if args.check:
            drift.append(f"{path.name} is not a slot in the number space")
        else:
            path.unlink()
            stale.append(path.name)

    if args.check:
        if drift:
            print("the split is NOT in step with the masters:", file=sys.stderr)
            for line in drift:
                print(f"  {line}", file=sys.stderr)
            return 1
        print(f"the split is in step with the masters: {len(written)} files")
        return 0

    if stale:
        print(f"removed {len(stale)} stale file(s) that no longer hold a slot: {', '.join(stale)}")
    print(f"wrote {len(written)} files to {args.target}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
