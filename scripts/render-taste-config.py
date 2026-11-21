#!/usr/bin/env python3
"""Render the tracked taste declaration into an ignored darkhold config."""
import sys
from pathlib import Path


def replace_taste_section(config_text: str, fragment: str) -> str:
    lines = config_text.splitlines()
    start = None
    for i, line in enumerate(lines):
        if line.strip() == '[taste]':
            start = i
            break
    if start is None:
        raise SystemExit('render: no [taste] section in the config')
    end = len(lines)
    for j in range(start + 1, len(lines)):
        stripped = lines[j].lstrip()
        if stripped.startswith('[') and not stripped.startswith('[[taste.source'):
            end = j
            break
    return '\n'.join(lines[:start] + fragment.rstrip().splitlines() + lines[end:]) + '\n'


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit('usage: render-taste-config.py FRAGMENT CONFIG')
    fragment_path, config_path = (Path(p) for p in sys.argv[1:])
    rendered = replace_taste_section(config_path.read_text(), fragment_path.read_text())
    config_path.write_text(rendered)
    print(f'rendered {fragment_path} into {config_path}')


if __name__ == '__main__':
    main()
