#!/usr/bin/env python3
"""Regenerate docs/plans/orphan-queries.txt — the M70-13 ratchet's baseline.

A function is an orphan when **no app source can reach it**, directly or through another db function. The list is debt,
not permission: it exists so `crates/app/tests/orphan_queries.rs` can fail on a *new* orphan while the existing ones are
worked off. Deleting a line is the goal; adding one needs a reason a reviewer can read.

Reachability is transitive, and that matters: the first version of this scan asked only "does an app source mention this
name", which reported every internal helper dead — `velocity_signals`, `award`, `policy_version`, five `work_characters`
writers — while a route reached each of them through one more call. Roots are the names an app source calls; edges are db
functions calling one another.

What is *not* a reader surface: `crates/db` (the producer) and `crates/test-support` (a test crate — a fixture mentioning a
function is the very shape this ratchet exists to catch).

Usage:
    python3 scripts/orphan-queries.py           # rewrite the file
    python3 scripts/orphan-queries.py --check   # exit 1 when the file is out of date
"""
from __future__ import annotations

import datetime
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "docs/plans/orphan-queries.txt"

DECLARED = re.compile(r"^\s*pub async fn ([A-Za-z0-9_]+)", re.MULTILINE)
# Any top-level or impl-level function, for the reachability graph. `pub(crate)` and private functions are included because a
# public function can be reached *through* them.
ANY_FN = re.compile(r"^[ \t]*(?:pub(?:\(crate\))?\s+)?(?:async\s+)?fn ([A-Za-z0-9_]+)", re.MULTILINE)


def calls(text: str, name: str) -> bool:
    """Whether `text` calls `name` — the identifier followed by `(`, not embedded in a longer identifier."""
    return re.search(rf"(?<![A-Za-z0-9_]){re.escape(name)}\(", text) is not None


def imported_modules(text: str) -> set[str]:
    """The db modules a file makes visible unqualified.

    `use darkhold_db::lexicon::normalise;` (or `use darkhold_db::lexicon;` then `lexicon::normalise(`) is evidence as strong as
    `lexicon::normalise(` — and without it a function reached through a `use` reads as dead. This is the false-positive half of
    the same defect that hid `decision_audit::record`: name-based matching is wrong in both directions.
    """
    out: set[str] = set()
    for statement in re.findall(r"^\s*use\s+([^;]+);", text, re.MULTILINE):
        path = statement.split("{")[0]
        for module in re.findall(r"darkhold_db::\s*([A-Za-z0-9_]+)", path):
            out.add(module)
        for module in re.findall(r"crate::\s*([A-Za-z0-9_]+)", path):
            out.add(module)
    return out


def bodies(db_src: Path) -> dict[str, list[tuple[set[str], str]]]:
    """Each function, keyed `module::name`, as (modules imported by its file, body).

    Keyed by module because *names repeat*: `record`, `list`, `get` and `count` exist in half a dozen db modules, and a graph
    keyed by the bare name merges them into one node — so a call to any `record(` marks every `record` reachable. Measured
    2026-11-09: `decision_audit::record` has no product caller at all, yet it was outside the orphan list because other db
    modules call their own `record(`.
    """
    out: dict[str, list[tuple[set[str], str]]] = {}
    for file in sorted(db_src.rglob("*.rs")):
        text = file.read_text(encoding="utf-8", errors="replace")
        module = file.stem
        imports = imported_modules(text)
        matches = list(ANY_FN.finditer(text))
        for index, match in enumerate(matches):
            end = matches[index + 1].start() if index + 1 < len(matches) else len(text)
            out.setdefault(f"{module}::{match.group(1)}", []).append((imports, text[match.end() : end]))
    return out


def mentions(text: str, module: str, name: str, unique: bool, imports: set[str]) -> bool:
    """Whether `text` reaches `module::name`.

    Qualified evidence always counts. A bare `name(` counts when no other db module defines that name, or when the calling file
    imports the module (`use …::module::name;` / `use …::module;`). Anything else is a guess, and a guess is what let 37 functions
    hide.
    """
    if re.search(rf"(?<![A-Za-z0-9_]){re.escape(module)}::\s*{re.escape(name)}\s*\(", text):
        return True
    return calls(text, name) and (unique or module in imports)


def main() -> int:
    db_src = ROOT / "crates/db/src"
    declared: list[tuple[str, str]] = []
    modules: dict[str, set[str]] = {}
    for file in sorted(db_src.rglob("*.rs")):
        text = file.read_text(encoding="utf-8", errors="replace")
        module = file.stem
        for name in DECLARED.findall(text):
            declared.append((name, str(file.relative_to(ROOT))))
            modules.setdefault(name, set()).add(module)

    app_sources: list[str] = []
    for crate in sorted((ROOT / "crates").iterdir()):
        if not crate.is_dir() or crate.name in {"db", "test-support"}:
            continue
        src = crate / "src"
        if src.is_dir():
            for file in sorted(src.rglob("*.rs")):
                if "tests" in file.parts:
                    continue
                app_sources.append(file.read_text(encoding="utf-8", errors="replace"))

    graph = bodies(db_src)

    # Unique among EVERY function in the graph, not among the public async ones. A private helper (`async fn record_decision` in
    # `positivity.rs`) is not a `pub async fn`, so it never reached `modules` — and a bare call to it then failed the uniqueness
    # test, got no edge, and every function it called stayed on the orphan list. Measured 2026-11-09: that is what kept
    # `decision_audit::record` listed after it had a caller.
    keys_by_name: dict[str, list[str]] = {}
    for key in graph:
        keys_by_name.setdefault(key.split("::", 1)[1], []).append(key)
    unique_names = {name for name, keys in keys_by_name.items() if len(keys) == 1}

    # Indexes, because the closure below is otherwise O(functions x files) with a regex per pair and takes minutes.
    def index(texts: list[str]) -> tuple[dict[str, list[set[str]]], set[str]]:
        """bare name -> [modules imported by each file that calls it], and every `module::name(` qualified."""
        bare: dict[str, list[set[str]]] = {}
        qualified: set[str] = set()
        for text in texts:
            imports = imported_modules(text)
            for name in set(re.findall(r"(?<![A-Za-z0-9_])([A-Za-z0-9_]+)\s*\(", text)):
                bare.setdefault(name, []).append(imports)
            for module, name in re.findall(
                r"(?<![A-Za-z0-9_])([A-Za-z0-9_]+)::\s*([A-Za-z0-9_]+)\s*\(", text
            ):
                qualified.add(f"{module}::{name}")
        return bare, qualified

    app_bare, app_qualified = index(app_sources)

    def reached(key: str) -> bool:
        module, _, name = key.partition("::")
        if key in app_qualified:
            return True
        callers = app_bare.get(name)
        if not callers:
            return False
        if name in unique_names:
            return True
        return any(module in imports for imports in callers)

    roots = [key for key in graph if reached(key)]

    # Edges per function, computed once: a function reaches the targets its OWN bodies name. A global `module::name(` set would
    # say every reachable function reaches every qualified target, which is the name-based mistake again in a new costume.
    edges: dict[str, set[str]] = {}
    for key, entries in graph.items():
        targets: set[str] = set()
        for imports, body in entries:
            for module, name in re.findall(
                r"(?<![A-Za-z0-9_])([A-Za-z0-9_]+)::\s*([A-Za-z0-9_]+)\s*\(", body
            ):
                candidate = f"{module}::{name}"
                if candidate in graph:
                    targets.add(candidate)
            for name in set(re.findall(r"(?<![A-Za-z0-9_])([A-Za-z0-9_]+)\s*\(", body)):
                for candidate in keys_by_name.get(name, []):
                    module = candidate.split("::", 1)[0]
                    if name in unique_names or module in imports:
                        targets.add(candidate)
        edges[key] = targets

    reachable: set[str] = set(roots)
    queue = list(roots)
    while queue:
        key = queue.pop()
        for other in edges.get(key, ()):
            if other not in reachable:
                reachable.add(other)
                queue.append(other)

    orphans = [(name, file) for name, file in declared if f"{Path(file).stem}::{name}" not in reachable]

    header = [
        "# M70-13 ratchet baseline. Generated by scripts/orphan-queries.py — do not hand-edit the names.",
        "#",
        "# Each line is a `pub async fn` in crates/db/src that no app source can reach, directly or through another db",
        "# function. They are debt, not permission: crates/app/tests/orphan_queries.rs fails when a NEW one appears, so",
        "# this list can only shrink. Delete a line by wiring the function to a route or deleting the function.",
        "#",
        "# Reachability is MODULE-QUALIFIED (2026-11-09). It used to be by bare name, so a call to any `record(` anywhere",
        "# marked every `record` in the crate reachable: `decision_audit::record` has no product caller at all and was",
        "# outside this list, which is why the operator audit route always answered an empty trail. Correcting the",
        "# measurement moved the baseline from 43 to 57. An intermediate version that ignored `use` imports said 80; the",
        "# debt was never new, the ruler was wrong in both directions.",
        f"# measured {datetime.date.today().isoformat()}: {len(orphans)} orphan(s)",
        "",
    ]
    text = "\n".join(header + [f"{name}  # {file}" for name, file in orphans]) + "\n"

    if "--check" in sys.argv:
        current = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if current != text:
            print("docs/plans/orphan-queries.txt is out of date; run python3 scripts/orphan-queries.py")
            return 1
        print(f"ok: {len(orphans)} orphan(s), file current")
        return 0

    OUT.write_text(text, encoding="utf-8")
    print(f"wrote {OUT.relative_to(ROOT)}: {len(orphans)} orphan(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
