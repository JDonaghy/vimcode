#!/usr/bin/env python3
"""Comment-history lint: flag issue refs and "narrates the past" phrasing in
comments (#1896, a port of quadraui#1112).

Policy (see vimcode's `CLAUDE.md`, "Comment policy" / Code Style section):
comments describe the code *as it is*; history (what it used to do, why a PR
changed it, what a reviewer said) belongs in the commit message and the issue
tracker, not inline. The one sanctioned exception is `TODO(#N)` / `FIXME(#N)`
— a workaround whose removal is gated on that issue closing.

This tool is a blunt counter, not a classifier. It flags every `//` comment
line (including `///` and `//!` doc comments, but not a doc comment's fenced
code-block lines, which are doctests) containing an issue reference
(`#\\d{2,5}` or `repo#\\d+`) or one of a fixed list of history phrases ("used
to", "no longer", "previously", "before #", "pre-#", "this pr", "the
reviewer", "adversarial review" — case-insensitive), groups the counts by
module, and compares each group's count against a per-group threshold. A
human mechanical pass (#1906 here) is what actually tells load-bearing
references apart from narrating ones and lowers the threshold as it cleans
each module up — this tool only stops the count from climbing back up
afterwards.

Usage:
    tools/comment_history_lint.py                    # report + exit 1 on any group over threshold
    tools/comment_history_lint.py --report-only       # report, always exit 0
    tools/comment_history_lint.py --thresholds FILE   # override the default thresholds file

Grouping mirrors issue #1896's module table:
    src             top-level src/*.rs (not in a listed subdir below)
    src/app         src/app/
    src/render      src/render/
    src/core/engine src/core/engine/
    src/core        src/core/ (the rest)
    tests           tests/*.rs
    backends        src/gtk/, src/tui_main/, src/win/, src/macos/, src/harness/

Run from anywhere; paths are resolved relative to this script's location.
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_THRESHOLDS_FILE = Path(__file__).resolve().parent / "comment_history_thresholds.json"

# src/ subdirectories that get their own group (or whose nested "engine"
# subdir does), per issue #1896's module table. Any other subdir name is
# untabled and falls back to "src", same policy quadraui#1112 uses.
BACKEND_SUBDIRS = {"gtk", "tui_main", "win", "macos", "harness"}

SCAN_ROOTS = ["src", "tests"]

# `#\d+` on its own already catches "before #123"; it's listed again here as
# a literal phrase so a bare "before #" (no digits following, e.g. a
# half-written comment) still gets flagged, matching #1896's phrase list
# verbatim. Likewise "pre-#" covers "pre-#1234"-style phrasing that doesn't
# read as "before #".
HISTORY_PHRASES = [
    "used to",
    "no longer",
    "previously",
    "before #",
    "pre-#",
    "this pr",
    "the reviewer",
    "adversarial review",
]

# A bare issue ref (`#1234`) or a cross-repo one (`quadraui#1234`). The
# second alternative must come first so `repo#123` doesn't get truncated to
# just its `#123` suffix by the first alternative alone (that would still
# flag it, but `is_issue_ref` detection should see the whole match for
# TODO/FIXME stripping below to line up positionally; keeping them as
# alternatives in one compiled pattern is simplest).
ISSUE_REF_RE = re.compile(r"[A-Za-z][A-Za-z0-9_-]*#\d+|#\d{2,5}")

# The sanctioned exception: `TODO(#1234)` / `FIXME(#1234)` is a workaround
# marker meant to be removed when the issue closes, not a narrating
# reference. Strip these out of the comment text before searching for
# ISSUE_REF_RE matches so they're never counted.
SANCTIONED_REF_RE = re.compile(r"(?:TODO|FIXME)\(#\d+\)", re.IGNORECASE)

# Avoid treating a URL's "://" as a Rust line-comment opener -- `// see
# https://example.com/foo` is one comment, not comment-then-garbage starting
# at the second "//".
RS_COMMENT_START_RE = re.compile(r"(?<!:)//")

FENCE_RE = re.compile(r"^(?:///|//!|//)\s*```")


def comment_text(line: str, suffix: str) -> str | None:
    """The comment portion of `line`, or None if it has no `.rs` comment."""
    if suffix != ".rs":
        return None
    m = RS_COMMENT_START_RE.search(line)
    return line[m.start() :] if m else None


def group_for(path: Path, repo_root: Path = REPO_ROOT) -> str:
    rel = path.relative_to(repo_root)
    parts = rel.parts
    if parts[0] == "tests":
        return "tests"
    if parts[0] != "src":
        return "other"
    if len(parts) == 2:
        # src/<file>.rs: no subdir.
        return "src"
    subdir = parts[1]
    if subdir in BACKEND_SUBDIRS:
        return "backends"
    if subdir == "app":
        return "src/app"
    if subdir == "render":
        return "src/render"
    if subdir == "core":
        if len(parts) >= 3 and parts[2] == "engine":
            return "src/core/engine"
        return "src/core"
    return "src"


def iter_scan_files(repo_root: Path = REPO_ROOT):
    for rel in SCAN_ROOTS:
        root = repo_root / rel
        if root.is_dir():
            yield from sorted(root.rglob("*.rs"))


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
    in_fenced_block = False
    for i, line in enumerate(lines, start=1):
        text = comment_text(line, path.suffix)
        if text is None:
            # Code resumes: a doc-comment's fenced block must close before
            # the comment block ends, so leaving one "open" here is a
            # malformed doctest, not this tool's problem to guess at.
            in_fenced_block = False
            continue
        if FENCE_RE.match(text):
            in_fenced_block = not in_fenced_block
            continue
        if in_fenced_block:
            # Doctest source inside a /// fenced code block, not prose.
            continue
        sanctioned_stripped = SANCTIONED_REF_RE.sub("", text)
        lowered = text.lower()
        is_issue_ref = ISSUE_REF_RE.search(sanctioned_stripped) is not None
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
        description="Flag issue-number and history-phrase comments, grouped by module (#1896)."
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
