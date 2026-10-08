#!/usr/bin/env python3
"""Validate `tests/smoke-spec/*.yaml` against coord's own real spec loaders
(vimcode#1646).

Each file under `tests/smoke-spec/` is consumed by one of coord's Tier-2
real-platform acceptance drivers — `coord/tui_pty_driver.py`,
`coord/win_native_driver.py`, `coord/mac_native_driver.py`,
`coord/gtk_native_driver.py` (claude-coordinator `main`, v0.5.551+). This
script imports each driver's own `parse_smoke_spec`/`parse_native_spec`
function and runs it against the matching file, so "does this spec parse"
is answered by the exact code that will actually run it in CI/on a fleet
host — not a hand-rolled YAML-shape check that could drift from what the
real driver accepts.

Requires the `coord` package (`pip install code-coordinator`) importable —
this script does not vendor or reimplement any parsing logic of its own.

Usage:
    python3 scripts/validate_smoke_specs.py
"""

from __future__ import annotations

import pathlib
import sys

try:
    from coord.tui_pty_driver import parse_smoke_spec
    from coord.win_native_driver import parse_native_spec as parse_win_native_spec
    from coord.mac_native_driver import parse_native_spec as parse_mac_native_spec
    from coord.gtk_native_driver import parse_native_spec as parse_gtk_native_spec
except ModuleNotFoundError as exc:
    print(
        f"error: could not import coord's driver modules ({exc}).\n"
        "  Install the coordinator package first:  pip install code-coordinator",
        file=sys.stderr,
    )
    sys.exit(2)

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
SPEC_DIR = REPO_ROOT / "tests" / "smoke-spec"

# (filename, parser, driver kind label) — the driver kind each file is
# authored for, per vimcode#1646's own table.
SPECS = [
    ("tui.yaml", parse_smoke_spec, "tui-pty"),
    ("win-gui.yaml", parse_win_native_spec, "win-native"),
    ("win-terminal.yaml", parse_win_native_spec, "win-native"),
    ("mac-gui.yaml", parse_mac_native_spec, "mac-native"),
    ("gtk-gui.yaml", parse_gtk_native_spec, "gtk-native"),
]


def main() -> int:
    all_ok = True
    for filename, parser, kind in SPECS:
        path = SPEC_DIR / filename
        if not path.exists():
            print(f"FAIL {filename:20s} [{kind}] file not found: {path}")
            all_ok = False
            continue
        text = path.read_text()
        try:
            spec = parser(text)
        except Exception as exc:  # noqa: BLE001 — report every parse error, not just the first
            print(f"FAIL {filename:20s} [{kind}] {type(exc).__name__}: {exc}")
            all_ok = False
            continue
        step_ids = [s.step_id for s in spec.steps]
        print(f"OK   {filename:20s} [{kind}] {len(step_ids)} steps: {step_ids}")
    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main())
