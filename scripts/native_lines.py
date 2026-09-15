#!/usr/bin/env python3
"""Count production lines that name a native toolkit type, per backend file.

Companion to `prod_lines.py`. `prod_lines.py` answers "how big is this
backend"; this answers "how much of it is actually bound to its toolkit" —
the question `docs/IRREDUCIBLE_SURFACE.md` exists to settle.

    python3 scripts/native_lines.py gtk src/gtk/*.rs
    python3 scripts/native_lines.py tui src/tui_main/*.rs

**What it counts:** production lines (`#[cfg(test)]` items stripped, same rule
as `prod_lines.py`) that mention a toolkit module path or type — `gtk4::`,
`gio::`, `glib::`, `gdk::`, `pango`, `cairo` for GTK; `ratatui::`,
`crossterm::`, `Buffer`, `Frame`, `Rect` for TUI.

**What it undercounts, deliberately:** a stored widget handle used without
naming its type (`self.window.as_ref()`, `da.queue_draw()`) does not match.
Measured on `src/gtk/mod.rs` at 2026-09-03 that residue was ~15 lines against
62 matched, so treat the result as a floor with roughly a 25% undercount on
the GTK side, not an exact figure. It is a screening measure: a file at 1%
is not platform-bound in any interesting sense, and that is the claim it
supports.

**What it does NOT claim:** that the other 99% is mechanically convergeable.
It establishes only that platform-specificity is not what keeps those lines
in a backend directory.
"""

from __future__ import annotations

import re
import sys

PATTERNS = {
    "gtk": re.compile(r"\b(gtk4?::|gio::|glib::|gdk4?::|pango|cairo|pangocairo)"),
    "tui": re.compile(r"\b(ratatui::|crossterm::|Buffer|Frame\b|Rect\b)"),
}


def strip_test_items(lines: list[str]) -> list[str]:
    """Drop every `#[cfg(test)]` item — same rule as prod_lines.py."""
    kept: list[str] = []
    i, n = 0, len(lines)
    while i < n:
        stripped = lines[i].strip()
        if stripped.startswith("#[cfg(test)]") or stripped.startswith("#[cfg(all(test"):
            i += 1
            while i < n and lines[i].strip().startswith("#["):
                i += 1
            depth, opened = 0, False
            while i < n:
                depth += lines[i].count("{") - lines[i].count("}")
                if "{" in lines[i]:
                    opened = True
                i += 1
                if opened and depth <= 0:
                    break
            continue
        kept.append(lines[i])
        i += 1
    return kept


def main(argv: list[str]) -> int:
    if len(argv) < 2 or argv[0] not in PATTERNS:
        print(__doc__)
        return 2
    pattern = PATTERNS[argv[0]]
    total_prod = total_native = 0
    for path in argv[1:]:
        with open(path, encoding="utf-8", errors="replace") as fh:
            lines = strip_test_items(fh.read().splitlines())
        native = sum(1 for line in lines if pattern.search(line))
        total_prod += len(lines)
        total_native += native
        pct = 100 * native / max(len(lines), 1)
        print(f"{path}: {len(lines)} prod, {native} native-touching ({pct:.1f}%)")
    if len(argv) > 2:
        pct = 100 * total_native / max(total_prod, 1)
        print(f"total: {total_prod} prod, {total_native} native-touching ({pct:.1f}%)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
