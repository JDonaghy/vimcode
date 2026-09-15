#!/usr/bin/env python3
"""Count *production* Rust lines — everything outside `#[cfg(test)]` items.

`GOALS.md` and `PROJECT_STATE.md` track the size of `src/gtk/`, `src/tui_main/`
and `src/render.rs` as the north star's only quantitative measure. Those numbers
were repeatedly re-typed into prose from ad-hoc measurements taken at different
commits, which is how the 2026-09-01 revision came to record a `src/gtk/` figure
that had actually been measured before #727/#728/#730 landed.

This script exists so every column of that table is measured the same way and can
be regenerated instead of trusted:

    python3 scripts/prod_lines.py src/gtk src/tui_main src/render.rs

and, for a historical column, against a worktree of the revision in question.

Method: walk each `.rs` file; on a `#[cfg(test)]` (or `#[cfg(all(test, ...))]`)
attribute, skip that attribute, any attributes stacked under it, and the whole
braced item it guards. Everything else counts, blank lines and comments included
— the point is comparability across revisions, not a true SLOC figure.
"""

from __future__ import annotations

import os
import sys


def prod_lines(path: str) -> int:
    with open(path, encoding="utf-8", errors="replace") as fh:
        lines = fh.read().splitlines()

    counted = 0
    i = 0
    n = len(lines)
    while i < n:
        stripped = lines[i].strip()
        if stripped.startswith("#[cfg(test)]") or stripped.startswith("#[cfg(all(test"):
            i += 1
            while i < n and lines[i].strip().startswith("#["):
                i += 1
            depth = 0
            opened = False
            while i < n:
                depth += lines[i].count("{") - lines[i].count("}")
                if "{" in lines[i]:
                    opened = True
                i += 1
                if opened and depth <= 0:
                    break
            continue
        counted += 1
        i += 1
    return counted


def total_for(root: str) -> int:
    if os.path.isfile(root):
        return prod_lines(root)
    total = 0
    for dirpath, _dirnames, filenames in os.walk(root):
        for name in filenames:
            if name.endswith(".rs"):
                total += prod_lines(os.path.join(dirpath, name))
    return total


def main(argv: list[str]) -> int:
    if not argv:
        print(__doc__)
        return 2
    grand = 0
    for root in argv:
        count = total_for(root)
        grand += count
        print(f"{root}: {count}")
    if len(argv) > 1:
        print(f"total: {grand}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
