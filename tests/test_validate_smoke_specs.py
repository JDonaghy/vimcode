"""Unit tests for `scripts/validate_smoke_specs.py` (vimcode#1884).

One passing and one failing fixture per static rule, plus a couple of
integration-shaped tests: the real `tests/smoke-spec/*.yaml` files run
clean (green, or explicitly KNOWN) through every rule, and the
`KNOWN_VIOLATIONS` allowlist actually downgrades a matching violation
instead of silently hiding ones that don't match.

Run with: `pytest tests/test_validate_smoke_specs.py` (needs `pyyaml` +
`pytest`; does NOT need `code-coordinator` -- these tests exercise only the
static checks, never `check_coord_parsing`).
"""

from __future__ import annotations

import importlib.util
import pathlib
import sys

import pytest
import yaml

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
SCRIPT_PATH = REPO_ROOT / "scripts" / "validate_smoke_specs.py"

_spec = importlib.util.spec_from_file_location("validate_smoke_specs", SCRIPT_PATH)
vss = importlib.util.module_from_spec(_spec)
sys.modules["validate_smoke_specs"] = vss
_spec.loader.exec_module(vss)  # type: ignore[union-attr]


def _steps(yaml_text: str) -> list[dict]:
    return vss._steps_of(yaml.safe_load(yaml_text))


# ── Rule 1: expect_file marker uniqueness ───────────────────────────────


def test_expect_file_unique_markers_pass():
    steps = _steps(
        """
        steps:
          - type: expect_file
            id: first-probe
            path: /tmp/a.txt
            contains: "MARKER_ONE"
          - type: expect_file
            id: second-probe
            path: /tmp/b.txt
            contains: "MARKER_TWO"
        """
    )
    assert vss.expect_file_marker_violations(steps) == []


def test_expect_file_duplicate_marker_fails():
    steps = _steps(
        """
        steps:
          - type: expect_file
            id: first-probe
            path: /tmp/a.txt
            contains: "SAME_MARKER"
          - type: expect_file
            id: second-probe
            path: /tmp/b.txt
            contains: "SAME_MARKER"
        """
    )
    violations = vss.expect_file_marker_violations(steps)
    assert len(violations) == 1
    assert violations[0]["rule"] == "expect_file_marker_uniqueness"
    assert violations[0]["step_id"] == "second-probe"


# ── Rule 2: known_bug must tag a step that can actually fail ───────────


def test_known_bug_on_failable_step_passes():
    steps = _steps(
        """
        steps:
          - type: expect_frontmost
            id: launch-is-frontmost
            known_bug: vimcode#1825
        """
    )
    assert vss.known_bug_inversion_violations(steps) == []


@pytest.mark.parametrize("step_type", ["wait", "launch", "expect_no_tofu"])
def test_known_bug_on_cannot_fail_step_fails(step_type):
    steps = [{"type": step_type, "id": "bogus-tag", "known_bug": "vimcode#1"}]
    violations = vss.known_bug_inversion_violations(steps)
    assert len(violations) == 1
    assert violations[0]["rule"] == "known_bug_inversion"
    assert violations[0]["step_id"] == "bogus-tag"


# ── Rule 3: no raw control chars in type_text on a native-GUI lane ─────


def test_type_text_without_control_chars_passes_on_gui_lane():
    steps = _steps(
        """
        steps:
          - type: type_text
            id: type-marker
            text: "hello world"
        """
    )
    assert vss.control_char_violations(steps, "gtk-gui.yaml") == []


def test_type_text_with_carriage_return_fails_on_gui_lane():
    steps = _steps(
        """
        steps:
          - type: type_text
            id: type-and-submit
            text: ":w\\r"
        """
    )
    violations = vss.control_char_violations(steps, "gtk-gui.yaml")
    assert len(violations) == 1
    assert violations[0]["rule"] == "type_text_control_char"
    assert violations[0]["step_id"] == "type-and-submit"


def test_type_text_with_carriage_return_allowed_on_raw_stream_lane():
    """tui.yaml/win-terminal.yaml send straight into a real pty/ConPTY byte
    stream, where `\\r` is the legitimate way to submit an ex command."""
    steps = _steps(
        """
        steps:
          - type: type_text
            id: type-and-submit
            text: ":w\\r"
        """
    )
    assert vss.control_char_violations(steps, "tui.yaml") == []
    assert vss.control_char_violations(steps, "win-terminal.yaml") == []


# ── Rule 4: additive-only for sealed specs ──────────────────────────────


def test_additive_only_new_step_appended_passes():
    old_steps = [{"id": "a", "type": "key", "key": "i"}]
    new_steps = [
        {"id": "a", "type": "key", "key": "i"},
        {"id": "b", "type": "key", "key": "esc"},
    ]
    assert vss.additive_only_violations(old_steps, new_steps) == []


def test_additive_only_modified_existing_step_fails():
    old_steps = [{"id": "a", "type": "key", "key": "i"}]
    new_steps = [{"id": "a", "type": "key", "key": "x"}]
    violations = vss.additive_only_violations(old_steps, new_steps)
    assert len(violations) == 1
    assert violations[0]["rule"] == "additive_only"
    assert violations[0]["step_id"] == "a"


def test_additive_only_removed_existing_step_fails():
    old_steps = [{"id": "a", "type": "key", "key": "i"}, {"id": "b", "type": "key", "key": "esc"}]
    new_steps = [{"id": "a", "type": "key", "key": "i"}]
    violations = vss.additive_only_violations(old_steps, new_steps)
    assert len(violations) == 1
    assert violations[0]["step_id"] == "b"


# ── KNOWN_VIOLATIONS allowlist ──────────────────────────────────────────


def test_known_violation_is_downgraded_not_hidden(monkeypatch, tmp_path, capsys):
    spec_dir = tmp_path / "smoke-spec"
    spec_dir.mkdir()
    (spec_dir / "fake-lane.yaml").write_text(
        """
        steps:
          - type: wait
            id: tagged-wait
            ms: 100
            known_bug: vimcode#999
        """
    )
    monkeypatch.setattr(vss, "SPEC_DIR", spec_dir)
    monkeypatch.setattr(
        vss,
        "KNOWN_VIOLATIONS",
        {("fake-lane.yaml", "known_bug_inversion", "tagged-wait"): "vimcode#999 -- test fixture"},
    )

    violations = vss.validate_file("fake-lane.yaml", base_commit=None)

    assert violations == []  # downgraded, not surfaced as a failure
    out = capsys.readouterr().out
    assert "KNOWN fake-lane.yaml" in out
    assert "vimcode#999" in out


def test_unlisted_violation_still_fails(monkeypatch, tmp_path):
    spec_dir = tmp_path / "smoke-spec"
    spec_dir.mkdir()
    (spec_dir / "fake-lane.yaml").write_text(
        """
        steps:
          - type: wait
            id: tagged-wait
            ms: 100
            known_bug: vimcode#999
        """
    )
    monkeypatch.setattr(vss, "SPEC_DIR", spec_dir)
    monkeypatch.setattr(vss, "KNOWN_VIOLATIONS", {})

    violations = vss.validate_file("fake-lane.yaml", base_commit=None)

    assert len(violations) == 1
    assert violations[0]["rule"] == "known_bug_inversion"


# ── Integration: the real specs in this repo ────────────────────────────


def test_real_sealed_specs_have_no_unlisted_violations():
    """This is the acceptance bar from vimcode#1884 itself: the validator
    runs green on the current specs, or flags known issues listed in the
    PR (the `KNOWN_VIOLATIONS` allowlist above IS that list)."""
    base_commit = vss._resolve_base_commit()
    failures = []
    for filename in vss.SEALED_SPEC_FILES:
        violations = vss.validate_file(filename, base_commit)
        for v in violations:
            failures.append(f"{filename}: [{v['rule']}] {v['message']}")
    assert failures == [], "\n".join(failures)


def test_main_exits_zero_against_the_real_repo():
    assert vss.main() == 0
