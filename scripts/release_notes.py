#!/usr/bin/env python3
"""Extract a `CHANGELOG.md` release-notes section for `release.yml` (#1919).

Ported from quadraui's `scripts/release_decide.py` `notes` subcommand (and
its `changelog_section` helper) — vimcode doesn't need that script's
`decide` subcommand (vimcode has no tag-exists-on-remote no-op path; its
release job already runs unconditionally on every push to `main`), so only
the changelog-extraction half is vendored here, trimmed to a single
responsibility.

`release.yml`'s GitHub release body is a static preamble (status,
installation instructions) plus this script's output: the `## [<version>]`
section of `CHANGELOG.md`. The job fails, before publishing anything, if
that section is missing or empty — a version bump with no changelog entry
is treated as incomplete, not released with stale or absent notes.

    python3 scripts/release_notes.py \\
        --changelog CHANGELOG.md --version 0.15.1 --out notes.md
"""
from __future__ import annotations

import argparse
import sys
from pathlib import Path


def changelog_section(text: str, version: str) -> str:
    """Return the body of CHANGELOG.md's `## [<version>]` section.

    Everything after the matching `## [<version>]` header line, up to (not
    including) the next `## [` header. Returns "" if no such header exists,
    which also covers a request for `version="Unreleased"` matching nothing
    (callers never pass that — `release.yml` only ever asks for a real
    released version, never the `## [Unreleased]` placeholder section).
    """
    header = f"## [{version}]"
    lines = text.splitlines()
    body: list[str] = []
    in_section = False
    for line in lines:
        if line.startswith(header):
            in_section = True
            continue
        if in_section and line.startswith("## ["):
            break
        if in_section:
            body.append(line)
    return "\n".join(body)


def cmd_notes(args: argparse.Namespace) -> int:
    changelog_path = Path(args.changelog)
    try:
        text = changelog_path.read_text(encoding="utf-8")
    except FileNotFoundError:
        print(f"::error::{changelog_path} not found", file=sys.stderr)
        return 1
    section = changelog_section(text, args.version)
    if not section.strip():
        print(
            f"::error::{changelog_path} has no non-empty '## [{args.version}]' section",
            file=sys.stderr,
        )
        return 1
    Path(args.out).write_text(section, encoding="utf-8")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--changelog", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args(argv)
    return cmd_notes(args)


if __name__ == "__main__":
    sys.exit(main())
