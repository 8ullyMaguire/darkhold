#!/usr/bin/env python3
"""Does the roadmap's parsing rule actually parse the list? Run it before anyone writes a seeder.

    python3 /tmp/parse_read_list.py /personal/literature/fanfiction/read.txt
"""
import re
import sys
from collections import Counter

RULE = re.compile(r"^(\d+)\s+(\d{1,2})\s+(\S.*?)\s{2,}(\S.*)$")


def main(path: str) -> int:
    lines = open(path, encoding="utf-8", errors="replace").read().splitlines()
    matched, unmatched, ratings = [], [], Counter()
    for n, line in enumerate(lines, 1):
        if not line.strip():
            continue
        m = RULE.match(line)
        if not m:
            unmatched.append((n, line[:70]))
            continue
        wid, rating, title, author = m.groups()
        ratings[int(rating)] += 1
        matched.append((wid, int(rating), title.strip(), author.strip()))
    print(f"lines          {len(lines)}")
    print(f"matched        {len(matched)}")
    print(f"unmatched      {len(unmatched)}")
    print(f"rating spread  {dict(sorted(ratings.items(), reverse=True))}")
    print(f"distinct ids   {len({m[0] for m in matched})}")
    print("\nsample (first 5):")
    for row in matched[:5]:
        print("   ", row)
    print("\nunmatched (first 8):")
    for n, text in unmatched[:8]:
        print(f"    line {n:>3}: {text}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1] if len(sys.argv) > 1 else "/personal/literature/fanfiction/read.txt"))
