#!/usr/bin/env python3
"""Comment-history lint: flag issue refs and "narrates the past" phrasing in
comments. Ported from quadraui's tools/comment_history_lint.py.

Policy (CLAUDE.md "Code Style"): comments describe the code *as it is*;
history (what it used to do, why a PR changed it, what a reviewer said)
belongs in the commit message and the issue tracker. The one exception is a
load-bearing issue reference -- a workaround whose removal is gated on that
issue closing.

This is a blunt counter and a ratchet: it counts every `//`/`#` comment line
with an issue reference (`#\\d+`) or a history phrase, per group, and fails
when a group's count rises above `scripts/comment_history_thresholds.json`.
A cleanup PR lowers its group's threshold in the same PR.

Usage:
    scripts/comment_history_lint.py                    # report + exit 1 on any group over threshold
    scripts/comment_history_lint.py --report-only       # report, always exit 0
    scripts/comment_history_lint.py --thresholds FILE   # override the thresholds file

Groups: core, gtk, macos, win, tui_main, harness (subdirs of src/), src
(everything else under src/ plus Cargo.toml), tests.
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_THRESHOLDS_FILE = Path(__file__).resolve().parent / "comment_history_thresholds.json"

# Directories under src/ that get their own group. Loose files under src/
# and any other subdir fall into "src"; tests/ is its own group.
SRC_SUBDIR_GROUPS = {"core", "gtk", "macos", "win", "tui_main", "harness"}

SCAN_ROOTS = ["Cargo.toml", "src", "tests"]

COMMENT_MARKER_BY_SUFFIX = {
    ".rs": "//",
    ".toml": "#",
}

# `#\d+` on its own already catches "before #123"; it's listed again here as
# a literal phrase so a bare "before #" (no digits following, e.g. a
# half-written comment) still gets flagged, matching #1112's phrase list
# verbatim.
HISTORY_PHRASES = [
    "used to",
    "no longer",
    "before #",
    "this pr",
    "the reviewer",
    "adversarial review",
]
ISSUE_REF_RE = re.compile(r"#\d+")

# Avoid treating a URL's "://" as a Rust line-comment opener -- `// see
# https://example.com/foo` is one comment, not comment-then-garbage starting
# at the second "//".
RS_COMMENT_START_RE = re.compile(r"(?<!:)//")


def comment_text(line: str, suffix: str) -> str | None:
    """The comment portion of `line`, or None if it has no comment."""
    marker = COMMENT_MARKER_BY_SUFFIX.get(suffix)
    if marker is None:
        return None
    if marker == "//":
        m = RS_COMMENT_START_RE.search(line)
        return line[m.start() :] if m else None
    # TOML: a bare '#' is always a comment opener (TOML has no '#' escape
    # inside strings in this codebase's usage), so first occurrence wins.
    idx = line.find("#")
    return line[idx:] if idx != -1 else None


def group_for(path: Path, repo_root: Path = REPO_ROOT) -> str:
    parts = path.relative_to(repo_root).parts
    if parts[0] == "tests":
        return "tests"
    if len(parts) > 2 and parts[0] == "src" and parts[1] in SRC_SUBDIR_GROUPS:
        return parts[1]
    return "src"


def iter_scan_files(repo_root: Path = REPO_ROOT):
    for rel in SCAN_ROOTS:
        root = repo_root / rel
        if root.is_file():
            yield root
        elif root.is_dir():
            for suffix in COMMENT_MARKER_BY_SUFFIX:
                yield from sorted(root.rglob(f"*{suffix}"))


class Finding:
    __slots__ = ("path", "line_no", "text", "is_issue_ref", "phrases")

    def __init__(self, path: Path, line_no: int, text: str, is_issue_ref: bool, phrases: list[str]):
        self.path = path
        self.line_no = line_no
        self.text = text
        self.is_issue_ref = is_issue_ref
        self.phrases = phrases


def scan_file(path: Path) -> list[Finding]:
    findings = []
    try:
        lines = path.read_text(errors="replace").splitlines()
    except OSError:
        return findings
    for i, line in enumerate(lines, start=1):
        text = comment_text(line, path.suffix)
        if text is None:
            continue
        lowered = text.lower()
        is_issue_ref = ISSUE_REF_RE.search(text) is not None
        phrases = [p for p in HISTORY_PHRASES if p in lowered]
        if is_issue_ref or phrases:
            findings.append(Finding(path, i, text.strip(), is_issue_ref, phrases))
    return findings


def load_thresholds(path: Path) -> dict[str, int]:
    if not path.exists():
        return {}
    return json.loads(path.read_text())


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Flag issue-number and history-phrase comments, grouped by module (#1112)."
    )
    parser.add_argument(
        "--report-only",
        action="store_true",
        help="Print the report but always exit 0 (ignore thresholds).",
    )
    parser.add_argument(
        "--thresholds",
        type=Path,
        default=DEFAULT_THRESHOLDS_FILE,
        metavar="FILE",
        help=(
            "JSON file mapping group name -> max allowed flagged-comment-line "
            "count (default: %(default)s). A group with no entry is treated "
            "as unthresholded (never fails)."
        ),
    )
    parser.add_argument(
        "--verbose",
        action="store_true",
        help="Print every flagged line, not just per-group counts.",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    thresholds = load_thresholds(args.thresholds)

    by_group: dict[str, list[Finding]] = {}
    for path in iter_scan_files():
        for finding in scan_file(path):
            by_group.setdefault(group_for(path), []).append(finding)

    if args.verbose:
        for group in sorted(by_group):
            print(f"-- {group} --")
            for f in by_group[group]:
                rel = f.path.relative_to(REPO_ROOT)
                print(f"  {rel}:{f.line_no}: {f.text}")

    all_groups = sorted(set(by_group) | set(thresholds))
    name_width = max((len(g) for g in all_groups), default=len("group"))
    name_width = max(name_width, len("group"))
    print(f"{'group':<{name_width}}  {'count':>6}  {'threshold':>9}  status")

    any_over = False
    for group in all_groups:
        count = len(by_group.get(group, []))
        threshold = thresholds.get(group)
        if threshold is None:
            status = "(unthresholded)"
        elif count > threshold:
            status = "OVER"
            any_over = True
        else:
            status = "ok"
        threshold_cell = "—" if threshold is None else str(threshold)
        print(f"{group:<{name_width}}  {count:>6}  {threshold_cell:>9}  {status}")

    total = sum(len(v) for v in by_group.values())
    print(f"\ntotal flagged comment lines: {total}")

    if args.report_only:
        return 0
    return 1 if any_over else 0


if __name__ == "__main__":
    sys.exit(main())
