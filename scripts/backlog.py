#!/usr/bin/env python3
"""Build the backlog inventory from the three sources that describe darkhold's remaining work.

Why this exists: "implement everything" is only meaningful against a counted list. The repository
already carries the tracker (`docs/requirements.csv`, 834 rows), the session roadmap
(`docs/plans/REMAINING-2026-10-03.md`) and the Concord project (checked-in snapshot at
`docs/backlog/concord-features.tsv`, refreshed by hand from
https://concord.polarisocial.xyz/projects/darkhold). This script joins them so the count is
reproducible and a stale claim shows up as a diff.

Usage:
    python3 scripts/backlog.py            # rewrite docs/backlog/backlog.json + INVENTORY.md
    python3 scripts/backlog.py --check    # exit 1 if the checked-in inventory is stale

Rules the counts obey:
  * a requirement counts once, by its `status` column — never by how many docs mention it;
  * a Concord card that is only a rollup (`M09: 41 implemented`) is NOT work; counting rollups as
    items is how a backlog of 80 becomes a backlog of 885 and nobody starts;
  * a Concord card whose title matches a requirement's text is the same item, and is reported as
    `linked` rather than added;
  * everything else in the Concord snapshot is `unlinked` work, which is the genuinely new surface.
"""

from __future__ import annotations

import csv
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
REQUIREMENTS = ROOT / "docs/requirements.csv"
ROADMAP = ROOT / "docs/plans/REMAINING-2026-10-03.md"
CONCORD = ROOT / "docs/backlog/concord-features.tsv"
OUT_JSON = ROOT / "docs/backlog/backlog.json"
OUT_MD = ROOT / "docs/backlog/INVENTORY.md"

ROLLUP = re.compile(r"^M\d+:\s")


def read_requirements() -> list[dict]:
    with open(REQUIREMENTS, encoding="utf-8") as handle:
        return list(csv.DictReader(handle))


def read_concord() -> list[dict]:
    rows = []
    with open(CONCORD, encoding="utf-8") as handle:
        for line in handle.read().splitlines()[1:]:
            if not line.strip():
                continue
            fid, title, status = line.split("\t")
            rows.append({"id": fid, "title": title, "status": status})
    return rows


def read_roadmap() -> list[str]:
    text = ROADMAP.read_text(encoding="utf-8")
    if "## The list — what is left" not in text:
        return []
    tail = text.split("## The list — what is left")[-1]
    items = []
    for line in tail.splitlines():
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) >= 2 and cells[0].isdigit():
            items.append(cells[1])
    return items


STOPWORDS = {
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "in", "is", "it", "its",
    "of", "on", "or", "so", "that", "the", "their", "then", "there", "these", "this", "to",
    "up", "was", "when", "which", "with", "not", "no", "every", "each", "one",
}


def tokens(text: str) -> set[str]:
    return {w for w in re.sub(r"[^a-z0-9 ]", " ", text.lower()).split() if w not in STOPWORDS}


def linked(concord_title: str, requirements: list[dict]) -> str | None:
    """The requirement id a Concord card restates, if any.

    **Token overlap, with a floor on both sides.** The first version used substring containment and
    reported that all 87 work cards were already tracked — because rows whose `requirement` cell is
    empty make the needle an empty string, and `"" in anything` is true. A matcher that says
    "everything is covered" is the most expensive kind of wrong, so this one requires at least three
    shared content words and a Jaccard of 0.6 before it will call two titles the same item. An
    unmatched card is reported, never dropped.
    """
    needle = tokens(concord_title)
    if len(needle) < 3:
        return None
    best: tuple[float, str] | None = None
    for row in requirements:
        hay = tokens(row["requirement"])
        if len(hay) < 3:
            continue
        shared = len(needle & hay)
        if shared < 3:
            continue
        score = shared / len(needle | hay)
        if score >= 0.6 and (best is None or score > best[0]):
            best = (score, row["id"])
    return best[1] if best else None


def build() -> dict:
    requirements = read_requirements()
    concord = read_concord()
    roadmap = read_roadmap()

    by_status: dict[str, int] = {}
    for row in requirements:
        by_status[row["status"].strip()] = by_status.get(row["status"].strip(), 0) + 1
    planned = [r for r in requirements if r["status"].strip() == "planned"]

    rollups = [c for c in concord if ROLLUP.match(c["title"])]
    cards = [c for c in concord if not ROLLUP.match(c["title"])]
    linked_cards, unlinked = [], []
    for card in cards:
        owner = linked(card["title"], requirements)
        if owner:
            linked_cards.append({**card, "requirement": owner})
        else:
            unlinked.append(card)

    by_milestone: dict[str, int] = {}
    for row in planned:
        key = row["milestone"].strip() or "unassigned"
        by_milestone[key] = by_milestone.get(key, 0) + 1

    return {
        "sources": {
            "requirements.csv": {"rows": len(requirements), "by_status": by_status},
            "concord": {
                "cards_in_snapshot": len(concord),
                "rollups": len(rollups),
                "work_cards": len(cards),
                "linked_to_a_requirement": len(linked_cards),
                "unlinked": len(unlinked),
            },
            "roadmap": {"items": len(roadmap)},
        },
        "open_requirements": [
            {
                "id": r["id"],
                "milestone": r["milestone"].strip(),
                "requirement": r["requirement"].strip(),
                "notes": r["notes"].strip(),
            }
            for r in planned
        ],
        "open_by_milestone": dict(sorted(by_milestone.items(), key=lambda kv: -kv[1])),
        "unlinked_concord": unlinked,
        "roadmap": roadmap,
    }


def render_md(data: dict) -> str:
    s = data["sources"]
    lines = [
        "# The backlog, counted",
        "",
        "Generated by `scripts/backlog.py` — do not edit by hand; edit the sources and regenerate.",
        "`--check` exits non-zero when this file is stale.",
        "",
        "## What \"everything\" is",
        "",
        "| source | count | what it means |",
        "|---|---:|---|",
        f"| requirements in the tracker | {s['requirements.csv']['rows']} | the authoritative list |",
        f"| …still `planned` | {len(data['open_requirements'])} | **unimplemented features** |",
        f"| Concord cards in the snapshot | {s['concord']['cards_in_snapshot']} | the project page's features |",
        f"| …rollup cards (`M09: 41 implemented`) | {s['concord']['rollups']} | not work; a milestone summary |",
        f"| …work cards | {s['concord']['work_cards']} | |",
        f"| …already restating a requirement | {s['concord']['linked_to_a_requirement']} | same item, not a second one |",
        f"| …genuinely unlinked | {s['concord']['unlinked']} | **new surface** |",
        f"| roadmap items in the current session plan | {s['roadmap']['items']} | ordered work, mostly process |",
        "",
        "## Open requirements by milestone",
        "",
        "| milestone | open |",
        "|---|---:|",
    ]
    for milestone, count in data["open_by_milestone"].items():
        lines.append(f"| {milestone} | {count} |")

    lines += ["", "## The open requirements", "", "| id | milestone | requirement |", "|---|---|---|"]
    for row in data["open_requirements"]:
        text = row["requirement"].replace("|", "\\|")
        lines.append(f"| `{row['id']}` | {row['milestone']} | {text[:150]} |")

    if data["unlinked_concord"]:
        lines += [
            "",
            "## Concord cards with no requirement behind them",
            "",
            "These are the cards that would otherwise be lost between the tracker and the project page.",
            "",
            "| card | status | title |",
            "|---|---|---|",
        ]
        for card in data["unlinked_concord"]:
            lines.append(f"| {card['id']} | {card['status']} | {card['title'][:120]} |")

    if data["roadmap"]:
        lines += ["", "## The roadmap's own list", ""]
        for i, item in enumerate(data["roadmap"], 1):
            lines.append(f"{i}. {item}")

    lines += [
        "",
        "## How a feature lands (the extension rule)",
        "",
        "Every surface is a **registered extension** (§59.3): a `Module` row in",
        "`crates/domain/src/modules/registry.rs` with `default_on`, `toggle` (`Fixed`/`Operator`/",
        "`PerUser`), `relevance`, and a `source` that names its file. `module_mount` in",
        "`crates/app/src/server.rs` gates the router, so an extension an instance does not run answers",
        "`MODULE_UNAVAILABLE` and the surface genuinely does not exist.",
        "",
        "- **Relevant by default:** `default_on: true`, and the row is `toggle: Fixed` when switching it",
        "  off would be a lie (safety surfaces) or `Operator` when the instance may legitimately not run",
        "  it.",
        "- **Speculative:** `default_on: false` with `toggle: PerUser` — the reader opts in, and the",
        "  instance may forbid overrides with `[modules] allow_user_overrides = false`.",
        "- **No feature-local boolean.** `[modules] disabled`/`enabled` is the switch. A second boolean",
        "  in a config struct is a second thing to keep true, and the registry test",
        "  (`m59_extensions::every_route_module_is_registered`) fails on a mounted module with no row.",
        "",
        "## Order of work",
        "",
        "1. The roadmap's own list, in its own order (it is ordered for a reason: process before",
        "   features, so a later feature does not inherit a broken gate).",
        "2. The `planned` requirements by milestone, smallest first within a milestone.",
        "3. The unlinked Concord cards, each landing as an extension with a doc section.",
        "",
    ]
    return "\n".join(lines)


def main() -> int:
    data = build()
    rendered_json = json.dumps(data, indent=2, sort_keys=False) + "\n"
    rendered_md = render_md(data)

    if "--check" in sys.argv:
        stale = []
        for path, rendered in ((OUT_JSON, rendered_json), (OUT_MD, rendered_md)):
            if not path.exists() or path.read_text(encoding="utf-8") != rendered:
                stale.append(str(path.relative_to(ROOT)))
        if stale:
            print("the backlog inventory is stale: " + ", ".join(stale))
            print("run: python3 scripts/backlog.py")
            return 1
        print(
            f"the backlog is current: {len(data['open_requirements'])} open requirements, "
            f"{len(data['unlinked_concord'])} unlinked Concord cards, {len(data['roadmap'])} roadmap items"
        )
        return 0

    OUT_JSON.write_text(rendered_json, encoding="utf-8")
    OUT_MD.write_text(rendered_md, encoding="utf-8")
    print(f"wrote {OUT_JSON.relative_to(ROOT)} and {OUT_MD.relative_to(ROOT)}")
    print(
        f"  open requirements: {len(data['open_requirements'])}"
        f" | by milestone: {data['open_by_milestone']}"
    )
    print(
        f"  concord: {data['sources']['concord']['cards_in_snapshot']} cards,"
        f" {data['sources']['concord']['rollups']} rollups,"
        f" {data['sources']['concord']['unlinked']} unlinked"
    )
    print(f"  roadmap items: {len(data['roadmap'])}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
