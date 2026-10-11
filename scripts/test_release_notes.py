#!/usr/bin/env python3
"""Tests for `scripts/release_notes.py` (#1919).

Stdlib-only (`unittest`), matching `tools/test_example_coverage.py`'s own
rationale for avoiding a pytest dependency. Run with:

    python3 scripts/test_release_notes.py

Covers the three branches `.github/workflows/release.yml`'s release-notes
step relies on:

  - a non-empty `## [X.Y.Z]` section for the requested version -> extracted
  - no such section (or an empty one) -> `notes` fails (exit 1, no output
    file written), before the release body is assembled
  - `## [Unreleased]` is never mistaken for a released version's section
"""
from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import release_notes  # noqa: E402

CHANGELOG = """\
# Changelog

## [Unreleased]

### Added
- nothing yet

## [0.15.1] - 2026-10-11

### Fixed
- a thing

## [0.15.0] - 2026-09-01

### Fixed
- another thing
"""


class ChangelogSectionTests(unittest.TestCase):
    def test_extracts_matching_section_only(self):
        section = release_notes.changelog_section(CHANGELOG, "0.15.1")
        self.assertIn("a thing", section)
        self.assertNotIn("another thing", section)
        self.assertNotIn("nothing yet", section)

    def test_missing_section_is_empty(self):
        section = release_notes.changelog_section(CHANGELOG, "9.9.9")
        self.assertEqual(section.strip(), "")

    def test_unreleased_section_ignored_when_version_requested(self):
        # A request for a real version must never fall through to the
        # `## [Unreleased]` placeholder's body, even though it is the first
        # `## [` header in the file.
        section = release_notes.changelog_section(CHANGELOG, "0.15.1")
        self.assertNotIn("nothing yet", section)

    def test_unreleased_itself_extracts_empty(self):
        # Asking for "Unreleased" literally returns its (here, empty-ish)
        # body rather than erroring — `cmd_notes` is what turns an
        # all-whitespace body into a hard failure.
        section = release_notes.changelog_section(CHANGELOG, "Unreleased")
        self.assertIn("nothing yet", section)


class NotesCommandTests(unittest.TestCase):
    def _write_changelog(self, tmp: Path, text: str) -> Path:
        path = tmp / "CHANGELOG.md"
        path.write_text(text)
        return path

    def _run(self, changelog: Path, version: str, out: Path) -> subprocess.CompletedProcess:
        return subprocess.run(
            [
                sys.executable,
                str(Path(release_notes.__file__)),
                "--changelog",
                str(changelog),
                "--version",
                version,
                "--out",
                str(out),
            ],
            capture_output=True,
            text=True,
        )

    def test_notes_extracts_non_empty_section(self):
        with tempfile.TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            changelog = self._write_changelog(tmp, CHANGELOG)
            out = tmp / "notes.md"
            result = self._run(changelog, "0.15.1", out)
            self.assertEqual(result.returncode, 0, result.stderr)
            notes = out.read_text()
            self.assertIn("a thing", notes)
            self.assertNotIn("another thing", notes)
            self.assertNotIn("nothing yet", notes)

    def test_notes_fails_on_missing_section(self):
        with tempfile.TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            changelog = self._write_changelog(tmp, CHANGELOG)
            out = tmp / "notes.md"
            result = self._run(changelog, "9.9.9", out)
            self.assertEqual(result.returncode, 1)
            self.assertIn("::error::", result.stderr)
            self.assertFalse(out.exists())

    def test_notes_fails_on_empty_section(self):
        with tempfile.TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            changelog = self._write_changelog(
                tmp, "## [0.15.1]\n\n\n## [0.15.0]\n\nsomething\n"
            )
            out = tmp / "notes.md"
            result = self._run(changelog, "0.15.1", out)
            self.assertEqual(result.returncode, 1)
            self.assertFalse(out.exists())

    def test_notes_never_extracts_unreleased_for_a_real_version(self):
        with tempfile.TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            # Only `## [Unreleased]` exists -- no section for 0.15.1 at all.
            changelog = self._write_changelog(
                tmp, "## [Unreleased]\n\n### Added\n- nothing yet\n"
            )
            out = tmp / "notes.md"
            result = self._run(changelog, "0.15.1", out)
            self.assertEqual(result.returncode, 1)
            self.assertFalse(out.exists())


if __name__ == "__main__":
    unittest.main()
